//! Patch and hunk parsing from `Services/GitService.swift`: whole patches held in memory
//! (`parsePatch`, `parseHunks`), C-quoted git paths, and disk patches streamed with bounded
//! memory (`PatchLineReader`, `indexPatch`).

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::model::{DiffLine, FileDiff, GitError, GitResult, Hunk, LineType, PatchSlice};
use super::runner::CancelToken;

/// Bytes of one disk patch line kept for display; the rest is marked as shortened.
pub const MAX_LINE_BYTES: usize = 2048;

/// Whether a patch line says the file is binary.
pub fn is_binary_marker(line: &str) -> bool {
    line.starts_with("Binary files ") || line == "GIT binary patch"
}

/// The start line of `-old,count` or `+new,count` (`Int(parts[n].dropFirst().split(",")[0]) ?? 0`).
fn range_start(part: &str) -> i64 {
    part.get(1..)
        .unwrap_or("")
        .split(',')
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0)
}

/// `(old, new)` start lines of a `@@ -a,b +c,d @@` header, split on spaces.
pub fn hunk_starts(header: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = header.split(' ').filter(|part| !part.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    Some((range_start(parts[1]), range_start(parts[2])))
}

/// Text after the first character (`String(line.dropFirst())`).
fn drop_first(line: &str) -> &str {
    line.char_indices()
        .nth(1)
        .map_or("", |(index, _)| &line[index..])
}

/// A whole patch for one file as hunks, with counts and binary evidence (`parsePatch`).
pub fn parse_patch(
    patch: &str,
    filename: &str,
    status: &str,
    old_filename: Option<String>,
) -> FileDiff {
    let mut hunks = Vec::new();
    let mut header = String::new();
    let mut lines = Vec::new();
    let (mut old_no, mut new_no, mut added, mut removed) = (0i64, 0i64, 0usize, 0usize);
    let mut binary = false;
    for line in patch.split('\n') {
        binary = binary || is_binary_marker(line);
        if line.starts_with("@@ ") {
            if !header.is_empty() {
                hunks.push(Hunk {
                    header: std::mem::take(&mut header),
                    lines: std::mem::take(&mut lines),
                });
            }
            header = line.to_string();
            lines = Vec::new();
            (old_no, new_no) = hunk_starts(line).unwrap_or((0, 0));
        } else if !header.is_empty() {
            let content = drop_first(line).to_string();
            if line.starts_with('+') {
                lines.push(DiffLine {
                    line_type: LineType::Addition,
                    content,
                    old_line_no: None,
                    new_line_no: Some(new_no),
                });
                new_no += 1;
                added += 1;
            } else if line.starts_with('-') {
                lines.push(DiffLine {
                    line_type: LineType::Deletion,
                    content,
                    old_line_no: Some(old_no),
                    new_line_no: None,
                });
                old_no += 1;
                removed += 1;
            } else if line.starts_with(' ') {
                lines.push(DiffLine {
                    line_type: LineType::Context,
                    content,
                    old_line_no: Some(old_no),
                    new_line_no: Some(new_no),
                });
                old_no += 1;
                new_no += 1;
            }
        }
    }
    if !header.is_empty() {
        hunks.push(Hunk { header, lines });
    }
    let mut file = FileDiff::new(filename, status, added, removed, hunks);
    file.old_filename = old_filename;
    file.is_binary = binary;
    file
}

/// The start number after `marker` (`-` or `+`) in a hunk header, or 1 when it does not parse.
fn header_number(rest: &str, marker: char) -> Option<i64> {
    let index = rest.find(marker)?;
    let part = &rest[index + 1..];
    let end = part.find([',', ' ']).unwrap_or(part.len());
    Some(part[..end].parse().unwrap_or(1))
}

/// The hunks of one file's patch (`parseHunks`), ignoring every line before the first hunk
/// so file content containing `diff --git` cannot create fake rows.
pub fn parse_hunks<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut header = String::new();
    let mut current: Vec<DiffLine> = Vec::new();
    let (mut old_no, mut new_no) = (0i64, 0i64);
    let mut in_hunk = false;
    for line in lines {
        if line.starts_with("@@") {
            if in_hunk && !current.is_empty() {
                hunks.push(Hunk {
                    header: std::mem::take(&mut header),
                    lines: std::mem::take(&mut current),
                });
            }
            header = line.to_string();
            current = Vec::new();
            in_hunk = true;
            let rest = line.get(3..).unwrap_or("");
            if let Some(number) = header_number(rest, '+') {
                new_no = number;
            }
            if let Some(number) = header_number(rest, '-') {
                old_no = number;
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        let content = drop_first(line).to_string();
        if line.starts_with('+') {
            current.push(DiffLine {
                line_type: LineType::Addition,
                content,
                old_line_no: None,
                new_line_no: Some(new_no),
            });
            new_no += 1;
        } else if line.starts_with('-') {
            current.push(DiffLine {
                line_type: LineType::Deletion,
                content,
                old_line_no: Some(old_no),
                new_line_no: None,
            });
            old_no += 1;
        } else if line.starts_with(' ') {
            current.push(DiffLine {
                line_type: LineType::Context,
                content,
                old_line_no: Some(old_no),
                new_line_no: Some(new_no),
            });
            old_no += 1;
            new_no += 1;
        }
        // "\ No newline at end of file" and anything else is skipped.
    }
    if in_hunk && !current.is_empty() {
        hunks.push(Hunk {
            header,
            lines: current,
        });
    }
    hunks
}

/// The `b/` path of a `diff --git` header whose `+++` line is missing (binary files).
pub fn pr_header_path(header: &str) -> Option<String> {
    let value = header.strip_prefix("diff --git ").unwrap_or("");
    if value.starts_with('"') {
        let mut escaped = false;
        for (index, char) in value.char_indices().skip(1) {
            if char == '"' && !escaped {
                return Some(value[index + 1..].trim_matches(' ').to_string());
            }
            if char == '\\' {
                escaped = !escaped;
            } else {
                escaped = false;
            }
        }
    }
    value
        .rfind(" b/")
        .map(|index| value[index + 1..].to_string())
}

/// A git path with C quoting undone (`"a\303\274\t"` → `aü<TAB>`); unquoted paths are returned as is.
pub fn decode_git_path(value: &str) -> String {
    if !(value.len() >= 2 && value.starts_with('"') && value.ends_with('"')) {
        return value.to_string();
    }
    let bytes = &value.as_bytes()[1..value.len() - 1];
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            output.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            break;
        }
        if (b'0'..=b'7').contains(&bytes[i]) {
            let (mut n, mut count) = (0u32, 0);
            while i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) && count < 3 {
                n = n * 8 + u32::from(bytes[i] - b'0');
                i += 1;
                count += 1;
            }
            output.push(n as u8);
        } else {
            output.push(match bytes[i] {
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                b'b' => 8,
                b'f' => 12,
                b'v' => 11,
                b'a' => 7,
                other => other,
            });
            i += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Name, status and old name of one file from its patch metadata lines (shared by the
/// in-memory PR patch and disk indexing). `fallback` is tried before the header path.
pub fn patch_identity(
    metadata: &[&str],
    fallback: Option<String>,
) -> Option<(String, String, Option<String>)> {
    let target = metadata
        .iter()
        .find(|line| line.starts_with("+++ ") && !line.starts_with("+++ /dev/null"))
        .or_else(|| metadata.iter().find(|line| line.starts_with("--- ")));
    let rename = metadata
        .iter()
        .find_map(|line| line.strip_prefix("rename to "));
    let raw = rename
        .map(str::to_string)
        .or_else(|| target.map(|line| line[4..].split('\t').next().unwrap_or("").to_string()))
        .or(fallback)
        .or_else(|| pr_header_path(metadata.first().copied().unwrap_or("")))?;
    let decoded = decode_git_path(&raw);
    let name = if rename.is_some() {
        decoded
    } else if decoded.starts_with("a/") || decoded.starts_with("b/") {
        decoded[2..].to_string()
    } else {
        decoded
    };
    let has = |prefix: &str| metadata.iter().any(|line| line.starts_with(prefix));
    let status = if has("new file mode") {
        "A"
    } else if has("deleted file mode") {
        "D"
    } else if rename.is_some() {
        "R"
    } else {
        "M"
    };
    let old = metadata
        .iter()
        .find_map(|line| line.strip_prefix("rename from "))
        .map(decode_git_path);
    Some((name, status.to_string(), old))
}

/// Splits one-process-per-group patch output into per-file sections.
pub fn split_sections(output: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut section = String::new();
    for line in output.split('\n') {
        if line.starts_with("diff --git ")
            || line.starts_with("diff --cc ")
            || line.starts_with("diff --combined ")
        {
            if !section.is_empty() {
                sections.push(std::mem::take(&mut section));
            }
            section = format!("{line}\n");
        } else if !section.is_empty() {
            section.push_str(line);
            section.push('\n');
        }
    }
    if !section.is_empty() {
        sections.push(section);
    }
    sections
}

/// One line of a disk patch: its (possibly shortened) text and where it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchRecord {
    pub text: String,
    pub start: u64,
    pub truncated: bool,
}

/// Streams a disk patch slice with bounded memory, even for minified single lines.
pub struct PatchLineReader {
    file: File,
    end: u64,
    buffer: Vec<u8>,
    index: usize,
    offset: u64,
}

impl PatchLineReader {
    pub fn new(snapshot: &Path, end: u64, offset: u64) -> std::io::Result<Self> {
        let mut file = File::open(snapshot)?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            file,
            end,
            buffer: Vec::new(),
            index: 0,
            offset,
        })
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn next_record(&mut self) -> std::io::Result<Option<PatchRecord>> {
        if self.offset >= self.end {
            return Ok(None);
        }
        let start = self.offset;
        let mut bytes = Vec::new();
        let mut truncated = false;
        while self.offset < self.end {
            if self.index == self.buffer.len() {
                let want = (self.end - self.offset).min(65_536) as usize;
                self.buffer.resize(want, 0);
                let read = self.file.read(&mut self.buffer)?;
                self.buffer.truncate(read);
                self.index = 0;
                if read == 0 {
                    self.offset = self.end;
                    break;
                }
            }
            let newline = self.buffer[self.index..]
                .iter()
                .position(|&byte| byte == b'\n')
                .map(|at| at + self.index);
            let stop = newline.unwrap_or(self.buffer.len());
            let available = stop - self.index;
            let retained = available.min(MAX_LINE_BYTES.saturating_sub(bytes.len()));
            if retained > 0 {
                bytes.extend_from_slice(&self.buffer[self.index..self.index + retained]);
            }
            if retained < available {
                truncated = true;
            }
            self.offset += available as u64;
            self.index = stop;
            if newline.is_some() {
                self.index += 1;
                self.offset += 1;
                break;
            }
        }
        Ok(Some(PatchRecord {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            start,
            truncated,
        }))
    }
}

/// File entries of a disk patch, each pointing at its slice, without retaining lines.
#[derive(Debug, Clone, Default)]
pub struct IndexedPatch {
    pub files: Vec<FileDiff>,
    pub unmerged_paths: HashSet<String>,
}

/// Indexes the patch at `snapshot` (`indexPatch`). `expected` is the file a review page asked
/// for; an unmerged file's raw `* Unmerged path` marker is matched exactly first.
pub fn index_patch(
    snapshot: &Path,
    expected: Option<&FileDiff>,
    cancel: &CancelToken,
) -> GitResult<IndexedPatch> {
    let size = std::fs::metadata(snapshot)?.len();
    let snapshot_name = snapshot.to_string_lossy().into_owned();
    let mut unmerged_paths = HashSet::new();
    let mut offset = 0u64;
    if let Some(expected) = expected.filter(|file| file.status_code == "U") {
        // Unmerged markers use raw filenames, unlike C-quoted patch headers: match the whole
        // known marker so quotes and newlines stay filename bytes.
        let marker = format!("* Unmerged path {}\n", expected.filename).into_bytes();
        let mut head = Vec::new();
        File::open(snapshot)?
            .take(marker.len() as u64)
            .read_to_end(&mut head)?;
        if head == marker {
            unmerged_paths.insert(expected.filename.clone());
            offset = marker.len() as u64;
        }
    }
    let mut reader = PatchLineReader::new(snapshot, size, offset)?;
    let mut files = Vec::new();
    let mut metadata: Vec<String> = Vec::new();
    let mut start = offset;
    let (mut added, mut removed, mut in_hunk, mut binary) = (0usize, 0usize, false, false);

    let finish = |metadata: &[String],
                  start: u64,
                  end: u64,
                  added: usize,
                  removed: usize,
                  binary: bool|
     -> GitResult<Option<FileDiff>> {
        if metadata.is_empty() {
            return Ok(None);
        }
        let lines: Vec<&str> = metadata.iter().map(String::as_str).collect();
        let fallback = expected.and_then(|file| {
            (lines[0] == format!("diff --git a/{0} b/{0}", file.filename))
                .then(|| format!("b/{}", file.filename))
        });
        let Some((name, status, old)) = patch_identity(&lines, fallback) else {
            return Err(GitError::review(
                "GitHub patch paths could not be read; refresh to retry.",
            ));
        };
        let mut file = FileDiff::new(name, status, added, removed, Vec::new());
        file.old_filename = old;
        file.is_binary = binary;
        file.patch_slice = Some(PatchSlice {
            snapshot: snapshot_name.clone(),
            start,
            end,
        });
        file.preview_version = snapshot_name.clone();
        Ok(Some(file))
    };

    while let Some(record) = reader.next_record()? {
        if cancel.is_cancelled() {
            return Err(GitError::Cancelled);
        }
        let line = record.text;
        if line.starts_with("diff --git ") {
            files.extend(finish(
                &metadata,
                start,
                record.start,
                added,
                removed,
                binary,
            )?);
            metadata = vec![line];
            start = record.start;
            (added, removed, in_hunk, binary) = (0, 0, false, false);
        } else if metadata.is_empty() {
            return Err(GitError::review(
                "Git returned an unrecognized patch; refresh to retry.",
            ));
        } else if line.starts_with("@@ ") {
            in_hunk = true;
        } else if in_hunk {
            if line.starts_with('+') {
                added += 1;
            } else if line.starts_with('-') {
                removed += 1;
            }
        } else {
            binary = binary || is_binary_marker(&line);
            if metadata.len() < 32 {
                metadata.push(line);
            }
        }
    }
    files.extend(finish(&metadata, start, size, added, removed, binary)?);
    Ok(IndexedPatch {
        files,
        unmerged_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_c_quoted_paths() {
        assert_eq!(
            decode_git_path(r#""a/quoted \"name\"\t\303\274.txt""#),
            "a/quoted \"name\"\tü.txt"
        );
        assert_eq!(decode_git_path("plain.txt"), "plain.txt");
        assert_eq!(decode_git_path(r#""line\nbreak""#), "line\nbreak");
    }

    #[test]
    fn header_paths_for_binary_files() {
        assert_eq!(
            pr_header_path("diff --git a/x.bin b/x.bin").as_deref(),
            Some("b/x.bin")
        );
        assert_eq!(
            pr_header_path(r#"diff --git "a/binary \"name\".bin" "b/binary \"name\".bin""#)
                .as_deref(),
            Some(r#""b/binary \"name\".bin""#)
        );
        assert_eq!(pr_header_path("diff --git "), None);
    }

    #[test]
    fn parses_hunks_with_line_numbers() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -3,2 +3,2 @@ fn x\n ctx\n-old\n+new\n\\ No newline at end of file\n";
        let file = parse_patch(patch, "f", "M", None);
        assert_eq!((file.added, file.removed, file.hunks.len()), (1, 1, 1));
        let lines = &file.hunks[0].lines;
        assert_eq!(
            (lines[0].old_line_no, lines[0].new_line_no),
            (Some(3), Some(3))
        );
        assert_eq!(
            (lines[1].old_line_no, lines[2].new_line_no),
            (Some(4), Some(4))
        );
        let hunks = parse_hunks(patch.split('\n'));
        assert_eq!(hunks[0].lines.len(), 3);
        assert_eq!(
            parse_hunks(["@@ -0,0 +1 @@", "+a"])[0].lines[0].new_line_no,
            Some(1)
        );
    }

    #[test]
    fn splits_only_real_file_headers() {
        let sections = split_sections(
            "diff --git a/x b/x\n@@ -1 +1 @@\n+diff --git inside\ndiff --git a/y b/y\n",
        );
        assert_eq!(sections.len(), 2);
        assert!(sections[0].contains("+diff --git inside"));
    }
}
