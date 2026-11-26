//! Branch, local, commit and working-tree reviews and paged patch reading, from
//! `Services/GitService.swift`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use super::host::HostFileStat;
use super::model::{
    DiffData, DiffLine, FileDiff, GitBranchReview, GitCommitInfo, GitError, GitLocalReview,
    GitResult, GitReviewSummary, Hunk, LineType, PatchSlice, ReviewPage, ReviewPageCursor,
};
use super::patch::{self, PatchLineReader};
use super::service::{GitService, UntrackedEntry};

/// Lines and bytes one review page holds at most.
pub const PAGE_LINES: usize = 200;
pub const PAGE_BYTES: usize = 32_768;
/// The bounded inline preview of one file.
pub const PREVIEW_LIMIT: usize = 1_000_000;

const OURS_BASELINE: &str = "ours (index stage 2)";
const EMPTY_BASELINE: &str = "empty (ours deleted)";

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

/// `split(separator:)`: pieces without the empty ones.
fn split_nonempty(text: &str, separator: char) -> Vec<String> {
    text.split(separator)
        .filter(|piece| !piece.is_empty())
        .map(str::to_string)
        .collect()
}

/// The `git diff --no-index` arguments that preview a new file against an empty baseline.
fn no_index_arguments(name: &str) -> Vec<String> {
    strings(&[
        "diff",
        "--no-index",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--",
        "/dev/null",
        name,
    ])
}

/// Whether `ls-files --unmerged` lists a stage 2 (ours) entry.
fn has_ours(stages: &[String]) -> bool {
    stages.iter().any(|entry| {
        entry
            .split('\t')
            .next()
            .and_then(|meta| meta.split(' ').filter(|p| !p.is_empty()).next_back())
            == Some("2")
    })
}

/// A file's text read for preview: binary when it has a NUL in its first 8 KB or is not UTF-8.
pub fn decode_preview(data: &[u8]) -> (String, bool) {
    if data[..data.len().min(8192)].contains(&0) {
        return (String::new(), true);
    }
    match String::from_utf8(data.to_vec()) {
        Ok(content) => (content, false),
        Err(_) => (String::new(), true),
    }
}

