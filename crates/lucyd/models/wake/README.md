# Wake-word models

`hey_lucy.onnx` is the Wake word lucyd ships: "Hey Lucy". It was trained with
[LiveKit WakeWord](https://github.com/livekit/livekit-wakeword) v2, which
adapts LiveKit's pretrained encoders (Apache-2.0, see
`LIVEKIT_WAKEWORD_LICENSE`) to the phrase from generated speech. The file
name names the phrase, so a user's own model is a file like
`hey_robot.onnx`, set as `[wake_word] model`.

A plain "Lucy" model is still to come: wayfinder ticket 27.

## How it was trained

On 2026-11-14, on an Apple M5 Pro Mac (24 GB, macOS 26.6.2), in an empty
folder:

```bash
brew install uv livekit-cli
uv tool install --python 3.13 "livekit-wakeword[train]"
# LIVEKIT_URL, LIVEKIT_API_KEY and LIVEKIT_API_SECRET from a LiveKit Cloud
# project (the free Build plan) in the environment, for the speech step.
livekit-wakeword setup --wakeword "hey lucy" --eval
livekit-wakeword train --wakeword "hey lucy" --type standard
livekit-wakeword eval --wakeword "hey lucy"
```

Versions: livekit-wakeword 2.1.2, livekit-wakeword-bench 0.1.1,
livekit-agents 1.8.12, torch 2.15.0, Python 3.13.18, uv 0.13.8; encoders from
the livekit-wakeword v2.0.0 release. `setup` wrote this config, all defaults:

```yaml
wakeword: hey lucy
language: en
output_dir: .
plan: build
n_positive: 500
n_confusable: 500
n_sentence: 150
steps: 5000
micro_steps: 5000
target_fpph: 0.5
```

Speech generation took 17 minutes (it also downloaded about 2.4 GB of noise,
room and speech sources once), and training, calibration and export 30
minutes. The speech comes from LiveKit Inference with a fixed seed (1234), but
the voices are remote, so a retrain gives a close model, not the same bytes.

The free plan's $2.50 a month of Inference credit covers about one phrase's
speech. When it runs out, every speech request fails with HTTP 429 until the
1st of the next month.

## How well it does

`eval` on its held-out test set: 500 "Hey Lucy" clips in voices kept out of
training, and 1.01 h of other speech and noise.

- Score (recall at 0.5 false alarms per hour): 0.822
- At the default threshold of 0.5: recall 0.874, 0.99 false alarms per hour,
  15.2% of sound-alikes ("he lucy", "ho lucy", ...) accepted

An hour of negatives gives a rough false-alarm rate; the soak test (wayfinder
ticket 24) measures it on real desktop audio.

sha256 `f73dc60cb33e362c305660119ad0d6ae8bf1d326dcb30f1faac2bcad75830464`
