# Which licence does Lucy ship under, given Piper and espeak-ng are GPL-3.0?

Type: grilling
Status: resolved
Blocked by: none

## Question

Piper (now OHF-Voice/piper1-gpl) and espeak-ng are GPL-3.0, and every Piper path embeds espeak-ng (see [Can local Whisper, Piper and the Superwhisper adapter respond fast enough on this machine?](05-local-stt-tts.md)). Decide: keep Lucy MIT/Apache-2.0 with Piper as a separate out-of-process helper, or make Lucy GPL-3.0 and link Piper directly. Then decide how the Piper helper reaches users: user-installed `piper-tts` plus its server, or a small GPL helper binary on libpiper shipped separately (e.g. an AUR split package). Also confirm only permissively licensed voices (e.g. `cori-medium`, CC BY 4.0) are offered by default.

## Comments

- Already decided by the user (2026-11-12, previous session): Lucy is dual-licensed **MIT/Apache-2.0**, and Piper runs as a separate long-running helper process. The reason is that TTS is a pluggable Provider, so one default Provider shouldn't set the whole project's licence. Still open: how the Piper helper reaches users, and confirming that only permissively licensed voices are offered by default.

## Answer

- **Licence:** Lucy is dual-licensed MIT/Apache-2.0. As a standing rule, Lucy's own binary links only permissively licensed crates, and copyleft components run only as separate processes (Provider helpers, Plugins). A `cargo-deny` licence allow-list enforces this in CI. Plugins, including ones Lucy writes herself, can carry any licence. Recorded as `docs/adr/0003-permissive-licence-gpl-out-of-process.md`.
- **Piper in v0.1:** the user installs upstream `piper-tts` (GPL-3.0). Lucy ships no Piper code. Lucy starts and supervises one `python -m piper.http_server` per daemon herself: bound to localhost on a free port, with the voice preloaded, and restarted if it crashes. Lucy sends text sentence by sentence. A Lucy-built libpiper helper (no Python dependency, streaming within a sentence) is a possible later TTS Provider, not v0.1.
- **Install when missing:** Setup detects Piper by checking that the `piper` Python module can be imported, not by looking for a `piper` binary. On Arch, `extra/piper` is an unrelated gaming-mouse configurator. If Piper is missing, Lucy offers to install it after a yes/no from the user. Lucy works out the distro at runtime from `/etc/os-release` (`ID`, then `ID_LIKE`, so Omarchy resolves to Arch and Pop!_OS/Mint resolve to Ubuntu/Debian), never from a hard-coded assumption. Each supported distro family maps to an installer: Arch to the AUR through the user's AUR helper, Debian/Ubuntu to `apt`, Fedora to `dnf`, and so on. Lucy uses the distro's package only if it exists and is current. Otherwise she falls back to `uv tool install piper-tts`, then `pipx`, offering to install `pipx` from the distro's package manager if neither is present. On Arch/Omarchy that means AUR `piper-tts` (1.8.0, current as of 2026-11-12) through the user's AUR helper (here `yay`). `piper-tts-bin` is the stale archived rhasspy build and must not be used. If the user declines, Lucy falls back to text only (notifications).
- **Voices:** defaults and recommendations are limited to voices whose whole training chain is permissively licensed. The default is `en_GB-cori-medium` (CC BY 4.0). Nothing traced to lessac, and nothing CC BY-NC(-SA). Voices are downloaded on first run, never bundled, and each one's `MODEL_CARD` is kept alongside it. A user can still choose a non-commercial voice; Lucy shows its licence when they do.

**Note from [Does the full voice loop feel fast enough?](07-voice-loop-prototype.md) (2026-11-12):** AUR `piper-tts` 1.8.0-1 can't be installed. It depends on `python-pathvalidate`, which no longer exists in the official repos or the AUR (`yay` stops with "could not find all required packages"). So "exists and is current" has to mean "installs cleanly". When the AUR install fails, Lucy falls back to `uv tool install piper-tts`, which worked here. Lucy also starts the server with `--host 127.0.0.1` (it defaults to `0.0.0.0`) and with `PYTHONPATH` cleared.
