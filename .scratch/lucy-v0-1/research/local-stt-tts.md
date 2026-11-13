# Local STT/TTS on this machine: Whisper, Piper, Superwhisper

Research for ticket 05, run 2026-11-12. The question: can local Whisper (via whisper-rs), Piper, and an optional Superwhisper adapter respond fast enough for Lucy, and what should the defaults be?

## TL;DR

- **STT:** whisper.cpp is easily fast enough for utterance-at-a-time commands. On this machine, a 3.4 s command takes about 0.1–0.13 s of inference with `base.en` on CPU or `small.en` on Vulkan. Default to **`small.en` on Vulkan, falling back to `base.en` on CPU**, and transcribe one utterance at a time after VAD endpointing rather than streaming. `large-v3-turbo-q5_0` on Vulkan (about 0.1–0.25 s) is a good "accurate" option. Use whisper-rs with the `vulkan` feature. whisper-rs now lives on Codeberg, and its GitHub repo is archived.
- **TTS:** Piper is fast enough. For a medium voice, time to first audio is about 40–80 ms per sentence once the voice is loaded, and RTF is about 0.02–0.05 on CPU. "High" voices take about 3–6x longer and are still fine.
- **Piper licensing is the catch.** Upstream moved to `OHF-Voice/piper1-gpl`, which is **GPL-3.0**. Every Piper path embeds **espeak-ng (GPL-3.0)**, including the Rust crate `piper-rs`, which compiles espeak-ng in. To keep Lucy MIT/Apache, run Piper **out of process** as a persistent helper that streams raw PCM. Do not link it.
- **Default voice:** `en_GB-cori-medium`. Its dataset is CC BY 4.0, and it was fine-tuned from a base trained on the same CC BY recordings. Avoid `lessac` and voices derived from it, which carry a non-commercial licence.
- **Superwhisper adapter:** this is viable as an optional Provider using `superwhisper transcribe FILE` (asynchronous: poll `superwhisper read`; 16 kHz mono only; the cleanup LLM always runs; each utterance is saved to its history; ~250 ms measured in 08, see `provider-config.md`). It is proprietary, but invoking a separately installed CLI does not affect Lucy's licence. Its warm latency on this GPU was about 130 ms from release to text in this run; 08 measured ~250 ms including the cleanup LLM. Do not drive Superwhisper's own `record/stop`: they paste into the focused window (`autoPasteEnabled: true`) and compete for the microphone.

## This machine

From `lscpu`, `free`, `lspci` and `nvidia-smi`:

| | |
|---|---|
| CPU | AMD Ryzen 7 7700X, 8 cores / 16 threads, AVX2 + AVX-512F + FMA + F16C |
| RAM | 30 GiB (about 12 GiB available at test time) |
| GPU | AMD Radeon RX 7800 XT (Navi 32), RADV Vulkan. No NVIDIA (`nvidia-smi` fails), so no CUDA. |
| Tools present | `cmake`, `glslc`, Vulkan headers, `espeak-ng`, `ffmpeg`, `cargo`, `uv` |

Superwhisper's resident workers were already holding the GPU through Vulkan while these tests ran (see `superwhisper status` → `workers.asr_backend: vulkan`). The GPU numbers below are therefore for a shared GPU.

## 1. whisper.cpp / whisper-rs

