# TTS Providers are engines behind one local-server seam

Lucy has more than one TTS Provider: Piper, and Kokoro (ticket 53), which sounds less robotic but takes about ten times as long to start a sentence. `[tts] kind` picks one, read at each Session like `[stt] kind`, so a Voice preview can speak through a Provider other than the configured one, and switching takes effect without restarting lucyd.

The seam has two layers. `tts::Engine` is all a Provider is to the rest of lucyd: start in a voice, and turn a sentence into samples. `tts::Engines` holds one engine per `TtsKind` and does everything shared exactly once: playback, levels, ducking and the echo canceller's 16 kHz reference (ADR 0009), whichever engine is speaking. Most engines are copyleft or Python, so they run out of process (ADR 0003). `local_server::LocalServer` is that out-of-process `Engine`, written once: it finds the engine on first use, keeps its server running on a free loopback port, restarts it when it dies, looks again at the next Session if it wasn't found, and speaks Piper's `POST /synthesize` protocol. An engine supplies only a `local_server::Server`: how to find it and fetch a voice, and the command that starts it. Piper runs its upstream `piper.http_server`. Kokoro runs a small server of Lucy's own (`crates/lucyd/src/kokoro/server.py`, embedded in lucyd) in a venv that `lucy setup` makes, because `kokoro-onnx` has no server of its own.

Adding an engine means one module implementing `Server` (or `Engine`, for one that runs in process), a `TtsKind` variant with its config section, and its voices in the setup voice section.

## Considered Options

- Choosing the Provider once, at lucyd's start: simpler, but a Voice preview in setup couldn't speak through the Provider being picked until lucyd restarted.
- Kokoro in process through `ort`: no Python, but Kokoro needs a phonemiser, and the good ones are Python (misaki) or GPL (espeak-ng).
- A third-party Kokoro server (such as Kokoro-FastAPI): more to install and trust, and a different protocol for the shared code to speak.
