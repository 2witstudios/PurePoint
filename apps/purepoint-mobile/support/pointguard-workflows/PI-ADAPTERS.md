# Pi host capabilities

This package contains instructions, not integrations. It never installs software,
connects accounts or provisions credentials. Read the original workflow in `source/`
in full. User instructions and repository rules retain precedence. These adapters
translate unavailable host mechanisms; they do not change the workflow's criteria.

## Native skills and tools

Pi 1.1.0 discovers `skills/` through `package.json` → `pi.skills`, includes names,
descriptions and locations in its progressive prompt catalog, and reads a skill
only when needed. Invoke `/skill:plan`, `/skill:review`, etc. Original `/plan`,
`/orchestrate`, `/rtc` or `/aidd-*` prose is not a registered Pi command. Resolve
other bundled workflows by their native skill names. Do not create a second registry.
Use Pi `read`, `bash`, `edit` and `write` only when actually available. A workflow
request does not provide browser, computer control, web search or subagent APIs.
Use an explicitly configured tool/extension for those capabilities, or explain the
missing capability and continue independent work. Never claim a simulated action
or self-review is an independent tool execution/review. For RTC, return a concise
decision rationale and evidence, not private internal reasoning transcripts.

## Codex / Claude workflow mechanisms

The pinned `codex-skills/plan` and `codex-skills/orchestrate` express planning and
delivery intent; they do not require Codex to read them. Codex plan modes, native
todo/memory tools, Claude Task agents and plugin slash commands are not Pi tools.
Maintain a durable delivery record using the project's existing files or available
board tooling. Delegate through an available native PurePoint `pu` executable and
the repository pipeline, after inspecting `pu help`/`pu status`. This package does
not bundle `pu` or `pu-cli`; the app provides those separately. If delegation is
unavailable, continue useful work and record independent review as NOT RUN. Do not
invent workers, review identities, page IDs, successful checks or completion states.
Planning-only requests stay planning-only; missing tools do not expand authority.

## PageSpace CLI, SDK and MCP

PageSpace workflows remain discoverable even without a connected account. Check
the actual tools and `command -v pagespace` before access; read the original
`source/skills/pagespace-cli/SKILL.md` and `pagespace help` for the supported verbs.
For original in-app names such as `read_page`, `edit_sheet_cells`, `replace_lines`
or `format_sheet`, use `source/skills/pagespace-cli/references/tool-mapping.md` to
resolve the available CLI/SDK/MCP operation. A similarly named tool is not proof of
equivalence. Unsupported operations remain unavailable; do not guess a command.
Pi can use MCP only when its MCP integration and the relevant server are explicitly
configured; `pagespace mcp` is a server command, not a builtin Pi tool. The package
does not ship `@pagespace/cli`, `@pagespace/sdk`, an MCP server or credentials.
Do not auto-run `npx`/install/login/key activation to hide a missing dependency.
Preserve credential consent and capability scoping from the original workflows.

Without access, still draft documents, sheet formulas, task criteria, plans or PR
text locally when authorized. Label them unpublished; report the exact blocked
read/write/publish step. Local records cannot satisfy a required verified PageSpace
link. Never invent a drive/page ID or silently mark that requirement PASS.

## AIDD, GitHub and review

AIDD is optional and not included (see `source/NOTICE.md`). Check project `ai/skills/`
or explicitly installed skills for the referenced `aidd-*` guidance. If unavailable,
use repository rules, inspect requirements/code/tests/security, and report the gap.
For `/aidd-fix` requests, apply a focused authorized fix with actual native tools or
available `pu` delegation, preserving independent delta review as an obligation.
No fabricated AIDD execution or automatic download. The original triage README is
historical usage documentation; read its SKILL.md as the workflow authority.
GitHub operations require an available authenticated `gh` or configured equivalent;
do useful local preparation if absent and report the blocked external operation.
Follow the repository's PR, review and merge rules and the owner's protected-action
boundaries. Tool availability never grants merge or production authority.

## Relative resources and attribution

After an adapter points to `source/.../SKILL.md`, resolve that workflow's relative
references from the original skill directory. Cross-skill `pagespace-cli/references`
references use the sibling source skill tree. References to project `AGENTS.md`,
`CLAUDE.md`, `docs/` or `ai/skills/` refer to the working project. The original
Pointguard license and NOTICE are at `source/LICENSE` and `source/NOTICE.md`.
All original files, modes and relative tree layout are preserved byte for byte.
