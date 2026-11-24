# Memory sits beside the Persona, is read back whole, and is kept only on request

Lucy needs to keep facts the user tells her ("remember my sister is called Anna") across Sessions. Until now the only place she could keep anything was the Persona, so facts ended up mixed into how she speaks. Memory is a second file beside the Persona, `~/.config/lucy/memory.md`: a Markdown list, one fact per bullet, each starting with the date it was saved. The Persona keeps her manner and what she calls the user; Memory holds every other fact.

At each Session start lucyd puts the whole of Memory into her instructions, after the Persona and before the Plugin scan report, under a heading that says it is what the user asked her to remember. A missing or empty file adds nothing; a file that can't be read is logged and the Session starts without it.

Two Built-in tools change it, and neither asks a Confirmation (as ADR 0010 holds for the Persona): `remember` adds one bullet, dated by lucyd rather than the model, and `forget` removes every bullet whose text matches exactly. A correction is a `forget` then a `remember`. She says what she saved or forgot, which is the user's check, and she saves only when the user asks her to. There is no tool to forget everything: deleting the file does that.

## Considered Options

- Keeping facts in the Persona: mixes what she knows with how she speaks, and every fact costs a rewrite of the whole Persona.
- A lookup tool instead of reading Memory back whole: scales, but she has to think to look, and a missed lookup feels like forgetting. A personal Memory should stay small.
- One `update_memory` tool rewriting the whole file, like `update_persona`: one slip drops facts, and Memory only grows.
- Saving whatever she judges worth keeping: with no Confirmation the user would never know what went in.
- A structured file or a database: the user couldn't read and edit it as easily as `persona.md`.
