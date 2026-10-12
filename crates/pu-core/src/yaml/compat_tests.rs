//! Characterization tests pinning how every YAML document type pu-core reads and
//! writes behaves. They were written against the previous `serde_yaml_ng` stack and
//! keep passing on the current one: same accepted inputs, same rejected inputs, same
//! parsed values, and lossless write → re-read. Tests documented with
//! `BEHAVIOR CHANGE` pin a deliberate, known difference from the old stack.

use super::legacy_fixtures as legacy;
use super::*;
use crate::agent_def::AgentDef;
use crate::config::GlobalAgentSettings;
use crate::error::PuError;
use crate::schedule_def::{Recurrence, ScheduleDef, ScheduleTrigger};
use crate::swarm_def::{SwarmDef, SwarmRosterEntry};
use crate::template::{parse_template, save_template_with_command};
use crate::trigger_def::{GateDef, TriggerAction, TriggerDef, TriggerEvent};
use crate::types::{AgentConfig, Config};
use chrono::{TimeZone, Utc};
use indexmap::IndexMap;
use std::collections::HashMap;

/// Strings that are easy to mangle: YAML 1.1/1.2 type look-alikes, indicators,
/// comment markers, block-scalar edge cases, unicode and control characters.
const TRICKY: &[&str] = &[
    "",
    "true",
    "True",
    "false",
    "yes",
    "no",
    "on",
    "off",
    "y",
    "n",
    "null",
    "Null",
    "~",
    "012",
    "1.10",
    "0x10",
    "0o10",
    "1e3",
    "+3",
    ".inf",
    ".nan",
    "-",
    "- x",
    "--sandbox",
    ": x",
    "a: b",
    "a:b",
    "#x",
    "x # y",
    "x#y",
    " lead",
    "trail ",
    "@x",
    "`x",
    "!x",
    "&x",
    "*x",
    "{x}",
    "[x]",
    "%x",
    "|",
    ">",
    "'",
    "\"",
    "\\",
    "\t",
    "café ☕ 日本語",
    "2025-01-01",
    "2025-01-01T00:00:00Z",
    "line1\nline2",
    "line1\nline2\n",
    "a\n---\nb",
    "trailing\n\n",
    "\nlead",
    "  indented\nx",
    "tab\there",
    "ctrl\u{7}",
    "=",
    "<<",
    "?x",
    "? x",
    ",x",
    "x,",
    "...",
    "---",
    "a  b",
    "\u{feff}bom",
    "crlf\r\nline",
    "long line that is well over eighty characters so that any folding or wrapping logic in an emitter kicks in   with  odd   spacing ",
];

/// Structural equality for types without `PartialEq` (`#[serde(skip)]` fields excluded).
fn assert_same<T: serde::Serialize>(actual: &T, expected: &T) {
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

/// Write with the current serializer, then read back.
fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    let yaml = to_string(value).unwrap();
    from_str(&yaml).unwrap_or_else(|e| panic!("re-read failed: {e}\n---\n{yaml}"))
}

fn agent_config(yaml_fields: &str) -> Result<AgentConfig, Error> {
    from_str(&format!("name: a\ncommand: b\n{yaml_fields}"))
}

fn inline_prompt(prompt: &str) -> ScheduleTrigger {
    ScheduleTrigger::InlinePrompt {
        prompt: prompt.to_string(),
        agent: "claude".to_string(),
    }
}

fn sample_config() -> Config {
    let mut agents = IndexMap::new();
    agents.insert(
        "codex".to_string(),
        AgentConfig {
            name: "codex".into(),
            command: "/usr/bin/env codex --profile: work # not a comment".into(),
            prompt_flag: Some("-p".into()),
            interactive: false,
            launch_args: Some(vec![
                "--sandbox".into(),
                "workspace-write".into(),
                "- dash".into(),
                "yes".into(),
                "012".into(),
            ]),
        },
    );
    agents.insert(
        "claude".to_string(),
        AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: Some(vec![]),
        },
    );
    agents.insert(
        "terminal".to_string(),
        AgentConfig {
            name: "terminal".into(),
            command: "shell".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None,
        },
    );
    Config {
        default_agent: "codex".into(),
        agents,
        env_files: vec![".env".into(), ".env.local".into(), "on".into()],
    }
}

fn sample_agent_def() -> AgentDef {
    AgentDef {
        name: "reviewer".into(),
        agent_type: "claude".into(),
        template: Some("review: strict".into()),
        inline_prompt: Some(
            "Review the diff.\n\n- Check: errors\n# not a heading\n  indented line\n".into(),
        ),
        tags: vec!["yes".into(), "#tag".into(), "café".into()],
        scope: String::new(),
        available_in_command_dialog: false,
        icon: Some("🔍".into()),
        command: Some("/review".into()),
    }
}

