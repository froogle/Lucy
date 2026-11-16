# Working in Lucy

Facts about the codebase that aren't obvious from reading it.

## Checks

`scripts/check.sh` runs every gate: fmt, clippy, tests, the client-deps check, the glossary-term check, the lint-allow check and `cargo deny`. The pre-commit hook runs it once you've run `git config core.hooksPath .githooks`; a commit that only touches markdown, `docs/` or `.scratch/` runs just the glossary check. Commit with the hook on; `--no-verify` leaves the gates to CI.

`target/` only grows (108 GB by slice 6). When a ticket is done and merged, with no agents running, run `scripts/clean.sh`: it clears the shared `target/` and the prototypes' own `target/` folders, prunes stale worktrees and says how much it freed. It refuses while another worktree exists. The next build is cold. Run it after removing a worktree too: its test binaries stay in the shared `target/` and still look for fixtures and the schema under the removed worktree's path.

## lucyd test harness

- **Where it is:** `crates/lucyd/tests/harness/mod.rs` starts lucyd in-process through `lucyd::spawn(Options { .. })` and connects through the real `lucy-ipc` client.
- **Fakes:** they plug in through `Options.adapters`.
- **Config:** `Harness::with_config(toml)` writes the `config.toml` lucyd reads at each Session start; `Harness::start()` has none, so defaults apply.
- **Clock:** lucyd's timers (the silence timeout, the 8 s end of a false wake, the wait for an answer, Authoring budgets) count on `Adapters.clock`. The harness's `clock` stands still until `advance`, which returns once every timer that went off has been handled. After an input the test can't see lucyd handle, `advance_once_set` or `advance_once_stopped` waits for the timer first; `Harness::barrier` makes earlier commands, Thinking and TTS events take effect first. A transcript is handled unseen, so `Harness::mumble_handled` waits until lucyd has handled an empty one. Timer tests never sleep.
- **Real binaries:** the harness runs the `lucy` binary cargo builds next to the test binary. `cargo test -p lucyd` on its own needs `cargo build -p lucy` first; `cargo test --workspace` handles this.
- **Fixture Plugin:** `crates/lucyd/examples/fixture-plugin.rs`, a tiny MCP stdio server. `Harness::install_plugin` copies it into a Plugin folder and `Harness::approve` pre-seeds `approvals.toml`. `cargo test -p lucyd` builds it, but `--test <name>` alone doesn't: run `cargo build -p lucyd --examples` first.
- **News Plugin in lucyd:** `tests/news.rs` installs the real `lucy-news` binary as a shipped Plugin; `cargo test -p lucyd` on its own needs `cargo build -p lucy-news` first.

## Plugins

