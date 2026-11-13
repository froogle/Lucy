# 20: TUI, setup and systemd

**What to build:** A new user gets Lucy running in one sitting. `lucy` with no arguments opens the TUI; `lucy setup` runs first-run setup:
- detect tools;
- offer the Piper install after a yes (distro from os-release `ID`, then `ID_LIKE`; the distro package only if it installs cleanly, otherwise `uv tool install`, then `pipx`; declining means text-only);
- pick Providers per tier from the recommendation table;
- pick the STT model;
- pick a voice with a preview, showing licences of non-commercial voices;
- pick mic and speaker with a level meter;
- test the wake word and see its score;
- approve shipped Plugins (spoken, via 14);
- offer to enable `lucyd.service`.

After setup: a Provider status page with "run real check", and an Approval list with revoke. Setup tests run inside lucyd over `test.mic_level`, `test.wake_word` and `test.voice_preview`.

Spec: Providers and configuration (TUI scope, Piper); Processes and crates (systemd, CLI).

**Blocked by:** 06, 07, 12, 15, 11, 19

**Seams:** uses `WakeWord` and `[wake_word]` (owned by 07); uses `[audio] speaker` and device resolution (owned by 11); uses the Approval flow (owned by 14); uses IPC `plugins.list`, `approvals.list` and `approvals.revoke` (owned by 15); uses `[stt]` and the Superwhisper warnings (owned by 19); uses `RECOMMENDATIONS` (owned by 12, resolved)

**Status:** ready-for-agent

- [ ] `lucyd.service` and `lucy-indicator.service` user units are part of and wanted by `graphical-session.target`, with the restart and kill semantics from the spec; children die with lucyd
- [ ] TUI writes config with comments preserved
- [ ] Granting Approval never happens in the TUI, only revoking
- [ ] Superwhisper warnings shown when it is chosen
- [ ] Everything else stays file-only (thresholds, timeouts, endpointing, paths)

## Comments

From slice 3: `uv tool install piper-tts` lacks Flask, so `piper.http_server` won't import. Install `piper-tts[http]`, and detect Piper by importing `piper.http_server` with `PYTHONPATH` cleared. Starting a spoken Approval for shipped Plugins goes through `Daemon::request_approval` (ticket 14); the TUI needs an IPC request that starts one, never one that grants it.

From the slice 4 review: setup no longer approves shipped Plugins. lucyd approves them itself when it scans them (spec, Plugins), so the "approve shipped Plugins" setup step and the IPC request to start a spoken Approval for them aren't needed.