fn sample_swarm_def() -> SwarmDef {
    SwarmDef {
        name: "full-stack".into(),
        worktree_count: 3,
        worktree_template: "feature-{index}".into(),
        roster: vec![
            SwarmRosterEntry {
                agent_def: "reviewer".into(),
                role: "review: all".into(),
                quantity: 2,
            },
            SwarmRosterEntry {
                agent_def: "builder".into(),
                role: "no".into(),
                quantity: 1,
            },
        ],
        include_terminal: true,
        scope: String::new(),
    }
}

fn sample_trigger_def() -> TriggerDef {
    let mut variables = HashMap::new();
    variables.insert("PORT".to_string(), "8080".to_string());
    TriggerDef {
        name: "gatekeeper".into(),
        description: Some("Runs: tests # always".into()),
        on: TriggerEvent::PreCommit,
        sequence: vec![
            TriggerAction {
                inject: Some("/simplify\nthen re-run {{PORT}}".into()),
                gate: None,
                max_retries: None,
            },
            TriggerAction {
                inject: None,
                gate: Some(GateDef {
                    run: "cargo test -- --nocapture".into(),
                    expect_exit: Some(-1),
                }),
                max_retries: Some(3),
            },
        ],
        variables,
        scope: String::new(),
    }
}

fn sample_schedule_def(trigger: ScheduleTrigger) -> ScheduleDef {
    ScheduleDef {
        name: "nightly".into(),
        enabled: false,
        recurrence: Recurrence::Weekdays,
        start_at: Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap(),
        next_run: Some(Utc.with_ymd_and_hms(2025, 1, 3, 3, 4, 5).unwrap()),
        trigger,
        project_root: "/projects/my app".into(),
        target: "".into(),
        root: false,
        agent_name: Some("overnight".into()),
        scope: String::new(),
        created_at: Utc.with_ymd_and_hms(2024, 12, 31, 23, 59, 59).unwrap(),
    }
}

fn sample_schedule_triggers() -> Vec<ScheduleTrigger> {
    let mut vars = HashMap::new();
    vars.insert("N".to_string(), "3".to_string());
    vec![
        ScheduleTrigger::AgentDef {
            name: "reviewer".into(),
        },
        ScheduleTrigger::SwarmDef {
            name: "full-stack".into(),
            vars,
        },
        ScheduleTrigger::InlinePrompt {
            prompt: "yes".into(),
            agent: "codex".into(),
        },
        inline_prompt("Audit deps:\n  - cargo deny\n  - cargo audit\n"),
    ]
}

/// Carries one string through a plain field, a sequence, and an internally tagged
/// enum (which serde buffers and resolves untyped, unlike plain struct fields).
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ScalarCarrier {
    plain: String,
    list: Vec<String>,
    tagged: ScheduleTrigger,
}

fn carrier(s: &str) -> ScalarCarrier {
    ScalarCarrier {
        plain: s.to_string(),
        list: vec![s.to_string()],
        tagged: ScheduleTrigger::InlinePrompt {
            prompt: s.to_string(),
            agent: s.to_string(),
        },
    }
}

// --- Empty / trivial documents ---

#[test]
fn given_empty_whitespace_comment_or_bare_marker_config_should_use_all_defaults() {
    for input in ["", "   \n\n", "\n", "# only a comment\n", "---\n"] {
        // when
        let config: Config = from_str(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));

        // then
        assert_same(&config, &Config::default());
    }
}

#[test]
fn given_empty_global_agent_settings_should_default_codex_yolo_off() {
    let settings: GlobalAgentSettings = from_str("").unwrap();
    assert!(!settings.codex_yolo);
}

/// BEHAVIOR CHANGE (lenient): serde_yaml_ng rejected an explicit null document
/// (`invalid type: unit value`); it now reads like an empty file.
#[test]
fn given_explicit_null_document_for_config_should_use_all_defaults() {
    for input in ["~\n", "null\n"] {
        assert_same(&from_str::<Config>(input).unwrap(), &Config::default());
    }
}

#[test]
fn given_empty_document_for_def_with_required_fields_should_reject() {
    assert!(from_str::<AgentDef>("").is_err());
    assert!(from_str::<TriggerDef>("").is_err());
    assert!(from_str::<SwarmDef>("").is_err());
    assert!(from_str::<ScheduleDef>("").is_err());
}

// --- Comments, ordering, defaults ---

