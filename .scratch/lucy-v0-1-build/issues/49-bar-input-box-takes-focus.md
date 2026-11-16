# 49: The bar's Input dialog takes keyboard focus

**What to build:** When a question opens the Input dialog in the bar (ticket 42), the cursor isn't in the first box. The user has to click into it, and Esc does nothing until they do, so Cancel is the only way out by keyboard. Give the first field keyboard focus when the dialog opens, so typing and Esc work at once.

Spec: [v0.1](../../lucy-v0-1/spec.md): Public IPC (Roles), Testing (Input dialog). Found in the hand check of ticket 42 (2026-11-16).

**Blocked by:** none

**Seams:** none (the bar plugin's own files).

**Status:** resolved

**Acceptance:**

- [ ] `omarchy-plugin/tests/run.sh` checks that the first field has active focus when a question arrives, and that Esc cancels without a click
- [ ] Once, by hand: "I'll type a URL for you", start typing at once, and Esc on a second box cancels it

## Comments

2026-11-16: hand-checked on the redeployed build; passed.
