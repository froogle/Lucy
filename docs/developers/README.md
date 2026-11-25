# Lucy developer guide

This guide is for people working on Lucy herself, or building on her: Plugins, widgets, Providers. If you only want to use Lucy, the [user guide](../user-guide/README.md) is the place to start.

## Contents

- **This page**: how Lucy fits together, the repository, and how to build, test and contribute.
- **[How Plugins work](plugins.md)**: discovery, Approval, the sandbox, how a call reaches a Plugin, Confirmations, credentials and mid-call questions. Includes the full Manifest reference.
- **[Writing a Plugin](writing-a-plugin.md)**: a step-by-step guide, from the template to an approved Plugin.
- **[The public socket](ipc.md)**: writing a client, such as a bar widget, against lucyd's JSON-RPC socket.
- **[Decision records](../adr/)**: why each part is built the way it is. Read the relevant one before changing a design.
- **[Glossary](../../GLOSSARY.md)**: the domain words, and the words to avoid. Code, docs and commits use these terms.

## How Lucy fits together

```
                    microphone                               speaker
                        │                                       ▲
┌───────────────────────┼───────────────────── lucyd ───────────┼──────────┐
│                       ▼                                       │          │
│   echo canceller → Wake word → VAD → Whisper ─┐      TTS engine (Piper,  │
│                                               │      Kokoro: local HTTP) │
│                                               ▼               ▲          │
│                                     Session runner ───────────┘          │
│                                       │       ▲                          │
│                         turns (stdin) │       │ replies, tool calls      │
│                                       ▼       │                          │
│                           thinking CLI: claude or codex                  │
│                                       │                                  │
│                         `lucy mcp-relay` (per-spawn token)               │
│                                       ▼                                  │
│                       Lucy's private MCP server (mcp.sock)               │
│                         ├─ Built-in tools: open_url, launch_app,         │
│                         │  find_files, system_status, update_persona …   │
│                         └─ Plugin tools, proxied as <plugin>__<tool>     │
│                                       │                                  │
│                                       ▼                                  │
│                  Plugins: MCP over stdio, each under Landlock            │
│                  Delegates: Authoring builder, Hand-off to `claude`      │
│                                                                          │
│   public socket: $XDG_RUNTIME_DIR/lucy/lucyd.sock (JSON-RPC 2.0)         │
└───────────────────────────────────┬──────────────────────────────────────┘
                                    │
             lucy (CLI and settings), lucy-indicator, the Omarchy bar panel
```

The key ideas:

- **One daemon owns everything** ([ADR 0006](../adr/0006-single-daemon-public-socket-contract.md)). `lucyd` owns the audio, the Session, every child process and every state write. Everything else (`lucy`, the Indicator, the bar panel) is a client of one public socket. The socket never carries transcripts, spoken text or tool arguments.
- **Thinking goes through the user's own CLI** ([ADR 0001](../adr/0001-subscription-backed-providers.md)). lucyd drives `claude` or `codex` as a child process, in a work folder of Lucy's own, with no built-in tools of its own. A **subscription guard** refuses a `claude` that would bill an API key.
- **The model sees one MCP server: Lucy's** ([ADR 0005](../adr/0005-proxied-plugin-tools-runtime-owned-confirmation.md)). Plugin tools are proxied through it. A model call to `news__briefing` *is* the Plugin selection.
- **The runtime, not the model, hears every yes.** Confirmations and Approvals are spoken by lucyd with fixed text, and the reply is matched locally against a list of yes-phrases. The model only learns whether the user agreed.
- **Providers are swappable.** Each capability (speech-to-text, thinking, speech) has a `kind` read from `config.toml` at the start of each Session, behind an adapter trait. See [ADR 0013](../adr/0013-tts-providers-are-engines-behind-one-local-server-seam.md) for how TTS engines plug in.
- **Copyleft runs out of process** ([ADR 0003](../adr/0003-permissive-licence-gpl-out-of-process.md)). Lucy is MIT/Apache-2.0; Piper (GPL) runs as its own process behind a local HTTP server.

### A Turn, end to end

1. The microphone feeds a DSP thread that runs every 10 ms frame, in and out of Sessions: the echo canceller (AEC3, through the pure-Rust `sonora`), the Wake word model, then the Silero VAD.
2. The Wake word opens a Session. The VAD cuts the utterance; Whisper turns it into text.
3. The Session runner sends the text to the fast tier's CLI as one Turn.
4. The CLI streams its reply. The adapter finds sentence ends, and each sentence goes to the TTS engine as soon as it's complete, so Lucy starts speaking before the model has finished.
5. A tool call reaches Lucy's MCP server through `lucy mcp-relay`. A Built-in tool runs in lucyd; a Plugin tool is checked (Confirmation, screen access, credentials), then forwarded to the Plugin.
6. While Lucy speaks, playback is fed to the echo canceller as its reference, so the user can cut in (Barge-in). A Barge-in drops queued speech, interrupts the model, and cancels any unconfirmed Plugin call.

## The repository

