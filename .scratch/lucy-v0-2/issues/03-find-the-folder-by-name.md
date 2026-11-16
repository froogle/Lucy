# 03: Find the Hand-off's folder by name

**What to build:** "Tell Claude to fix the failing test in lucy" finds `~/develops/lucy`. lucyd searches `[handoff] roots` (default `~`, three levels deep), git repos first. Several matches mean Lucy asks which one; none means she says so and nothing starts. Absolute and `~` paths are still used as given.

Spec: [v0.2](../spec.md), Hand-off (folder resolution). Stories 4, 5.

**Blocked by:** 01

**Seams:** owns `[handoff] roots`. Uses `hand_to_claude` (owned by 01).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.

**Acceptance:**

- [ ] One match is used; several are asked about by name; none is said plainly and nothing starts (harness, with a temp home)
- [ ] Git repos rank before plain folders; the search stops at three levels
- [ ] Once, by hand: "tell Claude to … in lucy" runs in `~/develops/lucy`

## Comments
