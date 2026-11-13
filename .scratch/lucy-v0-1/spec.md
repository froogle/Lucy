# Lucy v0.1

Status: ready-for-agent

Synthesised from [map.md](map.md), resolved tickets 01–22, `GLOSSARY.md` and ADRs 0001–0006. Vocabulary follows the glossary (Session, Turn, Barge-in, Wake word, Closing phrase, Indicator, Provider, Model tier, Plugin, Manifest, Approval, Confirmation, Built-in tool, Delegate, Authoring, Persona, Source).

## Problem Statement

A Linux desktop user who already pays for Claude (or ChatGPT) subscriptions has no voice assistant that fits how they work. Commercial assistants are closed, cloud-bound and phone-shaped. They can't act on the user's own machine. They can't grow new abilities, and they ignore the subscriptions the user already has. The usual alternative is pay-per-use voice and model APIs, which put a running meter on something meant to be always available. When an assistant meets a request it can't handle, it gives up or asks the user to explain how to wire things together. Assistants that can act often hide the safety gate inside the model, so a mishearing or a prompt injection can approve a risky action.

## Solution

Lucy is a voice-driven desktop assistant for Linux (Hyprland/Wayland first). Saying "Hey Lucy" opens a Session. The user talks naturally and can cut in on her at any time. A small clickable Indicator in the bottom-right corner shows what she is doing. She answers in a Persona the user shapes, opens links and apps, answers system questions, and gives a spoken news briefing from official feeds.

Her thinking runs on the subscription CLIs the user already has (`claude`, `codex`). Speech-to-text, the wake word and speech run locally. Every capability is a pluggable Provider, set up through a TUI.

Her abilities are Plugins: small separate programs, each described by a Manifest. When no Plugin fits a request, Lucy works out what only the user can tell her. She then hands the goal to a sandboxed Delegate that researches, writes and tests a new Plugin on its own. Before the new Plugin can run, she asks the user's spoken Approval. If the platform can't be reached with permitted access, she says so plainly and offers the best permitted fallback. Lucy's runtime hears Approvals and Confirmations itself, never the model.

v0.1 succeeds when (a) the news briefing works end to end as a hand-written reference Plugin, and (b) "find my Mount Rainier photo" works both as an honest-limit check (Google Photos only) and as a Plugin Lucy writes herself over a local Takeout folder.

## User Stories

### Waking and Sessions

1. As a user, I want to say "Hey Lucy" to open a Session, so that I can talk to her hands-free.
2. As a user, I want the Wake word detected entirely on my machine, so that nothing I say leaves it before I address her.
3. As a user, I want "Hey Lucy, what's the time" in one breath to work, so that I don't have to pause after the Wake word.
4. As a user, I want a short start sound and the Indicator to appear when a Session opens, so that I know she is listening.
5. As a user, I want a false wake to end on its own after 8 s with no speech, so that stray triggers clear quickly.
6. As a user, I want to change the Wake word by pointing config at another wake-word model file, so that I can call her something else.
7. As a user, I want a published plain "Lucy" model I can switch to, so that I can accept more false triggers in exchange for a shorter phrase.
8. As a user, I want a "train your own" doc, so that I can make a model for any phrase.
9. As a user, I want a leading "Hey Lucy" said within a Session stripped from what she hears, so that saying her name mid-conversation doesn't confuse her.
10. As a user, I want only one Session at a time, so that she never talks over herself.
11. As a user, I want the Session to stay open across many Turns, so that I can have a real back-and-forth with follow-ups.
12. As a user, I want to end a Session by saying something like "that's all, thanks", understood from meaning, so that I don't have to remember a command.
13. As a user, I want her to finish her goodbye before the Session closes, so that the ending feels natural.
14. As a user, I want to end a Session instantly by clicking the Indicator, from any state, so that I always have a physical off switch.
15. As a user, I want the Session to end quietly after about 20 s of silence while she is listening (configurable), with a soft sound and the Indicator fading, so that she doesn't linger.
16. As a user, I want the silence timer paused while she is thinking, speaking or confirming, so that slow answers don't time out.
17. As a user, I want `lucy listen` and `lucy end` commands, so that I can bind a key to start and stop Sessions.

### Conversation and latency

18. As a user, I want her first audio within about 2.5 s of when I stop talking, so that the conversation feels natural.
19. As a user, I want her to speak each sentence as soon as it is ready, so that long answers start quickly.
20. As a user, I want short spoken replies (one to three sentences, no markdown), so that answers sound like speech, not a document.
21. As a user, I want her to say a short "Let me check" before slow work, so that silence doesn't feel like a hang.

### Barge-in

22. As a user, I want to cut in while she is speaking just by talking, so that I don't have to wait for her to finish.
23. As a user, I want her voice to duck the moment I start talking and stop once I keep going, so that cutting in feels immediate.
24. As a user, I want saying "Hey Lucy" while she speaks to interrupt instantly, so that I have a guaranteed way to cut in.
25. As a user with poor echo cancellation, I want to limit Barge-in to the Wake word only, so that her own voice doesn't interrupt her.
26. As a user, I want her own voice cancelled from what she hears, so that she doesn't interrupt herself on laptop speakers.
27. As a user, I want her to know how much of her reply I actually heard when I cut in, so that her next answer picks up sensibly.
28. As a user, I want a cough or empty noise during her speech to just return her to listening, so that small sounds don't derail things.
29. As a user, I want speaking while she is thinking to count as a Barge-in, so that I can correct myself before she answers.
30. As a user, I want an interrupted action I already confirmed to still finish and be reported, so that a confirmed send isn't left half-done.
31. As a user, I want unconfirmed actions in flight cancelled when I cut in, so that interrupting really stops her.

