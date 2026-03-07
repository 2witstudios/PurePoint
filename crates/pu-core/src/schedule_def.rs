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
        } else if self.agent_name.as_ref().is_none_or(|n| n.is_empty()) {
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
        Recurrence::None => {
            if after <= base {
                Some(base)
            } else {
                None
            }
        }
        Recurrence::Hourly => {
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
        Recurrence::Daily => {
            let mut candidate = after
                .date_naive()
                .and_hms_opt(base.hour(), base.minute(), base.second())
                .unwrap();
            if Utc.from_utc_datetime(&candidate) <= after {
                candidate += Duration::days(1);
            }
            Some(Utc.from_utc_datetime(&candidate))
        }
        Recurrence::Weekdays => {
            let mut candidate = after
                .date_naive()
                .and_hms_opt(base.hour(), base.minute(), base.second())
                .unwrap();
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
        Recurrence::Weekly => {
            let mut candidate = after
                .date_naive()
                .and_hms_opt(base.hour(), base.minute(), base.second())
                .unwrap();
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
        Recurrence::Monthly => {
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
    }
}

fn scan_dir(dir: &Path, scope: &str) -> Vec<ScheduleDef> {
    let mut defs = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return defs,
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_trigger() -> ScheduleTrigger {
        ScheduleTrigger::AgentDef {
            name: "security-review".to_string(),
        }
    }

    fn make_schedule_def(name: &str) -> ScheduleDef {
        ScheduleDef {
            name: name.to_string(),
            enabled: true,
            recurrence: Recurrence::Daily,
            start_at: Utc::now(),
            next_run: None,
            trigger: make_trigger(),
            project_root: "/projects/myapp".to_string(),
            target: String::new(),
            root: true,
            agent_name: None,
            scope: String::new(),
            created_at: Utc::now(),
        }
    }

    // --- Deserialization (REQ-SCHED-001) ---

    #[test]
    fn given_schedule_def_yaml_should_deserialize() {
        let yaml = r#"
name: nightly-review
enabled: true
recurrence: daily
start_at: "2025-01-01T03:00:00Z"
trigger:
  type: agent_def
  name: security-review
project_root: /projects/myapp
created_at: "2025-01-01T00:00:00Z"
"#;
        let def: ScheduleDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.name, "nightly-review");
        assert!(def.enabled);
        assert_eq!(def.recurrence, Recurrence::Daily);
        assert!(
            matches!(def.trigger, ScheduleTrigger::AgentDef { ref name } if name == "security-review")
        );
        assert_eq!(def.project_root, "/projects/myapp");
    }

    #[test]
    fn given_minimal_schedule_yaml_should_use_defaults() {
        let yaml = r#"
name: quick
start_at: "2025-06-01T12:00:00Z"
trigger:
  type: agent_def
  name: test
project_root: /tmp
created_at: "2025-06-01T00:00:00Z"
"#;
        let def: ScheduleDef = serde_yml::from_str(yaml).unwrap();
        assert!(def.enabled); // default true
        assert_eq!(def.recurrence, Recurrence::None); // default none
        assert_eq!(def.target, ""); // default empty
        assert!(def.next_run.is_none()); // default none
        assert!(def.root); // default true (backward compat)
        assert!(def.agent_name.is_none()); // default none
    }

    #[test]
    fn given_schedule_with_worktree_fields_should_round_trip() {
        let yaml = r#"
name: overnight-build
start_at: "2025-06-01T22:30:00Z"
trigger:
  type: inline_prompt
  prompt: "build a feature"
project_root: /projects/myapp
root: false
agent_name: overnight-build
created_at: "2025-06-01T00:00:00Z"
"#;
        let def: ScheduleDef = serde_yml::from_str(yaml).unwrap();
        assert!(!def.root);
        assert_eq!(def.agent_name.as_deref(), Some("overnight-build"));

        // Round-trip through YAML
        let serialized = serde_yml::to_string(&def).unwrap();
        let reparsed: ScheduleDef = serde_yml::from_str(&serialized).unwrap();
        assert!(!reparsed.root);
        assert_eq!(reparsed.agent_name.as_deref(), Some("overnight-build"));
    }

    // --- Validation ---

    #[test]
    fn given_root_true_with_no_agent_name_should_validate() {
        let def = make_schedule_def("test");
        assert!(def.validate().is_ok());
    }

    #[test]
    fn given_root_true_with_agent_name_should_reject() {
        let mut def = make_schedule_def("test");
        def.agent_name = Some("bad".to_string());
        assert!(def.validate().is_err());
    }

    #[test]
    fn given_root_true_with_empty_agent_name_should_reject() {
        let mut def = make_schedule_def("test");
        def.agent_name = Some(String::new());
        assert!(def.validate().is_err());
    }

    #[test]
    fn given_root_false_with_agent_name_should_validate() {
        let mut def = make_schedule_def("test");
        def.root = false;
        def.agent_name = Some("my-worktree".to_string());
        assert!(def.validate().is_ok());
    }

    #[test]
    fn given_root_false_with_no_agent_name_should_reject() {
        let mut def = make_schedule_def("test");
        def.root = false;
        assert!(def.validate().is_err());
    }

    #[test]
    fn given_root_false_with_empty_agent_name_should_reject() {
        let mut def = make_schedule_def("test");
        def.root = false;
        def.agent_name = Some(String::new());
        assert!(def.validate().is_err());
    }

    #[test]
    fn given_trigger_agent_def_should_round_trip() {
        let trigger = ScheduleTrigger::AgentDef {
            name: "reviewer".to_string(),
        };
        let yaml = serde_yml::to_string(&trigger).unwrap();
        let parsed: ScheduleTrigger = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed, trigger);
    }

    #[test]
    fn given_trigger_swarm_def_with_vars_should_round_trip() {
        let mut vars = HashMap::new();
        vars.insert("branch".to_string(), "main".to_string());
        let trigger = ScheduleTrigger::SwarmDef {
            name: "full-stack".to_string(),
            vars,
        };
        let yaml = serde_yml::to_string(&trigger).unwrap();
        let parsed: ScheduleTrigger = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed, trigger);
    }

    #[test]
    fn given_trigger_inline_prompt_should_round_trip() {
        let trigger = ScheduleTrigger::InlinePrompt {
            prompt: "Review all deps".to_string(),
            agent: "claude".to_string(),
        };
        let yaml = serde_yml::to_string(&trigger).unwrap();
        let parsed: ScheduleTrigger = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed, trigger);
    }

    // --- CRUD (REQ-SCHED-002 through REQ-SCHED-006) ---

    #[test]
    fn given_local_and_global_schedule_defs_should_list_local_first() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::schedules_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let mut def = make_schedule_def("nightly");
        save_schedule_def(&local_dir, &def).unwrap();
        def.name = "weekly".to_string();
        save_schedule_def(&local_dir, &def).unwrap();

        let defs = list_schedule_defs(root);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "nightly");
        assert_eq!(defs[1].name, "weekly");
        assert_eq!(defs[0].scope, "local");
    }

    #[test]
    fn given_schedule_def_name_should_find_by_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::schedules_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let def = make_schedule_def("nightly");
        save_schedule_def(&local_dir, &def).unwrap();

        let found = find_schedule_def(root, "nightly");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "nightly");
    }

    #[test]
    fn given_no_schedule_defs_should_return_empty_list() {
        let tmp = TempDir::new().unwrap();
        let defs = list_schedule_defs(tmp.path());
        assert!(defs.is_empty());
    }

    #[test]
    fn given_schedule_def_should_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("schedules");
        let def = make_schedule_def("test-schedule");
        save_schedule_def(&dir, &def).unwrap();

        let path = dir.join("test-schedule.yaml");
        assert!(path.is_file());

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: ScheduleDef = serde_yml::from_str(&content).unwrap();
        assert_eq!(loaded.name, "test-schedule");
        assert_eq!(loaded.recurrence, Recurrence::Daily);
    }

    #[test]
    fn given_invalid_name_should_reject() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("schedules");
        let mut def = make_schedule_def("../evil");
        def.name = "../evil".to_string();
        assert!(save_schedule_def(&dir, &def).is_err());
    }

    #[test]
    fn given_existing_schedule_def_should_delete_and_return_true() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("schedules");
        std::fs::create_dir_all(&dir).unwrap();
        let def = make_schedule_def("nightly");
        save_schedule_def(&dir, &def).unwrap();

        let deleted = delete_schedule_def(&dir, "nightly").unwrap();
        assert!(deleted);
        assert!(!dir.join("nightly.yaml").exists());
    }

    #[test]
    fn given_nonexistent_schedule_def_should_return_false() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("schedules");
        std::fs::create_dir_all(&dir).unwrap();

        let deleted = delete_schedule_def(&dir, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn given_duplicate_name_in_local_and_global_should_prefer_local() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::schedules_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let def = make_schedule_def("nightly");
        save_schedule_def(&local_dir, &def).unwrap();

        let found = find_schedule_def(root, "nightly").unwrap();
        assert_eq!(found.scope, "local");
    }

    #[test]
    fn given_empty_project_root_should_reject() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("schedules");
        let mut def = make_schedule_def("test");
        def.project_root = String::new();
        assert!(save_schedule_def(&dir, &def).is_err());
    }

    // --- Recurrence calculator (REQ-SCHED-010 through REQ-SCHED-018) ---

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
    }

    #[test]
    fn given_none_recurrence_before_base_should_return_base() {
        let base = utc(2025, 6, 15, 10, 0, 0);
        let after = utc(2025, 6, 14, 10, 0, 0);
        assert_eq!(next_occurrence(base, &Recurrence::None, after), Some(base));
    }

    #[test]
    fn given_none_recurrence_after_base_should_return_none() {
        let base = utc(2025, 6, 15, 10, 0, 0);
        let after = utc(2025, 6, 16, 10, 0, 0);
        assert_eq!(next_occurrence(base, &Recurrence::None, after), None);
    }

    #[test]
    fn given_hourly_recurrence_should_return_next_hour() {
        let base = utc(2025, 6, 15, 10, 30, 0);
        let after = utc(2025, 6, 15, 11, 0, 0);
        let next = next_occurrence(base, &Recurrence::Hourly, after).unwrap();
        assert_eq!(next, utc(2025, 6, 15, 11, 30, 0));
    }

    #[test]
    fn given_hourly_after_same_minute_should_advance_one_hour() {
        let base = utc(2025, 6, 15, 10, 30, 0);
        let after = utc(2025, 6, 15, 10, 30, 0);
        let next = next_occurrence(base, &Recurrence::Hourly, after).unwrap();
        assert_eq!(next, utc(2025, 6, 15, 11, 30, 0));
    }

    #[test]
    fn given_daily_recurrence_should_return_next_day() {
        let base = utc(2025, 6, 15, 3, 0, 0);
        let after = utc(2025, 6, 15, 4, 0, 0);
        let next = next_occurrence(base, &Recurrence::Daily, after).unwrap();
        assert_eq!(next, utc(2025, 6, 16, 3, 0, 0));
    }

    #[test]
    fn given_daily_before_time_today_should_return_today() {
        let base = utc(2025, 6, 15, 15, 0, 0);
        let after = utc(2025, 6, 15, 10, 0, 0);
        let next = next_occurrence(base, &Recurrence::Daily, after).unwrap();
        assert_eq!(next, utc(2025, 6, 15, 15, 0, 0));
    }

    #[test]
    fn given_weekdays_on_friday_should_skip_to_monday() {
        // 2025-06-13 is a Friday
        let base = utc(2025, 6, 13, 9, 0, 0);
        let after = utc(2025, 6, 13, 10, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekdays, after).unwrap();
        // Should skip to Monday 2025-06-16
        assert_eq!(next, utc(2025, 6, 16, 9, 0, 0));
        assert_eq!(next.weekday(), Weekday::Mon);
    }

    #[test]
    fn given_weekdays_on_saturday_should_skip_to_monday() {
        // 2025-06-14 is a Saturday
        let base = utc(2025, 6, 14, 9, 0, 0);
        let after = utc(2025, 6, 14, 0, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekdays, after).unwrap();
        assert_eq!(next, utc(2025, 6, 16, 9, 0, 0));
        assert_eq!(next.weekday(), Weekday::Mon);
    }

    #[test]
    fn given_weekdays_on_sunday_should_skip_to_monday() {
        // 2025-06-15 is a Sunday
        let base = utc(2025, 6, 15, 9, 0, 0);
        let after = utc(2025, 6, 15, 0, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekdays, after).unwrap();
        assert_eq!(next, utc(2025, 6, 16, 9, 0, 0));
        assert_eq!(next.weekday(), Weekday::Mon);
    }

    #[test]
    fn given_weekdays_on_wednesday_should_return_thursday() {
        // 2025-06-11 is a Wednesday
        let base = utc(2025, 6, 11, 9, 0, 0);
        let after = utc(2025, 6, 11, 10, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekdays, after).unwrap();
        assert_eq!(next, utc(2025, 6, 12, 9, 0, 0));
        assert_eq!(next.weekday(), Weekday::Thu);
    }

    #[test]
    fn given_weekly_should_return_same_weekday_next_week() {
        // 2025-06-11 is a Wednesday
        let base = utc(2025, 6, 11, 14, 0, 0);
        let after = utc(2025, 6, 11, 15, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekly, after).unwrap();
        assert_eq!(next, utc(2025, 6, 18, 14, 0, 0));
        assert_eq!(next.weekday(), Weekday::Wed);
    }

    #[test]
    fn given_weekly_same_day_before_time_should_return_same_day() {
        // 2025-06-11 is a Wednesday
        let base = utc(2025, 6, 11, 14, 0, 0);
        let after = utc(2025, 6, 11, 10, 0, 0);
        let next = next_occurrence(base, &Recurrence::Weekly, after).unwrap();
        assert_eq!(next, utc(2025, 6, 11, 14, 0, 0));
    }

    #[test]
    fn given_monthly_should_return_same_day_next_month() {
        let base = utc(2025, 6, 15, 3, 0, 0);
        let after = utc(2025, 6, 15, 4, 0, 0);
        let next = next_occurrence(base, &Recurrence::Monthly, after).unwrap();
        assert_eq!(next, utc(2025, 7, 15, 3, 0, 0));
    }

    #[test]
    fn given_monthly_on_31st_should_skip_short_months() {
        let base = utc(2025, 1, 31, 3, 0, 0);
        let after = utc(2025, 1, 31, 4, 0, 0);
        let next = next_occurrence(base, &Recurrence::Monthly, after).unwrap();
        // Feb has no 31st, March does
        assert_eq!(next, utc(2025, 3, 31, 3, 0, 0));
    }

    #[test]
    fn given_monthly_on_29th_should_skip_non_leap_feb() {
        let base = utc(2025, 1, 29, 3, 0, 0);
        let after = utc(2025, 1, 29, 4, 0, 0);
        let next = next_occurrence(base, &Recurrence::Monthly, after).unwrap();
        // 2025 is not a leap year, Feb has no 29th
        assert_eq!(next, utc(2025, 3, 29, 3, 0, 0));
    }

    #[test]
    fn given_daily_with_after_before_start_at_should_not_precede_start_at() {
        // start_at is in the future, after is now (before start_at)
        let base = utc(2025, 6, 20, 9, 0, 0);
        let after = utc(2025, 6, 15, 10, 0, 0);
        let next = next_occurrence(base, &Recurrence::Daily, after).unwrap();
        // Should return start_at itself, never a date before it
        assert!(next >= base);
        assert_eq!(next, utc(2025, 6, 20, 9, 0, 0));
    }
}
