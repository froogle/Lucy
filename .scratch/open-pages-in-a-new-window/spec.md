# Open pages in a new window on the user's workspace

**Status:** ready-for-agent

Build ticket: [51](../lucy-v0-1-build/issues/51-apps-open-on-the-wrong-workspace.md).

## Problem Statement

When the user asks Lucy to open a web page, it can appear on a different workspace from the one they're on, or from the one they named, and the desktop switches them there. If they're on workspace 5 and ask for a YouTube search, the search opens on workspace 1 and they are pulled to 1. If they ask her to open YouTube on workspace 3, she switches to 3, then the page opens on workspace 1 and pulls them back.

The page goes into the browser's last-used window as a new tab, wherever that window is. Two things then focus that window and its workspace: the desktop's focus-on-activate setting, and Omarchy's own browser launcher, which focuses the browser after opening a URL. Reproduced on 2026-11-18. With Brave windows on workspaces 1 and 5, last used on 5, opening a page from workspace 4 added a tab on 5 and switched the desktop to 5.

## Solution

Lucy opens every web page in a new window of the user's default browser, so it appears on the active workspace and the desktop stays where it is. To open a page on a named workspace, she switches there first, then opens it. If she can't tell which browser is the default, she opens the page the way she does today.

## User Stories

1. As a Lucy user, I want a page I ask for to open on the workspace I'm on, so that I don't lose my place.
2. As a Lucy user, I want the desktop to stay on my workspace when Lucy opens a page, so that I'm not pulled somewhere else.
3. As a Lucy user, I want "search YouTube for X" to show results on my current workspace, even when the browser is already open on another one, so that I see them where I am.
4. As a Lucy user, I want "open YouTube on workspace 3" to leave me on workspace 3 with the page there, so that I can arrange my desktop by voice.
5. As a Lucy user, I want a page opened on a named workspace to stay there even when my last-used browser window is elsewhere, so that the request works however my windows are arranged.
6. As a Lucy user, I want each page Lucy opens in a window of its own, so that it never adds a tab to a window I'm using for something else.
7. As a Lucy user, I want my existing browser windows and tabs left as they were, so that Lucy doesn't rearrange my work.
8. As a Lucy user, I want pages to open in my default browser, so that my sign-ins, bookmarks and extensions are there.
9. As an Omarchy user, I want Lucy to find the browser Omarchy's default-browser setting names, so that she honours my `omarchy default browser` choice.
10. As an Omarchy user, I want Lucy to avoid Omarchy's browser launcher, so that its focus step doesn't pull me to another browser window.
11. As a user of Brave, Chrome, Chromium, Edge or Firefox, I want a new window to open the same way, so that the fix works whichever of them I use.
12. As a user whose default browser Lucy can't work out, I want the page to open the way it does today, so that a page always opens, even if not in a new window.
13. As a user whose default browser's entry is missing or unreadable, I want the page still to open, so that a broken setting doesn't stop Lucy from showing me things.
14. As a user without uwsm, I want the new browser window started directly, so that the fix works outside a uwsm session.
15. As a user with uwsm, I want the new window started through uwsm as other apps are, so that it runs in its own unit like anything launched from the desktop.
16. As a Lucy user who accepted her offer to turn on browser accessibility, I want the browser's settings page to open in a new window on my workspace too, so that it doesn't pull me elsewhere either.
17. As a Lucy user, I want Lucy to say she opened the page only when the browser was started, so that she doesn't claim something that didn't happen.
18. As a Lucy user, I want `open_url` to keep refusing anything but http and https, so that a page request can't run something else.
19. As a person reading the code later, I want the desktop tools to stay independent of Hyprland, so that Lucy works on desktops other than Omarchy.

## Implementation Decisions

