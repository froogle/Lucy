# Provider and configuration facts

Gathered for ticket 08 on 2026-11-12, from docs, repos and a few local runs. Claude Code was 2.1.322 and Codex 0.167.2.

## Claude Code auth

- **`--bare` is still slated to become the `-p` default.** The headless docs say it "will become the default for `-p` in a future release". No version or date is given, and the CHANGELOG has nothing on it yet. Under `--bare`, Anthropic auth is "strictly ANTHROPIC_API_KEY or apiKeyHelper via --settings (OAuth and keychain are never read)". So a subscription can't be used under `--bare`.
- **`claude setup-token`** gives a one-year subscription OAuth token, used through `CLAUDE_CODE_OAUTH_TOKEN`. `--bare` doesn't read that variable either, so `setup-token` is no way around `--bare`.
- **Credential precedence**, highest first, per the authentication docs:
  1. `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` or `CLAUDE_CODE_USE_FOUNDRY`
  2. `ANTHROPIC_AUTH_TOKEN`
  3. `ANTHROPIC_API_KEY` (in `-p` mode it is always used when present)
  4. `apiKeyHelper`
  5. `CLAUDE_CODE_OAUTH_TOKEN`
  6. a profile or the federation variables
  7. `/login` OAuth

  Lucy scrubs everything above `CLAUDE_CODE_OAUTH_TOKEN`.
- **Checking the auth mode**:
  - `claude auth status --json` makes no model call. It reports `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType` and `loginExpiresAt`.
  - On a real turn, `system/init` reports `apiKeySource: "none"`, and a `rate_limit_event` arrives (with `five_hour` and `seven_day` windows). That combination confirms subscription billing.

## Codex auth

- **Billing env vars:** `CODEX_API_KEY` isn't present in the 0.167.2 binary. An `OPENAI_API_KEY` in the environment didn't override the ChatGPT login (`codex login status`).
- **Pinning the login:** pin `-c forced_login_method="chatgpt"`.

## ACP adapters

Both are Apache-2.0, read from source and not run.
- **`codex-acp` v2.6.0** is now an npm package that starts a bundled `codex app-server`.
- **`claude-agent-acp` v0.106.0** is Node ≥22, built on `@anthropic-ai/claude-agent-sdk`, which is under Anthropic's Commercial Terms. It uses the claude.ai login by default.
- **The SDK clause:** the Agent SDK overview says: "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK."
- **Capability:** cancel works and streaming is at token level. No latency figures are published.
- **Verdict:** no gain over the native clients, and more policy exposure.

## ONNX runtime

- **What each component uses:**
  - `livekit-wakeword` uses `ort` 2.0.0-rc.11 with `ort-tract`, so it is pure Rust.
  - `voice_activity_detector` uses native ORT, which it downloads.
  - `silero-vad-rust` uses native ORT through `load-dynamic`.
- **tract:** it doesn't support dynamic `If`. Silero now ships an If-less model, but whether it runs on tract is unverified (ticket 26).
- **`ort` 2.0.0-rc.15** (MIT/Apache) downloads pyke's ORT 1.31 by default, or links a system ORT. Arch has `onnxruntime-cpu` 1.29, which needs `default-features=false` plus `api-29`.
- **sherpa-onnx 2.0.1** (Apache-2.0, official crate) removed espeak-ng and piper-phonemize in 2.0 and covers ASR, VAD, KWS and TTS. It bundles its own static ORT. Whether that clashes with a native ORT in the same process is unverified.

## Superwhisper `transcribe` probe

One file: a Piper WAV resampled to 16 kHz mono.
- **Async:** `transcribe FILE` returns `{"ok":true,"session_id":N}` immediately. `superwhisper read` returns the result, with `text`, `raw`, timings, `delivery: "none"` and `route: "none"`.
- **Input format:** it rejects anything that isn't mono 16 kHz ("audio must be mono 16kHz, at least 0.1 seconds"). The failure is reported only through `read` and `status`.
- **Latency:** `release_to_text_ms` was ~246, made up of 160 ms of ASR (Cohere Transcribe q4) plus ~50 ms of cleanup through the S1 Mini LLM. That's slower than the ~130 ms recorded in 05.
- **Cleanup LLM:** it is always applied (`cleanup_ms`, `llmResult`), and the raw and cleaned text matched here.
- **History:** every successful transcription is written to Superwhisper's history (`recordingDevice: "From File"`). The failed one was not. `superwhisper history delete TIMESTAMP --confirm` removes an entry, and the probe's entry was removed.

## Sources

- https://code.claude.com/docs/en/headless
- https://code.claude.com/docs/en/authentication
- https://code.claude.com/docs/en/agent-sdk/overview
- https://learn.chatgpt.com/docs/auth
- https://github.com/agentclientprotocol/claude-agent-acp
- https://github.com/agentclientprotocol/codex-acp
- https://github.com/pykeio/ort
- https://github.com/sonos/tract/issues/1029
- https://github.com/snakers4/silero-vad/issues/728
- https://github.com/k2-fsa/sherpa-onnx/pull/4055
