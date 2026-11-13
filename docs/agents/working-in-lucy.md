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
- **Private data:** after a Session ends, `assert_nothing_private_on_bus()` checks that nothing private reached the public socket.

## Adapters (STT, Thinking, TTS)

- **Lifetime:** an adapter keeps the event sender it was given alive for as long as its handle. Today, a dropped sender makes the runner spin on the closed channel.
- **Sentences:** the Thinking adapter finds sentence boundaries itself and emits `SentenceEnd`. The Session sends each sentence to TTS.
- **Placeholders:** `Adapters::inert()` stands in for Providers that haven't been built yet.

## Public IPC

New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Regenerate the committed schema with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`; a test fails if it drifts.

## Real `claude`

Automated tests use the fake `claude` and the recorded fixtures in `crates/lucyd/tests/fixtures/claude/`. The fake reads its `--version`, `auth status` and turn overrides from its working directory; its header says how. `Harness::with_thinking` runs lucyd with the real adapter against it (`tests/subscription_guard.rs`). The live check is `cargo test -p lucyd --test claude -- --ignored`, and it spends the user's subscription, so run it only when a ticket needs a real round trip.

## Hearing (mic, VAD, Whisper)

- **Harness:** `Harness::hearing(config)` runs the real DSP thread and endpointing between `FakeMic` and `FakeVad`; the fake VAD reads each sample's value as the speech probability, so tests feed levels (`SPEECH`, `SILENCE`).
- **Models:** lucyd loads `ggml-<model>.bin` from `$XDG_DATA_HOME/lucy/models`. Silero is bundled in `crates/lucyd/models/`.
- **Wake word:** `Harness::wake()` scripts a detection. Under `Harness::hearing`, the fake wake word hears it where a run of `WAKE` frames starts, and the fake VAD hears those as speech. The real engine's model is `[wake_word] model`; unset, lucyd installs the bundled interim `hey_livekit.onnx` into `$XDG_DATA_HOME/lucy/models/wake/`. The model's file name is the phrase stripped from transcripts.
- **Live checks:** `cargo test -p lucyd --test whisper --test silero --test mic -- --ignored`. Outside the main checkout set `LUCY_WHISPER_MODELS` (a dir of ggml models) and `LUCY_SPEECH_WAV` (a speech recording); they default to the voice-loop prototype's.
- **PipeWire in a smoke run:** a short `XDG_RUNTIME_DIR` hides PipeWire, so cpal falls back to ALSA. Also set `PIPEWIRE_RUNTIME_DIR=/run/user/$UID`.

## Unix sockets

Socket paths are limited to about 108 bytes. The session scratchpad path is too long for a smoke run, so put the socket under `/tmp` or a short tempdir.

## Implementer subagents

The `tdd` skill asks the user to approve seams first, and a subagent can't ask. The ticket's acceptance checks driven through the harness, plus the adapter traits above, are the approved seams. A ticket that needs a new adapter trait names it in the implementer's report. The ticket's `Seams:` line says which shared seams it owns and which it only uses.

## Merging

A merge that combines two tickets' changes to the same seam (both edit one adapter trait, the Session start, the notification fallback) gets one harness test of the combination before the merge commit. Each ticket's own tests pass alone; the combination is where the bugs were in slice 2 (a refused Provider swallowing the first-run introduction; a TTS-failure notification replacing the End button).
