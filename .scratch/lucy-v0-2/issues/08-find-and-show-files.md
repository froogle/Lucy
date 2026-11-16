# 08: Find and show files

**What to build:** "Show me my naval conference files in Documents" finds `~/Documents/1921 Naval Conference` and opens it in the file manager. Three new Built-in tools, all with no Confirmation:

- `find_files {words, near?}` matches file and folder **names** only. A name matches when it contains every word, case-insensitive and in any order, so `naval` + `conference` matches `1921 Naval Conference`. The model drops filler words ("files", "my", "the") before calling it. `near` is a loose folder hint ("documents"). Matches under a folder whose name matches it rank first, then the rest of the search.
  - A matching folder is returned once, not with each file inside it.
  - Folders rank before files, then newest first. At most 20 results, with a total count.
  - It returns `found` with paths, or `not_found`.
- `show_files {paths}` opens the file manager with those items highlighted, using the freedesktop `org.freedesktop.FileManager1.ShowItems` D-Bus call. Nautilus and most others implement it. If nothing answers on D-Bus, it opens the parent folder with `xdg-open`.
- `open_file {path}` opens one file in its default app with `xdg-open`. It refuses executables, `.desktop` files and anything outside the search roots.

Lucy says what she found ("I found a folder called 1921 Naval Conference in Documents") and shows it. "Open the second one" opens that file in its default app.

The search runs in lucyd with the `ignore` crate, the walker ripgrep and `fd` are built on, so nothing new needs installing. Roots are `[files] roots` (default `~`). Hidden folders and the entries in `[files] skip` are left out (default: `node_modules`, `target`, `.cache`, `.git`, `venv`, `__pycache__`).

Spec: [v0.2](../spec.md), Finding files. Stories 44–48.

**Blocked by:** lucy-v0-1-build 24 (v0.1 acceptance)

**Seams:** owns the `find_files`, `show_files` and `open_file` Built-in tools, the `[files]` config section, and `show_items` and `open_file` on the `Launcher` adapter (`FakeLauncher` records them). Ticket 03's Hand-off folder search also walks `~`. Whichever of 03 and 08 is built second reuses the other's walker rather than writing a second one.

**Status:** ready-for-agent

**Notes:**

- Raised by the user on 2026-11-16. They asked Lucy to open Nautilus, go to Documents and show the naval conference files. She could only open Nautilus, because her only desktop tools were `open_url`, `launch_app` and `system_status`.
- Names only, by the user's choice. Searching file contents is out of scope for now as too slow.
- Paths go to the Thinking Provider as tool results, like any tool result. They never go on the public socket, in notifications or in logs.
- This doesn't need screen access (06/07). It drives the file manager over D-Bus, not by clicking.

**Acceptance:**

- [ ] In the harness, with a temp home: words match in any order and case; `near` ranks matches in that folder first; hidden and `skip` folders are left out; a matching folder hides its children; the 20-result cap reports the total
- [ ] `show_files` calls `show_items` with the paths; `open_file` calls `open_file`, and refuses an executable, a `.desktop` file and a path outside the roots
- [ ] A custom `[files] roots` and `skip` work
- [ ] No path reaches the bus (`assert_nothing_private_on_bus`)
- [ ] Once, by hand on Omarchy: "show me my naval conference files in Documents" opens Nautilus on `1921 Naval Conference`, highlighted

## Comments
