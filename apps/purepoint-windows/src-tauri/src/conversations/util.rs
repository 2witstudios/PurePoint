//! Helpers shared by the three indexes: `parseJSONLine` (`Services/ShellUtilities.swift`),
//! `ClaudeConversationIndex`'s shared helpers (`locatePurePointProjectRoot`, `parseTimestamp`,
//! `compact`, bounded prefix/suffix reads) and `String.trimmedNonEmpty`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// Foundation's `Date.distantPast` (0001-01-01T00:00:00Z) in Unix milliseconds: the Mac's
/// "unknown" modification date. It sorts last and stays inside JavaScript's safe integers.
pub const DISTANT_PAST_MS: i64 = -62_135_596_800_000;

/// A JSON object (`[String: Any]` on the Mac).
pub type Object = Map<String, Value>;

/// Parses one JSONL line into an object; anything else (bad JSON, arrays, scalars) is `None`.
pub fn parse_json_line(line: &str) -> Option<Object> {
    match serde_json::from_str::<Value>(line).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// `String.trimmedNonEmpty`: trimmed, or `None` when only whitespace is left.
pub fn trimmed_non_empty(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// `record[key] as? String`.
pub fn str_field<'a>(object: &'a Object, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

/// `record[key] as? [String: Any]`.
pub fn obj_field<'a>(object: &'a Object, key: &str) -> Option<&'a Object> {
    object.get(key).and_then(Value::as_object)
}

/// `URL(fileURLWithPath:).lastPathComponent` for either separator, so Windows paths
/// (`C:\Users\me\repo`) and Unix paths both name their last directory.
pub fn last_path_component(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return path.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

/// Collapses whitespace runs to single spaces and truncates to `max_length` characters
/// (ending in `...`), as the Mac's `compact`.
pub fn compact(text: Option<&str>, max_length: usize) -> Option<String> {
    let collapsed = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.chars().count() <= max_length {
        return Some(collapsed);
    }
    let head: String = collapsed
        .chars()
        .take(max_length.saturating_sub(1))
        .collect();
    Some(format!("{}...", head.trim_matches(' ')))
}

/// The nearest ancestor of `path` (itself included) holding `.pu/manifest.json`, memoised in
/// `cache` because many sessions share a project.
pub fn locate_pure_point_project_root(
    path: &str,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if let Some(cached) = cache.get(path) {
        return cached.clone();
    }
    let found = Path::new(path)
        .ancestors()
        .find(|dir| !dir.as_os_str().is_empty() && dir.join(".pu").join("manifest.json").is_file())
        .map(|dir| dir.to_string_lossy().into_owned());
    cache.insert(path.to_string(), found.clone());
    found
}

/// Reads up to `byte_count` bytes from the start of a file, decoding UTF-8 lossily.
pub fn read_prefix(path: &Path, byte_count: u64) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?.take(byte_count).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads the last `byte_count` bytes of a file, decoding UTF-8 lossily.
pub fn read_suffix(path: &Path, byte_count: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(size.saturating_sub(byte_count)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A file's modification time in Unix milliseconds.
pub fn file_mtime_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(system_time_ms(modified))
}

/// A `SystemTime` in Unix milliseconds (negative before 1970).
pub fn system_time_ms(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// The current time in Unix milliseconds (`Date()`).
pub fn now_ms() -> i64 {
    system_time_ms(SystemTime::now())
}

/// Whether a directory entry name is hidden (`.skipsHiddenFiles`).
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Parses an ISO 8601 internet date-time (`2026-03-01T09:00:00.000Z`,
/// `2026-03-01T09:00:00+02:00`), with or without fractional seconds, into Unix milliseconds.
/// Matches the Mac's pair of `ISO8601DateFormatter`s (`.withInternetDateTime`, with and
/// without `.withFractionalSeconds`).
pub fn parse_timestamp(value: Option<&str>) -> Option<i64> {
    let value = trimmed_non_empty(value?)?;
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = value.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let year = digits(0..4)?;
    let month = digits(5..7)?;
    let day = digits(8..10)?;
    let hour = digits(11..13)?;
    let minute = digits(14..16)?;
    let second = digits(17..19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }

    let mut rest = &value[19..];
    let mut millis = 0i64;
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return None;
        }
        let padded: String = fraction[..end]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        millis = padded.parse().ok()?;
        rest = &fraction[end..];
    }

    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = rest[1..].replace(':', "");
            if zone.len() != 4 || !zone.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let hours: i64 = zone[..2].parse().ok()?;
            let minutes: i64 = zone[2..].parse().ok()?;
            sign * (hours * 60 + minutes)
        }
    };

    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1_000 + millis)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractional_and_whole_second_timestamps() {
        assert_eq!(
            parse_timestamp(Some("1970-01-01T00:00:00Z")),
            Some(0),
            "epoch"
        );
        assert_eq!(
            parse_timestamp(Some("2026-03-01T09:00:00.000Z")),
            Some(1_772_355_600_000)
        );
        assert_eq!(
            parse_timestamp(Some("2026-03-01T09:00:00.5Z")),
            Some(1_772_355_600_500)
        );
        assert_eq!(
            parse_timestamp(Some("2026-03-01T11:00:00+02:00")),
            Some(1_772_355_600_000)
        );
        assert_eq!(parse_timestamp(Some("2026-03-01")), None);
        assert_eq!(parse_timestamp(Some("not a date at all!!")), None);
        assert_eq!(parse_timestamp(Some("  ")), None);
        assert_eq!(parse_timestamp(None), None);
    }

    #[test]
    fn distant_past_is_year_one() {
        assert_eq!(days_from_civil(1, 1, 1) * 86_400_000, DISTANT_PAST_MS);
    }

    #[test]
    fn last_path_component_handles_both_separators() {
        assert_eq!(last_path_component(r"C:\Users\me\repo"), "repo");
        assert_eq!(last_path_component(r"C:\Users\me\repo\"), "repo");
        assert_eq!(last_path_component("/tmp/normal"), "normal");
        assert_eq!(last_path_component("/"), "/");
    }

    #[test]
    fn compact_collapses_and_truncates() {
        assert_eq!(compact(Some("  a \n\t b  "), 10).as_deref(), Some("a b"));
        assert_eq!(compact(Some("   "), 10), None);
        assert_eq!(compact(Some("abcdefghij"), 5).as_deref(), Some("abcd..."));
    }
}