#[test]
fn given_commented_config_should_ignore_comments_and_preserve_agent_order() {
    // given
    let yaml = r##"# project config
defaultAgent: codex # inline comment
agents:
  # codex first on purpose
  codex:
    name: codex
    command: codex --x#y
    launchArgs:
      - --sandbox # trailing comment
      - "# quoted hash"
  claude:
    name: claude
    command: claude
envFiles: [.env, ".env.local"] # flow sequence
"##;

    // when
    let config: Config = from_str(yaml).unwrap();

    // then
    assert_eq!(config.default_agent, "codex");
    let names: Vec<&str> = config.agents.keys().map(String::as_str).collect();
    assert_eq!(names, ["codex", "claude"]);
    assert_eq!(config.agents["codex"].command, "codex --x#y");
    assert_eq!(
        config.agents["codex"].launch_args.as_deref(),
        Some(&["--sandbox".to_string(), "# quoted hash".to_string()][..])
    );
    assert_eq!(config.env_files, [".env", ".env.local"]);
}

#[test]
fn given_missing_optional_agent_fields_should_apply_defaults() {
    let config = agent_config("").unwrap();
    assert_eq!(config.prompt_flag, None);
    assert!(config.interactive);
    assert_eq!(config.launch_args, None);
}

#[test]
fn given_null_spellings_for_optional_field_should_read_none() {
    for spelling in ["null", "~", "Null", "NULL", ""] {
        let config = agent_config(&format!("promptFlag: {spelling}\n")).unwrap();
        assert_eq!(config.prompt_flag, None, "spelling {spelling:?}");
    }
}

#[test]
fn given_empty_launch_args_list_should_differ_from_absent_launch_args() {
    assert_eq!(
        agent_config("launchArgs: []\n").unwrap().launch_args,
        Some(vec![])
    );
    assert_eq!(agent_config("").unwrap().launch_args, None);
    assert_eq!(
        agent_config("launchArgs: null\n").unwrap().launch_args,
        None
    );
}

#[test]
fn given_minimal_defs_should_apply_serde_defaults() {
    let agent: AgentDef = from_str("name: a\n").unwrap();
    assert_eq!(agent.agent_type, "claude");
    assert!(agent.available_in_command_dialog);
    assert!(agent.tags.is_empty());

    let swarm: SwarmDef = from_str("name: s\nroster:\n- agent_def: a\n  role: r\n").unwrap();
    assert_eq!(swarm.worktree_count, 1);
    assert_eq!(swarm.roster[0].quantity, 1);
    assert!(!swarm.include_terminal);

    let trigger: TriggerDef = from_str("name: t\non: pre_push\nsequence: []\n").unwrap();
    assert!(trigger.variables.is_empty());
    assert_eq!(trigger.description, None);

    let schedule: ScheduleDef = from_str(
        "name: n\nstart_at: 2025-01-01T00:00:00Z\ntrigger:\n  type: inline_prompt\n  prompt: p\nproject_root: /p\ncreated_at: 2025-01-01T00:00:00Z\n",
    )
    .unwrap();
    assert!(schedule.enabled);
    assert!(schedule.root);
    assert_eq!(schedule.recurrence, Recurrence::None);
    assert_eq!(schedule.trigger, inline_prompt("p"));
}

// --- Booleans (YAML 1.2 core schema; YAML 1.1 spellings are NOT booleans) ---

#[test]
fn given_core_schema_boolean_spellings_should_parse_as_booleans() {
    for (spelling, expected) in [
        ("true", true),
        ("True", true),
        ("TRUE", true),
        ("false", false),
        ("False", false),
        ("FALSE", false),
    ] {
        let config = agent_config(&format!("interactive: {spelling}\n")).unwrap();
        assert_eq!(config.interactive, expected, "spelling {spelling:?}");

        let settings: GlobalAgentSettings = from_str(&format!("codexYolo: {spelling}\n")).unwrap();
        assert_eq!(settings.codex_yolo, expected, "spelling {spelling:?}");
    }
}

#[test]
fn given_yaml11_boolean_spellings_for_bool_field_should_reject() {
    for spelling in [
        "yes", "no", "on", "off", "y", "n", "Yes", "NO", "On", "OFF", "\"true\"", "'false'", "1",
        "0",
    ] {
        assert!(
            agent_config(&format!("interactive: {spelling}\n")).is_err(),
            "interactive: {spelling} should be rejected"
        );
        assert!(
            from_str::<GlobalAgentSettings>(&format!("codexYolo: {spelling}\n")).is_err(),
            "codexYolo: {spelling} should be rejected"
        );
    }
}

#[test]
fn given_yaml11_boolean_spellings_for_string_fields_should_keep_source_text() {
    for spelling in ["yes", "no", "on", "off", "y", "n", "Yes", "OFF"] {
        // plain struct field
        let config = agent_config(&format!("promptFlag: {spelling}\n")).unwrap();
        assert_eq!(config.prompt_flag.as_deref(), Some(spelling));

        // internally tagged enum field (untyped resolution path)
        let trigger: ScheduleTrigger =
            from_str(&format!("type: inline_prompt\nprompt: {spelling}\n")).unwrap();
        assert_eq!(trigger, inline_prompt(spelling));

        // HashMap<String, String> inside a tagged enum
        let swarm: ScheduleTrigger = from_str(&format!(
            "type: swarm_def\nname: s\nvars:\n  K: {spelling}\n"
        ))
        .unwrap();
        let ScheduleTrigger::SwarmDef { vars, .. } = swarm else {
            panic!("expected swarm_def");
        };
        assert_eq!(vars["K"], spelling);
    }
}