- **News Plugin:** `crates/lucy-news` is the shipped reference Plugin; `manifest.toml` beside it is the Manifest Lucy ships. Its Source catalogue (feeds and "AI summary allowed" flags) is `src/catalogue.rs`; re-check the flags before each release, and keep `[access] network` matching the readable feeds' hosts (a unit test checks). Sites the user names outside the catalogue (`src/discover.rs`) can't be listed there; the Manifest's comment says so.
- **Fixtures test harness:** `crates/lucy-news/tests/support/mod.rs` runs a Plugin's MCP server in-process through a real MCP client and answers its web requests from `tests/fixtures/web/<host>/<path>`. The Plugin template copies it. Plugin tests never touch the network.
- **Confirming:** Confirmations, Approvals and Plugin mid-call questions all go through `runner::ask`, which queues a `Command::Ask` for the open Session. Tests shorten the 8 s wait with `[confirmation] timeout_secs`. `Harness::request_approval` starts a spoken Approval (`Daemon::request_approval`; nothing on IPC or MCP reaches it), and `FakeThinking::watch_tools` holds a relay open to see `tools/list_changed`. The fixture Plugin's `ask` tool asks a question through `input_required`.
- **Fake executables:** write them from a child process (see `fake_notify_send` in `notify_send.rs`). A fd this process holds open for writing can leak into a child another test forks, and running the file then fails with "Text file busy".
- **Authoring:** `Thinking::open_delegate` spawns a Delegate under Landlock (`plugins::sandbox::confine_delegate`). In the harness, `thinking.delegate` is the scripted Delegate: read its Turns, write the Plugin into `workspace().dir` with `stage_plugin`, and call the Delegate tools through its relay. On `submit_plugin` lucyd runs the staging dir's `build.sh` then `test.sh`, sandboxed, so a test scripts a pass or a failure there. Lucy's build lines and questions wait in the runner until the Session is quiet (`Command::TurnOrNextSession`, `runner::When`); `barrier` in `tests/authoring.rs` orders a test after them. A build outlives its Session: its question and Approval go through `runner::ask_when_quiet_or_next_session`, which keeps them in the runner for the next Session; "Is that the one?" is `When::Kept`, kept behind the answer's Turn without a notification of its own. A budget counts on `Adapters.clock` (see **Clock** above). With no Session open, or if it ends first, a build line is shown as its Build notification, then told once at the next Session start; its text is for the model, never shown. "cancel it" is matched locally in `session::Session`, never sent to the model. The template (`crates/lucyd/plugin-template/`) is built into lucyd; build it by hand after changing it.
- **Credentials:** `lucy_config::credentials::CredentialStore` is the seam to the Secret Service (`SecretService`, through `oo7`), shared by lucyd (`Adapters.credentials`, read when a Plugin starts for a call) and `lucy credentials <plugin>` (writes). Tests use `Harness::credentials` or their own fake; never `SecretService`, which is the user's real keyring. `assert_nothing_private_on_bus` counts entered values as private.
- **Input dialog:** `input::Dialogs` (lucyd) holds the `input` role and the one open dialog. A question reaches it through `runner::ask` with `Expects::Typed(Dialog)`: the runner opens it on the holder, or runs `lucy input --socket <path>` through `Launcher::open_terminal`, and the Session sits in Typing until the holder answers, a cancel phrase, the `[input]` timeout or the Session's end; dropping the runner's `OpenDialog` sends `input.close`. `input::ask` is `ask_typed`'s one-field question, and `Daemon::ask_typed` asks it from a test (the `lucy` crate's `Home::ask_typed`). In the harness, `Harness::input` claims the role and `Harness::answer_input` types an answer, which `assert_nothing_private_on_bus` counts as private; `launcher.terminals()` and `launcher.no_terminal()` cover the fallback.
- **Private data:** after a Session ends, `assert_nothing_private_on_bus()` checks that nothing private reached the public socket.
- **Model text:** when a Harness is dropped, it fails the test if any notification showed text written for the model; the markers are `lucyd::model_text::MARKERS`, so new model text starts with one of them.

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
- **Barge-in:** under `Harness::hearing`, `Harness::echo` is Lucy's echo canceller (set its ERLE, and `set_echo_heard` for whether Lucy's voice reaches the mic); `FakeMic::use_pulseaudio` turns Barge-in by speech off. The gate counts time in samples, so call `mic.wait_until_heard()` before a state change whose timing matters, and end a speech feed with a little silence so its last 10 ms frame and VAD chunk are complete. Lucy's echo canceller is `sonora` (pure-Rust AEC3), not the C++ `webrtc-audio-processing` the spec names.
- **Idle is the usual state:** the DSP thread runs every frame, in and out of Sessions, and lucyd mostly sits idle in a quiet room. Check anything adaptive in it (the canceller, its pre-gain, the meter) against minutes of room noise.
- **AEC bench:** `tools/aec-bench/` (README there) replays real recordings through lucyd's `Sonora`, Silero and a copy of the gate; tickets 27 and 28 judged their acceptance on it. It isn't in the workspace or `scripts/check.sh`, so build it with `cargo build --release` in its folder after changing lucyd's audio API. Its recordings (`runs/`) are gitignored and stay on the machine that made them.
- **Mid-utterance:** the DSP thread reports whether the user is speaking (`Adapters::speaking`); the Session's timers hold while they are. Without `Harness::hearing`, `Harness::start_speaking` starts an utterance and the next `say` ends it.
- **Models:** lucyd loads `ggml-<model>.bin` from `$XDG_DATA_HOME/lucy/models`. Silero is bundled in `crates/lucyd/models/`.
- **Wake word:** `Harness::wake()` scripts a detection. Under `Harness::hearing`, the fake wake word hears it where a run of `WAKE` frames starts, and the fake VAD hears those as speech. The real engine's model is `[wake_word] model`; unset, lucyd installs the bundled `hey_lucy.onnx` into `$XDG_DATA_HOME/lucy/models/wake/`. The model's file name is the phrase stripped from transcripts and named in the bar panel (`lucy_config::WakeWord::word`).
- **Live checks:** `cargo test -p lucyd --test whisper --test silero --test mic -- --ignored`. Outside the main checkout set `LUCY_WHISPER_MODELS` (a dir of ggml models) and `LUCY_SPEECH_WAV` (a speech recording); they default to the voice-loop prototype's.
- **PipeWire in a smoke run:** a short `XDG_RUNTIME_DIR` hides PipeWire, so cpal falls back to ALSA. Also set `PIPEWIRE_RUNTIME_DIR=/run/user/$UID`.

