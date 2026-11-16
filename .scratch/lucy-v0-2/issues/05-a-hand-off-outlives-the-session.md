# 05: A Hand-off outlives the Session

**What to build:** A Hand-off keeps working after the Session ends. When it finishes, a notification goes out and the summary is offered at the next Session start. A question it asks with no Session open is parked for the next one. While one runs, the Indicator shows a "working" mark (beside Authoring's "building"). Other rules:

- One Hand-off at a time, with a queue of one: "Claude's still on the lucy test; want me to queue this?".
- A 60 min budget (`[handoff] budget_mins`); at the budget it is stopped, and the notification carries the resume command.
- "Cancel it" stops it. With an Authoring build also running, Lucy asks which one.

Spec: [v0.2](../spec.md), Hand-off (Results, Session while running, Limits). Stories 13, 18, 19, 22–25.

**Blocked by:** 01

**Seams:** owns the "working" `state` flag and `[handoff] budget_mins`. Uses the keep-for-next-Session path and the "cancel it" match (Authoring).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- Hand-off and Authoring queues are separate; both can run at once.

**Acceptance:**

- [ ] A Hand-off survives Session end; its summary is offered at the next Session, and a parked question is asked then
- [ ] The "working" flag is in `state` while it runs (schema regenerated)
- [ ] A second request offers to queue; a third is refused while one waits
- [ ] The budget stops it on the harness clock, with the resume command notified
- [ ] "Cancel it" stops it; with a build also running, Lucy asks which

## Comments
