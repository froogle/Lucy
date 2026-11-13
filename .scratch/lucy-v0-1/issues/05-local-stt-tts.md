# Can local Whisper, Piper and the Superwhisper adapter respond fast enough on this machine?

Type: research
Status: resolved
Blocked by: none

## Question

Measure or estimate on this machine: whisper.cpp via whisper-rs (model sizes tiny/base/small, streaming vs utterance-at-a-time) latency for short commands; Piper TTS time-to-first-audio and voice quality options, and how to stream it from Rust; and whether the Superwhisper CLI (transcribe FILE, read, record/stop) can be used as a programmatic STT Provider (output format, latency, licensing for an optional adapter). Recommend defaults.

## Answer

Yes, all three are fast enough on this machine (Ryzen 7 7700X, RX 7800 XT with Vulkan, no CUDA).
- STT: whisper-rs with the `vulkan` feature, resident model, transcribing one utterance at a time after VAD endpointing. Default model is `small.en` on Vulkan (about 0.1 s for a 3 s command), falling back to `base.en` on CPU with `audio_ctx` set to about 384 (about 0.1 s). `large-v3-turbo-q5_0` is the accurate option.
- TTS: Piper medium voices produce the first audio for a sentence in under 100 ms on CPU. Default voice is `en_GB-cori-medium` (CC BY 4.0, no lessac in its chain). Feed it sentence by sentence. Piper and espeak-ng are GPL-3.0, so run Piper as a persistent separate process that streams PCM, not linked into Lucy (that includes `piper-rs`).
- Superwhisper: works as an optional spawn Provider through `superwhisper transcribe FILE` (about 130 ms warm, JSON output). Don't use `record`/`stop`, which auto-paste into the focused window and take the mic. Calling the CLI doesn't affect Lucy's licence.

[findings](../research/local-stt-tts.md)