/// Finder-style name order (`localizedStandardCompare`): case-insensitive, digit runs by value.
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits
                };
                let (dx, dy) = (take(&mut left), take(&mut right));
                let (tx, ty) = (dx.trim_start_matches('0'), dy.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

struct Entry {
    name: String,
    status: String,
    old: Option<String>,
}

impl GitService {
    fn trimmed(&self, args: &[&str], root: &str) -> GitResult<String> {
        Ok(self.checked(args, root)?.trim().to_string())
    }

    fn all_refs(&self, root: &str) -> GitResult<Vec<String>> {
        Ok(split_nonempty(
            &self.checked(
                &[
                    "for-each-ref",
                    "--format=%(refname:short)",
                    "refs/heads",
                    "refs/remotes",
                ],
                root,
            )?,
            '\n',
        )
        .into_iter()
        .filter(|name| !name.ends_with("/HEAD"))
        .collect())
    }

    /// The comparison base: the requested one, else upstream, `origin/HEAD`, `main`, `master`.
    fn resolved_base(
        &self,
        root: &str,
        requested: Option<&str>,
    ) -> GitResult<(String, Vec<String>)> {
        let refs = self.all_refs(root)?;
        if let Some(requested) = requested {
            return Ok((requested.to_string(), refs));
        }
        let upstream = self.run_git(
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
            root,
            None,
        );
        if upstream.success() {
            return Ok((upstream.stdout.trim().to_string(), refs));
        }
        let remote_head = self.run_git(
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            root,
            None,
        );
        if remote_head.success() {
            return Ok((remote_head.stdout.trim().to_string(), refs));
        }
        if let Some(base) = ["main", "master"]
            .into_iter()
            .find(|name| refs.iter().any(|r| r == name))
        {
            return Ok((base.to_string(), refs));
        }
        Err(GitError::review(
            "Choose a comparison base; no recorded base, upstream or default branch is available.",
        ))
    }

    fn head_and_base(&self, root: &str, base: &str) -> GitResult<(String, String)> {
        let head = self.trimmed(&["rev-parse", "--verify", "HEAD^{commit}"], root)?;
        let base_sha = self.trimmed(
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
            root,
        )?;
        Ok((head, base_sha))
    }

    /// The branch's cumulative patch since its merge base with `base_branch`, and its commits.
    pub fn fetch_branch_review(
        &mut self,
        root: &str,
        base_branch: Option<&str>,
        summaries_only: bool,
    ) -> GitBranchReview {
        let mut review = GitBranchReview::default();
        if let Err(error) = self.branch_review_into(&mut review, root, base_branch, summaries_only)
        {
            let base = if review.comparison_base.is_empty() {
                base_branch.unwrap_or("a base").to_string()
            } else {
                review.comparison_base.clone()
            };
            review.error = Some(format!("Cannot compare with {base}: {error}"));
            // Still offer an explicit picker when resolution failed.
            if review.available_bases.is_empty() {
                let refs = self.run_git(
                    &[
                        "for-each-ref",
                        "--format=%(refname:short)",
                        "refs/heads",
                        "refs/remotes",
                    ],
                    root,
                    None,
                );
                review.available_bases = split_nonempty(&refs.stdout, '\n');
            }
        }
        review
    }

    fn branch_review_into(
        &mut self,
        review: &mut GitBranchReview,
        root: &str,
        base_branch: Option<&str>,
        summaries_only: bool,
    ) -> GitResult<()> {
        let (base, refs) = self.resolved_base(root, base_branch)?;
        review.comparison_base = base.clone();
        review.available_bases = refs;
        let (head, base_sha) = self.head_and_base(root, &base)?;
        let merge = self.trimmed(&["merge-base", &base_sha, &head], root)?;
        if merge.is_empty() {
            return Err(GitError::review(
                "The comparison base and HEAD have no common ancestor.",
            ));
        }
        let cache_key = format!("{root}\0{base}\0{base_sha}\0{head}");
        if summaries_only && let Some(cached) = self.branch_summary_cache.get(&cache_key) {
            review.files = cached.files.clone();
            review.commits = cached.commits.clone();
            return Ok(());
        }
        review.files = self.patches(
            &[merge.as_str(), head.as_str()],
            root,
            false,
            summaries_only,
        )?;
        let log = self.checked(
            &[
                "log",
                "--format=%H%x00%s%x00%an%x00%aI%x00",
                &format!("{base_sha}..{head}"),
                "--",
            ],
            root,
        )?;
        let fields: Vec<&str> = log.split('\0').collect();
        let mut i = 0;
        while i + 3 < fields.len() {
            review.commits.push(GitCommitInfo {
                sha: fields[i].trim_matches(['\n', '\r']).to_string(),
                subject: fields[i + 1].into(),
                author: fields[i + 2].into(),
                date: fields[i + 3].into(),
            });
            i += 4;
        }
        if summaries_only {
            if self.branch_summary_cache.len() >= 8 {
                self.branch_summary_cache.clear();
            }
            self.branch_summary_cache.insert(cache_key, review.clone());
        }
        Ok(())
    }

    /// Staged, unstaged and untracked changes; each group fails on its own.
    pub fn fetch_local_review(&mut self, root: &str, summaries_only: bool) -> GitLocalReview {
        let mut review = GitLocalReview::default();
        match self.patches(&["--cached"], root, false, summaries_only) {
            Ok(files) => review.staged = files,
            Err(error) => review.staged_error = Some(error.to_string()),
        }
        match self.patches(&[], root, false, summaries_only) {
            Ok(files) => review.unstaged = files,
            Err(error) => review.unstaged_error = Some(error.to_string()),
        }
        match self.untracked(root, summaries_only) {
            Ok(files) => review.untracked = files,
            Err(error) => review.untracked_error = Some(error.to_string()),
        }
        let errors: Vec<&str> = [
            &review.staged_error,
            &review.unstaged_error,
            &review.untracked_error,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
        review.error = (!errors.is_empty()).then(|| errors.join("\n"));
        review
    }

    fn untracked(&mut self, root: &str, summaries_only: bool) -> GitResult<Vec<FileDiff>> {
        let names = split_nonempty(
            &self.checked(&["ls-files", "--others", "--exclude-standard", "-z"], root)?,
            '\0',
        );
        let local = self.host.is_local();
        let remote_stats = if local {
            HashMap::new()
        } else {
            self.batched_stats(
                &names
                    .iter()
                    .map(|name| self.join(root, name))
                    .collect::<Vec<_>>(),
            )
        };
        let mut remaining = 32;
        let mut files = Vec::new();
        for name in names {
            let key = self.join(root, &name);
            if summaries_only {
                let mut file = FileDiff::new(name.clone(), "??", 0, 0, Vec::new());
                // An untracked nested repository is listed as `name/`; it has no patch.
                if !name.ends_with('/') {
                    file.patch_arguments = Some(no_index_arguments(&name));
                }
                file.preview_version = if local {
                    self.file_stamp(&name, root)
                } else {
                    Self::stamp_of(
                        remote_stats
                            .get(&key)
                            .copied()
                            .unwrap_or(HostFileStat::MISSING),
                        true,
                    )
                };
                files.push(file);
                continue;
            }
            let stamp = if local {
                Self::local_stamp(&key)
            } else {
                Self::stamp_of(
                    remote_stats
                        .get(&key)
                        .copied()
                        .unwrap_or(HostFileStat::MISSING),
                    false,
                )
            };
            if let Some(cached) = self
                .untracked_cache
                .get(&key)
                .filter(|cached| cached.stamp == stamp)
            {
                files.push(cached.file.clone());
            } else if remaining > 0 && !self.cancel.is_cancelled() {
                remaining -= 1;
                let known = remote_stats.get(&key).copied();
                files.push(self.untracked_preview(&name, root, known));
            } else {
                let mut file = FileDiff::new(name, "??", 0, 0, Vec::new());
                file.preview_deferred = true;
                files.push(file);
            }
        }
        Ok(files)
    }

    /// One untracked file's preview, cached by its stamp. `known_stat` saves a remote round trip.
    pub fn untracked_preview(
        &mut self,
        name: &str,
        root: &str,
        known_stat: Option<HostFileStat>,
    ) -> FileDiff {
        let key = self.join(root, name);
        let (is_directory, stamp) = if self.host.is_local() {
            let is_directory =
                std::fs::symlink_metadata(&key).is_ok_and(|metadata| metadata.is_dir());
            (is_directory, Self::local_stamp(&key))
        } else {
            let stat =
                known_stat.unwrap_or_else(|| self.host.stat(&key).unwrap_or(HostFileStat::MISSING));
            (stat.is_directory, Self::stamp_of(stat, false))
        };
        if let Some(cached) = self
            .untracked_cache
            .get(&key)
            .filter(|cached| cached.stamp == stamp)
        {
            return cached.file.clone();
        }
        let file = if is_directory {
            Err(GitError::Unsupported)
        } else {
            self.empty_baseline_patch(name, "??", root, Some(PREVIEW_LIMIT))
        };
        let file = file.unwrap_or_else(|error| {
            let mut file = FileDiff::new(name, "??", 0, 0, Vec::new());
            file.preview_error = Some(error.to_string());
            file
        });
        // Cache explicit and lazy previews too, with a fixed count and text budget.
        let bytes: usize = file
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .map(|line| line.content.len())
            .sum();
        if let Some(previous) = self.untracked_cache.remove(&key) {
            self.untracked_cache_bytes -= previous.bytes;
        }
        self.untracked_cache_order.retain(|entry| entry != &key);
        while self.untracked_cache.len() >= 128 || self.untracked_cache_bytes + bytes > 8_000_000 {
            let Some(oldest) = self.untracked_cache_order.pop_front() else {
                break;
            };
            if let Some(removed) = self.untracked_cache.remove(&oldest) {
                self.untracked_cache_bytes -= removed.bytes;
            }
        }
        self.untracked_cache.insert(
            key.clone(),
            UntrackedEntry {
                stamp,
                file: file.clone(),
                bytes,
            },
        );
        self.untracked_cache_order.push_back(key);
        self.untracked_cache_bytes += bytes;
        file
    }

    /// Commit count against the base and the number of locally changed paths.
    pub fn review_summary(&mut self, root: &str, base_branch: Option<&str>) -> GitReviewSummary {
        let mut errors = Vec::new();
        let commits = (|| -> GitResult<usize> {
            let (base, _) = self.resolved_base(root, base_branch)?;
            let (head, base_sha) = self.head_and_base(root, &base)?;
            self.checked(&["merge-base", &base_sha, &head], root)?;
            Ok(self
                .trimmed(
                    &["rev-list", "--count", &format!("{base_sha}..{head}")],
                    root,
                )?
                .parse()
                .unwrap_or(0))
        })();
        let commit_count = commits.unwrap_or_else(|error| {
            errors.push(error.to_string());
            0
        });
        let local = (|| -> GitResult<usize> {
            let output = self.checked(
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
                root,
            )?;
            let fields: Vec<&str> = output.split('\0').collect();
            let mut paths = HashSet::new();
            let mut i = 0;
            while i < fields.len() {
                let field = fields[i];
                i += 1;
                if field.len() < 3 {
                    continue;
                }
                paths.insert(&field[3..]);
                let code = &field[..2];
                if code.contains('R') || code.contains('C') {
                    i += 1;
                }
            }
            Ok(paths.len())
        })();
        let local_file_count = local.unwrap_or_else(|error| {
            errors.push(error.to_string());
            0
        });
        GitReviewSummary {
            commit_count,
            local_file_count,
            error: (!errors.is_empty()).then(|| errors.join("\n")),
        }
    }

    /// One commit's patch: first-parent for merges, against the empty tree for a root commit.
    pub fn fetch_commit_diff(
        &mut self,
        root: &str,
        sha: &str,
        summaries_only: bool,
    ) -> GitResult<Vec<FileDiff>> {
        let commit = self.trimmed(
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{sha}^{{commit}}"),
            ],
            root,
        )?;
        let parents = self.checked(&["rev-list", "--parents", "-n", "1", &commit], root)?;
        let parents: Vec<&str> = parents.split_whitespace().collect();
        if parents.len() > 1 {
            return self.patches(&[parents[1], commit.as_str()], root, false, summaries_only);
        }
        self.patches(&[commit.as_str()], root, true, summaries_only)
    }

    /// The editor's unstaged diff: unstaged and untracked files.
    pub fn fetch_unstaged_diff(&mut self, root: &str) -> DiffData {
        let local = self.fetch_local_review(root, false);
        DiffData {
            files: local.unstaged.into_iter().chain(local.untracked).collect(),
        }
    }

    fn unmerged_stages(&self, name: &str, root: &str) -> GitResult<Vec<String>> {
        Ok(split_nonempty(
            &self.checked(&["ls-files", "--unmerged", "-z", "--", name], root)?,
            '\0',
        ))
    }

    fn patches(
        &mut self,
        revisions: &[&str],
        root: &str,
        is_root: bool,
        summaries_only: bool,
    ) -> GitResult<Vec<FileDiff>> {
        let command = if is_root {
            strings(&["diff-tree", "--root", "--no-commit-id", "-r"])
        } else {
            strings(&["diff"])
        };
        let mut common = strings(&[
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]);
        // Unmerged index stages cannot be expressed as a staged patch. In the working-file
        // group use stage 2 (ours) as the disclosed conflict baseline.
        if revisions.is_empty() && !is_root {
            common.push("--ours".into());
        }
        let revisions = strings(revisions);
        let args = |extra: &[&str]| -> Vec<String> {
            let mut all = command.clone();
            all.extend(common.iter().cloned());
            all.extend(strings(extra));
            all.extend(revisions.iter().cloned());
            all.push("--".into());
            all
        };
        let names_output = self.checked_owned(args(&["--name-status", "-z"]), root)?;
        let names: Vec<&str> = names_output.split('\0').collect();
        let mut entries = Vec::new();
        let mut i = 0;
        while i + 1 < names.len() && !names[i].is_empty() {
            let code = names[i];
            let old = names[i + 1];
            i += 2;
            let renamed = code.starts_with('R') || code.starts_with('C');
            let name = if renamed {
                let Some(name) = names.get(i) else { break };
                i += 1;
                *name
            } else {
                old
            };
            entries.push(Entry {
                name: name.into(),
                status: code.chars().next().map(String::from).unwrap_or_default(),
                old: renamed.then(|| old.to_string()),
            });
        }
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let conflicts: HashSet<String> = entries
            .iter()
            .filter(|e| e.status == "U")
            .map(|e| e.name.clone())
            .collect();
        let is_cached = revisions == ["--cached"];

        if summaries_only {
            let local = !is_root && (revisions.is_empty() || is_cached);
            let options: &[&str] = if local {
                &["--raw", "--no-abbrev", "--numstat", "-z"]
            } else {
                &["--numstat", "-z"]
            };
            let stats_output = self.checked_owned(args(options), root)?;
            let stats: Vec<&str> = stats_output.split('\0').collect();
            let mut counts: HashMap<String, (usize, usize, bool)> = HashMap::new();
            let mut identities: HashMap<String, String> = HashMap::new();
            let mut index = 0;
            while index < stats.len() {
                let record = stats[index];
                index += 1;
                if record.starts_with(':') {
                    let fields: Vec<&str> = record.split(' ').filter(|f| !f.is_empty()).collect();
                    if fields.len() != 5 || index >= stats.len() {
                        continue;
                    }
                    let mut name = stats[index];
                    index += 1;
                    if fields[4].starts_with('R') || fields[4].starts_with('C') {
                        if index >= stats.len() {
                            break;
                        }
                        name = stats[index];
                        index += 1;
                    }
                    // Mode and per-path blob identities ignore whole-index rewrites.
                    identities.insert(name.into(), fields.join("/"));
                    continue;
                }
                let parts: Vec<&str> = record.splitn(3, '\t').collect();
                if parts.len() != 3 {
                    continue;
                }
                let mut name = parts[2].to_string();
                if name.is_empty() {
                    if index + 1 >= stats.len() {
                        break;
                    }
                    name = stats[index + 1].into();
                    index += 2;
                }
                counts.insert(
                    name,
                    (
                        parts[0].parse().unwrap_or(0),
                        parts[1].parse().unwrap_or(0),
                        parts[0] == "-",
                    ),
                );
            }
            let staged_base = if is_cached {
                let head = self.run_git(&["rev-parse", "--verify", "HEAD"], root, None);
                if head.success() {
                    head.stdout.trim().to_string()
                } else {
                    "unborn".into()
                }
            } else {
                String::new()
            };
            let mut seen = HashSet::new();
            let mut files = Vec::new();
            for entry in &entries {
                // A conflict may also appear as M; keep one path with the U identity.
                if !seen.insert(entry.name.clone()) {
                    continue;
                }
                let (added, removed, binary) =
                    counts.get(&entry.name).copied().unwrap_or((0, 0, false));
                let status = if conflicts.contains(&entry.name) {
                    "U".to_string()
                } else {
                    entry.status.clone()
                };
                let mut file = FileDiff::new(
                    entry.name.clone(),
                    status.clone(),
                    added,
                    removed,
                    Vec::new(),
                );
                file.old_filename = entry.old.clone();
                file.is_binary = binary;
                let mut patch_args = args(&["--patch"]);
                if let Some(old) = &entry.old {
                    patch_args.push(old.clone());
                }
                patch_args.push(entry.name.clone());
                file.patch_arguments = Some(patch_args);
                file.preview_version = format!(
                    "{root}{}/origin:{}",
                    revisions.join("/"),
                    entry.old.clone().unwrap_or_default()
                );
                let identity = identities
                    .get(&entry.name)
                    .map_or("missing", String::as_str);
                if is_cached {
                    file.preview_version += &format!("/HEAD:{staged_base}/index:{identity}");
                } else if local {
                    file.preview_version += &format!(
                        "/index:{identity}/working:{}",
                        self.file_stamp(&entry.name, root)
                    );
                }
                if status == "U" && revisions.is_empty() && !is_root {
                    let stages = self.unmerged_stages(&entry.name, root)?;
                    file.preview_version += &format!("/stages:{}", stages.join("\0"));
                    let ours = has_ours(&stages);
                    file.conflict_baseline =
                        Some(if ours { OURS_BASELINE } else { EMPTY_BASELINE }.into());
                    if !ours && self.exists(&self.join(root, &entry.name)) {
                        file.patch_arguments = Some(no_index_arguments(&entry.name));
                    }
                }
                files.push(file);
            }
            files.sort_by(|a, b| a.filename.cmp(&b.filename));
            return Ok(files);
        }

        // One patch process per group, rather than one per file on every poll.
        let mut patch_args = strings(&["-c", "core.quotePath=true"]);
        patch_args.extend(args(&["--patch"]));
        let output = self.checked_owned(patch_args, root)?;
        let sections = patch::split_sections(&output);
        let ordinary: Vec<&Entry> = entries.iter().filter(|e| e.status != "U").collect();
        if ordinary.len() != sections.len() {
            return Err(GitError::review(
                "Git paths changed while patches loaded. Refresh to retry.",
            ));
        }
        let mut files: Vec<FileDiff> = ordinary
            .iter()
            .zip(&sections)
            .map(|(entry, section)| {
                let conflicted = conflicts.contains(&entry.name);
                let status = if conflicted {
                    "U"
                } else {
                    entry.status.as_str()
                };
                let mut file = patch::parse_patch(section, &entry.name, status, entry.old.clone());
                if conflicted && revisions.is_empty() {
                    file.conflict_baseline = Some(OURS_BASELINE.into());
                }
                file
            })
            .collect();
        let mut missing: Vec<&String> = conflicts
            .iter()
            .filter(|name| !files.iter().any(|f| &f.filename == *name))
            .collect();
        missing.sort();
        for name in missing {
            let mut file = FileDiff::new(name.clone(), "U", 0, 0, Vec::new());
            if revisions.is_empty() && !is_root {
                let stages = self.unmerged_stages(name, root)?;
                let ours = has_ours(&stages);
                file.conflict_baseline =
                    Some(if ours { OURS_BASELINE } else { EMPTY_BASELINE }.into());
                // Modify/delete conflicts can have no stage 2. --ours then emits no patch,
                // though the surviving working file has reviewable content.
                let full_path = self.join(root, name);
                let exists = if self.host.is_local() {
                    std::fs::symlink_metadata(&full_path).is_ok()
                } else {
                    self.exists(&full_path)
                };
                if !ours && exists {
                    file = self.empty_baseline_patch(name, "U", root, None)?;
                    file.conflict_baseline = Some(EMPTY_BASELINE.into());
                }
            }
            files.push(file);
        }
        files.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(files)
    }

    /// One page of a file's patch. The patch is written to a disk snapshot once; later pages
    /// (`cursor`, from `ReviewPage::continuation`) read the same snapshot and never rerun git.
    pub fn fetch_review_page(
        &mut self,
        file: &FileDiff,
        root: &str,
        cursor: Option<&ReviewPageCursor>,
    ) -> GitResult<ReviewPage> {
        self.check_cancelled()?;
        let slice = match (cursor, &file.patch_slice) {
            (Some(cursor), _) => cursor.slice.clone(),
            (None, Some(stored)) => stored.clone(),
            (None, None) => self.snapshot_patch(file, root)?,
        };
        if !self.snapshots.contains(Path::new(&slice.snapshot)) {
            return Err(GitError::review("Refresh to load this file's diff."));
        }
        let mut reader = PatchLineReader::new(
            Path::new(&slice.snapshot),
            slice.end,
            cursor.map_or(slice.start, |c| c.next_offset),
        )?;
        let (mut old, mut new) = cursor.map_or((0, 0), |c| (c.old_line, c.new_line));
        let mut header = cursor.map_or(String::new(), |c| c.header.clone());
        let mut binary = cursor.map_or(file.is_binary, |c| c.is_binary);
        let (mut hunks, mut lines) = (Vec::new(), Vec::new());
        let (mut count, mut characters, mut long_lines) = (0usize, 0usize, false);
        while let Some(record) = reader.next_record()? {
            self.check_cancelled()?;
            let line = record.text;
            if line.starts_with("@@ ") {
                if !lines.is_empty() {
                    hunks.push(Hunk {
                        header: header.clone(),
                        lines: std::mem::take(&mut lines),
                    });
                }
                header = line.clone();
                if let Some((o, n)) = patch::hunk_starts(&line) {
                    (old, new) = (o, n);
                }
            } else if patch::is_binary_marker(&line) {
                binary = true;
            } else if !header.is_empty() {
                let line_type = match line.chars().next() {
                    Some('+') => LineType::Addition,
                    Some('-') => LineType::Deletion,
                    Some(' ') => LineType::Context,
                    _ => continue,
                };
                let mut content = line[1..].to_string();
                if record.truncated {
                    content.push_str(" … [long line; open full patch]");
                }
                characters += content.len();
                lines.push(DiffLine {
                    line_type,
                    content,
                    old_line_no: (line_type != LineType::Addition).then_some(old),
                    new_line_no: (line_type != LineType::Deletion).then_some(new),
                });
                if line_type != LineType::Addition {
                    old += 1;
                }
                if line_type != LineType::Deletion {
                    new += 1;
                }
                long_lines = long_lines || record.truncated;
                count += 1;
                if count == PAGE_LINES || characters >= PAGE_BYTES {
                    break;
                }
            }
        }
        if !lines.is_empty() {
            hunks.push(Hunk {
                header: header.clone(),
                lines,
            });
        }
        let next_offset = reader.offset();
        Ok(ReviewPage {
            has_more: next_offset < slice.end,
            slice,
            hunks,
            next_offset,
            old_line: old,
            new_line: new,
            header,
            is_binary: binary,
            has_long_lines: long_lines,
        })
    }

    /// Runs a file's patch command into a new snapshot and finds the file's slice in it.
    fn snapshot_patch(&mut self, file: &FileDiff, root: &str) -> GitResult<PatchSlice> {
        let Some(arguments) = &file.patch_arguments else {
            return Err(GitError::review(if file.filename.ends_with('/') {
                "This is an untracked nested repository; it has no diff here."
            } else {
                "Refresh to load this file's diff."
            }));
        };
        let snapshot = self.snapshots.create("purepoint-review");
        let mut command = self.git_command(arguments.clone(), root, None);
        command.timeout = Some(Duration::from_secs(15));
        command.output_file = Some(snapshot.clone());
        let result = self.host.run(&command);
        self.check_cancelled()?;
        let no_index = arguments.iter().any(|arg| arg == "--no-index");
        if !(result.success() || (no_index && result.exit_code == 1)) {
            return Err(GitError::Review(if result.stderr.is_empty() {
                "Diff loading timed out or failed. Retry to load this file.".into()
            } else {
                result.stderr
            }));
        }
        let size = std::fs::metadata(&snapshot)?.len();
        let indexed = patch::index_patch(&snapshot, Some(file), &self.cancel)?;
        if let Some(slice) = indexed
            .files
            .iter()
            .find(|f| f.filename == file.filename && f.old_filename == file.old_filename)
            .and_then(|f| f.patch_slice.clone())
        {
            return Ok(slice);
        }
        if size == 0
            || (file.status_code == "U"
                && indexed.unmerged_paths.contains(&file.filename)
                && indexed.files.is_empty())
        {
            return Ok(PatchSlice {
                snapshot: snapshot.to_string_lossy().into_owned(),
                start: 0,
                end: 0,
            });
        }
        Err(GitError::review(
            "The file changed while its diff loaded. Refresh to retry.",
        ))
    }

    fn empty_baseline_patch(
        &self,
        name: &str,
        status: &str,
        root: &str,
        output_limit: Option<usize>,
    ) -> GitResult<FileDiff> {
        let args = strings(&[
            "-c",
            "core.quotePath=true",
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            "/dev/null",
            name,
        ]);
        let result = self.run_git_owned(args, root, output_limit);
        if result.output_exceeded_limit {
            return Err(GitError::TooLarge);
        }
        if result.exit_code != 0 && result.exit_code != 1 {
            return Err(GitError::Review(result.stderr));
        }
        Ok(patch::parse_patch(&result.stdout, name, status, None))
    }

    // MARK: - Workspace review (HEAD to working tree, including staged and new files)

    /// Every path that differs from HEAD, with counts. Renames are shown as a deletion and
    /// an addition so each row refers to one real path.
    pub fn fetch_working_tree_changes(&self, root: &str) -> GitResult<Vec<FileDiff>> {
        let status = self.run_git(
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--no-renames",
            ],
            root,
            None,
        );
        if !status.success() {
            return Err(GitError::WorkingTree(status.stderr));
        }
        let has_head = self
            .run_git(&["rev-parse", "--verify", "HEAD"], root, None)
            .success();
        let stats = self.run_git(
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--numstat",
                "-z",
                "--no-renames",
                if has_head { "HEAD" } else { "--cached" },
                "--",
            ],
            root,
            None,
        );
        if !stats.success() {
            return Err(GitError::WorkingTree(stats.stderr));
        }
        let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
        for record in stats.stdout.split('\0').filter(|r| !r.is_empty()) {
            let parts: Vec<&str> = record.splitn(3, '\t').collect();
            if parts.len() == 3 {
                counts.insert(
                    parts[2],
                    (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0)),
                );
            }
        }
        let mut codes: HashMap<String, String> = HashMap::new();
        for record in status.stdout.split('\0').filter(|r| !r.is_empty()) {
            if record.len() < 4 {
                continue;
            }
            let name = &record[3..];
            // Before the first commit there is no HEAD content to delete. A staged path
            // removed or renamed since contributes no working-tree file.
            if !has_head && !self.exists(&self.join(root, name)) {
                continue;
            }
            let bytes = record.as_bytes();
            let (x, y) = (bytes[0] as char, bytes[1] as char);
            let code = if x == '?' {
                "??".to_string()
            } else {
                (if y == ' ' { x } else { y }).to_string()
            };
            // rm --cached can report a tracked deletion and an untracked file at one path.
            // Keep the tracked change; the untracked copy stays reachable in Files mode.
            if !codes.contains_key(name) || code != "??" {
                codes.insert(name.to_string(), code);
            }
        }
        let mut files: Vec<FileDiff> = codes
            .into_iter()
            .map(|(name, code)| {
                let (added, removed) = counts.get(name.as_str()).copied().unwrap_or((0, 0));
                FileDiff::new(name, code, added, removed, Vec::new())
            })
            .collect();
        files.sort_by(|a, b| standard_compare(&a.filename, &b.filename));
        Ok(files)
    }

    /// One working-tree file against HEAD; a new file (or any file before the first commit)
    /// as all additions. Output beyond 1 MB declines the inline preview.
    pub fn fetch_working_tree_file_diff(&self, root: &str, file: &FileDiff) -> GitResult<FileDiff> {
        let has_head = self
            .run_git(&["rev-parse", "--verify", "HEAD"], root, None)
            .success();
        if file.status_code == "??" || !has_head {
            let (content, binary) =
                self.read_preview(&self.join(root, &file.filename), PREVIEW_LIMIT)?;
            if binary {
                return Ok(file.clone());
            }
            let mut lines: Vec<&str> = content.split('\n').collect();
            if lines.last() == Some(&"") {
                lines.pop();
            }
            let diff_lines: Vec<DiffLine> = lines
                .iter()
                .enumerate()
                .map(|(offset, line)| DiffLine {
                    line_type: LineType::Addition,
                    content: line.to_string(),
                    old_line_no: None,
                    new_line_no: Some(offset as i64 + 1),
                })
                .collect();
            let count = lines.len();
            let hunks = if diff_lines.is_empty() {
                Vec::new()
            } else {
                vec![Hunk {
                    header: format!("@@ -0,0 +1,{count} @@"),
                    lines: diff_lines,
                }]
            };
            return Ok(FileDiff::new(
                file.filename.clone(),
                file.status_code.clone(),
                count,
                0,
                hunks,
            ));
        }
        let result = self.run_git(
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--no-renames",
                "HEAD",
                "--",
                &file.filename,
            ],
            root,
            Some(PREVIEW_LIMIT),
        );
        if result.output_exceeded_limit {
            return Err(GitError::TooLarge);
        }
        if !result.success() {
            return Err(GitError::WorkingTree(result.stderr));
        }
        // The path comes from status, not a quoted header. Parse only this file's hunks so
        // code containing "diff --git" cannot create fake rows.
        let hunks = patch::parse_hunks(result.stdout.split('\n'));
        let lines = hunks.iter().flat_map(|hunk| &hunk.lines);
        let added = lines
            .clone()
            .filter(|line| line.line_type == LineType::Addition)
            .count();
        let removed = lines
            .filter(|line| line.line_type == LineType::Deletion)
            .count();
        Ok(FileDiff::new(
            file.filename.clone(),
            file.status_code.clone(),
            added,
            removed,
            hunks,
        ))
    }

    /// A bounded text read through the host (`FileIOService.readFile(at:limit:host:)`):
    /// `TooLarge` past `limit`, `Unsupported` for anything but a regular local file.
    pub fn read_preview(&self, path: &str, limit: usize) -> GitResult<(String, bool)> {
        if self.host.is_local() {
            return read_local_preview(Path::new(path), limit);
        }
        let read = self.host.read(path, limit)?;
        if read.truncated {
            return Err(GitError::TooLarge);
        }
        Ok(decode_preview(&read.data))
    }
}

/// `FileIOService.readFile(at:limit:)` for local files.
pub fn read_local_preview(path: &Path, limit: usize) -> GitResult<(String, bool)> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(GitError::Unsupported);
    }
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(GitError::TooLarge);
    }
    Ok(decode_preview(&data))
}
