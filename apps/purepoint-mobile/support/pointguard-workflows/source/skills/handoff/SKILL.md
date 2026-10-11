---
name: handoff
description: Deliver a branch candidate with a PR, durable continuation/evidence record, accurate task status and a meaningful parent report.
---

# Handoff

Use AGENTS.md and the repository's pipeline. Handoff is delivery administration,
not another permission to work or a mandatory stop before continuing. Never
grant yourself Done, weaken criteria or publish secrets. Apply receiving branch
authority below for authorized integration; handoff alone does not request it.

1. Commit the coherent branch snapshot and record the full SHA. Branch snapshots
   may retain failing/deferred checks; list reason, remaining work, responsible
   agent and discharge point. Main acceptance still requires all applicable proof.
2. Push the branch normally; do not bypass hooks. Use `/pr` to open/update a
   normal, ready-for-review PR so external reviewers can run. Use draft only
   when the owner explicitly requests it. Report provisional scope and failing
   or deferred checks in the PR; they do not imply draft. Ready-for-review does
   not assert main acceptance or completed delivery.
3. Publish/update one Handoff page in Plans/<Epic>: objective, exact candidate,
   criteria → code/test/evidence, checks, producer SHAs/contracts/ownership,
   provisional gaps, remaining work and reproduction commands. Link task, plan,
   prompt and review pages. A continuation updates this record rather than
   starting another authorization process.
4. Keep statuses truthful: active incomplete work stays In Progress; a delivered
   candidate awaiting acceptance goes In Review. Main merge goes Merged; Done
   comes from independent acceptance evidence. Never self-Done on a push.
5. Verify live PR head/body, artifact links and task status. Send one meaningful
   direct report to the spawning parent with native `pu send`: PR, SHA, outcome
   and real blockers. Inspect delivery result; do not resend blindly or require
   acknowledgment before continuing authorized work.
6. Spawn independent reviews yourself when needed, fix findings and request
   delta review. Continue toward the whole requested outcome across context
   handoffs; unresolved in-scope work remains yours.

Return PR URL, candidate, concise change summary, truthful validation and open
obligations. `/handoff` runs this delivery flow; it never requests a merge.

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
