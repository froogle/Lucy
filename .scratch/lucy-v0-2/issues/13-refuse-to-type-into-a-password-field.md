# 13: Refuse to type into a password field

**What to build:** The Omarchy Plugin refuses `type` when the focused element is a password field (AT-SPI role `password text`), and the refusal tells the model to use `ask_typed` or the Plugin's credential path. When the focused element can't be found, `type` goes ahead and the Plugin logs that the check couldn't run. The model never holds the user's secrets (they go through the Input dialog), so this stops a mistake, not a leak.

Spec: [v0.2](../spec.md), Screen access (Plugin: password fields). ADR 0008. Story 40.

**Blocked by:** 10

**Seams:** owns the `Screen` trait's focused-role method. Uses the Plugin's AT-SPI connection (owned by 10).

**Status:** resolved

**Notes:**

- Settled in a grilling session on 2026-11-17 (the Plugin owns the check, not the core; unknown focus fails open) and split out of 10 the same day.
- AT-SPI has no "what's focused" query. Follow `object:state-changed:focused` events while the Plugin runs, and fall back to searching the active window's app for `STATE_FOCUSED`.
- The log line says the check couldn't run; it never contains the text being typed.

**Acceptance:**

- [ ] Through the Plugin's MCP interface with a fake `Screen`: `type` into a password field is refused with the pointer to `ask_typed`; a plain field types; unknown focus types and logs it
- [ ] Once, by hand on Omarchy: asking her to type into a login page's password box is refused

## Comments

2026-11-18: built in slice 4 (`lucy-v0-2-slice-4`). Focus events start at the first `type` in a Plugin run, not at Plugin start, so the shipped binary in lucyd's tests stays off the real bus; that first `type` always uses the fallback search, and a listener that ends restarts at a later `type`. A remembered focus counts only when its parent chain reaches a frame titled like the focused window. The search covers that window's frames, skips hidden elements and lists that manage their own items, and stops at 4000 elements. Unknown focus types and logs without the text. Hand check left: on Omarchy, typing into a login page's password box is refused.