### Indicator

32. As a user, I want a small marker in the bottom-right corner for the whole Session, so that I always know whether she is listening.
33. As a user, I want the Indicator to look different for listening, thinking, speaking and confirming, so that I can tell her state at a glance.
34. As a user, I want the speaking Indicator to move with her voice level, so that it feels alive.
35. As a user, I want the Indicator to use my Omarchy theme colours and follow theme changes, so that it matches my desktop.
36. As a user, I want the Indicator never to steal focus or block clicks around it, so that it doesn't get in my way.
37. As a user, I want a brief red flash on errors, so that I notice when something went wrong.
38. As a GNOME or headless user, I want start, Confirming and end shown as notifications instead, so that Lucy still works without layer-shell.
39. As a user, I want a crash in the Indicator not to affect the voice loop, so that a drawing bug can't break my Session.
40. As a widget author, I want to replace the Indicator with my own by claiming the `indicator` role on the socket, so that I can build a richer bar widget.

### Built-in tools

41. As a user, I want to say "open github.com" and have it open in my browser, so that I can navigate by voice.
42. As a user, I want to say "open Firefox" and have it launch by its desktop-entry name, generic name or keywords, so that I can start apps by voice.
43. As a user, I want her to ask which one I mean when several apps match, so that she doesn't guess wrong.
44. As a user, I want her to tell me when no app matches, so that I know it isn't installed.
45. As a user, I want apps launched through uwsm when it is running, so that they behave like apps I launch myself.
46. As a user, I want to ask about battery, disk, memory/CPU, network, volume, time and uptime, so that I can check my machine by voice.
47. As a user, I want to enable, disable, roll back and remove Plugins by voice, so that I can manage her abilities without a screen.
48. As a user, I want removing a Plugin to need a spoken Confirmation, so that I don't delete one by accident.

### Confirmation

49. As a user, I want risky actions (send, delete, spend) to need my spoken yes first, so that she never does them on her own.
50. As a user, I want my yes heard by Lucy herself, not by the model, so that a mishearing or a prompt injection can't fake it.
51. As a user, I want a specific prompt like "Send an email to Sam about lunch?", so that I know exactly what I'm agreeing to.
52. As a user, I want the full arguments of a risky action shown in a notification, so that I can check the details.
53. As a user, I want anything other than a clear yes, including 8 s of silence, to mean no, and to hear "Okay, I won't", so that the safe outcome is the default.
54. As a user, I want a "yes" said while she is still asking to count, so that I don't have to wait for the prompt to end.
55. As a user, I want to edit the list of yes-phrases in config, so that my natural "yep" or "go for it" works.
56. As a user, I want an Indicator click during Confirming to decline and end the Session, so that clicking is always a safe exit.

### News briefing

57. As a user, I want to ask "what's the news?" and get a greeting by name plus the top one or two stories in one or two sentences, so that I get the gist fast.
58. As a user, I want the news read only from official feeds of Sources whose terms allow AI summaries, so that Lucy stays within each site's rules.
59. As a user, I want CNN, NPR and Sky News as the default rotation, so that it works out of the box.
60. As a user, I want the front pages of the Sources she spoke from opened in my browser, so that I can read further.
61. As a user, I want "show me that story" to open that article, so that I can jump straight to it.
62. As a user, I want "don't open anything" respected, so that she doesn't spam tabs.
63. As a user, I want a Source I like but she can't read to get a cheerful "their rules, not mine" plus an offer to open its page, so that I still get to it.
64. As a user, I want "I'm not into Fox, try something else" to swap in a suitable Source and tell me what changed, so that I can tune the briefing by talking.
65. As a user, I want news preferences to persist across Sessions, so that I don't repeat myself.
66. As a user, I want a detailed roundup of about two minutes on request, built from feed descriptions only, so that I can get more without her reading article pages.
67. As a user, I want Barge-in to cut a long roundup short, so that I can stop it any time.
68. As a user, I want serious news never treated flippantly, so that the Persona's sass stays appropriate.

### Persona

69. As a new user, I want Lucy to introduce herself in the first Session and ask what to call me, so that she knows my name from then on.
70. As a user, I want a default Persona that is friendly and jovial with a touch of sass, so that she has some character.
71. As a user, I want to change her manner by telling her ("be a bit more sarcastic"), with the change read back, so that I can shape her without editing files.
72. As a user, I want her Persona in an editable markdown file, so that I can fine-tune it by hand.

### Plugins and Manifests

