# 13: Refuse to type into a password field

**What to build:** The Omarchy Plugin refuses `type` when the focused element is a password field (AT-SPI role `password text`), and the refusal tells the model to use `ask_typed` or the Plugin's credential path. When the focused element can't be found, `type` goes ahead and the Plugin logs that the check couldn't run. The model never holds the user's secrets (they go through the Input dialog), so this stops a mistake, not a leak.

Spec: [v0.2](../spec.md), Screen access (Plugin: password fields). ADR 0008. Story 40.

**Blocked by:** 10

**Seams:** owns the `Screen` trait's focused-role method. Uses the Plugin's AT-SPI connection (owned by 10).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-17 (the Plugin owns the check, not the core; unknown focus fails open) and split out of 10 the same day.
- AT-SPI has no "what's focused" query. Follow `object:state-changed:focused` events while the Plugin runs, and fall back to searching the active window's app for `STATE_FOCUSED`.
- The log line says the check couldn't run; it never contains the text being typed.

**Acceptance:**

- [ ] Through the Plugin's MCP interface with a fake `Screen`: `type` into a password field is refused with the pointer to `ask_typed`; a plain field types; unknown focus types and logs it
- [ ] Once, by hand on Omarchy: asking her to type into a login page's password box is refused

## Comments