## Setup and the `lucy` pages

- **Shape:** `lucy` (no arguments), `lucy setup` and `lucy credentials <plugin>` are one full screen (ratatui on crossterm), `menu::run`, opened at `menu::Open`; they refuse a stdin or stdout that isn't a terminal. It is a list of sections (`sections::Section`), each a page: `ui::Rows` lists its rows and the letters that act on them, and a row's questions run one at a time on `ui::Ui`. Esc anywhere is `Stop::Back`: on a row's question it goes back to the page, on a page to the list. The walk (no `config.toml`, or `lucy setup`) opens each section with a last "Next" row; after a row is picked the cursor moves to the next one, so Enter alone walks every suggestion. Sections read their paths and `PATH` from `lucy::Env`, never the process environment. Ctrl-C arrives as a key in raw mode; ratatui's panic hook restores the terminal. An installer runs outside the full screen (`Keys::outside`), since it may ask for a password.
- **Settings:** `lucy::settings` is the one writer of `config.toml` (`settings::write` of `Change::Set`/`Unset`, through `lucy_config::set`/`unset`, comments kept): `settings::Key` is each setting `lucy` changes, and `Key::applies` says whether lucyd takes it up at the next Session or only after a restart (Whisper's model, which stays loaded). Installing Piper also needs a restart: lucyd looks for it once, at start. A new setting gets a `Key` and its `Applies`, read from where lucyd loads it. `lucy set`/`unset` also take `stt` (`settings::SPEECH`), the panel's Speech-to-text row: `superwhisper` or one of Whisper's models, written as `stt.kind` and `whisper.model` together.
- **Tests:** `crates/lucy/tests/support/mod.rs` gives each test its own XDG dirs, an in-process lucyd with inert adapters, and a `PATH` holding only the fakes a test writes (`Home::fake`, logged to `<name>.log`). `Home::with_mic(level)` gives its lucyd a microphone that stays at that RMS level, for the meter. `Home::tui` runs `lucy` in-process on ratatui's `TestBackend`, pressed by a `ui::Script`, with a `FakeStore` keyring (`Home::tui_with` takes an `Env` and store); a script that runs out, or has keys left over, fails the test. `Tui::shown` is every piece of text shown, rows and options included; `Tui::screen` is the last screen. `Home::spawn_lucyd` starts lucyd mid-test, as a fake `systemctl start` would. Plain commands still run the real binary.
- **Tests inside lucyd:** `test.mic_level` and `test.wake_word` run in the Session runner (`runner/setup.rs`), refused while a Session is open. Under `Harness::hearing`, the fake wake word's score is each frame's first sample.
- **systemd:** the units are `dist/systemd/*.service`. When systemd doesn't know `lucyd.service`, the Service section and starting lucyd write them to `$XDG_CONFIG_HOME/systemd/user/` running the binaries beside `lucy`. `systemd::run` (the Service section's letters, and `lucy service start|stop|restart`) clears a crashed lucyd's start limit before a start or restart. The Service section's row asks whether Lucy starts at login, so a yes enables again even when lucyd already is. Tests use a fake `systemctl`; never enable the units in your own session.