73. As a user, I want each ability to be a separate Plugin with a short, readable Manifest, so that I can see what each one does and can access.
74. As a user, I want Lucy to pick the right Plugin from my request by reading Manifests, so that I don't have to name tools.
75. As a user, I want her to ask a clarifying question when a request is ambiguous, and to ask which one when two Plugins fit equally, so that she picks correctly.
76. As a user, I want every new or changed Plugin, including shipped and read-only ones, to need my Approval, so that nothing gains access without my yes.
77. As a user, I want a new Plugin's Approval to tell me what it handles, what it can access and what it will ask before doing, so that I make an informed choice.
78. As a user, I want an updated Plugin's Approval to tell me only what changed, with the full diff on screen, so that re-approving is quick.
79. As a user, I want Approvals stored where no Plugin can write, and pinned to exact content, so that no Plugin can approve itself or slip in a change.
80. As a user, I want the previous approved version kept so that I can roll back by voice with no new Approval, so that a bad update is easy to undo.
81. As a user, I want the current version to keep working until I approve its update, so that updates never leave me without the ability.
82. As a user, I want a Plugin's file access enforced by the sandbox, not just declared, so that it can only touch what it says.
83. As a user, I want a Plugin's network hosts and credential names shown to me, so that I know what it reaches out to.
84. As a user, I want a broken Plugin left out of selection, with her saying plainly what failed, so that failures are visible and contained.
85. As a user, I want Lucy to offer to fix her own broken Plugins, so that they recover without me.
86. As a user, I want Lucy never to edit a human-written Plugin, only describe the change or offer her own replacement, so that my code stays mine.
87. As a user, I want Plugins I drop into the Plugins folder by hand to be found, shown as awaiting Approval, and mentioned once at my next Session, so that installing one is easy.
88. As a user, I want my own copy of a shipped Plugin to override the shipped one, so that I can customise it.
89. As a user, I want shipped Plugins to be disableable but not removable, so that I can't break the install.
90. As a user, I want Plugins to stop when the Session ends, so that nothing runs in the background.
91. As a Plugin author, I want to write a Plugin in any language that speaks MCP over stdio, under any licence, so that I'm not tied to Rust.
92. As a Plugin author, I want a template folder (a minimal MCP stdio server, a Manifest skeleton, a fixtures test harness), so that I can start quickly.
93. As a Plugin author, I want the news Plugin as a worked reference, so that I can copy a real example.
94. As a Plugin author, I want each Plugin to get a private writable state folder, so that it can keep preferences and tokens.
95. As a Plugin author, I want credentials my Plugin declares by name to arrive as environment variables, stored in the system keyring and entered by the user in the TUI or a desktop prompt, so that my Plugin never handles the user's secrets in plain files.
96. As a user, I want a Plugin tool to be able to ask me something mid-call, so that it can confirm details as it goes.

### Authoring

97. As a user, I want Lucy to build a Plugin herself when none fits my request, so that she gains new abilities by being asked.
98. As a user, I want her to ask only what only I can answer (where my photos are, which platform, a sign-in), never how to connect, so that I'm not doing her job.
99. As a user, I want her to say once "This is going to take a few minutes" and keep the Session open, so that I can ask other things meanwhile.
100. As a user, I want build progress shown as notifications and in the TUI but never spoken, with a setting to switch the notifications off, so that progress doesn't interrupt me.
101. As a user, I want the Indicator to show a "building" mark while a build runs, so that I know work is going on.
102. As a user, I want her questions and the Approval to wait until neither of us is speaking, so that she doesn't cut me off.
103. As a user, I want the authoring Delegate sandboxed, with no access to my data before Approval, so that unapproved code never sees my files.
104. As a user, I want the new Plugin built and tested before I'm asked to approve it, so that I only approve working code.
105. As a user, I want her to answer my original request right after I approve, and ask "Is that the one?", so that the build ends with the result I wanted.
106. As a user, I want a "no" to "Is that the one?" to send it back for a fix, so that she gets it right.
107. As a user, I want her to tell me plainly when a platform can't be reached with permitted access, and offer the best permitted fallback (for example opening Google Photos search), so that I'm never misled.
108. As a user, I want a build to stop after a 20 min budget (configurable) and keep its work so that "try again" resumes, so that a stuck build doesn't run forever.
109. As a user, I want a second build request queued with "I'm already building the photos Plugin", so that builds don't collide.
110. As a user, I want a build to keep running if the Session ends, with a notification when done and the Approval offered at my next Session, so that I don't have to wait around.
111. As a user, I want "cancel it" to stop a build, so that I can call it off.
112. As a user, I want a question the Delegate parked raised at my next Session, so that builds can continue later.

### Providers and setup

113. As a user with a Claude subscription, I want Lucy's thinking to run through my own `claude` login, so that I pay nothing extra.
114. As a user with ChatGPT, I want to select Codex instead, so that I can use the subscription I have.
115. As a user, I want to choose a fast tier and an advanced tier separately, with recommended models, so that everyday talk is quick and heavy work is capable.
116. As a user, I want Lucy to strip API-key settings that would outrank my subscription, so that I'm never billed by accident.
117. As a user who does want API billing, I want a per-Provider opt-out of that guard, so that I can choose.
118. As a user, I want Lucy to re-verify subscription auth whenever the `claude` version changes, and refuse that Provider with a reason if it fails, so that a CLI update can't silently switch me to API billing.
119. As a user, I want Lucy never to store, read or proxy my CLI logins, so that my credentials stay with the vendor tools.
120. As a user, I want local Whisper speech-to-text by default, with the model sized to my GPU or CPU, so that recognition is fast and private.
121. As a Superwhisper user, I want to use it as my STT Provider, with clear warnings about its latency, cleanup model and history, so that I can choose knowingly.
122. As a user, I want Piper speech by default with a permissively licensed voice, so that her voice is local and free to use.
123. As a user, I want Lucy to offer to install Piper for my distro, after a yes, so that setup doesn't need me to find packages.
124. As a user who declines Piper or whose TTS fails, I want replies shown as notification text, so that Lucy still works.
125. As a user, I want to be shown a voice's licence when I pick a non-commercial one, so that I know the terms.
126. As a user, I want first-run setup in a TUI: detect tools, offer Piper install, pick Providers per tier, the STT model, a voice with preview, mic and speaker with a level meter, and a wake-word test with its score, so that I can get going in one sitting.
127. As a user, I want a Provider status page with a "run real check" button, so that I can see whether everything works.
128. As a user, I want to revoke Approvals from the TUI, taking effect even mid-Session, so that I can withdraw access quickly.
129. As a user, I want granting Approvals to stay voice-only, so that the yes is always one I spoke.
130. As a user, I want every other setting (thresholds, timeouts, endpointing, paths) in one hand-editable TOML file whose comments survive TUI edits, so that power tuning is possible.
131. As a user, I want config reloaded at the start of each Session, so that edits apply without restarting.
132. As a user, I want Lucy to run with no config file at all using defaults, so that a fresh install just works.
133. As a user, I want Provider failures spoken plainly with the fix, and never a silent switch to another Provider, so that I know what's happening.

