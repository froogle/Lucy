# 40: Readable audio device names

**What to build:** The bar panel's Microphone and Speaker rows, and the `lucy` TUI's Audio devices section, show raw node names (`alsa_input.usb-Generic_ATH-M50xSTS-USB-00.mono-fallback`) instead of names a person recognises. Show `pactl`'s description of each device, exactly as given ("ATH-M50xSTS-USB Mono", "Pebble V3 Analog Stereo"), in both places, through the shared device list (`crates/lucy/src/devices.rs`, which already leaves out monitor sources).

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Audio pipeline (Device names), Testing (Device names); stories 178–182.

**Blocked by:** none

**Seams:** owns the description in the device list both callers use. The panel snapshot gains a label for each option; `key`/`current`/`options` keep node names.

**Status:** resolved

**Notes:**

- **Decisions from the grilling session (2026-11-16):**
  - The description is shown as given, with no suffix trimming.
  - `config.toml` and `lucy set` keep the node name; the description is for display only.
  - Devices with the same description are numbered "(1)", "(2)" in node-name order.
  - `default` shows as "Default (<description of the current default>)".
  - A pinned device that is unplugged has no description, so it shows its raw node name plus "(not connected)". No last-seen description is stored.
- The panel's QML holds no product knowledge (ticket 36), so labels come from the snapshot, not from QML.

**Acceptance:**

- [ ] The panel's rows and the TUI's Audio devices section show descriptions; choosing one still writes the node name
- [ ] Duplicates are numbered; the default and unplugged cases show as above (tests with a fake `pactl`)
- [ ] `omarchy-plugin/tests/run.sh` passes
- [ ] Once, by hand: the panel and the TUI on this machine show "ATH-M50xSTS-USB Mono" for the headset mic

## Comments

Found during ticket 36's desktop hand check (2026-11-15). The user wants it grilled later.

2026-11-16: settled in a grilling session.

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Hand check left: the panel and the TUI show "ATH-M50xSTS-USB Mono" for the headset mic.