## Bar panel

- **Shape:** `omarchy-plugin/` is an Omarchy (Quickshell) bar-widget plugin. Its QML holds no product knowledge: it reads `lucy status --follow --json` (`lucy::panel`), writes through `lucy set`/`lucy unset` (a choice's `key`), and runs `lucy service` and `lucy end`, never `systemctl`. Option labels come from `i18n/en.json` where it has an `option.<value>` string (Superwhisper), else the value itself. A new choice goes in `panel::Snapshot` with its key, plus a row in `Panel.qml` that binds it by its snapshot name; `Service.qml` reads every choice the snapshot holds.
- **States:** lucyd's socket says waiting or session; while it's away, `systemd::activity` (`systemctl show`) says stopped, starting or failed. lucyd is `Type=exec`, so systemd calls it active before it listens: active without a socket is starting for `panel::STARTUP` (a minute), then failed.
- **Install:** each yes to starting at login in the Service section links the plugin to `$XDG_CONFIG_HOME/omarchy/plugins/lucy-panel` (`Env.omarchy`) when that dir exists, replacing an older link (`panel::link`). It is found through `Env.bin`: `omarchy-plugin/` beside `lucy`, else in the checkout whose `target/<profile>/` holds `lucy` (with the shared `target/`, the main checkout). If neither has it, the section says so and the enable stands. The user adds it to the bar. Lucy never edits `shell.json`.
- **Checks:** `omarchy-plugin/tests/run.sh` runs `Panel.qml` offscreen in Quickshell against a fake `lucy`, `systemctl` and `omarchy-launch-tui`, then checks the commands it ran and that none was `systemctl`; `omarchy plugin validate omarchy-plugin` checks the manifest. Neither is in `scripts/check.sh`, since CI has no Quickshell: run both after changing the plugin.

## Unix sockets

Socket paths are limited to about 108 bytes. The session scratchpad path is too long for a smoke run, so put the socket under `/tmp` or a short tempdir.

## Implementer subagents

The `tdd` skill asks the user to approve seams first, and a subagent can't ask. The ticket's acceptance checks driven through the harness, plus the adapter traits above, are the approved seams. A ticket that needs a new adapter trait names it in the implementer's report. The ticket's `Seams:` line says which shared seams it owns and which it only uses.

The last step before reporting: stop every background Bash task you started (TaskStop each one) and confirm none remain. A running one keeps your worktree locked.

Worktrees share the main checkout's `target/` and build with 8 jobs, through `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS` in the machine's `.claude/settings.local.json`. Keep them as set. "Blocking waiting for file lock on build directory" means another worktree is building; wait for it. On a new machine, create that file with its own checkout's path:

```json
{ "env": { "CARGO_TARGET_DIR": "/path/to/lucy/target", "CARGO_BUILD_JOBS": "8" } }
```

Without it, each worktree builds everything cold in its own `target/`, and parallel agents can stall the desktop.

## Merging

A merge that combines two tickets' changes to the same seam (both edit one adapter trait, the Session start, the notification fallback) gets one harness test of the combination before the merge commit. Each ticket's own tests pass alone; the combination is where the bugs were in slice 2 (a refused Provider swallowing the first-run introduction; a TTS-failure notification replacing the End button).
