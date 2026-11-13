# Which Rust toolkit can draw a clickable bottom-right Indicator on Hyprland?

Type: research
Status: resolved
Blocked by: none

## Question

Compare ways to render a small always-on-top, clickable, bottom-right overlay from Rust on Wayland/Hyprland: gtk4-layer-shell, smithay-client-toolkit layer-shell, iced/egui layer-shell crates, or delegating to an existing bar/notification. Cover click-through vs clickable input regions, animation for listening/thinking/speaking states, theming (follow Omarchy theme), multi-monitor placement, and portability to other wlroots/KDE/GNOME desktops. Recommend one.

## Answer

Use **smithay-client-toolkit layer-shell + tiny-skia** (pure Rust, `wl_shm` buffers), on its own thread or in a small helper. The surface sits on the `overlay` layer, anchored bottom|right with margins. It has keyboard interactivity `none` and exclusive zone `0`, and is sized to the marker so it is clickable with nothing around it blocked.
Animate listening/thinking/speaking per `frame` callback. Take colours from Omarchy's `~/.local/state/omarchy/current/theme/colors.toml` and reload when `theme.generation` changes, with a built-in fallback palette. Use a `null` output so the compositor picks the active monitor.
Works on Hyprland, KDE, COSMIC, Sway and other wlroots compositors; GNOME has no layer-shell, so fall back to a notification with an "End" action. Runner-up: gtk4 + gtk4-layer-shell, if the Indicator ever needs text or menus. An Omarchy Quickshell plugin is a possible later extra, not the core.
[findings](../research/indicator-toolkit.md)
