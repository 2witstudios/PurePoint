---
name: handoff
description: Deliver a branch candidate with a PR, durable continuation/evidence record, accurate task status and a meaningful parent report.
---

# Handoff

Use AGENTS.md and the repository's pipeline. Handoff is delivery administration,
not another permission to work or a mandatory stop before continuing. Never
merge, grant yourself Done, weaken criteria or publish secrets.

1. Commit the coherent branch snapshot and record the full SHA. Branch snapshots
   may retain failing/deferred checks; list reason, remaining work, responsible
   agent and discharge point. Main acceptance still requires all applicable proof.
2. Push the branch normally; do not bypass hooks. Use `/pr` to open/update a draft
   for provisional work or an acceptance candidate for completed work. State the
   difference explicitly; a draft is not completed delivery.
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
