# Working in Lucy

Facts about the codebase that aren't obvious from reading it.

## Checks

`scripts/check.sh` runs every gate: fmt, clippy, tests, the client-deps check, the glossary-term check, the lint-allow check and `cargo deny`. The pre-commit hook runs it once you've run `git config core.hooksPath .githooks`; a commit that only touches markdown, `docs/` or `.scratch/` runs just the glossary check. Commit with the hook on; `--no-verify` leaves the gates to CI.

## lucyd test harness

- **Where it is:** `crates/lucyd/tests/harness/mod.rs` starts lucyd in-process through `lucyd::spawn(Options { .. })` and connects through the real `lucy-ipc` client.
- **Fakes:** they plug in through `Options.adapters`.
- **Config:** `Harness::with_config(toml)` writes the `config.toml` lucyd reads at each Session start; `Harness::start()` has none, so defaults apply.
- **Real binaries:** the harness runs the `lucy` binary cargo builds next to the test binary. `cargo test -p lucyd` on its own needs `cargo build -p lucy` first; `cargo test --workspace` handles this.
- **Fixture Plugin:** `crates/lucyd/examples/fixture-plugin.rs`, a tiny MCP stdio server. `Harness::install_plugin` copies it into a Plugin folder and `Harness::approve` pre-seeds `approvals.toml`. `cargo test -p lucyd` builds it, but `--test <name>` alone doesn't: run `cargo build -p lucyd --examples` first.
- **News Plugin in lucyd:** `tests/news.rs` installs the real `lucy-news` binary as a shipped Plugin; `cargo test -p lucyd` on its own needs `cargo build -p lucy-news` first.

## Plugins

- **News Plugin:** `crates/lucy-news` is the shipped reference Plugin; `manifest.toml` beside it is the Manifest Lucy ships. Its Source catalogue (feeds and "AI summary allowed" flags) is `src/catalogue.rs`; re-check the flags before each release, and keep `[access] network` matching the readable feeds' hosts (a unit test checks). Sites the user names outside the catalogue (`src/discover.rs`) can't be listed there; the Manifest's comment says so.
- **Fixtures test harness:** `crates/lucy-news/tests/support/mod.rs` runs a Plugin's MCP server in-process through a real MCP client and answers its web requests from `tests/fixtures/web/<host>/<path>`. The Plugin template copies it. Plugin tests never touch the network.
- **Confirming:** Confirmations, Approvals and Plugin mid-call questions all go through `runner::ask`, which queues a `Command::Ask` for the open Session. Tests shorten the 8 s wait with `[confirmation] timeout_secs`. `Harness::request_approval` starts a spoken Approval (`Daemon::request_approval`; nothing on IPC or MCP reaches it), and `FakeThinking::watch_tools` holds a relay open to see `tools/list_changed`. The fixture Plugin's `ask` tool asks a question through `input_required`.
- **Fake executables:** write them from a child process (see `fake_notify_send` in `notify_send.rs`). A fd this process holds open for writing can leak into a child another test forks, and running the file then fails with "Text file busy".
- **Authoring:** `Thinking::open_delegate` spawns a Delegate under Landlock (`plugins::sandbox::confine_delegate`). In the harness, `thinking.delegate` is the scripted Delegate: read its Turns, write the Plugin into `workspace().dir` with `stage_plugin`, and call the Delegate tools through its relay. On `submit_plugin` lucyd runs the staging dir's `build.sh` then `test.sh`, sandboxed, so a test scripts a pass or a failure there. Lucy's build lines and questions wait in the runner until the Session is quiet (`Command::Turn`, `runner::ask_when_quiet`); `barrier` in `tests/authoring.rs` orders a test after them. The template (`crates/lucyd/plugin-template/`) is built into lucyd; build it by hand after changing it.
- **Private data:** after a Session ends, `assert_nothing_private_on_bus()` checks that nothing private reached the public socket.

## Adapters (STT, Thinking, TTS)

- **Lifetime:** an adapter keeps the event sender it was given alive for as long as its handle. Today, a dropped sender makes the runner spin on the closed channel.
- **Sentences:** the Thinking adapter finds sentence boundaries itself and emits `SentenceEnd`. The Session sends each sentence to TTS.
- **Placeholders:** `Adapters::inert()` stands in for Providers that haven't been built yet.

## Public IPC

