# 11: Hand-off limits

**What to build:** A Hand-off can't run away or collide.

- One Hand-off at a time, with a queue of one: a second request is answered "Claude's still on the lucy test; want me to queue this?", and a third is refused while one waits. The Hand-off and Authoring queues are separate, and both can run at once.
- A 60 min budget (`[handoff] budget_mins`) on the harness clock. At the budget the process is stopped and the notification carries the `claude --resume` command and the folder.
- "Cancel it" stops it, matched locally. With an Authoring build also running, Lucy asks which one.

Spec: [v0.2](../spec.md), Hand-off (Limits). Stories 22–25.

**Blocked by:** 01

**Seams:** owns `[handoff] budget_mins` and the Hand-off queue. Uses the "cancel it" match (Authoring).

**Status:** resolved

**Notes:**

- Split out of 05 on 2026-11-17.

**Acceptance:**

- [ ] A second request offers to queue; a third is refused while one waits; a Hand-off and an Authoring build run together
- [ ] The budget stops it on the harness clock, with the resume command and folder notified
- [ ] "Cancel it" stops it; with a build also running, Lucy asks which

## Comments

2026-11-18: built in slice 2 (`lucy-v0-2-slice-2`). The model words the queue offer and calls `hand_to_claude` again with `queue: true` on a yes. `budget_mins` is a checked `Minutes` (bad values refuse at config load). "Cancel it" also drops the queued request and says so; with a build running, Lucy asks which and matches build, Claude or both locally; an unclear answer leaves both running, and the answer only cancels the Hand-off it asked about.
