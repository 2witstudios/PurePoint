# Spec Advancement Rules

Imperative instructions for advancing Architecture and Product specs through maturity levels.

## Maturity Transitions

| From | To | Gate | Agent Action |
|---|---|---|---|
| SEED | EXPLORING | First research note added | Research at least one open question; add findings to Research Notes |
| EXPLORING | CONVERGING | Key questions have candidate answers | Each open question has 1-2 researched options with trade-offs documented |
| CONVERGING | DECIDED | Author/team picks a direction | Convert `?` to `!` for each question with rationale |
| DECIDED | SPECIFIED | Full requirements, interfaces, edge cases | Write Given/should requirements, sum sheets, interface definitions, edge cases |

## Process

### 1. Assess the Spec

Before starting, read the spec and evaluate:
- What open questions exist? (look for `? [DOMAIN-NNN]`)
- What sections are missing? (check against the completeness checklist in spec-system.md)
- What maturity level is it at, and what's needed for the next level?

### 2. Research Open Questions

For each open question:
1. **State the question clearly** — what exactly needs to be decided?
2. **Identify constraints** — what do other specs, architecture decisions, or technical realities constrain?
3. **Research options** — investigate 2-3 viable approaches. Sources: existing codebase (reference inventory), similar systems, technical documentation, trade-off analysis.
4. **Document findings** — append to the spec's Research Notes section:

```markdown
### Research Notes

#### [DOMAIN-NNN] Question text
**Researched: {date}**

Option A: {description}
- Pro: {advantage}
- Con: {disadvantage}

Option B: {description}
- Pro: {advantage}
- Con: {disadvantage}

Recommendation: {option} — {brief rationale}
```

### 3. Add Missing Content

Each maturity level has minimum content requirements (see spec-system.md). Fill gaps:
- **SEED**: Ensure purpose, conceptual model, and at least 2 open questions exist
- **EXPLORING**: Add research notes for each open question
- **CONVERGING**: Narrow to 1-2 options per question with clear trade-offs
- **DECIDED**: Convert all `?` to `!` with rationale
- **SPECIFIED**: Write requirements (Given/should format), interfaces, edge cases, sum sheet

### 4. Propose the Advancement

When the gate criteria are met:
1. Update the maturity level in the spec header
2. In supervised mode: present the changes for approval before committing
3. In autonomous mode: commit the advancement with a clear commit message

## Constraints

- Advance one maturity level at a time — no skipping levels
- Every `!` decision must include rationale after the `—` separator
- Research notes must cite sources or reasoning — no unsupported claims
- Don't invent requirements — derive them from the purpose, conceptual model, and decisions
- Cross-check dependencies: if advancing Spec A, verify consistency with specs it depends on (check the Dependencies section in the spec header)
- If research reveals a new open question, add it as `? [DOMAIN-NNN]` — this is progress, not failure

## Parallel Spec Advancement

When multiple agents advance different specs simultaneously:
- Each agent owns a specific spec — no two agents advance the same spec concurrently
- If Agent A discovers information relevant to Agent B's spec, record it in a structured finding (see Agent Communication Protocol in spec-system.md) rather than writing directly to the other spec
- Check dependency specs before finalizing: if your spec depends on another that's also being advanced, coordinate through the conductor
