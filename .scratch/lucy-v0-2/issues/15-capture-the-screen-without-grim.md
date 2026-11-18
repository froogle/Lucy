# 15: Capture the screen without grim

**What to build:** A bug fix to 06. Every look fails with "grim failed: failed to create buffer". `grim` gets its buffer through `shm_open`, a file in `/dev/shm`, and the Omarchy Plugin's Landlock sandbox doesn't allow that (reproduced under an equivalent ruleset; with `/dev/shm` writable, grim works). The fix: the Omarchy Plugin's `Wayland::capture` captures the screen itself over Wayland and stops running `grim`. Granting `/dev/shm` instead was turned down: Landlock can't narrow it to grim's own file, so the Plugin would reach every app's shared memory.

- **Protocol:** `ext-image-copy-capture-v1`, output source only (from `wayland-protocols` 0.32 with its `staging` feature). No `wlr-screencopy` fallback. If the compositor doesn't offer it, the look fails with a clear message (e.g. "this desktop doesn't offer screen capture").
- **Connection:** a fresh Wayland connection per look, in `spawn_blocking`, closed after the look, like `wayland::act`.
- **Buffer:** a shm buffer in a memfd, never dmabuf. Only the 8-bit formats ARGB8888, XRGB8888, ABGR8888 and XBGR8888 are accepted; if the compositor offers none of them, the look fails clearly. The cursor is left out.
- **Transforms:** the buffer's transform from the frame is applied, all eight (rotations and flips), before cropping.
- **Patience:** a look waits at most 5 s for the frame to be ready, then fails with "the screen capture didn't finish".
- **Region:** a monitor look captures the monitor. A window look captures the window's own monitor (the one `hyprctl` names, whose scale the look already uses) and crops the window's rectangle out of it, mapping layout coordinates to buffer pixels at the monitor's scale. That shows what is drawn there, overlapping windows included, so click coordinates stay true. Any part of the window on another monitor is clipped.
- **Scale and encode:** to the `scale` that `screenshot_for_model` passes, with `image`'s Triangle filter, then encoded as PNG. Add `image` with only its PNG feature (MIT/Apache, ADR 0003). `Screen::capture`'s signature and `Image` don't change.
- **Remove grim:** take `grim` out of the manifest's `commands` and rewrite that `[access]` comment to say capture uses a memfd over the Wayland socket. Remove `Plugin::with_grim` and its fake `grim` from the tests. The manifest's hash changes, so the Omarchy Plugin needs approving again after the redeploy (ADR 0004).
- **Docs:** update the screen section of `docs/agents/working-in-lucy.md` ("`grim` for capture"), and add a comment to 06 pointing here.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Fixes [06](06-look-at-the-screen.md).

**Blocked by:** none

**Seams:** owns the real capture inside the Omarchy Plugin's `Screen` (`Wayland::capture`). Uses the `Screen` trait (owned by 06).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-18. Built alone as slice 5 on `lucy-v0-2-slice-5`, branched from `main`.
- Not a driver or Hyprland regression: nothing was upgraded after 2026-10-24, and looks were built on 2026-11-17. Looks have never worked inside the sandbox; the tests missed it because they use a fake `grim`.
- Hyprland 0.56.2 offers `ext_image_copy_capture_manager_v1` with `ext_output_image_capture_source_manager_v1`.
- Keep the Wayland I/O thin, and put the format conversion, transforms, cropping and scaling in pure functions so they can be unit-tested. Tests never reach the real compositor, as with input.
- Out of scope: a `wlr-screencopy` fallback, stitching several monitors, capturing a covered window's own contents (the per-window source), dmabuf and 10-bit/HDR formats, and GNOME/KDE (they would need an xdg-desktop-portal `Screen`; the `Screen` trait leaves room for it).

**Acceptance:**

- [ ] Unit tests: each accepted format converts to RGBA with the right byte order, honouring a stride wider than the row; each of the eight transforms; cropping a layout region to buffer pixels at fractional and integer scales, clipped to the monitor; scaling gives `screen::scaled`'s size
- [ ] The existing `FakeScreen` tests of `screenshot_for_model` still pass; `grim` is gone from the manifest, the tests and the docs
- [ ] Once, by hand on Omarchy after the redeploy and re-approval: "what's on my screen?" and a look at one window both work, with the Plugin sandboxed and no `/dev/shm` grant

## Comments
