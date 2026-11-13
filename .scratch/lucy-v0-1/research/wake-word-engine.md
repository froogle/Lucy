# Local wake-word engine for "Lucy" from Rust

Ticket: [01-wake-word-engine](../issues/01-wake-word-engine.md). Researched 2026-11-12.

## Recommendation

Use **LiveKit WakeWord v2** as the default engine, through LiveKit's official Rust crate [`livekit-wakeword`](https://github.com/livekit/rust-sdks/tree/main/livekit-wakeword). The Lucy project trains one `lucy.onnx` model once and ships it. Its code, its encoders and the models built from them are all Apache-2.0, so they are compatible with MIT/Apache Lucy. To change the wake word, a user points config at another `.onnx` file. They can train one with LiveKit's documented recipe, or the project can publish a few pre-trained alternatives. Capture audio with `cpal` 0.18 and its native `pipewire` host feature, falling back to ALSA through `pipewire-alsa`. Pass 16 kHz or native-rate i16 PCM to the crate, which resamples internally. Put the engine behind a small `WakeWordDetector` seam so that something else can be swapped in later.

Why:

- **Licence.** It is the only candidate where both the runtime *and* the shipped model weights are explicitly permissive.
- **Accuracy.** It has the best published accuracy at a fixed false-alarm budget. The benchmark is the vendor's own, so see the caveats below.
- **Cost.** It needs about 1.65% of one core.
- **Footprint.** The default backend is pure Rust (tract), so Lucy needs no native ONNX Runtime.

Main risks:

- The v2 Rust support was merged on 2026-11-05 but **is not yet on crates.io**. crates.io has 0.1.3, which runs the deprecated v1 openWakeWord-style models. Until a release lands, Lucy has to use a git dependency.
- Training a new word needs a CUDA GPU or Apple-silicon Mac **plus a LiveKit Cloud account** (free plan) for TTS. Training is a one-off build-time step and inference never touches the network, but an end user cannot simply type a new word.

Runner-up and complement: **sherpa-onnx open-vocabulary keyword spotting**. It is Apache-2.0, has an official Rust crate, and changes the word from a text string with no training. That makes it the only option where "end users can change the word" is truly zero-effort. It is not the default because recall on short keywords is reported to collapse, and "Lucy" is only about 4 phones.

## Comparison

| Engine | Licence (code / models) | Custom word "Lucy" | Can end users change it? | Idle CPU | Reported accuracy | Rust story |
|---|---|---|---|---|---|---|
| **LiveKit WakeWord v2** | Apache-2.0 / **Apache-2.0** (NOTICE) | Train with synthetic TTS. About 11 min on a data-centre GPU, 59 min on an M4 Pro. Needs a LiveKit Cloud free project for TTS | Yes, by dropping in another `.onnx` (load/unload at runtime). Making one needs GPU and LiveKit account | 16.5 ms CPU per s of audio on 1 thread (about 1.65%). About 1 ms per 80 ms step. Model about 7 MB | 89% recall at 0.5 FA/h ("hey livekit", vendor bench). 0.5–2.5 FA/h at the default 0.5 threshold | Official crate. Pure-Rust `ort-tract` default. v2 unreleased on crates.io |
| **openWakeWord** (0.6) | Apache-2.0 / **CC BY-NC-SA 4.0** for all pre-trained models | Colab notebook (<1 h) or detailed notebook. Several thousand synthetic clips plus large negative set (about 30k h used upstream) | Same model-file swap. Training is heavier (94 min, 21–35 GB downloads per LiveKit's comparison) | 29.5 ms per s (LiveKit's measurement). "Pi 3 core runs 15–20 models" | Target <5% FRR at <0.5 FA/h. 46% recall at 0.5 FA/h on LiveKit's bench | No official crate. Community `oww-rs` 0.3.3 (MIT, tract, 7 stars) bundles NC-licensed alexa/hey_mycroft |
| **sherpa-onnx KWS** | Apache-2.0 / Apache-2.0 | **No training**: keyword is a tokenised text line (`text2token`), with per-keyword boost and threshold | **Yes, trivially**, even at runtime (`create_stream_with_keywords`) | Small (3.3M-param zipformer). No official idle figure | No published FA/FR figures. Issue #3809: a 3-phone keyword capped at 5/10 recall whatever the tuning | Official crate `sherpa-onnx` 2.0.1 (bundles native ORT. Prebuilt libs need GLIBCXX 3.4.29+, issue #3779) |
| **Porcupine** (Picovoice) | Apache-2.0 SDK, but runtime **requires an AccessKey** | Train `.ppn` in Picovoice Console | Only via the vendor console | Low (vendor claims) | Strong in vendor bench | Rust SDK exists |
| **rustpotter** 3.0.2 | Apache-2.0 | 3–8 personal recordings (DTW "reference"), or train a small classifier on your own labelled wavs | Yes (record yourself) | Unpublished | Author: "not intended to be a production-grade tool". References are "less accurate" | Pure Rust, but **no push since 2023-10-01** |
| **Vosk** grammar mode | Apache-2.0 / Apache-2.0 (models) | Restrict recogniser grammar to `["lucy", "[unk]"]` | Yes (text) | A full Kaldi ASR running continuously, with about a 50 MB model. Heaviest option | None for KWS use. It is not a KWS engine | Community crate `vosk` 0.3.1 (MIT, last 2024-10) |

