# 55: One yes to the screen lasts until lucyd restarts and covers every screen action

**What to build:** Today Lucy asks for a Confirmation before her first look at the screen in each Session (ADR 0008). Once she is looking, she asks again before any click or key press whose `describe`, or the text under the pointer, matches `[screen] risky_words`. She also asks before any click whose target the probe can't read. So the user says yes to the screen, then gets asked again to act on it, and the whole thing starts over in the next Session. The user finds this really annoying.

Change it so that:

- The first screen tool call of any kind (look, click, type, key or scroll) after lucyd starts asks one spoken Confirmation. Today only a look asks; a click with no look before it skips the question.
- The question says how far the yes reaches, along the lines of "I need to look at and use your screen. Okay? That lasts until Lucy restarts."
- On a yes, every screen tool runs with no further Confirmation until lucyd restarts, in this Session and in every later one. The "looking" mark on the Indicator still shows in a Session where a screen tool has run, as today; there is no standing mark for the yes.
- On a no, the next screen tool call asks again. A no doesn't last. No answer (timeout, Barge-in, Session ending) counts the same: nothing is remembered.
- The yes is held only in memory, on `Host`, not in the per-Session `SessionTools`. It is never written to disk, so restarting lucyd always asks again. A restart is the only way to take it back; there is no revoke command.
- Remove the risk machinery, which can no longer gate anything: `[screen] risky_words`, `risk::check`, `risk::under_click` and `Risk::Undescribed`, the core's probe call before clicks, the offer to turn a browser's accessibility on, the `probe` tool in `lucy-omarchy`, and `describe` on `click` and `key`.
- Keep the password-field refusal in `lucy-omarchy`'s `type` tool: secrets still go through the Input dialog.
- `[screen] enabled = false` still leaves the screen tools out.
- A manifest's `confirm = true` on a screen tool still asks for its Confirmation. That is the Plugin author's call, not part of ADR 0008. No shipped screen tool sets it.

Spec: none; raised by the user on 2026-11-18. Amend ADR 0008 (Privacy and Clicks), the v0.2 spec's screen section, and any `GLOSSARY.md` wording that leans on the risk list.

**Blocked by:** none

**Seams:** changes the screen Confirmation in `crates/lucyd/src/plugins/mod.rs` (`look`, `call`) and removes `crates/lucyd/src/plugins/risk.rs`'s screen checks. Moves the screen yes from Session state to daemon state (`Host`), held for the life of the lucyd process. Changes the `[screen]` config section and the `lucy-omarchy` screen tool schemas.

**Status:** ready-for-agent

**Notes:**

- The user chose this knowing the risk (2026-11-18). After the yes, a click or key that sends, buys or deletes needs no Confirmation. Screen content is untrusted and can carry a prompt injection, so a hostile page could get a send or a purchase through with no check. Record that tradeoff in the ADR amendment, under Considered Options, along with the choice to delete the risk list and probe rather than keep them dormant for a later opt-in (git history has them).
- `[screen]` doesn't deny unknown keys, so a config that still sets `risky_words` keeps loading.
- Existing harness tests for risky words, the probe, unreadable targets, the accessibility offer and `describe` refusals go with the code they test.

**Acceptance:**

- [ ] Harness test: the first screen tool call asks for a Confirmation, whether it is a look or a click with no look before it
- [ ] Harness test: after a yes, clicks, typing and keys run with no Confirmation, including a click whose target says "send"
- [ ] Harness test: a yes in one Session carries into the next Session in the same lucyd process with no new Confirmation
- [ ] Harness test: a no, or no answer, means the next screen tool call asks again
- [ ] Harness test: typing into a password field is still refused
- [ ] Once, by hand: say yes once, use the screen in two Sessions without being asked again, restart lucyd, and get asked again

## Comments

- 2026-11-18, grilling: any screen tool triggers the one question; the question names its reach; delete the risk list, probe, accessibility offer and `describe` outright; a manifest `confirm = true` is still honoured; restart is the only revoke; no answer counts as a no; the Indicator stays as it is. Moved to ready-for-agent.