- **The module that changes:** lucyd's system launcher, the real implementation of the `Launcher` adapter that the desktop Built-in tools use. Neither the `Launcher` trait nor any tool's schema changes.
- **`open_url`:** the system launcher stops passing the URL to `xdg-open`. It finds the default browser's executable and runs it with `--new-window <url>`: through `uwsm app --` when uwsm is running, directly otherwise. This applies to the `open_url` Built-in tool, which every web page request uses.
- **Finding the default browser:** ask `xdg-settings get default-web-browser` for the default desktop entry's name. Look it up in the XDG data dirs, in order (`$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`, with the usual defaults), and take the program from its `Exec` line.
- **Omarchy's wrapper:** if the default entry is Omarchy's browser wrapper (its `Exec` runs `omarchy-launch-browser`), ask `omarchy-cmd-default-browser` for the browser it stands for and look that entry up the same way. Lucy never runs `omarchy-launch-browser`, because it focuses the browser after opening a URL. This puts knowledge of one Omarchy command into lucyd, alongside its existing knowledge of uwsm. The user accepted that.
- **Fallback:** if any step fails (no answer from `xdg-settings`, no entry found, no `Exec`, wrapper not resolved), open the URL with `xdg-open` as today. A failed lookup is logged at debug level, not reported to the model.
- **`--new-window`:** passed to every resolved browser. Brave, Chrome, Chromium, Edge and Firefox all accept it. Lucy doesn't keep a list of browsers she knows.
- **`open_in_browser`:** the accessibility offer's settings page, opened in a named browser, also gets `--new-window` before the page.
- **Named workspaces:** "open it on workspace N" stays two calls by the model: the Omarchy Plugin's `workspace {action: switch}`, then `open_url`. Neither `open_url` nor `launch_app` gains a workspace argument, and lucyd doesn't talk to Hyprland. The new window opens on whatever workspace is active.
- **Shape for testing:** working out the browser command is a pure function. It takes the URL, the default entry's name (already resolved, including through the Omarchy wrapper) and the data dirs, and returns the command to run: the browser with `--new-window <url>`, or the `xdg-open` fallback. Running `xdg-settings` and `omarchy-cmd-default-browser`, and starting the process, stay outside it.

## Testing Decisions

- A good test checks the command Lucy would run for a given setup, not how the lookup is done inside, so the lookup can change without breaking tests.
- **One seam: the system launcher's browser-command function.** Tests build temporary data dirs with desktop entries and check the command it returns:
  - an ordinary browser entry gives that browser with `--new-window <url>`;
  - Omarchy's wrapper, once resolved to the browser's entry, gives that browser, not `omarchy-launch-browser`;
  - an earlier data dir wins over a later one;
  - a missing entry, an entry without `Exec`, or no default at all gives the `xdg-open` fallback;
  - `open_in_browser`'s command has `--new-window` before the page.
- Prior art: the system launcher's own unit tests. `a_configured_launcher_is_run_with_the_entry_path_appended` checks the exact command line a launcher runs, and the file-URI test checks a pure helper.
- The desktop Built-in tool tests in lucyd's harness stay as they are. They drive `open_url` against a fake `Launcher`, and the tool's behaviour as the model sees it doesn't change.
- No test changes the process environment, and none talks to Hyprland. Starting the process, and which workspace the window lands on, are covered by the hand checks:
  - with Brave open only on workspace 1, a YouTube search asked for from workspace 4 opens in a new window on 4, and the desktop stays on 4;
  - from workspace 4, "open YouTube on workspace 3" leaves the desktop on 3 with the page there.

## Out of Scope

- `launch_app` for an app that runs only one copy and is already open on another workspace, such as Slack or Obsidian. It focuses its existing window and pulls the user there. These apps usually can't open a second window, so this is a known limit.
- Opening files and showing them in the file manager (`open_file`, `show_items`), which still use `xdg-open` and D-Bus.
- Changing the user's Hyprland settings, such as focus-on-activate.
- A workspace argument on any Built-in tool, or lucyd placing windows itself.
- Logging tool calls, which is [ticket 52](../lucy-v0-1-build/issues/52-log-tool-calls.md).

## Further Notes

- `[desktop] launcher` in the config applies to `launch_app` only and doesn't affect `open_url`.
- If a browser ignores `--new-window`, the page opens as a tab as before. Nothing else breaks.
