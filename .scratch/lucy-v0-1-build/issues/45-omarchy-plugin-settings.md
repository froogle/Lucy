# 45: The Omarchy Plugin: Omarchy's own settings

**What to build:** The Omarchy Plugin also handles Omarchy's own settings by voice: "switch to the Tokyo Night theme", "hide the bar", "turn on night light", "take a screenshot", "lock the screen". Each tool runs a curated `omarchy-*` command, declared in the Manifest's `commands`, never an arbitrary one.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Omarchy Plugin (shipped), Testing (Omarchy Plugin). Story 169.

**Blocked by:** 35

**Seams:** none new. Extends the Omarchy Plugin (owned by 35).

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16); split into tracer slices by `to-tickets` the same day.
- **Tools:** `theme`, `bar`, `night_light`, `screenshot` and `idle_lock`, each with an `action` enum where it has several. None needs Confirmation.
- Check each `omarchy-*` command on this machine before naming it: read its `--help` or source, and don't guess flags. Theme names come from Omarchy's list, so `theme` can offer the installed ones.

**Acceptance:**

- [ ] The five tools run the right `omarchy-*` commands, tested with fake executables on `PATH`
- [ ] The Manifest's `commands` lists exactly the commands used, and matches `tools/list`
- [ ] Once, by hand on Omarchy: switch the theme, toggle the bar and night light, take a screenshot

## Comments

2026-11-16: built in slice 9 (`lucy-v0-1-slice-9`). Declared paths may start with `$NAME` (absolute values only, no `..`; spec amended). Write access is narrowed to the three `omarchy-theme-set` lock files, `~/.local/state/omarchy`, `~/.cache/omarchy` and `~/Pictures`. Hand checks left: switch the theme, toggle the bar and night light, take a screenshot, and lock the screen. Some apps may keep old colours, and omasnap may need more read paths.

2026-11-16: hand-checked on the deployed build; passed.
