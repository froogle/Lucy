# 04: Claude's questions during a Hand-off

**What to build:** A Hand-off's Claude can ask the user things. `ask_user {question}` is spoken once nobody is speaking. `ask_typed {question, kind}` opens the Input dialog (no `secret`). `report_progress` goes to notifications only, as in Authoring. The answer goes back to Claude, and it carries on.

Spec: [v0.2](../spec.md), Hand-off (Delegate tools). Stories 14, 15.

**Blocked by:** 01, lucy-v0-1-build 34 (Input dialog)

**Seams:** none new. Exposes `ask_user` and `report_progress` (Authoring) and `ask_typed` (owned by lucy-v0-1-build 34) to the Hand-off caller.

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.

**Acceptance:**

- [ ] `ask_user` waits until nobody is speaking, then is spoken; the answer reaches the Delegate
- [ ] `ask_typed` opens the Input dialog and returns the typed answer; `secret` is refused
- [ ] `report_progress` notifies and is never spoken

## Comments