### Sources
- whisper.cpp: https://github.com/ggml-org/whisper.cpp. MIT, active, built at `d2c6f05` (2026-11-10), libwhisper 1.9.5.
- whisper-rs: crates.io `whisper-rs` 0.16.0 (2026-03-12), licence **Unlicense**. Its repository field and the README of the archived GitHub repo (https://github.com/tazz4843/whisper-rs) say it moved to https://codeberg.org/tazz4843/whisper-rs and remains maintained. Crate features include `vulkan`, `hipblas`, `cuda`, `openblas` and `openmp`. It bundles `whisper-rs-sys` 0.15.0 (March 2026), so it lags upstream whisper.cpp by months.
- Models: https://huggingface.co/ggerganov/whisper.cpp (ggml files; the OpenAI Whisper weights are MIT).

### Benchmark method
- whisper.cpp was built twice in scratch: a CPU build and a `-DGGML_VULKAN=ON` build. No system installs.
- `whisper-cli -t 8`. Each run was preceded by a warm-up run. The numbers are whisper's own `load time` and `total time`. Inference is roughly total minus load. In a resident daemon, load is paid once.
- Clip A: a 3.37 s command ("Turn the volume down and open my calendar for tomorrow."), synthesised with Piper and resampled to 16 kHz. It is clean audio, so it measures speed, not accuracy. Every model transcribed it perfectly.
- Clip B: `samples/jfk.wav` (11 s of real speech).
- `-ac 384` sets `audio_ctx`, which shrinks the encoder's fixed 30 s window to about 7.7 s. This is the standard whisper.cpp trick for short commands, at a small accuracy cost.

### Results: 3.4 s command (ms, total − load ≈ inference)

| model (size) | CPU, full ctx | CPU, `-ac 384` | Vulkan, full ctx | Vulkan, `-ac 384` |
|---|---|---|---|---|
| tiny.en (75 MB) | 188 − 27 ≈ **161** | 97 − 29 ≈ **68** | 98 − 42 ≈ **56** | 89 − 42 ≈ **47** |
| base.en (142 MB) | 351 − 37 ≈ **314** | 150 − 38 ≈ **112** | 126 − 57 ≈ **70** | 111 − 58 ≈ **53** |
| small.en (466 MB) | 1294 − 111 ≈ **1183** | 513 − 114 ≈ **399** | 265 − 139 ≈ **126** | 231 − 139 ≈ **92** |
| large-v3-turbo q5_0 (574 MB) | 5306 − 141 ≈ 5165 | 1463 − 132 ≈ 1331 | 384 − 171 ≈ **213** | 265 − 159 ≈ **106** |

### Results: 11 s real speech (jfk.wav), full ctx, ms total (load)

| model | CPU | Vulkan |
|---|---|---|
| base.en | 411 (37) | 173 (58) |
| small.en | 1436 (118) | 341 (140) |
| large-v3-turbo q5_0 | 5482 (143) | 432 (159) |

### Streaming vs utterance-at-a-time
- Whisper's encoder always processes a fixed window: 30 s, or less if `audio_ctx` is set. Short audio therefore costs about the same as long audio, which is why `-ac` matters so much on CPU. Re-running the model every 500 ms ("streaming") multiplies that fixed cost. whisper.cpp's own `examples/stream` README calls this "a naive example". Its better mode is sliding-window with VAD (`--step 0`), which transcribes only after speech activity ends. That is effectively utterance-at-a-time.
- whisper.cpp has built-in Silero VAD support (README, "Voice Activity Detection (VAD)" section).
- **Recommendation:** use utterance-at-a-time. Lucy already needs endpointing for Sessions, so detect end of speech with VAD and transcribe the whole utterance once. With `small.en` on Vulkan, the post-speech delay is about 0.1–0.15 s, well under anything a user would notice next to LLM latency. Partial live captions are not needed for v0.1.

### Recommended STT defaults
- Default model: `small.en` with Vulkan when a Vulkan device exists, otherwise `base.en` on CPU with `audio_ctx` about 384–512 (about 0.1 s here). Offer `large-v3-turbo-q5_0` as an "accurate, needs GPU" choice and `tiny.en` for weak machines. Use multilingual (non-`.en`) variants when the configured language is not English.
- Keep the model resident in the Lucy daemon. Load is only 30–170 ms, but there is no reason to pay it on every utterance.
- Build whisper-rs with `features = ["vulkan"]`. This needs `glslc` and Vulkan headers at build time, which matters for packaging.

## 2. Piper TTS

### Sources
- https://github.com/rhasspy/piper is **archived** (MIT). Its README says: "Development has moved: https://github.com/OHF-Voice/piper1-gpl".
- https://github.com/OHF-Voice/piper1-gpl is **GPL-3.0** and active. The latest release is v1.8.0 (2026-09-04), published on PyPI as `piper-tts`. The README says it "embeds espeak-ng for phonemization", and espeak-ng is GPL-3.0 (https://github.com/espeak-ng/espeak-ng).
- `docs/LICENSING.md`: "The Piper engine is GPL-3.0-or-later". Voice rights come from each voice's `MODEL_CARD`, and you have to walk the fine-tune chain. Lessac-derived voices are limited to "research and exploration" and exclude commercial use.
- `libpiper/README.md`: a C API (`piper_create`, `piper_synthesize_next`). Its build downloads and builds espeak-ng and downloads onnxruntime.
- `docs/STREAMING.md`: by default, libpiper synthesises a whole sentence per call. If a voice is split into encoder and decoder files (`python3 -m piper.split_voice_for_streaming model.onnx`), `piper_synthesize_next` returns sub-sentence chunks.
- `docs/CLI.md`: the CLI reloads the model on every run and is "slow". Use the HTTP server, or `--output-raw` for streaming raw audio to stdout.
- Rust crate `piper-rs` 0.2.0 (MIT, https://github.com/thewh1teagle/piper-rs). It uses `ort` (onnxruntime) plus its own `espeak-rs` crate, which pulls espeak-ng in as a git submodule and compiles it. **Linking it puts GPL-3.0 code into Lucy's binary.** The README links the old rhasspy repo, so it is unclear whether it supports newer v1.5+ voices (MRD/SDP, which `cori-medium` needs).

### Benchmark: piper-tts 1.8.0 (onnxruntime 1.31.1, CPU), Python API in a scratch venv
Measured after warm-up. "First chunk" is the time until the first sentence's audio is ready.

| voice | load | short (2.2–2.9 s audio) first chunk | long (10 s, 2 sentences) first chunk / total | RTF |
|---|---|---|---|---|
| en_GB-cori-medium (63 MB) | 648 ms | **70 ms** | 39 ms / 360 ms | 0.03 |
| en_GB-alba-medium (63 MB) | 570–1130 ms | **46–83 ms** (one cold outlier at 209) | 72 ms / 555 ms | 0.02–0.07 |
| en_GB-cori-high (114 MB) | 1599 ms | 647 ms | 196 ms / 1918 ms | 0.18–0.29 |

Takeaways:
- Medium voices on this CPU produce a short sentence in under 100 ms. That is fast enough without the encoder/decoder streaming split, provided Lucy sends text **sentence by sentence** as the LLM streams it and starts playback on the first sentence.
- A "high" voice puts about 0.6 s before the first sound of a one-sentence reply. That is acceptable, but use medium as the default.
- Voice load (0.6–1.6 s) must happen once, at daemon start, not per reply. This rules out spawning the `piper` CLI per utterance.

### Streaming from Rust without GPL contamination
Options, best first:
1. **Persistent helper process (recommended):** spawn one long-lived Piper process per Session or per daemon and write sentences to it. It streams raw PCM back (s16le, 22050 Hz mono for these voices), and Lucy plays it with a Rust audio crate (cpal/rodio, or a `pw-play` pipe). Candidates for the helper:
   - `python -m piper.http_server` (documented in `docs/API_HTTP.md`). This needs Python on the user's machine.
   - A tiny separately distributed GPL helper binary built on libpiper. This would ship as its own GPL package, which also enables sub-sentence streaming through split voices.
   - Note: the CLI's `--output-raw` streams one invocation's text, and each invocation reloads the model. Check whether the CLI keeps reading lines from stdin without reloading before relying on it (not verified here).
2. In-process `piper-rs`/libpiper: lowest latency and simplest packaging, **but makes the Lucy binary GPL-3.0**. This is only acceptable if Lucy chooses GPL.

### Voice defaults
- Default: **`en_GB-cori-medium`**. Its dataset is the Hi-Fi TTS dataset (CC BY 4.0, NVIDIA), and it was fine-tuned from `hifi_tts_92`, which was trained on the same reader. The chain is clean, the voice was fastest in the tests, and it needs Piper 1.5 or later.
- Avoid as defaults: `en_US-lessac-*` and anything whose Training line traces to lessac (for example `en_GB-northern_english_male-medium`), `en_US-ryan-high` (CC BY-NC-SA), and `hfc_*` (CC BY-NC-SA).
- Do not bundle voices. Download on first run and keep each voice's `MODEL_CARD` (LICENSING.md asks redistributors to carry it).

## 3. Superwhisper as an optional STT Provider

### Sources (local, read-only)
- `superwhisper --help`, `superwhisper status`, `superwhisper models show` and `superwhisper general show`.
- The vendor's agent skill at `/opt/superwhisper/assets/agent-skill/superwhisper/SKILL.md`.
- The `pacman -Qi superwhisper-bin` package metadata (version 1.2.0-1, licence `LicenseRef-proprietary MIT`).
- Not run, per the brief: `record/start/stop` and `transcribe`. Transcribing a file probably adds a history entry, and I did not want to touch history. The exact stdout shape of `transcribe` therefore still needs confirming.

### Interface facts
- It runs as the user service `superwhisper.service`. The CLI at `~/.local/bin/superwhisper` talks to that service and starts it if needed. "Commands print JSON."
- Relevant commands: `transcribe FILE [--insert]` (pastes only with `--insert`), `read` (most recent dictation), `record|start|stop [--device NAME]`, `cancel`, and `status [--bar --follow]` (a live status stream).
- `status.last` exposes `raw` (the ASR output) separately from `text` (after the "cleanup" LLM `s1-mini` applies tone and rewrites). For commands to Lucy, prefer `raw`, or make sure cleanup does not paraphrase the user.
- Modes are Offline (Cohere Transcribe q4 plus the S1 Mini cleanup model, already installed here), Offline Lite (Parakeet TDT 0.6B v3, not downloaded), and Cloud ("works on the free tier"). The current licence state is `unlicensed` with free-tier credentials.

### Latency (from `status.last` of a real dictation on this machine)
- 2.46 s of audio, Offline mode, on Vulkan (RX 7800 XT): `asr_ms` 87, `cleanup_ms` 25, `release_to_text_ms` **130**. That is comparable to whisper `small.en` or turbo on Vulkan. `transcribe FILE` adds the cost of spawning the CLI, an IPC round trip and a file write, probably tens of milliseconds (not measured).

### Fit as a Lucy Provider
- **Usable:** Lucy records and endpoints audio itself, writes a WAV file to `$XDG_RUNTIME_DIR`, runs `superwhisper transcribe file.wav`, and parses the JSON. This is a clean spawn-transport Provider, and Lucy keeps control of the mic, VAD and wake word.
- **Avoid** `record/stop` and `read`. With `autoPasteEnabled: true` (the current setting), `stop` pastes text into whatever window has focus. It would also grab the mic while Lucy's wake-word listener holds it. `read` only returns the most recent dictation, which can race with the user's own dictations.
- **Side effects to document:** transcriptions probably appear in Superwhisper's history (`linux.historyEnabled: true`, `recordingPreservationPeriod: 0` keeps entries forever), and vocabulary auto-learning is on.
- **Licensing:** Lucy only executes a binary the user installed separately. Lucy ships no Superwhisper code, so its MIT/Apache licence is unaffected. The adapter should detect `superwhisper` on PATH and degrade gracefully. Superwhisper's own terms (proprietary, free tier versus licensed) are the user's business. Do not make it a default.

## Recommended defaults (summary)

| Capability | Default | Alternatives |
|---|---|---|
| STT engine | whisper.cpp through whisper-rs (Unlicense, in process), resident model, VAD-endpointed utterance-at-a-time | Superwhisper (`transcribe FILE`, spawn), `whisper-stream`-style partials later |
| STT model | `small.en` + Vulkan if available, else `base.en` CPU with `audio_ctx` ≈ 384–512 | `large-v3-turbo-q5_0` (GPU), `tiny.en` (weak HW), multilingual variants |
| TTS engine | Piper as a **separate persistent process** streaming PCM, fed sentence by sentence | in-process `piper-rs` only if Lucy goes GPL |
| TTS voice | `en_GB-cori-medium` (CC BY 4.0 chain), downloaded on first run with its MODEL_CARD | `en_GB-cori-high` (≈0.6 s first audio), other non-lessac voices |

Expected voice-loop overhead on this machine, excluding the LLM: about 0.1–0.15 s for STT after end of speech, plus about 0.05–0.1 s to the first TTS audio. That is roughly 0.25 s plus VAD hang time. The thinking Provider will dominate.

## Open questions surfaced
1. **Licence choice versus Piper/espeak-ng (GPL-3.0):** pick MIT/Apache with an out-of-process Piper helper, or pick GPL-3.0 for Lucy and link Piper directly. This choice also affects any other espeak-based TTS.
2. **How to ship the Piper helper:** require `pip install piper-tts` and its HTTP server, or build and ship a small libpiper-based GPL helper binary (an AUR split package)? Linked to the Publishing item in the map.
3. **Exact output of `superwhisper transcribe`**, its history side effects, and whether it applies the cleanup LLM. Confirm with the user's consent on a throwaway WAV file.
4. **Parakeet as an alternative local STT:** whisper.cpp now ships `include/parakeet.h` and `examples/parakeet-cli` (Parakeet TDT 0.6B v3, 25 European languages), but whisper-rs does not expose it yet. It is worth a later look for speed and accuracy.
5. **GPU contention:** Superwhisper's resident Vulkan workers and Lucy's whisper model would share the GPU and VRAM. This was untested; check if both run together.
