# 32: A shorter greeting and sign-off

**What to build:** Lucy says too much at either end of a Session. When the user opens with just a greeting ("hi", "morning"), she answers with a long greeting. When they say goodbye, she gives a long sign-off. A word or two would do for both. A bare "Hey Lucy" is already stripped and gets no reply, so both come from the model following the Persona's "one to three short spoken sentences" rule. Add one rule to the shipped default Persona (Speaking section): when the user only greets you, greet back in a word or two; when they say goodbye, say a word or two ("Bye", "See ya") and call `end_session`.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Persona; stories 174–177. No code change beyond the default text.

**Blocked by:** none

**Seams:** none (the default Persona text only).

**Status:** resolved

**Notes:**

- The user's own `persona.md` is a copy of the old default, written at first run, so this change doesn't reach it. The user updates theirs by telling Lucy ("keep greetings and goodbyes to a word or two"), which also checks `update_persona` by hand. Don't edit the user's file.
- Keep the rule about manner, not exact words, so a user's Persona can still shape how she says it.

**Acceptance:**

- [ ] The default Persona has the rule; the existing Persona tests still pass
- [ ] Once, by hand after the user has told her: "hi" gets a word or two, and "bye Lucy" gets a word or two and ends the Session

## Comments

2026-11-16: settled in a grilling session. The fix lives in the Persona (it is manner), not in fixed instructions or a runtime sign-off.

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Hand check left: tell Lucy to keep greetings and goodbyes short (your `persona.md` has the old default), then try "hi" and "bye Lucy".

2026-11-16: hand-checked on the deployed build; passed.
