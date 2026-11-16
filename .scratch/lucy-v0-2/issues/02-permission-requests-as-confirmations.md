# 02: Claude's permission requests become Confirmations

**What to build:** When a Hand-off's Claude wants to do something the user's settings don't already allow, Lucy asks aloud: "Claude wants to {description}. Okay?". The full tool name and input go to a notification. A yes, matched locally against the yes-phrases, allows it. Anything else, including silence, denies it, and the deny goes back to Claude.

Spec: [v0.2](../spec.md), Hand-off (permission tool). ADR 0007. Stories 9–11.

**Blocked by:** 01

**Seams:** none new. Makes the permission tool (owned by 01) ask through `runner::ask`, as every Confirmation does.

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- The description is Claude's, not a Manifest's, so the notification with the full input is what the user can check. ADR 0007 argues why this still fits ADR 0005.

**Acceptance:**

- [ ] A permission request is spoken with Claude's description, and the full input is notified
- [ ] Yes allows; no, silence and an Indicator click deny (the click also ends the Session)
- [ ] Once, by hand: "tell Claude to fix the failing test in ~/develops/lucy", and approve one command it asks to run

## Comments