// --- Numbers vs strings ---

#[test]
fn given_number_looking_scalars_for_string_fields_should_keep_source_text() {
    // given
    let yaml =
        "name: 012\ncommand: 1.10\npromptFlag: 0x1F\nlaunchArgs: [1e3, +3, .inf, 007, true]\n";

    // when
    let config: AgentConfig = from_str(yaml).unwrap();

    // then
    assert_eq!(config.name, "012");
    assert_eq!(config.command, "1.10");
    assert_eq!(config.prompt_flag.as_deref(), Some("0x1F"));
    assert_eq!(
        config.launch_args.unwrap(),
        ["1e3", "+3", ".inf", "007", "true"]
    );

    let trigger: TriggerDef = from_str(
        "name: t\non: agent_idle\nsequence: []\nvariables:\n  PORT: 8080\n  F: 1.10\n  B: true\n",
    )
    .unwrap();
    assert_eq!(trigger.variables["PORT"], "8080");
    assert_eq!(trigger.variables["F"], "1.10");
    assert_eq!(trigger.variables["B"], "true");
}

#[test]
fn given_empty_value_for_string_fields_should_read_empty_string() {
    // Hand-written `key:` with no value for a non-optional string, at any depth.
    let config: AgentConfig = from_str("name:\ncommand: c\nlaunchArgs:\n-\n- x\n").unwrap();
    assert_eq!(config.name, "");
    assert_eq!(config.launch_args.unwrap(), ["", "x"]);

    let swarm: SwarmDef = from_str("name: s\nworktree_template:\n").unwrap();
    assert_eq!(swarm.worktree_template, "");

    let trigger: TriggerDef =
        from_str("name: t\non: agent_idle\nsequence: []\nvariables:\n  EMPTY:\n").unwrap();
    assert_eq!(trigger.variables["EMPTY"], "");

    let schedule: ScheduleDef = from_str(
        "name: n\nstart_at: 2025-01-01T00:00:00Z\ntrigger:\n  type: agent_def\n  name: r\nproject_root: /p\ntarget:\ncreated_at: 2025-01-01T00:00:00Z\n",
    )
    .unwrap();
    assert_eq!(schedule.target, "");
}

/// BEHAVIOR CHANGE (value): serde_yaml_ng kept the literal text of a plain `~` / `null`
/// in a non-optional string (`"~"`, `"null"`); it is now read as `""`, the same as an
/// empty value. Quoted `'null'` / `'~'` (what the writer always emitted) are unchanged.
#[test]
fn given_plain_null_literal_for_non_optional_string_should_read_empty_string() {
    let config: AgentConfig =
        from_str("name: ~\ncommand: null\nlaunchArgs: [~, null, NULL, 'null', '~']\n").unwrap();
    assert_eq!(config.name, "");
    assert_eq!(config.command, "");
    assert_eq!(config.launch_args.unwrap(), ["", "", "", "null", "~"]);

    let trigger: TriggerDef =
        from_str("name: t\non: agent_idle\nsequence: []\nvariables:\n  N: null\n").unwrap();
    assert_eq!(trigger.variables["N"], "");
}

#[test]
fn given_typed_scalars_inside_tagged_enum_string_fields_should_follow_untyped_resolution() {
    // Values that resolve to float/bool/null/int are type errors for String targets.
    for scalar in ["1.10", "true", "null", "~", "42"] {
        assert!(
            from_str::<ScheduleTrigger>(&format!("type: inline_prompt\nprompt: {scalar}\n"))
                .is_err(),
            "prompt: {scalar} should be rejected"
        );
    }
    assert!(from_str::<ScheduleTrigger>("type: swarm_def\nname: s\nvars:\n  N: 3\n").is_err());
}

/// BEHAVIOR CHANGE (stricter, hand-written files only): inside an internally tagged
/// enum (schedule `trigger:`), serde resolves scalars untyped. serde_yaml_ng kept a
/// leading-zero number like `012` as the string "012"; serde-saphyr resolves it as a
/// number, so it is rejected for a string field. The writer always quotes such values
/// (`'012'`), which still read back as "012".
#[test]
fn given_leading_zero_number_inside_tagged_enum_string_field_should_require_quotes() {
    assert!(from_str::<ScheduleTrigger>("type: inline_prompt\nprompt: 012\n").is_err());
    let quoted: ScheduleTrigger = from_str("type: inline_prompt\nprompt: '012'\n").unwrap();
    assert_eq!(quoted, inline_prompt("012"));
}