| Path | What's there |
|---|---|
| `crates/lucyd` | The daemon: audio, Session runner, Providers, MCP server, Plugin host, sandbox, Authoring and Hand-off. |
| `crates/lucy` | The `lucy` CLI and its full-screen settings menu (ratatui). The only writer of `config.toml`. |
| `crates/lucy-ipc` | The public socket's types, client, and generated JSON Schema (`schema/lucy-ipc.schema.json`). |
| `crates/lucy-config` | `config.toml`'s types and defaults, and the keyring seam for credentials. |
| `crates/lucy-indicator` | The on-screen Indicator (Wayland layer-shell). |
| `crates/lucy-news` | The shipped news Plugin. The reference for a network Plugin. |
| `crates/lucy-omarchy` | The shipped Omarchy desktop Plugin: Hyprland, Omarchy's settings, screen capture and input. |
| `crates/lucyd/plugin-template` | The template the Authoring builder starts each Plugin from. Also a good start for yours. |
| `omarchy-plugin/` | The Omarchy bar panel (QML for Quickshell). Holds no product logic: it reads and writes through `lucy`. |
| `dist/systemd/` | The systemd user units. |
| `scripts/` | `check.sh` (every gate), `build.sh` (the release bundle), `install.sh` and `uninstall.sh` (shipped in that bundle too), `clean.sh`, and the individual checks. |
| `tools/aec-bench/` | A bench that replays recordings through the echo canceller and Barge-in gate, for tuning. |
| `docs/adr/` | Architecture decision records. |
| `docs/agents/` | Notes for AI coding agents working in this repo, and the deepest notes on each area (see below). |

## Building

Install the build dependencies listed in the [README](../../README.md#2b-or-build-from-source), then:

```sh
cargo build --workspace
```

`rust-toolchain.toml` pins the Rust version. The workspace denies `unwrap()` outside tests (`clippy::unwrap_used`).

To run your build as your real Lucy, use `scripts/install.sh`: it builds for release through `scripts/build.sh`, installs, and restarts Lucy if she was running. `scripts/uninstall.sh` takes it all off again. The README's [Install](../../README.md#install) section says what goes where, and [Making a release](../../README.md#making-a-release) how a tagged release is built.

## Testing

```sh
cargo test --workspace
```

The tests never touch your microphone, speakers, keyring, desktop, network or subscription:

- **lucyd's harness** (`crates/lucyd/tests/harness/`) starts lucyd in-process with fakes for every Provider, the microphone, the speaker, the keyring and the clock, and talks to it through the real public socket. Timer tests advance a fake clock instead of sleeping.
- **Thinking CLIs** are replaced by a fake `claude` and `codex` that replay recorded transcripts (`crates/lucyd/tests/fixtures/`).
- **Plugins** are tested through their real MCP interface, with web requests answered from recorded files.
- **The settings menu** is driven in-process on ratatui's test backend.

Some tests are `#[ignore]`d because they use real hardware or a real subscription. They're run by hand, when a change needs one:

```sh
cargo test -p lucyd --test whisper --test silero --test mic -- --ignored   # real models and mic
cargo test -p lucyd --test claude -- --ignored    # spends your Claude subscription
cargo test -p lucyd --test codex -- --ignored     # spends your ChatGPT subscription
```

When you run one crate's tests alone, some need binaries built first: `cargo test -p lucyd` needs `cargo build -p lucy -p lucy-news -p lucy-omarchy` and `cargo build -p lucyd --examples`. `cargo test --workspace` takes care of this.

## Checks

`scripts/check.sh` runs every gate, the same as CI: `rustfmt`, `clippy -D warnings`, the tests, and these checks:

| Check | What it enforces |
|---|---|
| `check-client-deps.sh` | `lucy` and `lucy-indicator` never link audio or ML crates. |
| `check-glossary-terms.sh` | No type name uses a word the glossary says to avoid. |
| `check-lint-allows.sh` | A Clippy lint is silenced only with `#[expect(..., reason = "...")]`. |
| `check-test-sleeps.sh` | Every sleep in a test carries a `// sleep-ok:` comment saying why. |
| `check-comments.sh` | No plain `//` comments: only rustdoc, `// SAFETY:` and `// sleep-ok:`. Code explains itself. |
| `check-doc-summaries.sh` | A rustdoc summary (its first sentence) is at most 15 words. |
| `cargo deny check licenses` | Every dependency has a permissive licence. |

Turn on the pre-commit hook once per clone, so they run before each commit:

```sh
git config core.hooksPath .githooks
```

A commit that only touches markdown, `docs/` or `.scratch/` runs just the glossary check.

The bar panel's tests need Quickshell, so they aren't in `check.sh`. Run `omarchy-plugin/tests/run.sh` after changing it.

## Contributing

- **Read [CODING_STANDARDS.md](../../CODING_STANDARDS.md)** first. In short: names carry the meaning, comments say why, types make wrong states unrepresentable, tests check behaviour and prove ordering deterministically.
- **Use the glossary's words.** If a concept is new, add it to [GLOSSARY.md](../../GLOSSARY.md) in the same change.
- **Record decisions.** A change to how something is designed, or a choice between real alternatives, gets an ADR in `docs/adr/`.
- **The public socket is a contract.** New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Within a protocol version, fields are only ever added. Regenerate the committed schema with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`; a test fails if it drifts.
- **Nothing private reaches the public socket or the log.** Transcripts, spoken text, tool arguments, secrets and screenshots stay inside lucyd. The harness checks this after each Session (`assert_nothing_private_on_bus`).

### Area notes

[`docs/agents/working-in-lucy.md`](../agents/working-in-lucy.md) and the files under `docs/agents/working-in-lucy/` hold the detailed, non-obvious facts about each area: the harness, Providers, hearing, Plugins, Authoring and Hand-off, setup, and the bar panel. They're written for AI coding agents, but they're the most precise notes there are. Read the one for the area you're changing.

### Disk space

Builds share one `target/` folder, which only grows. `scripts/clean.sh` clears it (and the prototypes' folders) when you need the space back. The next build is cold.
