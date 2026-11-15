# Silero on ort-tract, and earshot against it

Ticket: [26-vad-runtime-ab](../issues/26-vad-runtime-ab.md). Measured 2026-11-15 on the dev machine (16 threads), one thread per VAD. Builds on [vad-barge-in.md](vad-barge-in.md). Prototype and raw output: `prototypes/vad-ab/` (`results.txt`).

## In short

- **Silero's If-less v6.2.3 export runs correctly on `ort-tract`.** It matches native ONNX Runtime on the same model to within 0.0001, and the standard (`If`) model to 4 decimals on every chunk. The mismatch reported in silero-vad #783 doesn't show up here. Keep the pure-Rust runtime.
- **lucyd runs it 45× slower than it should.** The "If-less" export still has one `If`, on the `sr` input (16 vs 8 kHz). Because `sr` arrives at runtime, tract runs that branch as a subgraph on every chunk: **7.3 ms per 32 ms chunk, ~23% of a core**. The running lucyd's `lucyd-dsp` thread sits at 25% of a core while idle (3.9 CPU-hours in 15 h). With `sr` fixed at 16000 inside the model **and** `ort`'s graph optimisation level set, tract folds the `If` away: **160 µs per chunk (~0.5% of a core)** and the same output (max difference 0.0001). It takes both: either change alone is still ~6.7 ms (`ort-tract` only optimises the graph when a level is set, and lucyd set none). Fixing the input shapes as well makes tract fail to parse the model. (A first pass of this write-up credited the fold alone; the prototype's `OPTIMIZE` switch was on in that run.)
- **earshot doesn't replace Silero.** It is 8× cheaper still (~20 µs per chunk), but in steady noise it calls most silence speech: 23% of silent chunks at 10 dB SNR in the room's noise, 62–74% at 0 dB. That would keep the 700 ms end-of-utterance silence from ever arriving in a noisy room. On clean speech, echo residual and real talk-overs it is level with Silero. CPU no longer decides anything once Silero is fixed.

## Method

- **Runtimes:** lucyd's exact setup (`ort` 2.0.0-rc.11 with `ort-tract` 0.2.0, 512-sample chunks plus 64 of context), the same If-less model on native ONNX Runtime (Python `onnxruntime`), the standard `silero_vad.onnx` v6.2.3 on native ORT as the reference, and `earshot` 1.2.2 (two 16 ms frames per Silero chunk; the second frame's score is used, the max of both reads the same). The bundled model is byte-identical to upstream v6.2.3's `silero_vad_op18_ifless.onnx`.
- **Labelled speech:** 16 phrases in Piper `cori-medium`, from "Yes." to full requests, each three times in random order with gaps of 0.4–3 s, 48 utterances in 141 s. Labels are exact (from the clean clips); chunks within 100 ms of an edge are ignored. Mixed clean, with the room's own noise (the headset mic's floor, recorded during `headphones-silent`, scaled up) at 10 and 0 dB SNR, and with pink noise at 10 and 0 dB.
- **Echo residual and human talk-overs:** the AEC bench's recordings from wayfinder 23 (`tools/aec-bench/runs/`), raw mic and after lucyd's `Sonora`.

## Results

Labelled speech, threshold 0.5 (miss = speech chunks called silence, false = silent chunks called speech; onset median/max; barge-in = how many utterances reach ≥ 0.7 for 300 ms):

| Set | VAD | ROC-AUC | F1 | Miss | False | Onset | Barge-in |
|---|---|---|---|---|---|---|---|
| clean | Silero (all three) | 1.000 | 1.000 | 0% | 0% | 48/106 ms | 47/48 |
| clean | earshot | 1.000 | 1.000 | 0% | 0% | 41/106 ms | 43/48 |
| room +10 dB | Silero | 0.988 | 0.971 | 2.6% | 2.2% | 69/699 ms | 45/48 |
| room +10 dB | earshot | 0.976 | 0.854 | 0.1% | **23.5%** | 39/131 ms | 44/48 |
| room 0 dB | Silero | 0.933 | 0.862 | 15.1% | 8.2% | 79/1241 ms | 36/48 |
| room 0 dB | earshot | 0.870 | 0.649 | 0% | **74.0%** | 19/119 ms | 48/48 |
| pink +10 dB | Silero | 1.000 | 0.994 | 0.9% | 0.1% | 48/347 ms | 46/48 |
| pink +10 dB | earshot | 1.000 | 0.988 | 0.2% | 1.5% | 48/136 ms | 41/48 |
| pink 0 dB | Silero | 0.992 | 0.977 | 4.0% | 0.3% | 64/667 ms | 41/48 |
| pink 0 dB | earshot | 0.997 | 0.689 | 0% | **61.9%** | 25/106 ms | 41/48 |

The utterances that never reach 300 ms at ≥ 0.7 on clean audio are the one-word ones ("Yes.", "Stop."), shorter than the gate's window. That is the gate's design, not the VAD's (Barge-in by the Wake word covers it).

Echo residual with the user silent (longest run ≥ 0.7; the gate fires at 300 ms):

| Run | Silero | earshot |
|---|---|---|
| desk, raw mic | 4.96 s (fires) | 3.33 s (fires) |
| desk, after `Sonora` | 0.10 s | 0.16 s |
| headphones, raw / after `Sonora` | 0 / 0 (max p 0.03) | 0 / 0 (max p 0.36) |
| synthetic room, after `Sonora` | 0 (max p 0.32) | 0 (max p 0.40) |

Human talk-overs after `Sonora` (seconds from each TALK NOW prompt, reaction time included): onset at 0.5 and gate firing are within 30 ms of each other for both VADs on every talk-over (desk 1.08/1.36/1.54 s Silero vs 1.11/1.36/1.54 s earshot; headphones 1.09/0.89/0.91 s vs 1.12/0.89/0.88 s). Neither has a run ≥ 0.7 longer than 60 ms outside the talk windows.

CPU per 32 ms chunk, one thread: Silero on tract as lucyd runs it 7.3 ms; `sr` folded or optimisation level 3 alone ~6.7 ms; both 160 µs; native ORT ~70 µs (Python loop included); earshot ~20 µs.

## What to change

Bundle the If-less model with `sr` folded in as a constant 16000 (`prototypes/vad-ab/sr_const.py`: `sr` becomes an initializer instead of an input), stop passing `sr` from `audio/silero.rs`, and build its session with `GraphOptimizationLevel::Level3`. The model stays MIT (a mechanical edit of Silero's file); keep its licence beside it and say in the model's comment how it was made. Build ticket 33.

## Limits

- Labelled speech is Lucy's own Piper voice, not a human one. Human speech was only judged on the three talk-overs per setup, without frame labels.
- One room and two mics. The room's noise is the headset mic's own floor scaled up, a steady hiss; music, TV and other people talking weren't tested (wayfinder 24's soak covers desktop audio for the wake word).
- No laptop speakers here, as in wayfinder 23.
