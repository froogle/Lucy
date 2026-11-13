# Lucy is MIT/Apache-2.0; copyleft components run out of process

Lucy is dual-licensed MIT/Apache-2.0. Her own binary links only permissively licensed crates, and any copyleft component runs as a separate process: a Provider helper or a Plugin. The trigger was Piper, the default TTS: it and the espeak-ng phonemiser it embeds are GPL-3.0, and every in-process route (`piper-rs`, libpiper) would make Lucy's binary GPL. TTS is a pluggable Provider, so one default Provider shouldn't set the licence of the whole project. Instead Lucy starts and supervises the user-installed upstream Piper HTTP server and talks to it over localhost. A `cargo-deny` licence allow-list enforces the rule in CI. Plugins are separate processes, so they may carry any licence. See `.scratch/lucy-v0-1/issues/17-licence.md`.

## Considered Options

- GPL-3.0 for Lucy, linking Piper directly: the lowest latency and the simplest packaging, but one default Provider would dictate the licence for every contributor and downstream user.
- A Lucy-built GPL helper binary on libpiper, shipped as its own package: no Python dependency, and it could stream within a sentence. Deferred, because whole-sentence synthesis already takes under 100 ms, so it would mean a second codebase for no v0.1 gain. It remains a possible later TTS Provider.