New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Regenerate the committed schema with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`; a test fails if it drifts.

## Real `claude`

Automated tests use the fake `claude` and the recorded fixtures in `crates/lucyd/tests/fixtures/claude/`. The fake reads its `--version`, `auth status` and turn overrides from its working directory; its header says how. `Harness::with_thinking` runs lucyd with the real adapter against it (`tests/subscription_guard.rs`). The live check is `cargo test -p lucyd --test claude -- --ignored`, and it spends the user's subscription, so run it only when a ticket needs a real round trip.

## Real `codex`

`fixtures/codex/fake-codex` stands in for `codex app-server`. Its turns are written from the protocol's generated types (`codex app-server generate-ts`), not recorded. Its header says what it reads from its working directory. `tests/codex_session.rs` runs Sessions through it. `codex app-server --print-startup-config` checks `-c` settings without a model call. The live check is `cargo test -p lucyd --test codex -- --ignored`. It spends the user's ChatGPT subscription.

## Real Piper

lucyd runs `python -m piper.http_server`, which needs the `http` extra: install with `uv tool install 'piper-tts[http]'`. Clear `PYTHONPATH` when running Piper by hand (`env -u PYTHONPATH ...`): a `PYTHONPATH` pointing at the system site-packages loads the wrong numpy and Piper fails to import. Voices live in `$XDG_DATA_HOME/lucy/voices/` (`en_GB-cori-medium.onnx`, its `.onnx.json` and `MODEL_CARD`); `piper -m <voice>.onnx -f out.wav` synthesises a line to check. With lucyd running, the hidden `lucy say "<text>"` (a Voice preview) speaks a line through lucyd's own voice and speaker, with no model call.

## Hearing (mic, VAD, Whisper)

- **Harness:** `Harness::hearing(config)` runs the real DSP thread and endpointing between `FakeMic` and `FakeVad`; the fake VAD reads each sample's value as the speech probability, so tests feed levels (`SPEECH`, `SILENCE`).
- **Barge-in:** under `Harness::hearing`, `Harness::echo` is Lucy's echo canceller (set its ERLE); `FakeMic::use_pulseaudio` turns Barge-in by speech off. The gate counts time in samples, so call `mic.wait_until_heard()` before a state change whose timing matters, and end a speech feed with a little silence so its last 10 ms frame and VAD chunk are complete. Lucy's echo canceller is `sonora` (pure-Rust AEC3), not the C++ `webrtc-audio-processing` the spec names.
- **Mid-utterance:** the DSP thread reports whether the user is speaking (`Adapters::speaking`); the Session's timers hold while they are. Without `Harness::hearing`, `Harness::start_speaking` starts an utterance and the next `say` ends it.
- **Models:** lucyd loads `ggml-<model>.bin` from `$XDG_DATA_HOME/lucy/models`. Silero is bundled in `crates/lucyd/models/`.
- **Wake word:** `Harness::wake()` scripts a detection. Under `Harness::hearing`, the fake wake word hears it where a run of `WAKE` frames starts, and the fake VAD hears those as speech. The real engine's model is `[wake_word] model`; unset, lucyd installs the bundled interim `hey_livekit.onnx` into `$XDG_DATA_HOME/lucy/models/wake/`. The model's file name is the phrase stripped from transcripts.
- **Live checks:** `cargo test -p lucyd --test whisper --test silero --test mic -- --ignored`. Outside the main checkout set `LUCY_WHISPER_MODELS` (a dir of ggml models) and `LUCY_SPEECH_WAV` (a speech recording); they default to the voice-loop prototype's.
- **PipeWire in a smoke run:** a short `XDG_RUNTIME_DIR` hides PipeWire, so cpal falls back to ALSA. Also set `PIPEWIRE_RUNTIME_DIR=/run/user/$UID`.

## Unix sockets

Socket paths are limited to about 108 bytes. The session scratchpad path is too long for a smoke run, so put the socket under `/tmp` or a short tempdir.

## Implementer subagents

The `tdd` skill asks the user to approve seams first, and a subagent can't ask. The ticket's acceptance checks driven through the harness, plus the adapter traits above, are the approved seams. A ticket that needs a new adapter trait names it in the implementer's report. The ticket's `Seams:` line says which shared seams it owns and which it only uses. Report only once every background job you started has finished or been stopped; a running one keeps your worktree locked.

## Merging

A merge that combines two tickets' changes to the same seam (both edit one adapter trait, the Session start, the notification fallback) gets one harness test of the combination before the merge commit. Each ticket's own tests pass alone; the combination is where the bugs were in slice 2 (a refused Provider swallowing the first-run introduction; a TTS-failure notification replacing the End button).
