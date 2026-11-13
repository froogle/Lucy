# Does the full voice loop (wake word → speech-to-text → Claude → speech) feel fast enough?

Type: prototype
Status: resolved
Blocked by: 03, 05

## Question

Build a throwaway end-to-end loop (wake word stub → STT → persistent Claude Code → streamed TTS) and judge with the user whether turn latency feels conversational. Decide the latency budget per stage and whether tricks (filler acknowledgements, streaming TTS from partial text) are needed. Also measure echo cancellation (echo reduction and residual) on laptop speakers vs desk speakers vs headphones, and whether `sonora` matches the C++ `webrtc-audio-processing`. Also decide how barge-in interrupts a Claude turn: the undocumented stream-json control/interrupt message vs SIGINT and respawn. Also measure the wake-word engine over a few hours of normal desktop audio: CPU, memory and false accepts, with and without the echo-cancellation chain in front of it. Also check GPU contention when Superwhisper's resident Vulkan workers and Lucy's Whisper model share the GPU. Also verify cpal's PipeWire backend accepts the 16 kHz capture request without a format mismatch, that `pw-top` shows both of Lucy's streams under one driver, and what happens when a pinned device disappears (cpal has no hot-plug events). Also check whether Silero's If-less ONNX export runs correctly on pure-Rust `ort-tract`, and A/B Silero against earshot.

## Answer

**Verdict: the loop is fast enough.** The user held a live conversation (Enter as the wake-word stub, `small.en` on Vulkan, persistent `claude -p` on Sonnet, Piper `cori-medium`) and judged it natural. Per-turn tables weren't recorded. The budget below is built from the stage measurements taken in this session and in 03 and 05.

**Latency budget (speech end → first audio ≤ 2.5 s):**

| Stage | Budget | Measured |
|---|---|---|
| End-of-utterance silence (ticket 14) | 700 ms | fixed; the one stage to tune |
| STT, `small.en` Vulkan, per utterance | ≤ 150 ms | ~100–150 ms (05); model load 0.3 s once at startup |
| Claude first token, warm | ≤ 1.5 s | 0.6–1.5 s (03), 1.4–2.0 s on Sonnet here |
| First complete sentence | + ~200 ms | |
| Piper, per sentence | ≤ 100 ms | 12–70 ms (RTF ~0.013) |

**Tricks:** keep the planned sentence chunking: each sentence goes to Piper as soon as Claude's stream completes it. Sub-sentence streaming Piper and filler acknowledgements aren't needed for v0.1. A persona prompt that asks for one to three short spoken sentences, with no markdown, keeps replies speakable and keeps the first sentence early.

**Barge-in on a Claude turn: use the stream-json control interrupt.** Write `{"type":"control_request","request_id":…,"request":{"subtype":"interrupt"}}` to stdin.

| Method | Turn ends | Process | Next turn |
|---|---|---|---|
| control interrupt | 6 ms, `control_response` success, `result` `error_during_execution` | survives | works; the partial reply stays in context |
| SIGINT | 7 ms | **exits** after the turn | needs a respawn (1–2 s); context lost |

The interrupt is undocumented. Lucy feature-detects it (a success `control_response`) and falls back to SIGINT plus respawn.

**Piper facts for the helper:** `POST /synthesize` returns a whole WAV (no streaming). The server binds **`0.0.0.0` by default**, so Lucy must pass `--host 127.0.0.1`. Lucy should launch it with a clean environment: a global `PYTHONPATH` (as on this machine) breaks a venv install. The AUR `piper-tts` package can't be installed right now (see the note on [Which licence does Lucy ship under, given Piper and espeak-ng are GPL-3.0?](17-licence.md)). `uv` into a venv worked.

**cpal:** the default PipeWire input accepted a 16 kHz mono f32 request with no format mismatch.

**Split out** (not answered here): [How well does echo cancellation work on real speakers, and does sonora match the C++ library?](23-aec-on-real-speakers.md), [Does the wake-word engine hold up over hours of normal desktop audio?](24-wake-word-soak.md), [Does Lucy's audio stack coexist with the desktop?](25-audio-stack-coexistence.md), [Does Silero run on pure-Rust ort-tract, and does earshot match it?](26-vad-runtime-ab.md).

[prototype](../prototypes/voice-loop/README.md) (`src/main.rs`, `interrupt_test.py`)
