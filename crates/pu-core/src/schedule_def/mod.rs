use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Recurrence {
    #[default]
    None,
    Hourly,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleTrigger {
    AgentDef {
        name: String,
    },
    SwarmDef {
        name: String,
        #[serde(default)]
        vars: HashMap<String, String>,
    },
    InlinePrompt {
        prompt: String,
        #[serde(default = "default_agent")]
        agent: String,
    },
}

fn default_agent() -> String {
    "claude".to_string()
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleDef {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub recurrence: Recurrence,
    pub start_at: DateTime<Utc>,
    #[serde(default)]
    pub next_run: Option<DateTime<Utc>>,
    pub trigger: ScheduleTrigger,
    pub project_root: String,
    #[serde(default)]
    pub target: String,
    /// Whether the scheduled agent spawns in the project root (true) or a worktree (false)
    #[serde(default = "crate::serde_defaults::default_true")]
    pub root: bool,
    /// Worktree/branch name when `root` is false
    #[serde(default)]
    pub agent_name: Option<String>,
    /// "local" or "global" — set at load time, not serialized
    #[serde(skip)]
    pub scope: String,
    pub created_at: DateTime<Utc>,
}

impl ScheduleDef {
    /// Validate that `root` and `agent_name` are consistent:
    /// - root=true → agent_name must be None
    /// - root=false → agent_name must be Some(non-empty)
    pub fn validate(&self) -> Result<(), std::io::Error> {
        if self.root {
            if self.agent_name.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "agent_name must not be set when root is true",
                ));
            }
        } else if self.agent_name.as_ref().is_none_or(String::is_empty) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "agent_name is required when root is false",
            ));
        }
        Ok(())
    }
}

/// Scan both local and global schedule definition directories. Local defs take priority.
pub fn list_schedule_defs(project_root: &Path) -> Vec<ScheduleDef> {
    let mut seen = HashMap::new();
    let mut result = Vec::new();

    let local_dir = paths::schedules_dir(project_root);
    if local_dir.is_dir() {
        for def in scan_dir(&local_dir, "local") {
            seen.insert(def.name.clone(), result.len());
            result.push(def);
        }
    }

    if let Ok(global_dir) = paths::global_schedules_dir() {
        if global_dir.is_dir() {
            for def in scan_dir(&global_dir, "global") {
                if !seen.contains_key(&def.name) {
                    result.push(def);
                }
            }
        }
    }

    result
}

/// Find a schedule definition by name. Checks local first, then global.
pub fn find_schedule_def(project_root: &Path, name: &str) -> Option<ScheduleDef> {
    let local_dir = paths::schedules_dir(project_root);
    if local_dir.is_dir() {
        if let Some(def) = find_in_dir(&local_dir, name, "local") {
            return Some(def);
        }
    }
    if let Ok(global_dir) = paths::global_schedules_dir() {
        if global_dir.is_dir() {
            if let Some(def) = find_in_dir(&global_dir, name, "global") {
                return Some(def);
            }
        }
    }
    None
}

/// Save a schedule definition as a YAML file. Creates the directory if needed.
pub fn save_schedule_def(dir: &Path, def: &ScheduleDef) -> Result<(), std::io::Error> {
    crate::validation::validate_name(&def.name)?;
    def.validate()?;
    if def.project_root.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "project_root must not be empty",
        ));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.yaml", def.name));
    let yaml = serde_yml::to_string(def).map_err(std::io::Error::other)?;
    std::fs::write(path, yaml)
}

