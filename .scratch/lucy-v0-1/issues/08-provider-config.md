# How are Providers and configuration structured, and what does the v0.1 TUI cover?

Type: grilling
Status: resolved
Blocked by: 03, 04, 05, 15, 17, 18

## Question

Define the Provider kinds (wake word, STT, thinking, TTS, …), the transport axis (spawn, persistent, Herdr), the config file format and location, how secrets/logins are handled, and which settings the v0.1 TUI must expose vs leave to the file. Also cover: guarding against `--bare` becoming the `-p` default (version pin or feature detection; does `claude setup-token` keep subscription auth?), stripping API-key env vars so subscriptions are used, and Provider health checks that make a real call rather than trusting `login status`. Also decide whether the wake-word engine is itself a Provider, and whether sherpa-onnx should be a shared local runtime covering STT, VAD, TTS and keyword spotting. Also confirm (with the user's consent, on a throwaway WAV) what `superwhisper transcribe FILE` outputs, whether it writes to Superwhisper's history, and whether its cleanup LLM is applied. Also: the TTS Provider interface exposes each voice's sample rate; decide whether barge-in officially requires PipeWire or PulseAudio (plain ALSA has no drift correction), and the fallback/rescan policy when a pinned audio device disappears. This includes the ONNX runtime question: pure-Rust tract vs linking native ONNX Runtime (Silero, wake word, Smart Turn all depend on it). Also: two Model tiers (fast, advanced) as separately chosen Providers; setup detects installed tools and researches/recommends models per tier, and the user chooses. Also decide whether Zed's Agent Client Protocol replaces the separate Claude (stream-json) and Codex (app-server) clients with one Rust client: measure `claude-agent-acp` latency (Node layer over the Agent SDK), check the subscription-terms picture through that adapter, and verify `codex-acp`'s licence.

## Answer

- **Provider kinds.** There are three: STT, Thinking and TTS. The wake word, VAD and AEC are internal components behind traits, with their own settings but no config slot for swapping them.
  - **Thinking:** each Thinking Provider is named in config (`[providers.<name>]`), and `[tiers] fast = "…", advanced = "…"` points each Model tier at one.
  - **STT:** kinds `whisper` (default) and `superwhisper` (optional).
  - **TTS:** kind `piper`. The TTS interface exposes each voice's sample rate.
- **No transport axis.** Each Thinking kind runs exactly one way, as one process per Session:
  - `claude`: `claude -p` stream-json.
  - `codex`: `codex app-server`.

  **Herdr is out of scope for v0.1.** The ACP adapters don't replace the native clients: they are Node wrappers that add nothing, and `claude-agent-acp` sits under the Agent SDK login clause. Delegates use the same native clients ([ADR 0002](../../../docs/adr/0002-plugins-over-mcp-delegates-over-zed-acp.md), amended).
- **Config.** One TOML file at `$XDG_CONFIG_HOME/lucy/config.toml`, editable by hand. The TUI writes it with `toml_edit`, so comments survive. It is reloaded at the start of each Session. Runtime state lives under `$XDG_STATE_HOME/lucy/`. Approvals and Plugins stay where 19 put them.
- **Secrets.** Lucy stores none in v0.1. Each CLI uses its own login, and Lucy never reads or proxies it. Any future API keys come from the Secret Service (`keyring`), never from the TOML.
- **Subscription guard.**
  - **Claude:** child processes have the vars that outrank the subscription scrubbed: the Bedrock, Vertex and Foundry switches, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `apiKeyHelper`. `CLAUDE_CODE_OAUTH_TOKEN` is kept.
  - **Codex:** pinned with `-c forced_login_method="chatgpt"`.
  - **Opt-out:** a per-Provider `allow_api_key = true` turns the guard off.
  - **`--bare`:** never passed, and there's no version pin. When `claude --version` changes, Lucy re-verifies before the next Session: `auth status --json` must show `authMethod: "claude.ai"`, and one turn must show `apiKeySource: "none"` plus a `rate_limit_event`. If that fails, she refuses that Provider and says why.
- **Health checks.**
  - On every launch: the binary is present and the auth status is right.
  - A real "Reply OK" round-trip at setup, from the TUI status page, and after a failure mid-Session.
  - Failures are spoken plainly with the fix, and Lucy never falls back silently to another Provider.
- **Model recommendations.** A per-tier table ships with Lucy and is updated each release. Setup detects `claude` and `codex` and recommends from it, and the user can pick any model the CLI accepts.
- **Audio.**
  - **Supported stack:** PipeWire. On PulseAudio or plain ALSA, barge-in is off by default, and a config switch forces it on with a warning.
  - **Pinned devices:** re-resolved by name at the start of each Session. If one is missing, Lucy uses the default with one notification and keeps the pin.
  - **Device lost mid-Session:** the Session ends, with a notification.
- **ONNX runtime.** The `ort` API everywhere, on the pure-Rust **tract** backend by default. If Silero fails on tract (26), it alone falls back to native ORT via `load-dynamic` against the system `onnxruntime`, with no CDN download. sherpa-onnx is the documented runner-up, not adopted in v0.1.
- **Superwhisper.** An optional STT Provider, offered only if it's installed.
  - **How Lucy calls it:** she writes 16 kHz mono, calls `transcribe FILE`, then polls `read`.
  - **What the TUI warns about:** ~250 ms, the cleanup LLM always runs, and every utterance is saved to Superwhisper's history.
- **TUI scope.** First-run setup:
  - detect tools and offer the Piper install;
  - pick a Provider for each tier;
  - pick the STT model;
  - pick a TTS voice, with a preview;
  - pick the mic and speaker, with a level meter;
  - test the wake word and see its score.

  After setup:
  - a Provider status page with a "run real check" button;
  - a list of Approvals that the user can revoke there (granting stays spoken).

  Everything else is file-only: thresholds, timeouts, silence length, endpointing, AEC/VAD tuning and paths.

[findings](../research/provider-config.md)
