# 05: A Hand-off outlives the Session

**What to build:** A Hand-off keeps working after the Session ends. When it finishes, a notification goes out and the summary is offered at the next Session start. A question it asks with no Session open is parked for the next one, and so is a permission request: a notification goes out at once, it is asked at the next Session, and the budget clock keeps running. While a Hand-off runs, the Indicator shows a "working" mark (beside Authoring's "building").

Spec: [v0.2](../spec.md), Hand-off (Results, Session while running). Stories 13, 18, 19, 51.

**Blocked by:** 01

**Seams:** owns the "working" `state` flag. Uses the keep-for-next-Session path (Authoring) and the permission tool (owned by 01).

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- Split on 2026-11-17: the limits (queue, budget, cancel) are now 11, and `continue` is 12.

**Acceptance:**

- [ ] A Hand-off survives Session end; its summary is offered at the next Session, and a parked question is asked then
- [ ] A permission request with no Session open is notified at once and asked at the next Session
- [ ] The "working" flag is in `state` while it runs (schema regenerated)

## Comments

2026-11-18: built in slice 2 (`lucy-v0-2-slice-2`). At Session end a "Claude is still working" notification goes out. Permission requests go through the keep-for-next-Session path too. The "working" mark stays on while Claude waits for an answer. Each outcome with no Session open sends one notification (review fix). Hand checks left: with real `claude`, end the Session mid-Hand-off and check the notifications and the summary at the next Session; check whether Claude's permission call survives a long wait for that Session.