### Errors

134. As a user, I want "Sorry, I didn't catch that" when STT fails, and the Session ended after two failures in a row, so that a broken mic doesn't loop forever.
135. As a user, I want her to recover silently once if her thinking process dies, so that a hiccup doesn't end my Session.
136. As a user, I want "I've lost my connection to Claude" and a notification if recovery fails, so that I know which Provider broke.
137. As a user, I want her to tell me when I've hit a plan limit, with the reset time if known, so that I know when she'll be back.
138. As a user, I want a pinned mic or speaker that's missing at Session start to fall back to the default with one notification, so that I can still talk.
139. As a user, I want losing a device mid-Session to end the Session with a notification, so that she doesn't hang.

### Running and integration

140. As a user, I want Lucy to run as a systemd user service tied to my graphical session, started from setup after my yes, so that she's always there when I'm logged in.
141. As a user, I want everything Lucy starts to stop when she stops, so that nothing is left running.
142. As a user, I want `lucy status` to print a snapshot, so that I can check her from a terminal.
143. As a user, I want every `lucy` command to tell me if lucyd isn't running and offer to start it, so that problems are obvious.
144. As a widget author, I want a documented, versioned JSON-RPC socket with a published schema and a `lucy events` stream, so that I can build integrations.
145. As a user, I want transcripts, spoken text and tool arguments never sent over that socket, so that other local programs can't read my conversation.
146. As a user, I want the model's access to Lucy's tools to use no TCP port, and unknown callers refused, so that nothing else on my machine can call her tools.
147. As a user, I want the Piper server bound to localhost only, so that it isn't exposed to my network.
148. As a user, I want Lucy to act only when spoken to, so that she never starts things on her own.

### Project

149. As a contributor, I want Lucy MIT/Apache-2.0 with copyleft parts only out of process, enforced in CI, so that the licence stays permissive.
150. As a user, I want nothing personal hard-coded, so that Lucy works for anyone.
151. As a maintainer, I want the vendor terms and the Source catalogue's AI-summary flags re-checked before each release, so that Lucy stays within the rules.
152. As a user, I want Lucy never to call herself a vendor product or imply endorsement, so that it's clear what she is.

## Implementation Decisions

### Processes and crates (ADR 0006)

- **`lucyd`** is the only process that owns audio (capture, playback, AEC, wake word, VAD), the Session, Lucy's MCP tool server, state writes (Approvals, Plugin enable/disable) and every child process (thinking CLI, Piper server, Plugins, Delegates). Children die with it (control-group kill plus parent-death signal).
- **`lucy`** is a thin CLI and TUI: no arguments opens the TUI; `setup`, `status`, `listen`, `end`, `events`, and a hidden `mcp-relay`.
- **`lucy-indicator`** is a separate client of the public socket (layer-shell plus a 2D rasteriser). With no layer-shell it exits cleanly.
- **Libraries:** `lucy-ipc` (protocol types, small client, generated JSON Schema; published) and `lucy-config` (config types, defaults, comment-preserving writer).
- Inside lucyd these are modules, not crates: `audio`, `providers`, `plugins`, `tools`, `session`. `session` is a pure, I/O-free state machine driven by events. A module becomes a crate only for a concrete reason. `lucy` and `lucy-indicator` must never link whisper, the echo canceller or ort.
- **systemd user units:** `lucyd.service` (part of and wanted by `graphical-session.target`, after PipeWire, restart on failure, no socket activation) and `lucy-indicator.service`.
- lucyd falls back to desktop notifications whenever no client holds the `indicator` role.

### Public IPC contract

- A Unix socket in `$XDG_RUNTIME_DIR/lucy/` (mode 0600), carrying newline-delimited JSON-RPC 2.0.
- Handshake `hello {protocol: 1, client, role}`. A major-version mismatch is refused with a clear error. Fields are only added within a major version.
- Requests: `status`, `subscribe {topics}`, `session.start`, `session.end`, `providers.status`, `provider.check`, `plugins.list`, `plugins.set_enabled`, `approvals.list`, `approvals.revoke`, `test.mic_level`, `test.wake_word`, `test.voice_preview`.
- Events: `state {state, session_id}` (Idle, Listening, Thinking, Speaking, Confirming, plus transient `error`), `level {rms}` at about 30 Hz for `level` subscribers only, `plugins.changed`, `notice {text}`.
- Never sent to a client: transcripts, spoken text, tool arguments. A client may send text in, for `test.voice_preview` to speak. There is no request that grants an Approval or answers a Confirmation.
- TUI setup tests run inside lucyd over this socket. The TUI writes `config.toml` directly.

### MCP tool server (ADR 0005)

