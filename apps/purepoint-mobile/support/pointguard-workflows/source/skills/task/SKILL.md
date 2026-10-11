---
name: task
description: Plan, sequence and track an owner-authorized delivery, including provisional branch dependencies and independently reviewed main acceptance.
---

# Task workflow

Read AGENTS.md and the relevant product/architecture documents. A request to
implement an outcome authorizes planning, tasking, spawning and execution toward
that outcome; do not request another approval for routine steps. A planning-only
request produces a plan and discussion, not implementation. Human-only actions
and changes to product intent still go to the owner. Use the repository's agent
pipeline when present.

Record work in the project's PageSpace drive. Missing board plumbing does not
revoke a direct instruction: continue independent branch work, record the
administrative gap and reconcile it before handoff. Never invent IDs or copy
credentials. Existing criteria remain intact; refinements to execution do not
silently shrink the requested outcome. Agent-created follow-ups remain part of
the original deliverable.

## Plan and manifest

Resume an existing plan; do not duplicate it. Read relevant Issues and accepted
ADRs. Record owner direction and provisional decisions. Maintain a manifest:

| Leaf | Outcome / criteria | Owner | Build inputs | Accepts after | Active shared writer | Page ID |
| ---- | ------------------ | ----- | ------------ | ------------- | -------------------- | ------- |

Build inputs are implementations or facts needed for the next proof, not global
completion statuses. An actual producer at a pinned unmerged SHA can satisfy
availability: record source SHA, contract, source owner, integration SHA and gaps.
A stub or branch name does not prove producer availability. Proposed policy can
be explored in an isolated branch without becoming accepted product authority.
Unavailable producers block dependent integration proof, not independent work.

Accepts after records prerequisites for completion/main, including composed
contract proof and human activation gates. Neither integration nor cherry-pick
marks a producer globally Done. Default-branch presence, separate producer review
and global Done are not branch build prerequisites.

Derive a dependency order from actual build inputs: reject unknown task codes and
cycles; topologically sort, with manifest order breaking ties. Mark acceptance
obligations separately. Coordinate live concurrent writers of the same file or
resource; migration generation is serialized. Do not serialize every reader of a
package or every independent route merely because the concept is shared. Stages
are useful planning views, not barriers to independent progress. No task dates.

## Independent plan review

For consequential or complex changes, request a read-only plan review using the
repository command or an independent reviewer. Review missing inputs, actual
writer conflicts, security/product boundaries and proof coverage. Unmerged inputs
are provisional risks to reconcile, not automatic rejection. Apply findings and
continue within existing authorization; do not introduce a reviewed-plan approval
relay when implementation was already requested. A planning-only request stops
at the reviewed plan as requested.

## Task administration

Create an epic/phase/leaf structure proportional to the work, with Given/should
criteria and Related pages linking plan, prompts and reviews. Size leaves by a
coherent provable result; no mandatory file-count, line-count or one-PR-per-leaf
rule. One composed PR may deliver several leaves.

Claim available work and keep status accurate. Ready means the agent can make
useful progress in its branch, not that every acceptance prerequisite is Done.
Do not mark human-only leaves Ready for agent execution. Agents may create and
claim in-scope leaves and review tasks without parent reauthorization. Preserve
other delegated criteria; owner steering can supersede them explicitly, with the
change recorded. Status beyond Ready belongs to the executing agent and review
workflow; Done needs independent acceptance evidence.

Keep task bodies for criteria and navigation; keep the objective, dependency
SHAs, provisional choices, remaining work and check obligations in the plan or
delivery record. Record each failed/deferred check with candidate, reason,
responsible agent, remaining proof and discharge point. Continue work that does
not depend on the missing evidence. Run focused proofs now and full applicable
checks on the composed candidate before main acceptance. Do not repeat expensive
checks while their known blocking inputs are unchanged.

## Continue, review and deliver

Builders may spawn their own independent reviewers through native PurePoint,
fix findings, request delta review and continue without owner relays or pass
limits. Code-writing children get their own worktrees; reviewers stay read-only.
Use stable commit snapshots; freeze a mutable worktree only while a reviewer is
actually using it. Branch feedback records cannot substitute for main approval.

Update the plan/manifest when execution changes. Replan independently within the
authorized outcome; bring scope cuts, conflicting product choices and protected
actions to the owner. Send meaningful milestones, final delivery, genuine blockers
or writer conflicts directly to the spawning parent; no routine acknowledgment
loop. At handoff link exact candidate, truthful tests, open obligations and
independent reviews. Main acceptance discharges all applicable proof obligations;
production activation retains human gates.

Commands: `/task <outcome>` plans/tasks within authorization; `/task sync` records
progress and available work; `/task replan` updates the execution manifest;
`/task validate` checks graph, IDs, ownership, criteria and proof obligations.

## Receiving branch authority

Within owner-authorized work, owners, point guards, root/main-level agents and
worktree agents may merge producers into their own allocated unprotected
non-main receiving branch. Root agents use an isolated receiving checkout, never
the parent-main checkout. Coordinate actual writers/resources; ordinary merges
and conflict resolution are allowed, never another agent's checkout/branch,
force-push, history rewrite, reset or dirty-work loss. Resolve symbolic default
and live branch protection/rulesets before acting; main, default and protected
release targets retain acceptance/human protections, other protected targets
follow their policy, and unknown protection facts refuse integration. Before PR
merge automation, re-read live repository/base/head and verify the candidate
and intended delivery target: the allocated receiving branch for producer
integration, or the repository's resolved default branch for main acceptance.
Compare the live base with that selected target; refuse mismatches or changed
targets.

Non-main integration needs no global Done, separate producer approval, green
whole-app CI or per-step root permission. Pin source/integration SHAs and gaps,
record failed/deferred checks honestly, and preserve security/tests. One
short-lived integration branch and umbrella PR may compose children before they
are main-ready. Integration grants neither Done nor main acceptance. Autonomous
agents never merge directly into main/default/protected release targets; request
main auto-merge only under the live required review-record ruleset and applicable
checks, otherwise report ready for owner merge. Production retains human gates.
