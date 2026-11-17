# 10: Check the text under a click

**What to build:** The risky-click check stops resting on the model's own `describe`. The Omarchy Plugin gains a probe, `text_at {x, y}`, marked `screen = "probe"`, which returns the accessible name, text and role under the point through AT-SPI, or nothing. Before forwarding `click`, Lucy's core calls the probe itself (it is never listed to the model) and matches its text, as well as `describe`, against `[screen] risky_words`. When the probe returns nothing, the click needs a Confirmation spoken from `describe`: an unreadable target fails closed.

The Plugin also refuses `type` when the focused element is a password field (AT-SPI role `password text`), telling the model to use `ask_typed` or the Plugin's credential path. When the focused element can't be found, `type` goes ahead and the Plugin logs that the check couldn't run.

When the probe finds a browser on the AT-SPI bus with an empty page tree (its accessibility is off), Lucy says "I can't read the page in {browser}, so I'll ask before each click. Want me to open {browser}'s accessibility settings?" and on a yes opens `brave://accessibility` (or `chrome://accessibility`) in a new tab of that browser. At most once per Session. She never turns accessibility on herself.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Stories 40, 53, 54.

**Blocked by:** 07

**Seams:** owns the probe call in the core, the `Screen` trait's AT-SPI methods (text at a point, focused role) and the browser-accessibility prompt. Uses the Manifest `screen` field (owned by 06) and `[screen] risky_words` (owned by 07).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-17, when the user chose to bring the AT-SPI risk check into v0.2 rather than accept a `describe`-only check.
- **Start with a spike.** With accessibility on in Brave, hit-test one button through AT-SPI and read its name. Also find out whether the `brave://accessibility` switch survives a browser restart and whether Chromium turns accessibility off again after about 30 s without AT-SPI use. If either goes wrong, the spoken prompt also suggests a lasting fix. Record the findings in Comments before building the rest.
- **Coordinates:** on Wayland, apps report AT-SPI positions relative to their own window, so screen-coordinate hit-testing doesn't work. Subtract the window's origin from `hyprctl clients -j` and hit-test in window coordinates. `GetAccessibleAtPoint` returns one level at a time, so descend. GTK apps with client-side decorations can be off by their shadow margin.
- **Focus:** AT-SPI has no "what's focused" query. Follow `object:state-changed:focused` events while the Plugin runs, and fall back to searching the active window's app for `STATE_FOCUSED`.
- **Coverage:** GTK3, Qt, Electron and Chromium-family browsers with accessibility on. GTK4 hit-testing is unreliable. Terminals, canvas and Xwayland apps return nothing (and so ask).
- **Crate:** `atspi` (Odilia, on zbus). Its API still moves between minor versions; pin it.
- On this machine (2026-11-17): at-spi2-core 2.62.1, the a11y bus running, `org.a11y.Status IsEnabled` false, so Brave's tree is empty today.

**Acceptance:**

- [ ] Spike findings recorded in Comments
- [ ] Probe and focused-role lookup tested through the Plugin's MCP interface with a fake `Screen`
- [ ] In the harness: probe text matching `risky_words` asks even when `describe` is plain; an empty probe asks; the probe never appears in the model's tool list
- [ ] `type` into a password field is refused with the pointer to `ask_typed`; an unknown focus lets it through and logs it
- [ ] An empty browser tree gets the accessibility offer once per Session; a yes opens the settings page in that browser
- [ ] Once, by hand on Omarchy in Brave with accessibility on: a click the model describes blandly on a "Send" button asks first

## Comments