#[test]
fn given_integer_spellings_should_parse_or_reject_consistently() {
    let quantity = |q: &str| {
        from_str::<SwarmRosterEntry>(&format!("agent_def: a\nrole: r\nquantity: {q}\n"))
            .map(|e| e.quantity)
            .ok()
    };
    assert_eq!(quantity("3"), Some(3));
    assert_eq!(quantity("+3"), Some(3));
    assert_eq!(quantity("0x10"), Some(16));
    assert_eq!(quantity("0o10"), Some(8));
    assert_eq!(
        quantity("012"),
        None,
        "leading-zero decimal is not an integer"
    );
    assert_eq!(quantity("\"3\""), None, "quoted number is a string");
    // BEHAVIOR CHANGE (lenient): YAML 1.1 digit separators are now accepted
    // (serde_yaml_ng rejected `1_000`).
    assert_eq!(quantity("1_000"), Some(1000));
    assert_eq!(quantity("3.0"), None);
    assert_eq!(quantity("-1"), None);

    let gate: GateDef = from_str("run: x\nexpect_exit: -1\n").unwrap();
    assert_eq!(gate.expect_exit, Some(-1));
}

// --- Quoting, indicators, escapes, unicode ---

#[test]
fn given_quoted_and_unquoted_strings_with_indicators_should_parse() {
    // given
    let yaml = r#"name: "a: b"
command: 'it''s # not a comment'
promptFlag: x#y
launchArgs:
  - --sandbox
  - "- dash"
  - '-'
  - key: value
"#;

    // when
    let result: Result<AgentConfig, _> = from_str(yaml);

    // then — a mapping inside a Vec<String> is a type error
    assert!(result.is_err());

    let yaml = yaml.replace("  - key: value\n", "  - \"key: value\"\n");
    let config: AgentConfig = from_str(&yaml).unwrap();
    assert_eq!(config.name, "a: b");
    assert_eq!(config.command, "it's # not a comment");
    assert_eq!(config.prompt_flag.as_deref(), Some("x#y"));
    assert_eq!(
        config.launch_args.unwrap(),
        ["--sandbox", "- dash", "-", "key: value"]
    );
}

#[test]
fn given_double_quoted_escapes_and_unicode_should_decode() {
    let config = agent_config(
        "promptFlag: \"tab\\there \\u00e9 \\u2603 \\\"q\\\" \\\\ \\n\"\nlaunchArgs: [café, ☕, 日本語, \"🔍\"]\n",
    )
    .unwrap();
    assert_eq!(
        config.prompt_flag.as_deref(),
        Some("tab\there é ☃ \"q\" \\ \n")
    );
    assert_eq!(config.launch_args.unwrap(), ["café", "☕", "日本語", "🔍"]);
}

#[test]
fn given_byte_order_mark_should_parse() {
    let config: Config = from_str("\u{feff}defaultAgent: codex\n").unwrap();
    assert_eq!(config.default_agent, "codex");
}

#[test]
fn given_crlf_line_endings_should_parse() {
    let config: Config = from_str("defaultAgent: codex\r\nenvFiles:\r\n  - .env\r\n").unwrap();
    assert_eq!(config.default_agent, "codex");
    assert_eq!(config.env_files, [".env"]);
}

// --- Block scalars (prompts / templates) ---

#[test]
fn given_block_scalars_should_preserve_prompt_text() {
    let cases = [
        ("|\n  line1\n\n  line3\n", "line1\n\nline3\n"),
        ("|-\n  line1\n  line2\n", "line1\nline2"),
        ("|+\n  line1\n\n", "line1\n\n"),
        (">\n  folded\n  text\n\n  para\n", "folded text\npara\n"),
        (">-\n  folded\n  text\n", "folded text"),
        ("|2\n    indented\n  base\n", "  indented\nbase\n"),
        (
            "|\n  # heading\n  - item: x\n  key: 'v'\n",
            "# heading\n- item: x\nkey: 'v'\n",
        ),
    ];
    for (block, expected) in cases {
        // plain struct field
        let agent: AgentDef = from_str(&format!("name: a\ninline_prompt: {block}")).unwrap();
        assert_eq!(agent.inline_prompt.as_deref(), Some(expected), "{block:?}");

        if block.starts_with("|2") {
            continue; // indentation indicator is relative to the parent; checked above
        }

        // tagged enum field, nested one level deeper
        let indented = block.replace("\n  ", "\n    ");
        let trigger: ScheduleTrigger =
            from_str(&format!("type: inline_prompt\nprompt: {indented}")).unwrap();
        assert_eq!(trigger, inline_prompt(expected), "{block:?}");

        // sequence item
        let def: TriggerDef = from_str(&format!(
            "name: t\non: agent_idle\nsequence:\n- inject: {indented}"
        ))
        .unwrap();
        assert_eq!(
            def.sequence[0].inject.as_deref(),
            Some(expected),
            "{block:?}"
        );
    }
}

