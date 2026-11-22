# 56: Lucy needs a memory

**What to build:** Give Lucy a Memory, separate from her Persona, so the user can say "remember that…" and she keeps it for later Sessions.

Today the only place she can keep anything is the Persona, through `update_persona`. So anything the user asks her to remember ends up in the personality file. That file should hold how she speaks and what she calls the user, not facts she's been told.

To define later:

- What goes in Memory and what stays in the Persona
- How she reads it back: all of it in every Session, or looked up when needed
- How the user sees, corrects or forgets what she remembers
- Where it's stored, and whether it needs a `GLOSSARY.md` term and an ADR

**Status:** moved to GitHub [#5](https://github.com/froogle/Lucy/issues/5)

Spec: none; raised by the user on 2026-11-20.

**Blocked by:** none
