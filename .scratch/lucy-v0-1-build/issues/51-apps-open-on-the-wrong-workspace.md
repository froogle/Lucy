# 51: A page Lucy opens lands on the wrong workspace

**What to build:** A web page Lucy opens should appear in a new browser window, on the workspace the user is on or the one they named. At the moment it opens as a tab in the browser's last-used window, wherever that is. The user is then pulled to that window's workspace.

Reported 2026-11-18:

- "Open YouTube on workspace N": Lucy switches to N, then the page opens on workspace 1.
- On workspace 5, "do a YouTube search": the search runs on workspace 1, and the desktop switches back to 1.

**Cause (reproduced 2026-11-18):** `open_url` runs `uwsm app -- xdg-open <url>` (`crates/lucyd/src/launcher.rs`). The default browser is `omarchy-browser.desktop`, whose `omarchy-launch-browser` passes the URL to the running Brave. Brave opens it as a tab in its last-used window. Then two things focus that window and switch to its workspace: Hyprland's `misc:focus_on_activate` (on) and `omarchy-launch-browser`'s own `omarchy-hyprland-focus-app`. Repro: Brave windows on workspaces 1 and 5, last used on 5. From workspace 4, `uwsm app -- xdg-open https://example.org` added a tab on 5 and switched the desktop to 5.

**The fix:**

- `open_url` opens a **new window** of the default browser. Find the default browser's desktop entry (`xdg-settings get default-web-browser`, then its `Exec`), following Omarchy's `omarchy-launch-browser` wrapper through to the browser it starts (`omarchy-cmd-default-browser` names it). Run that browser with `--new-window <url>` through `uwsm app --` (or directly when uwsm isn't running), without going through `omarchy-launch-browser`. Brave, Chrome, Chromium, Edge and Firefox all accept `--new-window`.
- If the browser can't be worked out, fall back to today's `xdg-open`.
- `open_in_browser` (the accessibility offer's settings page, `crates/lucyd/src/plugins/mod.rs:898`) passes `--new-window` the same way.
- "Open it on workspace N" stays as two calls: the Omarchy Plugin's `workspace {action: switch}`, then `open_url`. The new window opens on the active workspace. Neither `open_url` nor `launch_app` takes a workspace, so lucyd stays independent of Hyprland.

Spec: [Open pages in a new window](../../open-pages-in-a-new-window/spec.md); the tools were shipped under [v0.1](../../lucy-v0-1/spec.md). Found in daily use (2026-11-18).

**Blocked by:** none

**Seams:** none new. It changes lucyd's `SystemLauncher` (`crates/lucyd/src/launcher.rs`).

**Status:** resolved

**Notes:**

- Out of scope: `launch_app` for an app that runs only one copy and is already open elsewhere (Slack, Obsidian). It focuses its existing window and pulls the user there. These apps usually can't open a second window, so this is a known limit.
- Don't change the user's Hyprland config (`focus_on_activate`). A new window opens on the active workspace either way.
- Tool calls aren't logged, which made this hard to trace. That is ticket 52.

**Acceptance:**

- [x] A test shows `open_url` running the default browser's executable with `--new-window <url>`, and falling back to `xdg-open` when the browser can't be worked out; `open_in_browser` passes `--new-window` too
- [ ] Once, by hand on Omarchy: with Brave open only on workspace 1, ask from workspace 4 for a YouTube search. It opens in a new Brave window on 4, and the desktop stays on 4
- [ ] Once, by hand on Omarchy: from workspace 4, "open YouTube on workspace 3" leaves the desktop on 3, with the page in a new window there

## Comments

2026-11-18: grilled with the user; cause reproduced on the live desktop.

Resolved on `open-pages-in-a-new-window` (9e6334e..7f48ca3). `open_url` runs the default browser's `Exec` program with `--new-window <url>` (through `uwsm app --` when uwsm runs), following `omarchy-launch-browser` (matched by file name) through `omarchy-cmd-default-browser`, and never running it. An `Exec` starting with `flatpak`, `env` or a quoted path, or any failed lookup, falls back to `xdg-open`. Reading an entry's program lives in `desktop_entries::exec_program`. `browser_command` takes the Omarchy lookup as a function, so the wrapper is followed behind the test seam (spec updated). Spawn success still means only that the process started, as before. The two hand checks on Omarchy are still to do.