/// BEHAVIOR CHANGE (value, files without a final newline only): a `|` / `>` block
/// scalar that ends the file with no line break now keeps one trailing newline
/// (serde_yaml_ng returned `"x"`). Files ending in a newline — everything the writer
/// produces — are unaffected (see `given_block_scalars_should_preserve_prompt_text`).
#[test]
fn given_block_scalar_at_end_of_file_without_newline_should_keep_clip_newline() {
    for block in ["|", ">", "|+"] {
        let agent: AgentDef = from_str(&format!("name: a\ninline_prompt: {block}\n  x")).unwrap();
        assert_eq!(agent.inline_prompt.as_deref(), Some("x\n"), "{block}");
    }
    let strip: AgentDef = from_str("name: a\ninline_prompt: |-\n  x").unwrap();
    assert_eq!(strip.inline_prompt.as_deref(), Some("x"));
}

// --- Unknown fields, duplicates, multi-doc, merge keys, tags ---

#[test]
fn given_unknown_fields_should_be_ignored_for_every_document_type() {
    let extra = "futureField: 1\nnested_unknown:\n  a: [1, 2]\n";
    let config: Config = from_str(&format!("defaultAgent: codex\n{extra}")).unwrap();
    assert_eq!(config.default_agent, "codex");
    let settings: GlobalAgentSettings = from_str(&format!("codexYolo: true\n{extra}")).unwrap();
    assert!(settings.codex_yolo);
    let agent: AgentDef = from_str(&format!("name: a\n{extra}")).unwrap();
    assert_eq!(agent.name, "a");
    let swarm: SwarmDef = from_str(&format!("name: s\n{extra}")).unwrap();
    assert_eq!(swarm.name, "s");
    let trigger: TriggerDef =
        from_str(&format!("name: t\non: agent_idle\nsequence: []\n{extra}")).unwrap();
    assert_eq!(trigger.name, "t");
    let schedule: ScheduleDef = from_str(&format!(
        "name: n\nstart_at: 2025-01-01T00:00:00Z\ntrigger:\n  type: agent_def\n  name: r\n  extra: 1\nproject_root: /p\ncreated_at: 2025-01-01T00:00:00Z\n{extra}"
    ))
    .unwrap();
    assert_eq!(schedule.name, "n");
}

#[test]
fn given_duplicate_map_keys_should_keep_last_value() {
    let config: Config =
        from_str("agents:\n  claude: {name: a, command: b}\n  claude: {name: c, command: d}\n")
            .unwrap();
    assert_eq!(config.agents.len(), 1);
    assert_eq!(config.agents["claude"].command, "d");

    let trigger: TriggerDef =
        from_str("name: t\non: agent_idle\nsequence: []\nvariables:\n  A: x\n  A: y\n").unwrap();
    assert_eq!(trigger.variables["A"], "y");
}

/// BEHAVIOR CHANGE (lenient): serde_yaml_ng rejected a repeated struct field
/// (`duplicate field`) while letting the last map entry win. serde-saphyr applies one
/// duplicate-key policy to both; last-wins is chosen so every file that loaded before
/// still loads identically.
#[test]
fn given_duplicate_struct_fields_should_keep_last_value() {
    let config: AgentConfig = from_str("name: a\ncommand: b\nname: c\n").unwrap();
    assert_eq!(config.name, "c");
}

#[test]
fn given_multiple_documents_should_reject() {
    assert!(from_str::<AgentConfig>("name: a\ncommand: b\n---\nname: c\ncommand: d\n").is_err());
}

#[test]
fn given_leading_document_marker_and_end_marker_should_parse() {
    let config = from_str::<AgentConfig>("---\nname: a\ncommand: b\n...\n").unwrap();
    assert_eq!(config.name, "a");
}

#[test]
fn given_merge_key_should_not_merge_into_struct() {
    // `<<` is treated as an ordinary (unknown) key, so `command` stays missing.
    assert!(from_str::<AgentConfig>("base: &b\n  command: x\nname: a\n<<: *b\n").is_err());
}

#[test]
fn given_tags_on_string_fields_should_keep_scalar_text() {
    let config: AgentConfig =
        from_str("name: !!str 012\ncommand: !!binary aGk=\npromptFlag: !custom x\n").unwrap();
    assert_eq!(config.name, "012");
    assert_eq!(config.command, "aGk=");
    assert_eq!(config.prompt_flag.as_deref(), Some("x"));
}

/// BEHAVIOR CHANGE (stricter, explicit tags only): a non-string core tag on a string
/// field (`!!int 3`) was read as "3"; it is now a type error.
#[test]
fn given_non_string_core_tag_on_string_field_should_reject() {
    assert!(from_str::<AgentConfig>("name: !!int 3\ncommand: c\n").is_err());
}

