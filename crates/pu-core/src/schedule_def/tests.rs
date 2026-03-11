use super::*;
use tempfile::TempDir;

fn isolate_home(tmp: &TempDir) {
    paths::set_home_override(Some(tmp.path().to_path_buf()));
}

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
    // Use separate temp dirs: one for project root (local), one for HOME (global)
    let home_tmp = TempDir::new().unwrap();
    let project_tmp = TempDir::new().unwrap();
    isolate_home(&home_tmp);

    let project_root = project_tmp.path();
    let local_dir = paths::schedules_dir(project_root);
    std::fs::create_dir_all(&local_dir).unwrap();

    // Write local schedules
    let mut def = make_schedule_def("nightly");
    save_schedule_def(&local_dir, &def).unwrap();
    def.name = "weekly".to_string();
    save_schedule_def(&local_dir, &def).unwrap();

    // Write a global schedule
    let global_dir = paths::global_schedules_dir().unwrap();
    std::fs::create_dir_all(&global_dir).unwrap();
    def.name = "global-only".to_string();
    save_schedule_def(&global_dir, &def).unwrap();

    let defs = list_schedule_defs(project_root);
    assert_eq!(defs.len(), 3);
    // Local schedules come first (sorted), then global (appended)
    assert_eq!(defs[0].name, "nightly");
    assert_eq!(defs[0].scope, "local");
    assert_eq!(defs[1].name, "weekly");
    assert_eq!(defs[1].scope, "local");
    assert_eq!(defs[2].name, "global-only");
    assert_eq!(defs[2].scope, "global");
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
    isolate_home(&tmp);
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
    // Use separate temp dirs: one for project root (local), one for HOME (global)
    let home_tmp = TempDir::new().unwrap();
    let project_tmp = TempDir::new().unwrap();
    isolate_home(&home_tmp);

    let project_root = project_tmp.path();
    let local_dir = paths::schedules_dir(project_root);
    std::fs::create_dir_all(&local_dir).unwrap();

    // Write global schedule first
    let global_dir = paths::global_schedules_dir().unwrap();
    std::fs::create_dir_all(&global_dir).unwrap();
    let mut global_def = make_schedule_def("nightly");
    global_def.project_root = "/global/path".to_string();
    save_schedule_def(&global_dir, &global_def).unwrap();

    // Write local schedule with same name
    let local_def = make_schedule_def("nightly");
    save_schedule_def(&local_dir, &local_def).unwrap();

    // Local should take priority
    let found = find_schedule_def(project_root, "nightly").unwrap();
    assert_eq!(found.scope, "local");
    assert_eq!(found.project_root, "/projects/myapp"); // local's project_root
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
fn given_none_recurrence_at_base_should_return_none() {
    // Exclusive-after semantics: after == base means already ran, so None
    let base = utc(2025, 6, 15, 10, 0, 0);
    let after = base;
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
