# 34: The Input dialog (core)

**What to build:** Lucy can ask for things typed rather than spoken: secrets such as passwords and API keys, and things that are hard to say, such as long URLs or exact spellings. She says a short line ("I've put a box on screen for your Linear API key"), an Input dialog opens, and the answer comes back without being spoken. Built-in tools, Plugins and the model all ask the same way, mid-task.

The pieces:

- **The `Typing` Session state**, with cancel phrases, the timeout and the paused silence timer.
- **The `input` socket role and its messages.** The holder draws the dialog.
- **MCP form elicitation** for Plugin questions, with a Lucy-only `secret` field limited to declared credentials. A secret goes to the Secret Service, never to the model.
- **The `ask_typed` Built-in tool** for the model's own questions (no secrets).
- **The `lucy input` page**, opened in a terminal when nothing holds the role.
- **Ticket 21's missing-credential case.** After any Confirmation, Lucy opens the dialog, stores the value and carries on with the call.

The Quickshell dialog is ticket 42.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Public IPC (Roles), Built-in tools (`ask_typed`), Session state machine (Typing, Input dialog fallback), Plugins (credentials, mid-call questions), Testing (Input dialog). Stories 153–166.

**Blocked by:** none

**Seams:** owns the `input` role, `input.open`/`input.close` events, `input.answer`/`input.cancel` requests, the `Typing` state, the `[input]` config section, the `secret` elicitation field, `ask_typed` (Session and Delegate; also used by lucy-v0-2 01 and 02) and the `lucy input` page. Uses `runner::ask`, `CredentialStore` (ticket 21) and the `Launcher` adapter.

**Status:** ready-for-agent

**Notes:**

- **Decisions from the grilling session (2026-11-16):**
  - A secret goes only to the requester or the keyring, and the model sees `provided`/`cancelled`/`timed_out`. Plain answers go to the requester.
  - Field types are MCP's plus `secret`, with no custom protocol.
  - One `input` holder at a time, the first to claim it, with no peer checks. The threat model is written down: the same user's processes can already read the unlocked Secret Service.
  - Speech in `Typing` is matched only against cancel phrases. Everything else is ignored.
  - A Barge-in doesn't close the dialog. An Indicator click or the Session ending does.
  - The timeout is 120 s by default.
- **Confirmed after the grilling session:** a Plugin question with one plain string field is still asked aloud, as today (`answers` in `plugins/mod.rs`). Only a `secret` or `uri` field, or more than one field, opens the dialog.
- The question text crosses the public socket to the `input` holder. That's allowed because it's the requester's text, never the user's speech. The answers never go out on the socket.

**Acceptance:**

- [ ] A Plugin elicitation with a `secret` field on a declared credential opens the dialog. The value lands in the credential store and reaches the Plugin as its env var. The model gets only `provided`.
- [ ] `secret` on an undeclared name is refused
- [ ] `ask_typed` (text, url, number, choice) opens the dialog and returns the answer; it refuses `secret`
- [ ] A missing credential: Confirmation first (when the tool has one), then the dialog, then the call carries on; cancel fails the call with the `lucy credentials <plugin>` pointer
- [ ] `Typing` shows in `state`; Lucy speaks the fixed opening line; the silence timer is paused; a cancel phrase closes it; other speech is ignored; the timeout returns `timed_out`; an Indicator click ends the Session and closes the dialog
- [ ] With no `input` holder, `lucy input` opens in a terminal through `xdg-terminal-exec` and answers; with no terminal, the requester gets `cancelled` and Lucy says why
- [ ] No secret appears in IPC, logs, notifications, transcripts or model traffic (`assert_nothing_private_on_bus` covers entered values)
- [ ] Schema regenerated; the `lucy input` page tested through `Home::tui`

## Comments

2026-11-16: the user wants this to be a core ability: anything in the system can open a dialog to ask for information (a URL, a password), then continue once it has the answer.