Porcupine is disqualified: Picovoice's Free Tier ended on **2026-06-30**. Picovoice support was quoted saying "The SDK does not run without a valid key", "features using those keys will stop working", and "There is no non-commercial tier planned" ([HA community thread](https://community.home-assistant.io/t/fyi-picovoice-confirmed-free-tier-accesskeys-will-stop-working-after-june-30-2026/1012744)). Picovoice's own FAQ now says "there are no dedicated free or paid plans for personal or non-commercial use" ([FAQ](https://picovoice.ai/docs/faq/general)). An always-on open-source assistant cannot depend on a per-user commercial key.

## Details and sources

### LiveKit WakeWord v2
- **What it is.** The README says the training CLI "generates speech with LiveKit Inference TTS and adapts a pretrained encoder" and that "You do not need to record your own training data." The v1 implementation (based on openWakeWord) is deprecated, and v1 models are incompatible with the v2 clients ([README](https://github.com/livekit/livekit-wakeword)).
- **Licence.** The repo [NOTICE](https://github.com/livekit/livekit-wakeword/blob/main/NOTICE) says: "The source code and the model weights (*.onnx, *.tflite, encoders.tar.gz) are licensed under the Apache License, Version 2.0." The Rust crate is Apache-2.0 on crates.io.
- **Training requirements.** "You need a CUDA GPU or an Apple silicon Mac, uv and a LiveKit Cloud project (the free plan works) for the TTS." The first run downloads about 3.8–4 GB. Training "hey livekit" took 11 min on an RTX PRO 6000. On an M4 Pro Mac, speech generation took 22 min and training 37 min. Only 1,000 positive clips are used. Training is multilingual through `--language`. The README describes no local-TTS path (such as Piper), so the TTS step needs a LiveKit Cloud account. That is acceptable for a project-side build step but rules out "train on the user's laptop offline".
- **Inference.** "Running detection requires neither a GPU nor LiveKit Cloud credentials."
- **Accuracy.** From the README comparison, using LiveKit WakeWord Bench (7,873 clips plus 24 h of TV, music and podcasts), recall at 0.1/0.5/1 FA/h was:
  - LiveKit: 87/89/90%
  - openWakeWord 0.6: 32/46/52%
  - nanowakeword: 0/49/55%
  - microWakeWord: 0/0/24%
  - **Caveat (the vendor's own):** "the benchmark also uses LiveKit Inference, which may favor our models". The other engines were also trained with Piper TTS rather than LiveKit's TTS.
- **CPU.** 16.5 ms of CPU per second of audio on one core and one thread, against 29.5 ms for openWakeWord. That is roughly 1.6% of one core. This machine has a Ryzen 7 7700X with 16 threads, so the load is negligible. No resident-memory figure is published. The model is about 7 MB, and the crate embeds the feature models at compile time. **Not measured here**: see the open questions.
- **Rust crate.** From the [rust-sdks/livekit-wakeword README](https://github.com/livekit/rust-sdks/tree/main/livekit-wakeword):
  - Model I/O: "Each wake word is a single streaming `.onnx` model … takes 80 ms of 16 kHz audio per step and keeps its state between steps". Input is i16 PCM in chunks of any size, at any input rate, because "Resampling to 16 kHz is handled internally".
  - Runtime model management: it has `load_model` and `unload_model`.
  - Threshold: "The default threshold of 0.5 is calibrated to about 0.5 false alarms per hour."
  - Backend: the default is `ort` with `ort-tract`, which is pure-Rust inference and needs no native libs ([Cargo.toml](https://github.com/livekit/rust-sdks/blob/main/livekit-wakeword/Cargo.toml)).
  - Release status: v2 support landed in commit "livekit-wakeword: support LiveKit WakeWord v2 streaming models (#1595)" on 2026-11-05. crates.io's latest is still **0.1.3 (2026-04-02)**, which predates v2. Lucy should pin a git rev until 0.2 is published.
- **Activity.** livekit-wakeword v2.1.2 was released 2026-11-11, and the repo was pushed 2026-11-11 (314 stars).

### openWakeWord
- **Licence.** The code is Apache-2.0. The README says: "All of the included pre-trained models are licensed under the Creative Commons Attribution-NonCommercial-ShareAlike 4.0 … due to the inclusion of datasets with unknown or restrictive licensing as part of the training data" ([README §License](https://github.com/dscripka/openWakeWord#license)).
  - The embedding backbone derives from Google's speech_embedding, which is Apache-2.0 on TF Hub. But the re-implemented `embedding_model.onnx` and `melspectrogram.onnx` ship in the same release as the NC models, and the README does not clearly separate their licence.
  - A "lucy" model trained with its recipe would also inherit questions about the negative-data licences.
  - **Net: shipping it in an MIT/Apache project is murky.**
- **Training.** There is a Google Colab notebook that "allows anyone to produce a custom model very quickly (<1 hour) … but the performance of the model may be low in some deployment scenarios". A detailed notebook gives higher quality. Training uses "a minimum of several thousand" synthetic clips and large negative data. Community models are collected in fwartner/home-assistant-wakewords-collection.
- **Accuracy targets.** The README says: "<5% false-reject rates and <0.5/hour false-accept rates with appropriate threshold tuning". False accepts are measured on DiPCo (about 5.5 h). It offers optional Silero VAD gating "which can significantly reduce false-positive activations", and Speex noise suppression on Linux.
- **Cost.** "a single core of a Raspberry Pi 3 can run 15–20 openWakeWord models simultaneously in real-time".
- **Activity.** The last release was v0.6.0 (2024-02-11), and the last commit 2025-12-30. It is in maintenance mode.
- **Rust.** There is no official port. [`oww-rs`](https://github.com/skoky/oww_rs) 0.3.3 (MIT) is a small community port that uses tract and cpal. It bundles the NC-licensed `alexa` and `hey_mycroft` models.

### sherpa-onnx open-vocabulary KWS
- **How it works.** Open-vocabulary KWS means "you can specify any keywords without re-training the model". It behaves as "a tiny ASR system" that decodes only the listed keywords. Each keyword line is made of tokens with an optional `:boost` and `#threshold` ([KWS docs](https://k2-fsa.github.io/sherpa/onnx/kws/index.html)).
- **Models.** The English model is `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`, plus a `zh-en-3M-2025-12-20` model.
- **Rust.** The official crate `sherpa-onnx` 2.0.1 (Apache-2.0) is published by the maintainer (csukuangfj). It has a [`keyword_spotter.rs`](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/keyword_spotter.rs) example, including `create_stream_with_keywords(extra_keywords)` for runtime keywords.
- **Short-word problem.** In [issue #3809](https://github.com/k2-fsa/sherpa-onnx/issues/3809) (2026-07), "KWS recall drops sharply for short (3-phone) English keywords":
  - "CALL" stayed at ≤5/10 recall across a boost of 0.5–4.0 and thresholds of 0.25–0.005, while 5-phone "TEXT" got 8–10/10.
  - The issue notes that all the documentation examples are two-word phrases.
  - "Lucy" (L UW S IY) is 4 phones, which is borderline.
  - Related reports: [#4150](https://github.com/k2-fsa/sherpa-onnx/issues/4150) (a custom keyword at 0/9) and [#3990](https://github.com/k2-fsa/sherpa-onnx/issues/3990) (misses near the 1.5 s reset).
- **Bonus.** The same crate also covers Whisper STT, Silero/TEN VAD and VITS (Piper-style) TTS (see the rust-api-examples). That could matter for consolidating Lucy's other local Providers.

### rustpotter
- Apache-2.0 and pure Rust. "Creating a functional wakeword reference requires 3 to 8 wav records". A reference "has less accuracy than a wakeword model trained with enough data". The author says it is "not intended to be a production-grade tool" ([README](https://github.com/GiviMAD/rustpotter)).
- The crate is at 3.0.2 and the repo was last pushed **2023-10-01**, so it is effectively unmaintained.
- Its one attractive idea is **personal enrollment** (the user records their own word). LiveKit and openWakeWord do not offer that.

### Vosk
- vosk-api is Apache-2.0 and its "models are small (50 Mb)" ([repo](https://github.com/alphacep/vosk-api)). A grammar can restrict the vocabulary, for example `KaldiRecognizer(model, rate, '["…", "[unk]"]')` ([test_words.py](https://github.com/alphacep/vosk-api/blob/master/python/example/test_words.py)).
- The Rust bindings are the community crate [`vosk`](https://crates.io/crates/vosk) 0.3.1 (MIT, last 2024-10).
- Using it as a wake-word engine means running continuous LVCSR all the time. It has no published FA/FR figures for that use and costs much more CPU. Not suitable as a wake-word default.

## Audio capture (PipeWire/ALSA)

- **This machine.** It runs PipeWire 1.6.9 with pipewire-pulse, pipewire-alsa, pipewire-jack and WirePlumber 0.5.18 (all active). The default source is a USB mic, and the default format is float32 stereo at 48 kHz. libpipewire-0.3 and ALSA headers are installed (checked read-only with `pactl info`, `pacman -Q` and `systemctl --user`).
- **cpal.** 0.18.0 (2026-06-06) added a "New host for Linux and BSD via the native PipeWire API" behind the `pipewire` cargo feature. The default Linux host is still ALSA, which on Omarchy goes through `pipewire-alsa` anyway ([CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md)). The current version is 0.18.2 (Apache-2.0).
- **Suggestion.** Enable the `pipewire` feature for proper node naming and routing, and keep ALSA as the fallback. The lower-level `pipewire` crate 0.10.1 (MIT) is also an option.
- **Format conversion.** None of the engines care about the device format beyond mono PCM. LiveKit resamples internally, while sherpa and oww want 16 kHz f32 mono.
- **Fit with the echo-cancellation research.** That note ([echo-cancellation.md](echo-cancellation.md)) decides that the wake word reads the AEC-cleaned capture. A single in-process capture is therefore enough.
- **Caution.** sherpa issue #3809 found that speech enhancement in front of KWS *hurt* recall badly. Validate wake-word accuracy with Lucy's AEC/NS chain in place, not only on raw mic audio.

## Open questions (candidates for their own tickets)

1. **"Lucy" vs "Hey Lucy".** A single two-syllable word is acoustically short. It is also a common name, so it will turn up in TV and conversation. Every engine (and sherpa's documentation) favours multi-word phrases. Is the default wake word "Lucy" or "Hey Lucy"? This affects GLOSSARY.md.
2. **Who trains and hosts `lucy.onnx`?** Training needs a CUDA GPU or Apple-silicon Mac and a LiveKit Cloud account, as a one-off. Decide where the model lives (repo or release asset) and how it is reproduced.
3. **How does an end user get a custom word?** Options are a documented recipe, a set of project-published models, or an optional sherpa-onnx "type any word" Provider for users who accept lower accuracy. Should the wake-word engine itself be a Provider?
4. **Spike: measure, don't trust vendor numbers.** Prototype `livekit-wakeword` (git rev) with cpal/PipeWire on this machine. Record RSS and CPU at idle, and the false accepts over a few hours of desktop audio and calls, with and without the AEC chain.
5. **sherpa-onnx as a shared runtime.** It also provides STT (Whisper), VAD and TTS (VITS/Piper). Is it worth standardising local Providers on it, as opposed to whisper-rs or Piper separately? This is relevant to the STT/TTS Provider tickets.
