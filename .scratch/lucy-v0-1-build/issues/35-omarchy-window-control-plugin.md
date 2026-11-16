# 35: The Omarchy Plugin

**What to build:** Lucy ships an Omarchy Plugin, so a person can ask for most things they'd want done to their Omarchy desktop. It covers windows (focus, close, move, full screen, float, pin, force-kill), workspaces, the tiling layout, and a curated set of Omarchy's own commands (theme, bar, night light, screenshot, idle and lock). It is a Plugin rather than more Built-in tools, so it can be the pattern for Plugins for other Linux desktops. A new `requires` Manifest field keeps it quietly out of the way on other desktops.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Plugins (`requires`), Omarchy Plugin (shipped), Testing (Omarchy Plugin, `requires`). Stories 167–173.

**Blocked by:** none

**Seams:** owns the `requires` Manifest field, and the Omarchy Plugin crate (extended by lucy-v0-2 02 with screen tools). Uses the Plugin host, shipped-Plugin Approval and Landlock as they are.

**Status:** ready-for-agent

**Notes:**

- **Decisions from the grilling session (2026-11-16):**
  - Hyprland is driven over its IPC socket, plus a curated list of `omarchy-*` commands, never arbitrary ones.
  - About ten tools grouped by area, each with an `action` enum.
  - Only `kill_window` needs Confirmation. A polite close goes through `window`, and the app asks about unsaved work itself.
  - No logout, reboot or shutdown.
  - `launch_app` stays a Built-in tool.
  - When `requires` isn't met, the Plugin is not broken and not mentioned, and `plugins.list` shows it as "not for this desktop".
- Check each `omarchy-*` command on this machine before naming it. Read their `--help` or source; don't guess flags.
- Hyprland 0.56 config is Lua, but dispatchers over the IPC socket are unchanged.

**Acceptance:**

- [ ] `requires = { env, commands }` in the Manifest, hash-covered; unmet means left out quietly and shown as "not for this desktop" in `plugins.list` (harness, fixture Plugin)
- [ ] The Plugin's MCP tests run against a fake Hyprland socket served by the test, which records dispatches, and fake `omarchy-*` executables on `PATH`
- [ ] Tools: `list_windows`, `window`, `kill_window` (Confirmation), `workspace`, `layout`, `theme`, `bar`, `night_light`, `screenshot`, `idle_lock`; the Manifest matches `tools/list`
- [ ] Shipped and approved at scan like the news Plugin
- [ ] Once, by hand on Omarchy: close a window, move one to workspace 3, switch the theme, toggle night light, force-kill with a yes and decline with a no

## Comments

2026-11-16: the user widened the scope. It should ship with Lucy, cover anything you'd want done to an Omarchy machine (not just windows), and be the model for Plugins for other Linux desktops.