#[test]
fn given_anchor_and_alias_should_resolve() {
    let config: AgentConfig = from_str("name: &n same\ncommand: *n\n").unwrap();
    assert_eq!(config.command, "same");
}

#[test]
fn given_unknown_enum_variants_should_reject() {
    assert!(from_str::<TriggerDef>("name: t\non: AgentIdle\nsequence: []\n").is_err());
    assert!(from_str::<TriggerDef>("name: t\non: agent-idle\nsequence: []\n").is_err());
    assert!(from_str::<Recurrence>("fortnightly\n").is_err());
    assert!(from_str::<ScheduleTrigger>("type: bogus\nname: s\n").is_err());
    assert!(from_str::<ScheduleTrigger>("name: s\n").is_err());
}

#[test]
fn given_timestamps_in_quoted_unquoted_and_offset_forms_should_parse() {
    let base = "name: n\ntrigger:\n  type: agent_def\n  name: r\nproject_root: /p\n";
    let expected = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    for ts in [
        "2025-01-01T00:00:00Z",
        "\"2025-01-01T00:00:00Z\"",
        "'2025-01-01T00:00:00Z'",
        "2025-01-01T02:00:00+02:00",
        "2025-01-01T00:00:00.000Z",
    ] {
        let def: ScheduleDef =
            from_str(&format!("{base}start_at: {ts}\ncreated_at: {ts}\n")).unwrap();
        assert_eq!(def.start_at, expected, "{ts}");
        assert_eq!(def.created_at, expected, "{ts}");
    }
    assert!(
        from_str::<ScheduleDef>(&format!(
            "{base}start_at: 2025-01-01\ncreated_at: 2025-01-01\n"
        ))
        .is_err()
    );
}