- The thinking process sees only Lucy's MCP server. It reaches it through `lucy mcp-relay` (stdio), which pipes to a private 0600 socket. There is no TCP port.
- Each relay gets a per-spawn token, so lucyd knows whether the caller is the Session's model or a Delegate, and refuses unknown callers.
- **Session model** sees Built-in tools plus every enabled, approved, working Plugin tool, listed as `<plugin>__<tool>`. Each description comes from the Manifest's `handles` and `examples`, and the input schema is the one saved at Approval. A call to one of these tools *is* the Plugin selection. On first use, Lucy starts the Plugin, applies Landlock and `confirm`, and forwards the call. She sends `tools/list_changed` on mid-Session Approval or revoke.
- **Built-in tools:**

  | Tool | Inputs → output | Confirmation |
  |---|---|---|
  | `open_url` | `url` (http/https only) → ok | no |
  | `launch_app` | `name` → launched / `candidates[]` / `not_found` | no |
  | `system_status` | `topic` ∈ battery, disk, memory/CPU, network, volume, time, uptime → facts | no |
  | `request_plugin` | `name` (short lowercase Plugin name, used for `.build/<name>/`), `goal`, `clarifications`, `request` (the user's words, answered once the Plugin is approved) → starts Authoring | no |
  | `manage_plugin` | `plugin`, `action` ∈ enable, disable, rollback, remove → result | remove only |
  | `update_persona` | `persona` (full new text of `persona.md`) → saved; applies in full from the next Session | no |
  | `end_session` | → ends the Session after current speech | no |

- `launch_app` matches XDG desktop entries by name, generic name and keywords. It launches through `uwsm app --` when uwsm is running and `gio launch` otherwise (config override). There is no fallback to `request_plugin`.
- **Delegate tools** (Delegate callers only; no Plugin or Built-in tools): `report_progress{text}`, `ask_user{question}`, `submit_plugin{}`, `cannot{reason, fallback}`.
- Not in v0.1: `speak`, `read_page`, Memory tools, raw shell.

### Confirmation and Approval are runtime-owned

- A tool call that needs Confirmation suspends while Lucy speaks a fixed prompt. The prompt is the tool's `confirm_prompt` with `{arg}` placeholders filled in, or a generic one built from `handles`. The full arguments also go to a notification.
- She listens for one utterance and matches it locally against a configurable list of yes-phrases ("yes", "yeah", "yep", "go ahead", "do it"). A yes spoken during the prompt counts. Anything else, or 8 s of silence, means no, and she says "Okay, I won't". The model receives only `confirmed` or `declined`.
- `confirm` can't be overridden in v0.1. For Plugins it lives in the Manifest (hash-covered); for Built-in tools it is fixed.
- Approval is spoken by Lucy, heard by the runtime, and has no model-callable tool.

### Session state machine

- States: Idle, Listening, Thinking, Speaking, Confirming. Speaking means TTS audio is queued or playing. Thinking means a Turn is in progress and nothing is playing (tool calls included). One Turn may alternate Thinking ↔ Speaking.
- **Start:** on the Wake word, play a start sound, show the Indicator in Listening and pre-spawn the thinking process. Audio straight after the Wake word is kept. With no speech at all, end after 8 s.
- **Wake word inside a Session:** while Listening it is ordinary speech, and a leading wake phrase is stripped. During Speaking it is an instant Barge-in.
- **Barge-in:** during Speaking, sustained speech passes the VAD gate (below), or the Wake word fires. Configured in a `[barge_in]` table: `mode = "speech" | "wake_word"` (default `speech`); `force = true` turns Barge-in by speech on under PulseAudio or ALSA (default `false`); and the gate values from the Audio pipeline, `threshold` (0.7), `sustain_ms` (300), `unconverged_threshold` (0.85), `unconverged_sustain_ms` (500), `converged_erle_db` (10), `warm_up_ms` (2000) and `tail_ms` (300). On Barge-in: stop and flush TTS; send the stream-json control interrupt if the model is generating (fall back to SIGINT plus respawn if feature detection fails); go to Listening. The next utterance is sent prefixed with `[You were interrupted; the user heard up to: "…"]`, or `[You were interrupted]` if nothing had played yet. An empty transcript returns silently to Listening. Speech during Thinking is also a Barge-in.
- **Tools on Barge-in:** unconfirmed in-flight calls get an MCP cancellation. Confirmed calls run to completion and are reported in the next Turn.
- **Confirming:** described above. An Indicator click declines and ends the Session.
- **End:** an Indicator click ends from any state, immediately. A Closing phrase ends only via `end_session`, after the current speech. A silence timeout (~20 s, configurable) runs only in Listening, and ends with a soft sound and nothing spoken.
- **While an Authoring build runs:** the Session sits in Listening with the silence timer paused, and the Indicator adds a "building" mark.
- **Errors:**
  - No speech recognised: back to Listening, timer not reset.
  - STT fails: "Sorry, I didn't catch that"; two in a row ends the Session.
  - Thinking process dies: respawn once and resend; on a second failure, speak and notify, then end.
  - Plan limit: speak the limit and the reset time, then end. No tier switch.
  - TTS fails: replies go to notification text and the Session continues.

### Audio pipeline

- cpal 0.18 (PipeWire → PulseAudio → ALSA), with both streams in one PipeWire node group for drift correction, behind an `AudioIo` seam. Lucy shows in mixers as `cpal-capture-<pid>` until an upstream cpal naming PR lands (no fork).
- Internally 16 kHz mono f32. TTS plays at the voice's native rate. A rubato downsampled copy is the AEC render reference. Callbacks only push into lock-free rings. One DSP thread reframes to 10 ms and runs AEC, then fans out to wake word, VAD and STT.
- **AEC:** in-process WebRTC AEC3 behind an `EchoCanceller` seam, through `sonora` (a pure-Rust AEC3 port, BSD-3) rather than the C++ `webrtc-audio-processing` 2.x, which needs a system package or meson to build (user decision, slice 5; wayfinder 23 compares the two on real speakers). It cancels only Lucy's own voice. PipeWire's echo-cancel module is opt-in (point the mic at it and disable in-process AEC).
- **Wake word:** `livekit-wakeword` v2 (pinned git rev until it is on crates.io), one fixed engine with a swappable `.onnx` path. It is not a Provider. Models: `hey_lucy` (default) and `lucy`, committed with their training config.
- **VAD:** Silero v6.2 in 32 ms chunks behind a `Vad` trait. End of utterance: start threshold 0.5, end threshold 0.35, 100 ms to start, 500 ms pre-roll, 700 ms silence to end, 30 s cap. Barge-in gate: p ≥ 0.7 for 300 ms; p ≥ 0.85 for 500 ms while AEC is unconverged (ERLE < 10 dB or the first 2 s). The window covers TTS plus a 300 ms tail, and TTS ducks on the first frame above threshold. All values are tunable starting points.
- ONNX through the `ort` API on the pure-Rust tract backend by default. Native ORT via dynamic load of the system library is allowed for Silero only, if tract fails. Nothing is downloaded from a CDN.
- **Devices:** `"default"` or a pinned node name, re-resolved each Session. A missing pin falls back to the default with one notification and keeps the pin. A device lost mid-Session ends the Session.
- PipeWire is the supported stack. On PulseAudio or ALSA, Barge-in is off by default (`[barge_in] force` turns it on, with a warning).

### Providers and configuration (ADR 0001)

- Three Provider kinds: **STT** (`whisper` default, `superwhisper` optional), **Thinking** (named `[providers.<name>]` entries of kind `claude` or `codex`, with `[tiers] fast`/`advanced` pointing at them), **TTS** (`piper`; the interface exposes each voice's sample rate). There is no transport axis: each Thinking kind runs one native process per Session.
- **`claude`:** one persistent `claude -p` with stream-json in and out, partial messages, an isolated config (strict MCP config naming only the relay, no setting sources, no built-in tools), `dontAsk` permission mode, and only Lucy's MCP tools allowed. Never `--bare`. It is pre-spawned on the Wake word. claude keeps the conversation on disk so that a process respawned after SIGINT can `--resume` it; Lucy deletes that transcript (`<config dir>/projects/<working dir>/<session id>.jsonl` and the `<session id>/` dir beside it) when the Session or Delegate ends, and when lucyd stops. No other transcript is touched.
- **`codex`:** `codex app-server` over stdio, with Lucy's MCP server injected via `-c` and `forced_login_method="chatgpt"`. Before each Session the account must be a ChatGPT login; otherwise refuse with the reason. A per-Provider `allow_api_key = true` opts out of both the ChatGPT pin and that check, as it opts `claude` out of the Subscription guard.
- **Subscription guard:** strip Bedrock/Vertex/Foundry switches, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `apiKeyHelper` from the child environment; keep `CLAUDE_CODE_OAUTH_TOKEN`. When `claude --version` changes, re-verify before the next Session: auth status must show `claude.ai`, and one turn must show `apiKeySource: "none"` plus a `rate_limit_event`; otherwise refuse with the reason. A per-Provider `allow_api_key = true` opts out.
- **Health:** at each Session start, when the CLI is spawned, check the binary is present and auth is right. Run a real "Reply OK" round-trip at setup, from the TUI status page, and after a mid-Session failure.
- A shipped per-tier model recommendation table (updated each release). Setup detects installed CLIs. Any model the CLI accepts may be chosen.
- **Whisper:** `whisper-rs` with Vulkan, resident model, one utterance at a time after endpointing. `small.en` on GPU, `base.en` on CPU (`audio_ctx` ≈ 384).
- **Superwhisper:** offered only if installed. Write a 16 kHz mono file, call `transcribe FILE`, poll `read`. The TUI warns about ~250 ms latency, the cleanup LLM, and history retention.
- **Piper (ADR 0003):** the user installs upstream `piper-tts` with its `http` extra (`piper-tts[http]`). Lucy supervises one HTTP server per daemon, started with `--host 127.0.0.1` on a free port, `PYTHONPATH` cleared and the voice preloaded, and restarts it on crash. Text goes over sentence by sentence. Setup detects it by importing `piper.http_server` with `PYTHONPATH` cleared (not by a `piper` binary). Install is offered after a yes: the distro is read from os-release (`ID`, then `ID_LIKE`) and mapped to its package manager (AUR via the user's helper on Arch). The distro package is used only if it installs cleanly; otherwise `uv tool install`, then `pipx`. Declining means text-only replies. The default voice is `en_GB-cori-medium`. Voices are downloaded on first run with their model card; recommended voices have a permissive training chain only.
- **Config:** one TOML in `$XDG_CONFIG_HOME/lucy/`, written comment-preserving, reloaded at each Session start. State lives under `$XDG_STATE_HOME/lucy/`. Lucy stores no secrets of her own. Plugin credentials go in the Secret Service.
- **TUI scope:** first-run setup (detect tools, offer the Piper install, Providers per tier, STT model, voice with preview, mic and speaker with level meter, wake-word test with score, offer to enable the service); a Provider status page with a real check; an Approval list with revoke. Everything else is file-only.

### Plugins (ADRs 0002, 0004)

- A Plugin is a folder holding `manifest.toml`, source, and an executable that speaks MCP over stdio (`rmcp` for Rust). Manifest shape (from ticket 19, with the 09 amendment):

  ```toml
  name = "photos"
  version = "0.2.0"          # shown only; Approval uses the hash
  author = "lucy"            # or "human"
  summary = "Find and show photos from your local photo folders"
  command = ["./target/release/photos"]

  [[tools]]
  name = "search"
  handles = "find photos by place, date or name"
  examples = ["find my Mount Rainier photo", "photos from June 2024"]   # ≤ 3
  confirm = false
  # confirm_prompt = "Send an email to {to} about {subject}?"

  [access]
  read = ["~/Pictures"]
  write = []
  network = []               # hosts, declared and shown, not enforced in v0.1
  credentials = []           # names only
  commands = ["xdg-open"]
  ```

- **Locations:** user and Lucy-written Plugins under `~/.local/share/lucy/plugins/<name>/`; shipped ones under `/usr/share/lucy/plugins/<name>/`; a user copy overrides a shipped one by name.
- At install or scan, Lucy checks the Manifest against the live `tools/list`. A mismatch, a failed build or a failure during a call marks the Plugin broken and drops it from selection.
- **Approval** records live in Lucy's own state (`approvals.toml`), never in the Plugin folder. Each pins a hash of the Manifest plus the executable and saves the tool input schemas. Any change needs a fresh Approval, except rolling back to the single kept previous version. A revoke withdraws only the current Approval: the kept previous version survives it, to roll back to once the Plugin is approved again. New Plugins get the full spoken summary; updates get a changes-only summary plus the full diff on screen. lucyd approves shipped Plugins itself when it scans them, with no spoken Approval, so they work from the first Session; that includes a shipped Plugin a removed user copy overrode. A revoked shipped Plugin is approved again at the next scan, so disabling is how to leave one out. Hand-installed Plugins show as awaiting Approval and are mentioned once at the next Session.
- **Sandbox:** Landlock enforces filesystem `read`/`write`, plus each Plugin's private state dir (`~/.local/state/lucy/plugins/<name>/`, allowed automatically). Network is unenforced in v0.1.
- Credentials declared by name are stored in the Secret Service (`oo7`), entered via the TUI or a desktop prompt (never by voice, never via the model), and passed as env vars at spawn. No general OAuth flow.
- Mid-call questions use MCP `input_required`. Slow calls use the MCP Tasks extension within a Session.
- **Lifecycle:** a Plugin starts on first use in a Session and stops at Session end. Disable keeps files and Approval. Remove deletes both (needs Confirmation; shipped Plugins can't be removed). Lucy edits only `author = "lucy"` Plugins.
- Selection when two Plugins fit equally: Lucy asks the user which one.

### Authoring

- The fast tier clarifies what only the user knows, then calls `request_plugin`. lucyd spawns the advanced-tier Delegate through the same native client as Thinking (ADR 0002 amendment), with the goal and the original request stored alongside it. The Delegate's system prompt is the shipped authoring brief: permitted access only, `cannot` when access isn't permitted, fixtures only, Rust by default. It starts from the shipped template folder.
- **Delegate sandbox (Landlock):** read/write only to the build staging dir (under the Plugins folder's `.build/<name>/`), toolchain caches, and the CLI's own config dir; read-only system and toolchain paths; open network; no user data.
- `submit_plugin`: lucyd builds, checks the Manifest against `tools/list`, and runs the Delegate's tests. On a pass the Plugin is awaiting Approval, and Lucy speaks the Approval summary straight away (Confirming). On a yes, she answers the stored original request and asks "Is that the one?". A no is fed back as a fix through the update flow.
- Lucy speaks only for `ask_user`, `cannot` and the Approval, and waits until nobody is speaking. `report_progress` goes to notifications (config toggle, default on) and the TUI.
- **Limits:** 20 min wall-clock per build (configurable), staging kept for retry, one build at a time with a queue of one. Builds survive Session end; Approval and parked questions are offered at the next Session start. Only "cancel it" stops a build.
- No Plugin SDK crate in v0.1.

### News Plugin (shipped reference)

- Plain code, no model calls. `briefing` fetches the official RSS/Atom feeds of the Sources in rotation concurrently. It returns each item's headline, description, link, Source and position. It never fetches article pages; there is no `article` tool. `set_preference` (no Confirmation) changes preferences, and Lucy always says what changed.
- A vetted Source catalogue ships with the Plugin: each Source's official feed plus an "AI summary allowed" flag from its terms. Default rotation: CNN, NPR, Sky News. Guardian and NYT are flagged forbidden; Reuters and AP have no public feed.
- A Source the user names that isn't in the catalogue is checked for an official feed: the `<link rel="alternate">` RSS/Atom feeds its home page declares, then the usual feed paths. If one reads, it is added to the rotation and kept in preferences; otherwise Lucy gives the honest limit and offers to open its page. Its terms aren't vetted, so Lucy tells the user so when it is added.
- Preferences live in the Plugin's state dir: rotation, liked-but-forbidden Sources (opened beside the summary), avoided Sources, topic weighting, default count and length.
- The fast tier picks the big stories (leading, and appearing across Sources), speaks the top 1–2 in one or two sentences in the Persona with a time-of-day greeting by name, then uses `open_url` for the front pages it drew from and for liked-but-forbidden ones. Feed text is untrusted input.

### Persona

- An editable `persona.md` in `$XDG_CONFIG_HOME/lucy/`, included in every Session prompt. The default is jovial with a little sass, never flippant on serious news, and asks for one to three short spoken sentences with no markdown and a "Let me check" before slow tools.
- The very first Session opens with a self-introduction that asks the user's name and stores it there. Changing it by voice rewrites the file and reads the change back, with no Approval.

### Licensing (ADR 0003)

- MIT/Apache-2.0. Lucy's binaries link only permissively licensed crates, enforced by a `cargo-deny` allow-list in CI. Copyleft runs only out of process (Piper, Plugins).

## Testing Decisions

- **What a good test is:** it drives Lucy through her external surfaces and checks what an outside observer would see: state events, notices, MCP traffic, spoken-text requests to TTS, files written to state. It never reaches into module internals. A test should survive rewriting the internals of `session`, `tools` or `plugins`.
- **One seam: the lucyd harness.** Tests start lucyd in-process with fake adapters behind the traits the design already names:
  - `AudioIo`: fed scripted capture and records playback.
  - Wake word and VAD: scripted detections and speech segments.
  - STT: scripted transcripts or failures.
  - Thinking: a scripted model process that emits stream-json-shaped events and tool calls, and records interrupts.
  - TTS: records sentences and can be made to fail.

  The harness observes through the real public IPC (`lucy-ipc` client) and the real MCP socket via the relay. There are no separate unit tests for the `session` state machine; its edge cases are covered through the harness.
- **What the harness covers:**
  - Session start, end and timers (including the 8 s post-wake end and the paused timer during Authoring).
  - Barge-in in each state, both `barge_in` modes, the interrupted-prefix message, and confirmed vs unconfirmed tools on Barge-in.
  - Confirming: yes-phrase match, yes during the prompt, 8 s silence, non-yes, Indicator click.
  - Every error path.
  - Built-in tool contracts.
  - Plugin proxying and `tools/list_changed`.
  - Per-spawn token refusal.
  - The "never on the bus" rule: no transcript or argument appears in any IPC message.
  - Notification fallback when no `indicator` role is connected.
  - The Authoring loop with a scripted Delegate (`report_progress`, `ask_user` parked across Sessions, `submit_plugin` pass and fail, `cannot`, budget timeout, queue of one, build surviving Session end).
- **Fixture Plugins:** tiny real MCP stdio executables used by the harness, covering:
  - Manifest/`tools/list` match and mismatch.
  - Hash-pinned Approval (any change to Manifest or executable invalidates; rollback doesn't).
  - User-overrides-shipped.
  - Broken marking.
  - Landlock read/write enforcement (a fixture that tries to read outside its declared paths must fail).
  - Credential env injection.
  - The private state dir.
  - Lifecycle stop at Session end.
- **News Plugin:** tested through its own MCP interface against recorded feed fixtures (no live network). This is also the reference for the template's fixtures test harness.
- **Provider adapters against real CLIs:** subscription guard, env scrubbing, version-change re-verification, control-interrupt feature detection and SIGINT fallback. These are manual or opt-in checks, because they need a real login. The Thinking fake follows the recorded stream shapes from the voice-loop prototype.
- **Manual acceptance (HITL):**
  - The news briefing's six success checks (ticket 11).
  - The two Rainier checks (ticket 20).
  - The ≤ 2.5 s speech-end-to-first-audio budget on real hardware.
  - AEC and Barge-in on laptop speakers, desk speakers and headphones.
  - Indicator on Hyprland and the GNOME fallback.
- **CI:** `cargo-deny` licence check. Schema generation is checked against the committed JSON Schema.
- **Prior art:** none in the repo yet. The voice-loop prototype (`prototypes/voice-loop`) and its interrupt test are the reference for stream-json shapes and interrupt behaviour.

## Out of Scope

- Herdr transport (attaching to agents in Herdr panes).
- Raw shell access for the model.
- A type-any-word wake engine (sherpa-onnx open vocabulary).
- Zed-ACP for Claude/Codex Delegates (kept only as an adapter for agents that speak it natively); A2A.
- Email and calendar, OAuth flows, Google Photos Picker integration.
- Content search in photos (vision/CLIP Provider).
- Memory beyond `persona.md` and Plugin-owned preferences.
- Learning from repetition; proactive or unprompted turns (including unprompted morning briefings); Plugins running between Sessions.
- Host-level network enforcement for Plugins.
- Resuming a reply after a false interruption.
- Cancelling other system audio (music/video) as an echo reference.
- Speaker recognition and multi-user support.
- API-key Providers as a priority (the design must not preclude them), cloud voices, a Lucy-built libpiper helper, Parakeet STT, Smart Turn endpointing.
- Packaging and publishing (AUR, release assets, CI wake-model training); a Plugin SDK crate; an Omarchy shell widget (the socket enables third parties).
- An article-reading tool for the news Plugin.

## Further Notes

- **Open tickets that may adjust defaults (not blockers for building):**
  - 23: AEC on real speakers, and sonora vs C++. May change the `barge_in` default or the gate.
  - 24: wake-word soak. Sets the default sensitivity and whether AEC runs before the wake word.
  - 25: audio-stack coexistence. GPU contention with Superwhisper, a single PipeWire driver, device loss.
  - 26: Silero on tract vs earshot. May trigger the native-ORT fallback or replace Silero.
  - 27: train and commit the wake models (HITL). The real models are needed for 24 and for acceptance.
- **Pinned dependencies:** `livekit-wakeword` v2 is not yet on crates.io (pin a git rev). The `claude` stream-json interrupt is undocumented (feature-detect it). AUR `piper-tts` currently fails to install (the fallback order handles it).
- **Terms risk:** ADR 0001 accepts that Anthropic's terms are ambiguous about third-party tools driving a user's own `claude` login. Lucy never handles or proxies credentials and is single-user. Re-check Anthropic and OpenAI terms and the Source catalogue flags before release; Sky News's terms are still unverified.
- Hyprland config Lucy documents (for example a `lucy listen` keybinding, which isn't shipped by default) uses Hyprland 0.56's Lua syntax.
- Lucy never brands herself as a vendor product.
