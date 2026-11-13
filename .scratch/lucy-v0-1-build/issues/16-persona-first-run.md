# 16: Persona and first-run introduction

**What to build:** Lucy has a personality the user shapes. An editable `persona.md` in the XDG config dir goes into every Session's prompt. The default is friendly and jovial with a touch of sass, never flippant about serious news, and keeps the spoken-style rules from 04. The very first Session opens with Lucy introducing herself and asking what to call the user, and she stores the name in `persona.md`. "Be a bit more sarcastic" rewrites the file and reads the change back, with no Approval.

Spec: Persona.

**Blocked by:** 04

**Status:** resolved

- [ ] Default `persona.md` is created when missing, and hand edits apply at the next Session
- [ ] First-run introduction happens exactly once and stores the name
- [ ] A Persona change by voice is read back to the user
- [ ] Nothing personal is hard-coded

## Comments

Resolved on `lucy-v0-1-slice-2` (0a4e21d..890dded). `persona.md` is written only when the introduction Turn completes. The `update_persona` Built-in tool was added to the spec table; changes reach the running process from the next Session.
