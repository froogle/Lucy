# Indicator toolkit: how to draw a clickable bottom-right marker from Rust on Hyprland

Ticket: `issues/06-indicator-toolkit.md`. Researched 2026-11-12 against crates.io, upstream repos and docs, the wlr-layer-shell spec, the Hyprland wiki, and this machine (Hyprland 0.56.2, Omarchy with the Quickshell-based `omarchy-shell`).

## Recommendation

Draw the Indicator with **smithay-client-toolkit (SCTK) layer-shell plus tiny-skia into `wl_shm` buffers**, running on its own thread (or as a small `lucy-indicator` helper process) and driven by Session state messages.

- Use a `zwlr_layer_shell_v1` surface on the `overlay` layer, anchored `BOTTOM | RIGHT` with margins, keyboard interactivity `none` and exclusive zone `0`. Size it to the marker itself, so it needs no click-through cut-outs.
- Listening, thinking and speaking are drawn per frame (pulse, spinner, level bars) using `wl_surface.frame` callbacks.
- Colours come from Omarchy's `colors.toml`, reloaded when the theme changes, with a built-in palette as the fallback.
- `null` output: the compositor puts it on the monitor the user last used.
- If the compositor has no layer-shell (GNOME), fall back to a freedesktop notification with an "End" action.

**Runner-up:** `gtk4` + `gtk4-layer-shell`. Use it if the Indicator grows text, transcripts or menus. It is mature, MIT-licensed and gives CSS theming and animation, but it pulls in GTK4 plus a C library and needs its own GLib main loop.

## Options compared

| | SCTK + tiny-skia | gtk4 + gtk4-layer-shell | iced_exwlshell (iced) | egui | Delegate to Omarchy shell / notifications |
|---|---|---|---|---|---|
| Crate / version | `smithay-client-toolkit` 0.21.1 (MIT, 22.8M recent dl); `tiny-skia` 0.12.0 (BSD-3) | `gtk4-layer-shell` 0.8.1 (MIT), on `gtk4` 0.11 / `glib` 0.22 | `iced_exwlshell` 0.21.4 (MIT, iced 0.14). Replaces `iced_layershell`, deprecated from 0.20 | No maintained layer-shell crate on crates.io (searched; nothing relevant) | Quickshell plugin (QML), or the `org.freedesktop.Notifications` D-Bus API |
| Native deps | libwayland, libxkbcommon only | GTK4 + `gtk4-layer-shell` system lib (Arch: `gtk4-layer-shell` 1.3.0 installed here). libwayland must be linked *after* gtk4-layer-shell | wgpu/tiny-skia renderer, large dependency tree | n/a | Omarchy-only (QML plugin) / any notification daemon |
| Click handling | `wl_pointer` button events via SCTK `PointerHandler`; input region is the surface (or `wl_surface.set_input_region`) | GTK gesture on the widget; `gdk4::SurfaceExt::set_input_region` for shaped regions | iced events | — | QML `MouseArea`, or a notification action |
| Animation | Own loop: redraw on `frame` callbacks, only while a Session is live | GTK CSS `@keyframes`/transitions, or a `DrawingArea` with a tick callback | iced subscriptions/animations | — | QML animations (very good) / none for notifications |
| Theming | Read Omarchy `colors.toml` ourselves | Generate GTK CSS from `colors.toml` | Same as SCTK | — | Free on Omarchy: the shell's `Style` singleton follows the theme |
| Maturity / risk | Core of the Rust Wayland stack (winit, cosmic use it). Low-level but stable | Mature, widely used for bars and panels | API churns (0.19→0.20→0.21 migrations in one year); popups experimental; docs thin per README | — | Ties Lucy to Omarchy internals and a non-Rust codebase |
| Portability | Any compositor with wlr-layer-shell | Same set | Same set | — | Omarchy only / everywhere (but not a persistent clickable marker) |

### Why SCTK + tiny-skia

