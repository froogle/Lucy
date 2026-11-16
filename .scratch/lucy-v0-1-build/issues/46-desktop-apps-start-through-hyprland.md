# 46: The Omarchy Plugin starts desktop programs through Hyprland

**What to build:** The Omarchy Plugin's `screenshot` and `night_light` tools fail on the desktop. Both run commands that start desktop programs inside the Plugin's Landlock sandbox:

- `omarchy-capture-screenshot` runs `omasnap`, a Qt app. It needs `$XDG_RUNTIME_DIR/omasnap/` (created on first use) and a `/dev/shm/omarchy-capture-*` buffer. It exits with "Could not create private runtime directory".
- `omarchy-toggle-nightlight` starts `hyprsunset` through `uwsm-app` when it isn't running. `uwsm-app` needs the D-Bus session bus, and the daemon would inherit the sandbox.

Start these two through Hyprland's `exec` dispatcher over the socket the Plugin already uses. They then run as a key binding would: outside Lucy's sandbox, with the whole session. The call can't see their exit status, so the tool says it started the screenshot or the night light, not that it finished.

Spec: [v0.1](../../lucy-v0-1/spec.md): Omarchy Plugin (shipped). Found in the hand checks for ticket 45 (2026-11-16).

**Blocked by:** none

**Seams:** none new. It extends the Omarchy Plugin (ticket 35's crate).

**Status:** resolved

**Notes:**

- Find the exact Lua form of Hyprland 0.56's exec dispatcher from Hyprland's stubs or binary, or from Omarchy's own scripts, as ticket 35 did. Don't dispatch anything to the live desktop.
- `night_light` keeps its `--status` check inside the sandbox; only the toggle goes through `exec`.
- Drop any `[access]` entries and `requires` that only the moved commands needed. The Manifest's `commands` must still list exactly the commands the tools run, whichever way they run.
- Write the decision down in an ADR: the Hyprland socket's `exec` already lets this Plugin run anything outside its sandbox, so for this shipped Plugin the sandbox is a safety rail, not a wall.

**Acceptance:**

- [ ] `screenshot` (screen, region, window) and the `night_light` toggle send an `exec` dispatch to the fake Hyprland socket with the right command line; the Manifest matches the commands run
- [ ] The ADR exists
- [ ] Once, by hand on Omarchy: "take a screenshot" saves one to `~/Pictures` with Omarchy's notification, and night light turns on and off

## Comments

2026-11-16: hand-checked: a screenshot saved to ~/Pictures/Screenshots, and the night light turned on and off.