/// Delete a schedule definition file. Returns true if the file existed.
pub fn delete_schedule_def(dir: &Path, name: &str) -> Result<bool, std::io::Error> {
    crate::validation::validate_name(name)?;
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        std::fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Compute the next occurrence of a recurring schedule after `after`.
/// Returns None if the schedule is one-shot and `after` >= `base`.
pub fn next_occurrence(
    base: DateTime<Utc>,
    recurrence: &Recurrence,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    // Clamp: never return an occurrence before start_at (base)
    let after = if after < base {
        base - Duration::seconds(1)
    } else {
        after
    };
    match recurrence {
        Recurrence::None => next_none_occurrence(base, after),
        Recurrence::Hourly => next_hourly_occurrence(base, after),
        Recurrence::Daily => next_daily_occurrence(base, after),
        Recurrence::Weekdays => next_weekdays_occurrence(base, after),
        Recurrence::Weekly => next_weekly_occurrence(base, after),
        Recurrence::Monthly => next_monthly_occurrence(base, after),
    }
}

/// Build a NaiveDateTime on `after`'s date at `base`'s hour/minute/second.
/// Shared by Daily, Weekdays, and Weekly recurrence calculations.
fn naive_at_base_time(base: DateTime<Utc>, after: DateTime<Utc>) -> chrono::NaiveDateTime {
    after
        .date_naive()
        .and_hms_opt(base.hour(), base.minute(), base.second())
        .unwrap()
}

fn next_none_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if after <= base { Some(base) } else { None }
}

fn next_hourly_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    // Next occurrence at base's minute, after `after`
    let mut candidate = after
        .with_minute(base.minute())
        .unwrap()
        .with_second(base.second())
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    if candidate <= after {
        candidate += Duration::hours(1);
    }
    Some(candidate)
}

fn next_daily_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut candidate = naive_at_base_time(base, after);
    if Utc.from_utc_datetime(&candidate) <= after {
        candidate += Duration::days(1);
    }
    Some(Utc.from_utc_datetime(&candidate))
}

fn next_weekdays_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut candidate = naive_at_base_time(base, after);
    if Utc.from_utc_datetime(&candidate) <= after {
        candidate += Duration::days(1);
    }
    // Skip weekends
    loop {
        let wd = candidate.weekday();
        if wd != Weekday::Sat && wd != Weekday::Sun {
            break;
        }
        candidate += Duration::days(1);
    }
    Some(Utc.from_utc_datetime(&candidate))
}

fn next_weekly_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut candidate = naive_at_base_time(base, after);
    // Align to same weekday as base
    let target_weekday = base.weekday();
    let current_weekday = candidate.weekday();
    let days_ahead = (target_weekday.num_days_from_monday() as i64
        - current_weekday.num_days_from_monday() as i64
        + 7)
        % 7;
    candidate += Duration::days(days_ahead);
    if Utc.from_utc_datetime(&candidate) <= after {
        candidate += Duration::weeks(1);
    }
    Some(Utc.from_utc_datetime(&candidate))
}

fn next_monthly_occurrence(base: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let target_day = base.day();
    let target_time = base.time();
    let mut year = after.year();
    let mut month = after.month();

    // Start from after's month
    loop {
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, target_day) {
            let candidate = Utc.from_utc_datetime(&date.and_time(target_time));
            if candidate > after {
                return Some(candidate);
            }
        }
        // Advance month
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
        // Safety: don't loop forever (covers 4 years = 48 months max)
        if year > after.year() + 4 {
            break;
        }
    }
    None
}

fn scan_dir(dir: &Path, scope: &str) -> Vec<ScheduleDef> {
    let mut defs = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return defs;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                match serde_yml::from_str::<ScheduleDef>(&content) {
                    Ok(mut def) => {
                        if let Err(e) = def.validate() {
                            eprintln!("warning: invalid schedule {}: {e}", path.display());
                            continue;
                        }
                        def.scope = scope.to_string();
                        defs.push(def);
                    }
                    Err(e) => {
                        eprintln!("warning: failed to parse {}: {e}", path.display());
                    }
                }
            }
        }
    }
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs
}

fn find_in_dir(dir: &Path, name: &str, scope: &str) -> Option<ScheduleDef> {
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut def) = serde_yml::from_str::<ScheduleDef>(&content) {
                if def.validate().is_err() {
                    return None;
                }
                def.scope = scope.to_string();
                return Some(def);
            }
        }
    }
    scan_dir(dir, scope)
        .into_iter()
        .find(|def| def.name == name)
}

#[cfg(test)]
mod tests;