The Indicator is a ~48–64 px glyph with three or four animated states and one click target. That is a few hundred lines of drawing, not a widget tree. SCTK's own `examples/simple_layer.rs` already shows the whole skeleton: `create_layer_surface`, `set_anchor`, `set_size`, `SlotPool` shm buffers, `frame` callbacks and pointer handling ([source](https://github.com/Smithay/client-toolkit/blob/master/examples/simple_layer.rs)). Pure-Rust drawing keeps the open-source build simple (`cargo build` with no GTK dev headers) and keeps a toolkit main loop out of Lucy's async daemon. gtk4 becomes worth its weight only if the Indicator needs text layout, accessibility or menus.

### Why not the others

- **iced_exwlshell**: fine technically, but the API has broken twice in a year. The upstream README calls docs thin and popups experimental ([repo](https://github.com/waycrate/exwlshelleventloop)). That is a heavy framework for one glyph.
- **egui**: no maintained layer-shell integration exists on crates.io.
- **Delegating to Omarchy's shell**: Omarchy runs one long-lived Quickshell process whose bar, panels, overlays and services are plugins (`/usr/share/omarchy/shell/README.md`, `plugins/README.md`). Third-party plugins live in `~/.config/omarchy/plugins/<id>/`, with kinds such as `bar-widget`, `overlay`, `panel` and `service`; the installed `superwhisper-panel` is an example. A Lucy plugin would theme and animate beautifully, but it is QML, works only on Omarchy, and depends on an internal plugin contract. Treat it as an optional extra later, not the core Indicator.
- **Notifications**: these sit top-right on Omarchy (`omarchy-notifications` layer at x=3041, y=0). They expire and cannot show live state animation. They are acceptable only as the no-layer-shell fallback.

## Specific design points

### Layer, anchoring, input

Per the wlr-layer-shell spec ([wayland.app](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)):

- **Layers:** the four are background, bottom, top and overlay. Use **overlay** so the Indicator stays above fullscreen windows (fullscreen sits at the top layer) and above Omarchy's dock, which lives on the `top` layer at the bottom edge (`hyprctl layers`: `omarchy-dock` 0,1233 3440x207).
- **Anchoring:** two perpendicular anchors (bottom + right) pin the surface to a corner, and margins offset it.
- **Exclusive zone:** `0` means "move me out of reserved areas". It reserves no space, so windows don't reflow when a Session starts.
- **Keyboard interactivity:** `none` is the default; the Indicator never takes focus.
- **Click-through vs clickable:**
  - Make the surface exactly the marker's size. The whole surface is then clickable and nothing around it is blocked.
  - For a round marker, set `wl_surface.set_input_region` to the circle's bounding box, or leave the full rect; at this size the difference is negligible.
  - A larger surface (e.g. room for a glow) needs an explicit input region; otherwise the transparent area swallows clicks. Omarchy does this: its OSD sets `mask: Region {}` to be fully click-through (`plugins/osd/Osd.qml:131-134`). Its bar surface is fullscreen-sized and masked.

### Multi-monitor

- **Default:** passing a `null` output lets the compositor pick, "generally the one the user last interacted with" (spec). This is the right default and needs no Hyprland-specific code.
- **Optional config:** let users pin the Indicator to a named output (match `wl_output` name via SCTK `OutputState`).
- **Monitor unplugged:** handle the `closed` event by recreating the surface.
- This machine has a single 3440x1440 monitor (DP-2, scale 1) whose only reserved area is the 26 px top bar. Multi-monitor behaviour is therefore untested here.
- HiDPI: honour `wl_surface` preferred buffer scale (or `wp_fractional_scale_v1` + `wp_viewporter`) so the glyph stays crisp.

### Animation states

These are drawn in tiny-skia, redrawing only while animating, and drive damage on `frame` callbacks:

- **Listening:** a breathing ring. Optionally scale it by mic RMS, which the audio thread already has.
- **Thinking:** a rotating arc.
- **Speaking:** bars or a ring pulse driven by TTS output level.

With no Session there is no surface at all; destroy it on Session end. Cost is a ~64x64 ARGB buffer, so CPU rendering is trivial.

### Theming (follow Omarchy)

- **Source of colours:** the current theme is at `~/.local/state/omarchy/current/theme/` (`theme.name` = `hackerman` here). `colors.toml` has flat keys: `accent`, `background`, `foreground`, `red`, `green`, ... and `mode = "dark"`.
- **Shell surface tokens:** `shell.toml` derives these from `colors.toml`. For example, `[bar] active` is "Modules calling attention to themselves (recording, voxtype, alerts, updates)". That is the natural colour for "listening".
- **Change signal:** `~/.local/state/omarchy/current/theme.generation` is rewritten on theme change. Watch that directory with inotify and reload. Omarchy also offers `~/.config/omarchy/hooks/theme-set.d/` (called with the theme name), but Lucy should not install hooks into the user's config.
- **Not hard-coded:** the path is read from config with the Omarchy location as the default. Lucy falls back to a built-in palette when the file is absent (non-Omarchy desktops), per the "nothing personal hard-coded" note.
- **Hyprland-side polish:** users may add a layer rule on Lucy's namespace (use a stable namespace such as `lucy-indicator`). Hyprland 0.56's config is Lua: `hl.layer_rule()` with effects such as `animation`, `blur`, `no_anim` ([wiki](https://wiki.hypr.land/configuring/core/rules/layer-rules/)). Older versions used `layerrule = noanim, namespace`. Lucy should document this, not depend on it.

### Portability

- **Works:** wlr-layer-shell is supported by Hyprland (v5), KWin/KDE Plasma (v5), COSMIC (v5), Sway (v4), niri, labwc, river, Wayfire, Mir and others ([wayland.app table](https://wayland.app/protocols/wlr-layer-shell-unstable-v1); [gtk4-layer-shell README](https://github.com/wmww/gtk4-layer-shell) lists wlroots, Smithay/COSMIC, Mir and KDE).
- **Does not work:** GNOME/Mutter (marked unsupported in both) and X11.
- **GNOME fallback:** check for `zwlr_layer_shell_v1` in the registry at startup. If it is absent, show a resident notification ("Lucy is listening", action "End") over `org.freedesktop.Notifications`. A GNOME Shell extension is the only way to get a true overlay there, which is out of scope.

## Sources

- crates.io API (versions/licences/deps, fetched 2026-11-12): `smithay-client-toolkit` 0.21.1, `tiny-skia` 0.12.0, `gtk4-layer-shell` 0.8.1 (deps `gtk4 ^0.11`, `gtk4-layer-shell-sys ^0.6`), `iced_exwlshell` 0.21.4 (`iced ^0.14`), `iced_layershell` 0.19.1, `relm4` 0.11.0. A search for an egui layer-shell crate found nothing relevant.
- https://github.com/wmww/gtk4-layer-shell: supported compositors, no GNOME/X11, MIT, link-order note.
- https://wayland.app/protocols/wlr-layer-shell-unstable-v1: layers, anchors, exclusive zone, keyboard interactivity, null output, compositor support table.
- https://github.com/waycrate/exwlshelleventloop: iced_exwlshell status, deprecations, experimental popups.
- https://github.com/Smithay/client-toolkit/blob/master/examples/simple_layer.rs: reference skeleton.
- https://docs.rs/gdk4/latest/gdk4/prelude/trait.SurfaceExt.html: `set_input_region(Option<&Region>)`.
- https://wiki.hypr.land/configuring/core/rules/layer-rules/: Lua `hl.layer_rule()`, `animation`/`blur`/`no_anim`.
- Local, read-only: `hyprctl version` (0.56.2), `hyprctl monitors`/`layers`; `/usr/share/omarchy/shell/{README.md,plugins/README.md,plugins/osd/Osd.qml}`; `~/.config/omarchy/plugins/superwhisper-panel/manifest.json`; `~/.local/state/omarchy/current/theme/{colors.toml,shell.toml}`, `theme.generation`.

## Open questions

- Should Lucy also ship an optional Omarchy shell plugin, either as a richer Indicator or a bar widget? That would need a stable IPC/status contract like Superwhisper's `PROTOCOL.md`.
- Is the Indicator in-process (a thread in the daemon) or a separate helper binary? A helper isolates Wayland/display crashes and lets a headless daemon run without it.
- Not verified here: multi-monitor and fractional-scale behaviour (single scale-1 monitor), and KDE/COSMIC behaviour.
