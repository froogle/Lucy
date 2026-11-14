# AEC bench

Built for wayfinder 23 (AEC on real speakers); tickets 27 and 28 judged their acceptance on it. The recordings under `runs/` stay on the machine that made them (gitignored).

Plays Lucy's Piper voice through the default speaker while recording the default mic, both opened the way lucyd opens them (mic mono 16 kHz, speaker mono at the voice's rate). Then it runs the recording through no canceller, lucyd's own `Sonora` and the C++ `webrtc-audio-processing` 2.1 (bundled build, AEC3 only), and through lucyd's Silero VAD and a copy of lucyd's Barge-in gate with the default `[barge_in]` settings.

## Run

```sh
cd tools/aec-bench
B=../../target/release/aec_bench      # CARGO_TARGET_DIR is the repo's target/
$B record laptop silent    # stay quiet for ~45 s
$B record laptop talk      # talk over Lucy at each TALK NOW
$B analyse runs/*/         # re-run the analysis on saved recordings
$B levels runs/*/          # each run's frame levels: percentiles, noise floor
$B synth ../../.scratch/lucy-v0-1/prototypes/voice-loop/hello.wav   # self-check with a simulated room
```

Each run saves `runs/<setup>-<scene>/` with `mic.wav`, `ref.wav` (the reference on the mic's timeline), `meta.txt`, and `residual-sonora.wav` / `residual-webrtc.wav` (what VAD sees; ticket 26 reuses them).

## Columns

- `ERLE2s+` / `ERLE<2s`: echo removed, measured as mic power over output power during Lucy's speech, after and within her first 2 s (talk windows excluded)
- `resid dB`, `over flr`: leftover echo level, and how far above the room's noise floor it sits
- `rep ERLE`, `conv s`: the canceller's own ERLE (median), and when it first reached `converged_erle_db` (10 dB), which the gate uses
- `maxP`, `>=.7 ms`: highest VAD probability on residual echo, and the longest run above `threshold`, with the gate window open outside talk windows (compare with `sustain_ms` 300)
- `false`: false ducks/false barge-ins outside talk windows
- `heard T/F/? %`: what lucyd's `echo_heard` (ticket 27) said over Lucy's speech: echo, no echo, unsure. The reference reaches lucyd's `Sonora` only while she speaks, as in lucyd; `ALWAYS_REF=1` feeds it the whole run, as wayfinder 23 did
- `gain`: lucyd's pre-gain (ticket 28) at the end of the pass, in dB. Each run is replayed `PASSES` times (default 2) through the same cancellers, as that many Sessions in one lucyd, so the gain learned in pass 1 applies in pass 2. In a boosted pass, the ERLE and residual columns compare the boosted output with the raw mic, so they read low by the gain
- `CANCELLERS=sonora,webrtc` runs only those (names as in the `aec` column); unset, all six run
- talk windows: barge-in latency from the first VAD ≥ 0.5 in each window, or `missed`
