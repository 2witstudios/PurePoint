//! Exact bytes produced by the previous serializer (`serde_yaml_ng` 0.10) for
//! [`super::compat_tests`]. Files already on disk look like this, so the current
//! reader must keep accepting them unchanged.

/// `(original string, document written for ScalarCarrier { plain, list: [s], tagged })`.
pub const SCALAR_DOCS: &[(&str, &str)] = &[
    (
        "",
        "plain: ''\nlist:\n- ''\ntagged:\n  type: inline_prompt\n  prompt: ''\n  agent: ''\n",
    ),
    (
        "true",
        "plain: 'true'\nlist:\n- 'true'\ntagged:\n  type: inline_prompt\n  prompt: 'true'\n  agent: 'true'\n",
    ),
    (
        "True",
        "plain: 'True'\nlist:\n- 'True'\ntagged:\n  type: inline_prompt\n  prompt: 'True'\n  agent: 'True'\n",
    ),
    (
        "false",
        "plain: 'false'\nlist:\n- 'false'\ntagged:\n  type: inline_prompt\n  prompt: 'false'\n  agent: 'false'\n",
    ),
    (
        "yes",
        "plain: yes\nlist:\n- yes\ntagged:\n  type: inline_prompt\n  prompt: yes\n  agent: yes\n",
    ),
    (
        "no",
        "plain: no\nlist:\n- no\ntagged:\n  type: inline_prompt\n  prompt: no\n  agent: no\n",
    ),
    (
        "on",
        "plain: on\nlist:\n- on\ntagged:\n  type: inline_prompt\n  prompt: on\n  agent: on\n",
    ),
    (
        "off",
        "plain: off\nlist:\n- off\ntagged:\n  type: inline_prompt\n  prompt: off\n  agent: off\n",
    ),
    (
        "y",
        "plain: y\nlist:\n- y\ntagged:\n  type: inline_prompt\n  prompt: y\n  agent: y\n",
    ),
    (
        "n",
        "plain: n\nlist:\n- n\ntagged:\n  type: inline_prompt\n  prompt: n\n  agent: n\n",
    ),
    (
        "null",
        "plain: 'null'\nlist:\n- 'null'\ntagged:\n  type: inline_prompt\n  prompt: 'null'\n  agent: 'null'\n",
    ),
    (
        "Null",
        "plain: 'Null'\nlist:\n- 'Null'\ntagged:\n  type: inline_prompt\n  prompt: 'Null'\n  agent: 'Null'\n",
    ),
    (
        "~",
        "plain: '~'\nlist:\n- '~'\ntagged:\n  type: inline_prompt\n  prompt: '~'\n  agent: '~'\n",
    ),
    (
        "012",
        "plain: '012'\nlist:\n- '012'\ntagged:\n  type: inline_prompt\n  prompt: '012'\n  agent: '012'\n",
    ),
    (
        "1.10",
        "plain: '1.10'\nlist:\n- '1.10'\ntagged:\n  type: inline_prompt\n  prompt: '1.10'\n  agent: '1.10'\n",
    ),
    (
        "0x10",
        "plain: '0x10'\nlist:\n- '0x10'\ntagged:\n  type: inline_prompt\n  prompt: '0x10'\n  agent: '0x10'\n",
    ),
    (
        "0o10",
        "plain: '0o10'\nlist:\n- '0o10'\ntagged:\n  type: inline_prompt\n  prompt: '0o10'\n  agent: '0o10'\n",
    ),
    (
        "1e3",
        "plain: '1e3'\nlist:\n- '1e3'\ntagged:\n  type: inline_prompt\n  prompt: '1e3'\n  agent: '1e3'\n",
    ),
    (
        "+3",
        "plain: '+3'\nlist:\n- '+3'\ntagged:\n  type: inline_prompt\n  prompt: '+3'\n  agent: '+3'\n",
    ),
    (
        ".inf",
        "plain: '.inf'\nlist:\n- '.inf'\ntagged:\n  type: inline_prompt\n  prompt: '.inf'\n  agent: '.inf'\n",
    ),
    (
        ".nan",
        "plain: '.nan'\nlist:\n- '.nan'\ntagged:\n  type: inline_prompt\n  prompt: '.nan'\n  agent: '.nan'\n",
    ),
    (
        "-",
        "plain: '-'\nlist:\n- '-'\ntagged:\n  type: inline_prompt\n  prompt: '-'\n  agent: '-'\n",
    ),
    (
        "- x",
        "plain: '- x'\nlist:\n- '- x'\ntagged:\n  type: inline_prompt\n  prompt: '- x'\n  agent: '- x'\n",
    ),
    (
        "--sandbox",
        "plain: --sandbox\nlist:\n- --sandbox\ntagged:\n  type: inline_prompt\n  prompt: --sandbox\n  agent: --sandbox\n",
    ),
    (
        ": x",
        "plain: ': x'\nlist:\n- ': x'\ntagged:\n  type: inline_prompt\n  prompt: ': x'\n  agent: ': x'\n",
    ),
    (
        "a: b",
        "plain: 'a: b'\nlist:\n- 'a: b'\ntagged:\n  type: inline_prompt\n  prompt: 'a: b'\n  agent: 'a: b'\n",
    ),
    (
        "a:b",
        "plain: a:b\nlist:\n- a:b\ntagged:\n  type: inline_prompt\n  prompt: a:b\n  agent: a:b\n",
    ),
    (
        "#x",
        "plain: '#x'\nlist:\n- '#x'\ntagged:\n  type: inline_prompt\n  prompt: '#x'\n  agent: '#x'\n",
    ),
    (
        "x # y",
        "plain: 'x # y'\nlist:\n- 'x # y'\ntagged:\n  type: inline_prompt\n  prompt: 'x # y'\n  agent: 'x # y'\n",
    ),
    (
        "x#y",
        "plain: x#y\nlist:\n- x#y\ntagged:\n  type: inline_prompt\n  prompt: x#y\n  agent: x#y\n",
    ),
    (
        " lead",
        "plain: ' lead'\nlist:\n- ' lead'\ntagged:\n  type: inline_prompt\n  prompt: ' lead'\n  agent: ' lead'\n",
    ),
    (
        "trail ",
        "plain: 'trail '\nlist:\n- 'trail '\ntagged:\n  type: inline_prompt\n  prompt: 'trail '\n  agent: 'trail '\n",
    ),
    (
        "@x",
        "plain: '@x'\nlist:\n- '@x'\ntagged:\n  type: inline_prompt\n  prompt: '@x'\n  agent: '@x'\n",
    ),
    (
        "`x",
        "plain: '`x'\nlist:\n- '`x'\ntagged:\n  type: inline_prompt\n  prompt: '`x'\n  agent: '`x'\n",
    ),
    (
        "!x",
        "plain: '!x'\nlist:\n- '!x'\ntagged:\n  type: inline_prompt\n  prompt: '!x'\n  agent: '!x'\n",
    ),
    (
        "&x",
        "plain: '&x'\nlist:\n- '&x'\ntagged:\n  type: inline_prompt\n  prompt: '&x'\n  agent: '&x'\n",
    ),
    (
        "*x",
        "plain: '*x'\nlist:\n- '*x'\ntagged:\n  type: inline_prompt\n  prompt: '*x'\n  agent: '*x'\n",
    ),
    (
        "{x}",
        "plain: '{x}'\nlist:\n- '{x}'\ntagged:\n  type: inline_prompt\n  prompt: '{x}'\n  agent: '{x}'\n",
    ),
    (
        "[x]",
        "plain: '[x]'\nlist:\n- '[x]'\ntagged:\n  type: inline_prompt\n  prompt: '[x]'\n  agent: '[x]'\n",
    ),
    (
        "%x",
        "plain: '%x'\nlist:\n- '%x'\ntagged:\n  type: inline_prompt\n  prompt: '%x'\n  agent: '%x'\n",
    ),
    (
        "|",
        "plain: '|'\nlist:\n- '|'\ntagged:\n  type: inline_prompt\n  prompt: '|'\n  agent: '|'\n",
    ),
    (
        ">",
        "plain: '>'\nlist:\n- '>'\ntagged:\n  type: inline_prompt\n  prompt: '>'\n  agent: '>'\n",
    ),
    (
        "'",
        "plain: ''''\nlist:\n- ''''\ntagged:\n  type: inline_prompt\n  prompt: ''''\n  agent: ''''\n",
    ),
    (
        "\"",
        "plain: '\"'\nlist:\n- '\"'\ntagged:\n  type: inline_prompt\n  prompt: '\"'\n  agent: '\"'\n",
    ),
    (
        "\\",
        "plain: \\\nlist:\n- \\\ntagged:\n  type: inline_prompt\n  prompt: \\\n  agent: \\\n",
    ),
    (
        "\t",
        "plain: \"\\t\"\nlist:\n- \"\\t\"\ntagged:\n  type: inline_prompt\n  prompt: \"\\t\"\n  agent: \"\\t\"\n",
    ),
    (
        "café ☕ 日本語",
        "plain: café ☕ 日本語\nlist:\n- café ☕ 日本語\ntagged:\n  type: inline_prompt\n  prompt: café ☕ 日本語\n  agent: café ☕ 日本語\n",
    ),
    (
        "2025-01-01",
        "plain: 2025-01-01\nlist:\n- 2025-01-01\ntagged:\n  type: inline_prompt\n  prompt: 2025-01-01\n  agent: 2025-01-01\n",
    ),
    (
        "2025-01-01T00:00:00Z",
        "plain: 2025-01-01T00:00:00Z\nlist:\n- 2025-01-01T00:00:00Z\ntagged:\n  type: inline_prompt\n  prompt: 2025-01-01T00:00:00Z\n  agent: 2025-01-01T00:00:00Z\n",
    ),
    (
        "line1\nline2",
        "plain: |-\n  line1\n  line2\nlist:\n- |-\n  line1\n  line2\ntagged:\n  type: inline_prompt\n  prompt: |-\n    line1\n    line2\n  agent: |-\n    line1\n    line2\n",
    ),
    (
        "line1\nline2\n",
        "plain: |\n  line1\n  line2\nlist:\n- |\n  line1\n  line2\ntagged:\n  type: inline_prompt\n  prompt: |\n    line1\n    line2\n  agent: |\n    line1\n    line2\n",
    ),
    (
        "a\n---\nb",
        "plain: |-\n  a\n  ---\n  b\nlist:\n- |-\n  a\n  ---\n  b\ntagged:\n  type: inline_prompt\n  prompt: |-\n    a\n    ---\n    b\n  agent: |-\n    a\n    ---\n    b\n",
    ),
    (
        "trailing\n\n",
        "plain: |+\n  trailing\n\nlist:\n- |+\n  trailing\n\ntagged:\n  type: inline_prompt\n  prompt: |+\n    trailing\n\n  agent: |+\n    trailing\n\n",
    ),
    (
        "\nlead",
        "plain: |2-\n\n  lead\nlist:\n- |2-\n\n  lead\ntagged:\n  type: inline_prompt\n  prompt: |2-\n\n    lead\n  agent: |2-\n\n    lead\n",
    ),
    (
        "  indented\nx",
        "plain: |2-\n    indented\n  x\nlist:\n- |2-\n    indented\n  x\ntagged:\n  type: inline_prompt\n  prompt: |2-\n      indented\n    x\n  agent: |2-\n      indented\n    x\n",
    ),
    (
        "tab\there",
        "plain: \"tab\\there\"\nlist:\n- \"tab\\there\"\ntagged:\n  type: inline_prompt\n  prompt: \"tab\\there\"\n  agent: \"tab\\there\"\n",
    ),
    (
        "ctrl\u{7}",
        "plain: \"ctrl\\a\"\nlist:\n- \"ctrl\\a\"\ntagged:\n  type: inline_prompt\n  prompt: \"ctrl\\a\"\n  agent: \"ctrl\\a\"\n",
    ),
    (
        "=",
        "plain: =\nlist:\n- =\ntagged:\n  type: inline_prompt\n  prompt: =\n  agent: =\n",
    ),
    (
        "<<",
        "plain: <<\nlist:\n- <<\ntagged:\n  type: inline_prompt\n  prompt: <<\n  agent: <<\n",
    ),
    (
        "?x",
        "plain: ?x\nlist:\n- ?x\ntagged:\n  type: inline_prompt\n  prompt: ?x\n  agent: ?x\n",
    ),
    (
        "? x",
        "plain: '? x'\nlist:\n- '? x'\ntagged:\n  type: inline_prompt\n  prompt: '? x'\n  agent: '? x'\n",
    ),
    (
        ",x",
        "plain: ',x'\nlist:\n- ',x'\ntagged:\n  type: inline_prompt\n  prompt: ',x'\n  agent: ',x'\n",
    ),
    (
        "x,",
        "plain: x,\nlist:\n- x,\ntagged:\n  type: inline_prompt\n  prompt: x,\n  agent: x,\n",
    ),
    (
        "...",
        "plain: '...'\nlist:\n- '...'\ntagged:\n  type: inline_prompt\n  prompt: '...'\n  agent: '...'\n",
    ),
    (
        "---",
        "plain: '---'\nlist:\n- '---'\ntagged:\n  type: inline_prompt\n  prompt: '---'\n  agent: '---'\n",
    ),
    (
        "a  b",
        "plain: a  b\nlist:\n- a  b\ntagged:\n  type: inline_prompt\n  prompt: a  b\n  agent: a  b\n",
    ),
    (
        "\u{feff}bom",
        "plain: \"\\uFEFFbom\"\nlist:\n- \"\\uFEFFbom\"\ntagged:\n  type: inline_prompt\n  prompt: \"\\uFEFFbom\"\n  agent: \"\\uFEFFbom\"\n",
    ),
    (
        "crlf\r\nline",
        "plain: \"crlf\\r\\nline\"\nlist:\n- \"crlf\\r\\nline\"\ntagged:\n  type: inline_prompt\n  prompt: \"crlf\\r\\nline\"\n  agent: \"crlf\\r\\nline\"\n",
    ),
];

