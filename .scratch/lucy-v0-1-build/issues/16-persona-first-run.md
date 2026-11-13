# 16: Persona and first-run introduction

**What to build:** Lucy has a personality the user shapes. An editable `persona.md` in the XDG config dir goes into every Session's prompt. The default is friendly and jovial with a touch of sass, never flippant about serious news, and keeps the spoken-style rules from 04. The very first Session opens with Lucy introducing herself and asking what to call the user, and she stores the name in `persona.md`. "Be a bit more sarcastic" rewrites the file and reads the change back, with no Approval.

Spec: Persona.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Default `persona.md` is created when missing, and hand edits apply at the next Session
- [ ] First-run introduction happens exactly once and stores the name
- [ ] A Persona change by voice is read back to the user
- [ ] Nothing personal is hard-coded
