# 52: Lucy logs the tools she calls

**What to build:** lucyd's journal doesn't show which tools the model called during a Session, or in what order. So a bug like ticket 51 can only be traced from what the user remembers. Log one line per tool call: the tool's name and its arguments. Use a level that `journalctl --user -u lucyd` shows by default.

Spec: none; found while writing up ticket 51 (2026-11-18).

**Blocked by:** none

**Seams:** none.

**Status:** needs-triage

**Notes:**

- Arguments can hold what the user said or typed. Leave out any argument a Plugin's tool marks as secret, and anything typed into the input dialog for a credential (ticket 44).

**Acceptance:**

- [ ] A test shows a tool call logs its name and arguments
- [ ] Once, by hand: after "open YouTube on workspace 3", the journal shows the `workspace` and `open_url` calls in order

## Comments