/// `compat_tests::sample_config()` as written by `update_agent_config`.
pub const CONFIG: &str = r#"defaultAgent: codex
agents:
  codex:
    name: codex
    command: '/usr/bin/env codex --profile: work # not a comment'
    promptFlag: -p
    interactive: false
    launchArgs:
    - --sandbox
    - workspace-write
    - '- dash'
    - yes
    - '012'
  claude:
    name: claude
    command: claude
    promptFlag: null
    interactive: true
    launchArgs: []
  terminal:
    name: terminal
    command: shell
    promptFlag: null
    interactive: true
    launchArgs: null
envFiles:
- .env
- .env.local
- on
"#;

/// `GlobalAgentSettings { codex_yolo: true }` as written by `save_global_agent_settings`.
pub const GLOBAL_AGENT_SETTINGS: &str = "codexYolo: true\n";

/// `compat_tests::sample_agent_def()` as written by `save_agent_def`.
pub const AGENT_DEF: &str = r#"name: reviewer
agent_type: claude
template: 'review: strict'
inline_prompt: |
  Review the diff.

  - Check: errors
  # not a heading
    indented line
tags:
- yes
- '#tag'
- café
available_in_command_dialog: false
icon: 🔍
command: /review
"#;

/// `compat_tests::sample_swarm_def()` as written by `save_swarm_def`.
pub const SWARM_DEF: &str = r#"name: full-stack
worktree_count: 3
worktree_template: feature-{index}
roster:
- agent_def: reviewer
  role: 'review: all'
  quantity: 2
