# Voice-activity detection, the barge-in gate and end-of-utterance

Ticket: [14-vad-barge-in](../issues/14-vad-barge-in.md). Researched 2026-11-12. Builds on [echo-cancellation.md](echo-cancellation.md). In short: AEC3 runs in-process, Lucy owns the TTS reference, and APM v2 reports no VAD.

## Recommendation

- **VAD: Silero VAD v6.2 (MIT, code and weights).** It has the best published accuracy and is by far the best on noise-only audio. Run it on the echo-cancelled 16 kHz signal in 512-sample (32 ms) chunks.
- **Wrapper and runtime.** Write a thin wrapper of about 100 lines over `ort` and bundle `silero_vad.onnx`. Do not take the existing wrapper crates (see below).
- **Prototype 07 must settle one question.** Does Silero's new If-less ONNX export run on the pure-Rust `ort-tract` backend that the wake word already uses? If not, the VAD needs native ONNX Runtime (Arch `extra/onnxruntime-cpu`).
- **Seam.** Put the VAD behind a small `Vad` trait (`frame_len()`, `prob(&mut self, &[f32]) -> f32`, `reset()`). **earshot** (MIT/Apache, pure Rust, 95 KiB) can then be A/B-tested in 07 and swapped in if it holds up. That would drop ONNX from the VAD path entirely.
- **Barge-in gate while Lucy speaks.**
  - Silero prob ≥ **0.7** sustained for **300 ms** (10 chunks, one dropped chunk tolerated).
  - Raise to ≥ **0.85 / 500 ms** while AEC is unconverged: the first ~2 s of render in a Session, or ERLE < ~10 dB.
  - Two stages. On a candidate onset, duck TTS (−12 dB, still fed to AEC as the reference). On confirmation, stop and flush. If not confirmed within ~600 ms, unduck.
- **End-of-utterance on normal turns.**
  - Silero hysteresis 0.5 on / 0.35 off.
  - Speech start after ≥ 100 ms of speech, with 500 ms of pre-roll kept for STT.
  - The turn ends after **700 ms** of continuous non-speech (configurable), with a hard cap of 30 s per utterance.
  - Smart Turn v3.2 (BSD-2, 8 MB int8 ONNX) is the upgrade path for a semantic end-of-turn in a later version. It is not in v0.1.

Every threshold above is a **starting point taken from other frameworks' defaults (cited below), not a measured value for Lucy**. Prototype 07 must tune them on laptop speakers and on the desk setup.

## Candidate VADs

