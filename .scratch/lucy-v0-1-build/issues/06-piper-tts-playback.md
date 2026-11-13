# 06: Piper TTS Provider and playback

**What to build:** Lucy speaks. lucyd supervises one upstream Piper HTTP server per daemon: bound to 127.0.0.1 on a free port, with `PYTHONPATH` cleared and the voice preloaded, restarted if it crashes. Sentences are synthesised one by one and played through cpal at the voice's native rate, with the `level` event fed from playback. If TTS fails, replies go out as notification text and the Session carries on. With 04 and 05 done, this gives the first real spoken conversation through `lucy listen`.

Spec: Providers (Piper); Audio pipeline. ADR 0003.

**Blocked by:** 02

**Status:** resolved

- [ ] Piper detected by importing its Python module; the server is never bound to 0.0.0.0
- [ ] Crash restarts the server; the TTS interface exposes the voice's sample rate
- [ ] Playback stream in the same PipeWire group as capture; a 16 kHz reference copy is produced (rubato) for 09
- [ ] `level {rms}` events at ~30 Hz, only to `level` subscribers
- [ ] When TTS fails, replies become notification text and the Session continues (harness test)
- [ ] Default voice `en_GB-cori-medium` downloaded on first run with its model card

## Comments

Resolved on `lucy-v0-1-slice-2` (0a4e21d..890dded). Piper is supervised in `piper.rs` and played back in `playback.rs` (cpal PipeWire). The 16 kHz reference ring is exposed for ticket 09. Unspoken sentences become one notification per reply. The voice is fixed to `en_GB-cori-medium` (no config key yet).
