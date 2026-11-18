# 55: One yes to the screen lasts until lucyd restarts and covers every screen action

**What to build:** Today Lucy asks for a Confirmation before her first look at the screen in each Session (ADR 0008). Once she is looking, she asks again before any click or key press whose `describe`, or the text under the pointer, matches `[screen] risky_words`. She also asks before any click whose target the probe can't read. So the user says yes to the screen, then gets asked again to act on it, and the whole thing starts over in the next Session. The user finds this really annoying.

Change it so that:

- The first screen tool call after lucyd starts asks one spoken Confirmation, as the first look does today.
- On a yes, every screen tool runs with no further Confirmation until lucyd restarts, in this Session and in every later one. That covers screenshots, clicks, typing, keys and scrolling, with risky words and unreadable targets included. The "looking" mark on the Indicator still shows.
- On a no, the next screen tool call asks again. A no doesn't last.
- The yes is held only in memory. It is never written to disk, so restarting lucyd always asks again.

Spec: none; raised by the user on 2026-11-18. Amend ADR 0008 (Privacy and Clicks) and the spec's screen section.

**Blocked by:** none

**Seams:** changes the screen Confirmation in `crates/lucyd/src/plugins/risk.rs` and the first-look Confirmation. Moves the screen yes from Session state to daemon state, held for the life of the lucyd process.

**Status:** needs-triage

**Notes:**

- The user chose this knowing the risk (2026-11-18). After the yes, risky-word clicks and keys need no Confirmation either. Screen content is untrusted and can carry a prompt injection, so a hostile page could get a send or a purchase through with no check. Record that tradeoff in the ADR amendment, under Considered Options.
- Decide what happens to `[screen] risky_words` and the AT-SPI probe once the yes is given. They could stop running, or stay for a later opt-in. Keep the password-field refusal: secrets still go through the Input dialog.
- `[screen] enabled = false` still leaves the screen tools out.

**Acceptance:**

- [ ] Harness test: the first screenshot asks for a Confirmation, and a yes lets a later click on a risky-word target, and one on an unreadable target, run with no Confirmation
- [ ] Harness test: a yes in one Session carries into the next Session in the same lucyd process with no new Confirmation
- [ ] Harness test: a no means the next screen tool call asks again
- [ ] Once, by hand: say yes once, use the screen in two Sessions without being asked again, restart lucyd, and get asked again

## Comments