- agent_def: builder
  role: no
  quantity: 1
include_terminal: true
"#;

/// `compat_tests::sample_trigger_def()` as written by `save_trigger_def`.
pub const TRIGGER_DEF: &str = r#"name: gatekeeper
description: 'Runs: tests # always'
on: pre_commit
sequence:
- inject: |-
    /simplify
    then re-run {{PORT}}
- gate:
    run: cargo test -- --nocapture
    expect_exit: -1
  max_retries: 3
variables:
  PORT: '8080'
"#;

/// `compat_tests::sample_schedule_def(t)` for each of `sample_schedule_triggers()`,
/// in order, as written by `save_schedule_def`.
pub const SCHEDULE_DEFS: &[&str] = &[
    r#"name: nightly
enabled: false
recurrence: weekdays
start_at: 2025-01-02T03:04:05Z
next_run: 2025-01-03T03:04:05Z
trigger:
  type: agent_def
  name: reviewer
project_root: /projects/my app
target: ''
root: false
agent_name: overnight
created_at: 2024-12-31T23:59:59Z
"#,
    r#"name: nightly
enabled: false
recurrence: weekdays
start_at: 2025-01-02T03:04:05Z
next_run: 2025-01-03T03:04:05Z
trigger:
  type: swarm_def
  name: full-stack
  vars:
    N: '3'
project_root: /projects/my app
target: ''
root: false
agent_name: overnight
created_at: 2024-12-31T23:59:59Z
"#,
    r#"name: nightly
enabled: false
recurrence: weekdays
start_at: 2025-01-02T03:04:05Z
next_run: 2025-01-03T03:04:05Z
trigger:
  type: inline_prompt
  prompt: yes
  agent: codex
project_root: /projects/my app
target: ''
root: false
agent_name: overnight
created_at: 2024-12-31T23:59:59Z
"#,
    r#"name: nightly
enabled: false
recurrence: weekdays
start_at: 2025-01-02T03:04:05Z
next_run: 2025-01-03T03:04:05Z
trigger:
  type: inline_prompt
  prompt: |
    Audit deps:
      - cargo deny
      - cargo audit
  agent: claude
project_root: /projects/my app
target: ''
root: false
agent_name: overnight
created_at: 2024-12-31T23:59:59Z
"#,
];

/// Template file written by `save_template_with_command("review", "Review: strict # x",
/// "yes", "Body line\n", Some("/review"))`.
pub const TEMPLATE_FILE: &str = "---\nname: review\ndescription: 'Review: strict # x'\nagent: yes\ncommand: /review\n---\nBody line\n";
