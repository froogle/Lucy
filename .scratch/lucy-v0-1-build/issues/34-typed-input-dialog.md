# 34: The Input dialog in a terminal

**What to build:** Lucy can ask the user to type an answer rather than say it. The model calls `ask_typed` ("type the URL for me"). Lucy says a short fixed line, enters the `Typing` state and sends the question to whichever client holds the `input` role. With no holder, she opens `lucy input`, a `lucy` page that claims the role for one dialog, in the user's terminal through `xdg-terminal-exec`. The answer comes back to the model, and she carries on. This slice delivers the whole path for the model's own questions. Plugin questions (43), missing credentials (44) and the Quickshell dialog (42) build on it.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Public IPC (Roles), Built-in tools (`ask_typed`), Session state machine (Typing, Input dialog fallback), Testing (Input dialog). Stories 153–155, 158–162, 164–166.

**Blocked by:** None (can start immediately)

**Seams:** owns the `input` role, the `input.open`/`input.close` events, the `input.answer`/`input.cancel` requests, the `Typing` state, the `[input]` config section, the `ask_typed` Built-in tool (also used by 43, and by lucy-v0-2 04 as a Delegate tool) and the `lucy input` page. Uses `runner::ask` and the `Launcher` adapter.

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16); split into tracer slices by `to-tickets` the same day.
- **Field kinds in this slice:** `ask_typed` takes text, url, number and choice. It never takes `secret`: the model can't ask for a secret for itself. Still, the role's messages carry a field kind from the start, so 43 can add `secret` without changing the protocol shape.
- **The role:** one `input` holder at a time, the first to claim it, with no peer checks. The threat model is written down: the same user's processes can already read the unlocked Secret Service.
- **The question text:** it crosses the socket to the holder because it's the requester's text, never the user's speech. Answers come back only on `input.answer`, and never go out on the socket.
- **While `Typing`:**
  - The silence timer is paused.
  - Speech is matched locally against `[input] cancel_phrases` only, and everything else said is ignored.
  - A Barge-in doesn't close the dialog.
  - An Indicator click, or the Session ending, closes it and ends the Session.
  - `[input] timeout_secs` defaults to 120.
- **Opening line:** fixed, built from the request, for example "I've put a box on screen for the URL".
- **New model text:** model-facing text starts with a `model_text` marker (see `working-in-lucy.md`).

**Acceptance:**

- [ ] `ask_typed` (text, url, number, choice) opens the dialog through a harness client holding `input`, and returns the answer; it refuses `secret`
- [ ] `Typing` shows in `state`; the opening line is spoken; the silence timer is paused; a cancel phrase returns `cancelled`; other speech is ignored; the timeout returns `timed_out`; an Indicator click ends the Session and sends `input.close`
- [ ] A second client can't claim `input` while one holds it; answers from a client without the role are refused
- [ ] With no holder, `lucy input` is opened through the `Launcher` and answers; with no terminal, the requester gets `cancelled` and Lucy says why
- [ ] The `lucy input` page is tested through `Home::tui`; the schema is regenerated; `assert_nothing_private_on_bus` passes with an answer typed
- [ ] Once, by hand with the bar plugin not holding the role: "Lucy, I'll type a URL for you" opens a terminal box, and she opens the URL typed

## Comments

2026-11-16: the user wants this to be a core ability: anything in the system can open a dialog to ask for information (a URL, a password), then continue once it has the answer.

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Hand check left: "Lucy, I'll type a URL for you" with the bar plugin not holding `input`. Review fixes: the box opens after the line, `lucy input` closes on `input.close`, and an unclaimed launch counts as no terminal after `CLAIM_WAIT` (5 s).