| VAD | Licence | Rust route | Frame | CPU (published) | Accuracy (published) | Notes |
|---|---|---|---|---|---|---|
| **Silero VAD v6.2** | MIT (code and weights; [LICENSE](https://github.com/snakers4/silero-vad/blob/master/LICENSE)) | ONNX via `ort` (native ORT, or maybe `ort-tract`). Wrappers: `voice_activity_detector` 0.2.1, `silero-vad-rust` 6.2.2 | 512 samples @16 kHz (32 ms) plus 64 samples of context; 256 @8 kHz ([utils_vad.py](https://github.com/snakers4/silero-vad/blob/master/src/silero_vad/utils_vad.py)) | V5 ONNX: 189 µs per 31.25 ms chunk, 1 thread, Threadripper 3960X (≈0.6% of a core) ([Performance Metrics](https://github.com/snakers4/silero-vad/wiki/Performance-Metrics)) | Multi-domain ROC-AUC 0.97; noise-only accuracy ESC-50 0.87, private noise 0.71 ([Quality Metrics](https://github.com/snakers4/silero-vad/wiki/Quality-Metrics)) | Model ~2.3 MB. Releases through v6.2.3 (2026-09). The de facto standard, used by LiveKit and Pipecat |
| **earshot** 1.2.2 | MIT OR Apache-2.0 | Pure Rust, `no_std` capable | 256 samples @16 kHz (16 ms) | RTF 0.0003 ("40x faster than Silero v6 and TEN"); ~8 KiB state, ~95 KiB binary ([README](https://github.com/pykeio/earshot)) | Vendor chart only: "performs markedly better than Silero VAD v6 and TEN VAD". No numbers, dataset or training data disclosed | 1.0 released 2026-02 (0.1 in 2024 was BSD-3, a different design). 1.2.0 changed the model again. Has `SegmenterOptions` (attack 0.6, decay 0.5, smoothing 0.4, pre-roll 5 frames, min 6 frames; [segments.rs](https://github.com/pykeio/earshot/blob/main/src/segments.rs)) |
| **WebRTC VAD** (`webrtc-vad` 0.4.0) | Crate MIT; builds libfvad (BSD-3) with `cc` | Safe wrapper over C | 10/20/30 ms @8–48 kHz | Negligible | Multi-domain ROC-AUC 0.73; ESC-50 accuracy **0** (calls every noise "speech") (same Silero wiki) | GMM-based, from 2019 and unmaintained since. Worst at telling noise from speech, which is exactly what residual echo and room noise need. Rejected |
| **TEN VAD** | Apache-2.0 **plus Agora conditions**: you "may not Deploy the ten-vad in a way that competes with Agora's offerings… including… enabling any third party to develop or deploy Applications" ([LICENSE](https://github.com/TEN-framework/ten-vad/blob/main/LICENSE)) | Prebuilt `.so` or ONNX; community `ten_vad` crate (44 downloads) | 160/256 samples @16 kHz | RTF 0.0086–0.015 on x86 Linux ([README](https://github.com/TEN-framework/ten-vad)) | ROC-AUC 0.93 multi-domain; ESC-50 0.42 (Silero wiki). Vendor claims faster speech→silence transitions than Silero | **Rejected on licence.** The conditions are not OSI-compatible for an MIT/Apache project that others redistribute and build on |

Also seen and not pursued: FireRed VAD (in Silero's table, ROC-AUC 0.94, licence not checked) and sherpa-onnx's VAD (it wraps Silero/TEN. It is worth it only if the STT ticket standardises on sherpa-onnx; see wake-word findings, question 5).

### Rust plumbing for Silero

- **`voice_activity_detector` 0.2.1** (MIT; [repo](https://github.com/nkeenan38/voice_activity_detector)). Bundles **Silero v5** (`src/silero_vad.onnx`, 2.3 MB) and pins `ort = "=2.0.0-rc.10"`. Its default build downloads Microsoft's ORT binaries. Last release 2025-08. It has a nice `label(threshold, padding_chunks)` iterator but not the hysteresis or duration logic Lucy needs.
- **`silero-vad-rust` 6.2.2** (MIT; [repo](https://github.com/sheldonix/silero-vad-rust)). Bundles **v6** opset 15/16 models and ports `VadIterator` and `get_speech_timestamps`. It requires `ort` `load-dynamic` with ORT **1.22.x**: "older releases are not supported; newer releases may be incompatible". Arch ships `onnxruntime-cpu` **1.29.0**, so that pin is a packaging hazard.
- **Own wrapper (recommended).** The model's streaming contract is small: input `[1, 64+512]` f32, state `[2,1,128]`, `sr`, output a probability. Lucy keeps the 64-sample context and the state between calls (see `OnnxWrapper.__call__` in utils_vad.py), so it controls the `ort` version and runtime alongside the wake word.
- **The pure-Rust question.** tract does not implement a dynamic ONNX `If`, which blocked Silero v4 ([tract#1029](https://github.com/sonos/tract/issues/1029), [tract#1062](https://github.com/sonos/tract/issues/1062), both still open). The standard v5/v6 exports still have 10 `If` nodes whose branch ranks differ ([silero-vad#728](https://github.com/snakers4/silero-vad/issues/728)).
  - Silero now ships `silero_vad_op18_ifless.onnx` ([PR #732](https://github.com/snakers4/silero-vad/pull/732), 2025-12), which should suit static engines like tract or burn.
  - But [#783](https://github.com/snakers4/silero-vad/issues/783) (2026-07) reports that it does not match the baseline outputs. The maintainer replied that it is "an older ifless model exported with opset 18".
  - The repo also has `silero_vad_16k_op15.onnx` and 16k `safetensors` weights with a tinygrad reference model.
  - **Unverified:** whether any of these runs correctly on `ort-tract`. If none does, Lucy either links native ORT for the VAD only, or uses earshot.

## Accuracy on residual echo

No project publishes VAD results on AEC residual echo. Reasoning from the primary facts:

- Residual echo of Lucy's TTS **is speech**. Any good VAD, Silero included, is trained to fire on it. The VAD cannot tell Lucy's leaked voice from the user's. Discrimination has to come from (a) how far AEC3 suppresses the echo and (b) a stricter gate while Lucy is speaking.
- What the VAD *can* reject is the non-speech-like part of the residual: nonlinear distortion, clicks, broadband artefacts from the suppressor. Silero v6's noise-only accuracy (ESC-50 0.87 vs WebRTC 0, v5 0.61, TEN 0.42) is the best published proxy for that, so it is the main reason to prefer Silero v6 over v5 and over WebRTC VAD.
- The AEC side already exposes signals the gate can use. `webrtc-audio-processing` `Stats` provides `echo_return_loss`, `echo_return_loss_enhancement` (ERLE) and `delay_ms`. It also provides `residual_echo_likelihood`, but **only with the `bundled` feature** ([stats.rs](https://github.com/tonarino/webrtc-audio-processing/blob/main/src/stats.rs)).

## Barge-in gate (while Lucy is speaking)

What production frameworks ship as defaults:

- **LiveKit Agents** `InterruptionOptions` ([turn.py](https://github.com/livekit/agents/blob/main/livekit-agents/livekit/agents/voice/turn.py)):
  - `min_duration` **0.5 s** of speech to register an interruption.
  - `resume_false_interruption` true, with `false_interruption_timeout` 2.0 s: if no real user turn follows, the agent resumes speaking.
  - A `backchannel_boundary` of (1.0, 1.0) s.
  - Silero plugin defaults ([vad.py](https://github.com/livekit/agents/blob/main/livekit-plugins/livekit-plugins-silero/livekit/plugins/silero/vad.py)): activation 0.5, deactivation threshold−0.15, `min_speech_duration` 0.05 s, `min_silence_duration` 0.55 s, `prefix_padding_duration` 0.5 s.
- **Pipecat** `VADParams` ([vad_analyzer.py](https://github.com/pipecat-ai/pipecat/blob/main/src/pipecat/audio/vad/vad_analyzer.py)): `confidence` **0.7**, `start_secs` **0.2**, `stop_secs` 0.2, `min_volume` 0.6.
- **Silero itself** ([utils_vad.py](https://github.com/snakers4/silero-vad/blob/master/src/silero_vad/utils_vad.py)): `threshold` 0.5, `neg_threshold` = threshold−0.15, `min_speech_duration_ms` 250, `min_silence_duration_ms` 100, `speech_pad_ms` 30.

Proposed for Lucy (all values configurable, tune in 07):

| Parameter | Default | Basis |
|---|---|---|
| `speaking` window | TTS render active, **plus a 300 ms tail** after the last rendered sample | The echo path delay (AEC3 `delay_ms`) and room reverb outlive playback |
| Speech threshold while speaking | **0.7** (Silero prob) | Pipecat's default confidence; higher than the idle 0.5 |
| Sustained duration | **300 ms** = 10 × 32 ms chunks, one sub-threshold chunk allowed | Middle of the ticket's 200–400 ms; Pipecat 0.2 s, LiveKit 0.5 s |
| Unconverged mode | Threshold **0.85**, duration **500 ms**, while ERLE < **10 dB** or during the first **2 s** of render in a Session | ERLE from `get_stats()`. The 10 dB / 2 s figures are estimates, not measured |
| Level floor | Ignore chunks whose post-AEC RMS < −50 dBFS | Cheap guard against faint residuals; tune |
| Stage 1 (candidate) | First chunk ≥ threshold: **duck** TTS by 12 dB. The ducked PCM stays the AEC reference | Sounds responsive and costs little if the onset was false |
| Stage 2 (confirm) | Duration met: **stop TTS and flush** the playback buffer, keep feeding AEC the tail actually played, open a user turn with **500 ms pre-roll** | From echo-cancellation findings; pre-roll as in LiveKit/Pipecat |
| Abort | Not confirmed within 600 ms of the candidate onset: unduck | |
| False interruption (optional) | No speech start within 2 s after stopping: go back to listening. v0.1 does **not** resume the reply | LiveKit resumes; resuming needs TTS position tracking, so it can wait |

The convergence check uses ERLE rather than "time since start" alone. AEC3 re-adapts when the user changes volume or the output device, and ERLE shows that, while a timer does not. Barge-in should never be disabled outright: an unconverged canceller just raises the bar.

## End-of-utterance (normal listening turns)

| Parameter | Default | Basis |
|---|---|---|
| Thresholds | on **0.5**, off **0.35** (hysteresis) | Silero defaults |
| Speech start | ≥ **100 ms** above threshold | Between LiveKit's 0.05 s and Silero's 250 ms. Short commands ("stop", "yes") must register |
| Pre-roll kept for STT | **500 ms** | LiveKit `prefix_padding_duration` 0.5 s; Pipecat Smart Turn `pre_speech_ms` 500 |
| End of utterance | **700 ms** continuous below the off threshold | LiveKit stacks 0.55 s VAD silence with a 0.5 s `min_delay` (≈1 s in total). Pipecat without Smart Turn would use a longer `stop_secs`. 700 ms favours snappy commands; raise it for dictation-like use |
| Max utterance | **30 s** hard cap, then flush to STT | Whisper's 30 s window (see STT ticket) |
| No speech after wake | Hand to the Session state machine (ticket 10); the Session-level ~20 s silence timeout already exists | |

**Semantic end-of-turn (later version).** Pipecat **Smart Turn v3.2** ([repo](https://github.com/pipecat-ai/smart-turn), [HF](https://huggingface.co/pipecat-ai/smart-turn-v3)):

- Licence and model: BSD-2-Clause for code and weights, with ONNX files on Hugging Face (`smart-turn-v3.2-cpu.onnx`, int8, ~8 MB). It is a Whisper-Tiny backbone over up to 8 s of 16 kHz audio.
- Speed: "as little as 10 ms on some CPUs"; ~65 ms on a Pipecat Cloud instance.
- Accuracy: 92.6% overall (FPR 4.7%, FNR 2.6%) and 94.3% for English on their 31.5k-sample benchmark ([v3.2-cpu benchmark](https://huggingface.co/pipecat-ai/smart-turn-v3/blob/main/benchmarks/smart-turn-v3.2-cpu.md)).
- Usage: Pipecat runs it after a short VAD silence (`stop_secs` 0.2), thresholds its sigmoid at 0.5, and falls back to `STOP_SECS` 3 s ([base_smart_turn.py](https://github.com/pipecat-ai/pipecat/blob/main/src/pipecat/audio/turn/smart_turn/base_smart_turn.py), [local_smart_turn_v3.py](https://github.com/pipecat-ai/pipecat/blob/main/src/pipecat/audio/turn/smart_turn/local_smart_turn_v3.py)).
- It would let Lucy cut the silence wait to ~200 ms on complete sentences while not cutting off "remind me to… um…". It is not needed for v0.1, and it needs the same ONNX runtime question answered.

## Pipeline sketch

```text
mic ─► AEC3 (ref = TTS PCM) ─► resample 16 kHz ─► 512-sample chunks ─► Vad::prob
                                                         │
             is_speaking, ERLE ─────────────────────────►├─► BargeInGate  (0.7 / 300 ms; 0.85 / 500 ms unconverged)
                                                         └─► Endpointer   (0.5/0.35, start 100 ms, end 700 ms, pre-roll 500 ms)
```

The same VAD instance serves both. Only the gate parameters change with `is_speaking`. Call `reset()` on the VAD state at Session start.

## Not measured / open

- **CPU and accuracy were not measured on this machine.** A local benchmark build (earshot, webrtc-vad and Silero via `ort` on an espeak sample plus pink noise) was blocked by the sandbox. All CPU and accuracy figures above are the projects' own. Every candidate is far below the wake word's ~1.65% of a core, so CPU does not decide this.
- Whether Silero's If-less (or 16k op15) ONNX runs correctly on `ort-tract`. That decides "pure Rust" vs "native ORT dependency".
- earshot's accuracy beyond its own chart, and its behaviour on residual echo. A/B it against Silero in 07 using recorded AEC output.
- Real ERLE on laptop speakers and the desk setup, and therefore the 10 dB convergence cut-off.
- Whether ducking during the candidate stage is audible or annoying in practice.
