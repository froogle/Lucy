# 35: The Omarchy Plugin: windows and workspaces

**What to build:** Lucy ships an Omarchy Plugin, so a person can manage their windows and workspaces by voice: "move Firefox to workspace 3", "make this full screen", "close the terminal", "switch to tiling". It is a Plugin rather than more Built-in tools, so it can be the pattern for Plugins for other Linux desktops. A new `requires` Manifest field keeps it quietly out of the way on other desktops. Omarchy's own settings (theme, bar and so on) are ticket 45.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Plugins (`requires`), Omarchy Plugin (shipped), Testing (Omarchy Plugin, `requires`). Stories 167, 168, 170–173.

**Blocked by:** None (can start immediately)

**Seams:** owns the `requires` Manifest field and the Omarchy Plugin crate (extended by 45, and by lucy-v0-2 06 and 07 with screen tools). Uses the Plugin host, shipped-Plugin Approval and Landlock as they are.

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16); split into tracer slices by `to-tickets` the same day.
- **How it drives the desktop:** Hyprland over its IPC socket, declared in `[access]`. Hyprland 0.56 config is Lua, but dispatchers over the socket are unchanged.
- **Tools in this slice**, grouped by area with an `action` enum:
  - `list_windows`
  - `window {action: focus|close|move_to_workspace|fullscreen|float|pin, target}`, where close is polite and the app asks about unsaved work
  - `kill_window`, the only one with Confirmation
  - `workspace`
  - `layout`
- No logout, reboot or shutdown. `launch_app` stays a Built-in tool.
- **When `requires` isn't met:** not broken, not mentioned, and `plugins.list` shows it as "not for this desktop".

**Acceptance:**

- [ ] `requires = { env, commands }` in the Manifest, hash-covered; unmet means left out quietly and shown as "not for this desktop" in `plugins.list` (harness, fixture Plugin)
- [ ] The Plugin's MCP tests run against a fake Hyprland socket served by the test, which records dispatches and answers queries
- [ ] Tools `list_windows`, `window`, `kill_window` (Confirmation), `workspace` and `layout`; the Manifest matches `tools/list`
- [ ] Shipped and approved at scan like the news Plugin
- [ ] Once, by hand on Omarchy: move a window to workspace 3, full-screen one, close one, force-kill with a yes and decline with a no

## Comments

2026-11-16: the user widened the scope. It should ship with Lucy, cover anything you'd want done to an Omarchy machine (not just windows), and be the model for Plugins for other Linux desktops.

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Hand checks left, on Omarchy: move to workspace 3, full-screen, polite close, force-kill yes/no, workspace rename and layout change under Landlock. The `kill` and `rename` Lua argument forms are unverified.

2026-11-16: hand-checked on the deployed build; passed.
