# 40: Readable audio device names

**What to build:** The bar panel's Microphone and Speaker rows show raw ALSA/PipeWire node names (`alsa_input.usb-Generic_ATH-M50xSTS-USB-00.mono-fallback`) instead of names a person recognises ("ATH-M50xSTS USB"). The same names probably show in the `lucy` TUI's Audio devices section. `lucy status --json` gets the options from `pactl`, which also knows each device's description. Still to settle: which name to show and where (panel, TUI, or both); how to tell apart two devices with the same description; whether `config.toml` keeps the node name; and what to show for `default` and for a configured device that's unplugged. Details to be settled in a grilling session.

**Status:** needs-triage

## Comments

Found during ticket 36's desktop hand check (2026-11-15). The user wants it grilled later.