#[test]
fn given_malformed_yaml_should_reject() {
    for bad in [
        "name: [unclosed\n",
        "name: a\n  command: b\n",
        "\tname: a\n",
        "name: \"unterminated\n",
        "name: a\ncommand: *missing\n",
    ] {
        assert!(from_str::<AgentConfig>(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn given_yaml_error_should_map_to_pu_yaml_error() {
    let err: PuError = from_str::<Config>("defaultAgent: [\n").unwrap_err().into();
    assert_eq!(err.code(), "YAML_ERROR");
    assert!(err.to_string().starts_with("yaml error: "));
    // Writers convert into io::Error, so the error must be Send + Sync.
    let io = std::io::Error::other(from_str::<Config>("defaultAgent: [\n").unwrap_err());
    assert_eq!(io.kind(), std::io::ErrorKind::Other);
}

// --- Template frontmatter ---

#[test]
fn given_template_frontmatter_with_block_and_quoted_values_should_parse() {
    let content = "---\nname: review\ndescription: |\n  Line one: x\n  # two\nagent: \"yes\"\ncommand: /review\n---\n\nBody: here\n";
    let tpl = parse_template(content, "file.md");
    assert_eq!(tpl.name, "review");
    assert_eq!(tpl.description, "Line one: x\n# two\n");
    assert_eq!(tpl.agent, "yes");
    assert_eq!(tpl.command.as_deref(), Some("/review"));
    assert_eq!(tpl.body, "Body: here\n");
}

#[test]
fn given_template_frontmatter_with_unquoted_yaml11_scalars_should_keep_text() {
    let tpl = parse_template("---\ndescription: no\nagent: on\n---\nB", "t.md");
    assert_eq!(tpl.name, "t");
    assert_eq!(tpl.description, "no");
    assert_eq!(tpl.agent, "on");
}

#[test]
fn given_invalid_template_frontmatter_should_fall_back_to_whole_body() {
    let content = "---\nname: [unclosed\n---\nBody\n";
    let tpl = parse_template(content, "broken.md");
    assert_eq!(tpl.name, "broken");
    assert_eq!(tpl.body, content);
}

// --- Files written by the previous serializer still parse ---

#[test]
fn given_documents_written_by_previous_serializer_should_parse_to_same_values() {
    assert_same(
        &from_str::<Config>(legacy::CONFIG).unwrap(),
        &sample_config(),
    );
    assert!(
        from_str::<GlobalAgentSettings>(legacy::GLOBAL_AGENT_SETTINGS)
            .unwrap()
            .codex_yolo
    );
    assert_same(
        &from_str::<AgentDef>(legacy::AGENT_DEF).unwrap(),
        &sample_agent_def(),
    );
    assert_same(
        &from_str::<SwarmDef>(legacy::SWARM_DEF).unwrap(),
        &sample_swarm_def(),
    );
    assert_same(
        &from_str::<TriggerDef>(legacy::TRIGGER_DEF).unwrap(),
        &sample_trigger_def(),
    );
    let triggers = sample_schedule_triggers();
    assert_eq!(legacy::SCHEDULE_DEFS.len(), triggers.len());
    for (doc, trigger) in legacy::SCHEDULE_DEFS.iter().zip(triggers) {
        assert_same(
            &from_str::<ScheduleDef>(doc).unwrap(),
            &sample_schedule_def(trigger),
        );
    }
}

#[test]
fn given_template_written_by_previous_serializer_should_parse_to_same_fields() {
    let tpl = parse_template(legacy::TEMPLATE_FILE, "review.md");
    assert_eq!(tpl.name, "review");
    assert_eq!(tpl.description, "Review: strict # x");
    assert_eq!(tpl.agent, "yes");
    assert_eq!(tpl.command.as_deref(), Some("/review"));
    assert_eq!(tpl.body, "Body line\n");
}

#[test]
fn given_scalars_written_by_previous_serializer_should_parse_to_same_text() {
    for (original, doc) in legacy::SCALAR_DOCS {
        let parsed: ScalarCarrier =
            from_str(doc).unwrap_or_else(|e| panic!("{original:?}: {e}\n{doc}"));
        assert_eq!(parsed, carrier(original), "{original:?}");
    }
}

// --- Current writer round-trips ---

#[test]
fn given_every_document_type_should_round_trip_through_writer() {
    assert_same(&round_trip(&sample_config()), &sample_config());
    assert_same(&round_trip(&Config::default()), &Config::default());
    for codex_yolo in [true, false] {
        let settings = GlobalAgentSettings { codex_yolo };
        assert_eq!(round_trip(&settings).codex_yolo, codex_yolo);
    }
    assert_same(&round_trip(&sample_agent_def()), &sample_agent_def());
    assert_same(&round_trip(&sample_swarm_def()), &sample_swarm_def());
    assert_same(&round_trip(&sample_trigger_def()), &sample_trigger_def());
    for trigger in sample_schedule_triggers() {
        let def = sample_schedule_def(trigger);
        assert_same(&round_trip(&def), &def);
    }
}

#[test]
fn given_config_round_trip_should_keep_agent_order_and_launch_args_presence() {
    let reread = round_trip(&sample_config());
    let names: Vec<&str> = reread.agents.keys().map(String::as_str).collect();
    assert_eq!(names, ["codex", "claude", "terminal"]);
    assert_eq!(reread.agents["claude"].launch_args, Some(vec![]));
    assert_eq!(reread.agents["terminal"].launch_args, None);
}

#[test]
fn given_tricky_strings_should_round_trip_through_writer() {
    for s in TRICKY {
        assert_eq!(round_trip(&carrier(s)), carrier(s), "{s:?}");

        let mut def = sample_agent_def();
        def.inline_prompt = Some(s.to_string());
        def.tags = vec![s.to_string()];
        assert_same(&round_trip(&def), &def);

        let mut trigger = sample_trigger_def();
        trigger.variables.insert(s.to_string(), s.to_string());
        trigger.sequence[0].inject = Some(s.to_string());
        assert_same(&round_trip(&trigger), &trigger);
    }
}

#[test]
fn given_writer_output_should_be_newline_terminated() {
    let outputs = [
        to_string(&sample_config()).unwrap(),
        to_string(&GlobalAgentSettings::default()).unwrap(),
        to_string(&sample_agent_def()).unwrap(),
        to_string(&sample_swarm_def()).unwrap(),
        to_string(&sample_trigger_def()).unwrap(),
        to_string(&sample_schedule_def(inline_prompt("x"))).unwrap(),
    ];
    for out in outputs {
        assert!(out.ends_with('\n'), "{out:?}");
        assert!(!out.starts_with("---"), "{out:?}");
    }
}

#[test]
fn given_tricky_template_fields_should_round_trip_through_save_and_parse() {
    let tmp = tempfile::TempDir::new().unwrap();
    for s in TRICKY {
        // given
        let body = "Body\n---\nmore";

        // when
        save_template_with_command(tmp.path(), "tpl", s, s, body, Some(s)).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("tpl.md")).unwrap();
        let tpl = parse_template(&content, "tpl.md");

        // then
        assert_eq!(tpl.name, "tpl", "{s:?}\n{content}");
        assert_eq!(tpl.description, *s, "{s:?}\n{content}");
        assert_eq!(tpl.agent, *s, "{s:?}\n{content}");
        // BEHAVIOR CHANGE (fix): parse_template hands the parser frontmatter without its
        // final newline. serde_yaml_ng then dropped the trailing newline of a block
        // scalar in the last field (`"x\n"` read back as `"x"`); it now round-trips.
        assert_eq!(tpl.command.as_deref(), Some(*s), "{s:?}\n{content}");
        assert_eq!(tpl.body, body, "{s:?}\n{content}");
    }
}
