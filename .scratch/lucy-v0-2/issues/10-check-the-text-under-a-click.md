# 10: Check the text under a click

**What to build:** The risky-click check stops resting on the model's own `describe`. The Omarchy Plugin gains a probe, `text_at {x, y}`, marked `screen = "probe"`, which returns the accessible name, text and role under the point through AT-SPI, or nothing. Before forwarding `click`, Lucy's core calls the probe itself (it is never listed to the model) and matches its text, as well as `describe`, against `[screen] risky_words`. When the probe returns nothing, the click needs a Confirmation spoken from `describe`: an unreadable target fails closed.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Story 53.

**Blocked by:** 07

**Seams:** owns the Plugin's AT-SPI connection and the `Screen` trait's text-at-a-point method (13 and 14 use it), and the probe call in the core. Uses the Manifest `screen` field (owned by 06) and `[screen] risky_words` (owned by 07).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-17, when the user chose to bring the AT-SPI risk check into v0.2 rather than accept a `describe`-only check. Split the same day: the password check is 13 and the browser-accessibility offer is 14.
- **Start with a spike.** With accessibility on in Brave, hit-test one button through AT-SPI and read its name. Also find out whether the `brave://accessibility` switch survives a browser restart and whether Chromium turns accessibility off again after about 30 s without AT-SPI use (14 needs both answers). Record the findings in Comments before building the rest.
- **Coordinates:** on Wayland, apps report AT-SPI positions relative to their own window, so screen-coordinate hit-testing doesn't work. Subtract the window's origin from `hyprctl clients -j` and hit-test in window coordinates. `GetAccessibleAtPoint` returns one level at a time, so descend. GTK apps with client-side decorations can be off by their shadow margin.
- **Coverage:** GTK3, Qt, Electron and Chromium-family browsers with accessibility on. GTK4 hit-testing is unreliable. Terminals, canvas and Xwayland apps return nothing (and so ask).
- **Crate:** `atspi` (Odilia, on zbus). Its API still moves between minor versions; pin it.
- On this machine (2026-11-17): at-spi2-core 2.62.1, the a11y bus running, `org.a11y.Status IsEnabled` false, so Brave's tree is empty today.

**Acceptance:**

- [ ] Spike findings recorded in Comments
- [ ] The probe is tested through the Plugin's MCP interface with a fake `Screen`
- [ ] In the harness: probe text matching `risky_words` asks even when `describe` is plain; an empty probe asks; the probe never appears in the model's tool list
- [ ] Once, by hand on Omarchy in Brave with accessibility on: a click the model describes blandly on a "Send" button asks first

## Comments
