# 10: Check the text under a click

**What to build:** The risky-click check stops resting on the model's own `describe`. The Omarchy Plugin gains a probe, `text_at {x, y}`, marked `screen = "probe"`, which returns the accessible name, text and role under the point through AT-SPI, or nothing. Before forwarding `click`, Lucy's core calls the probe itself (it is never listed to the model) and matches its text, as well as `describe`, against `[screen] risky_words`. When the probe returns nothing, the click needs a Confirmation spoken from `describe`: an unreadable target fails closed.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Story 53.

**Blocked by:** 07

**Seams:** owns the Plugin's AT-SPI connection and the `Screen` trait's text-at-a-point method (13 and 14 use it), and the probe call in the core. Uses the Manifest `screen` field (owned by 06) and `[screen] risky_words` (owned by 07).

**Status:** resolved

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

2026-11-18: spike findings (Brave 157.1.99.55, throwaway profiles, `org.a11y.Status IsEnabled` left false).

### Spike: Answers

- **Hit-testing a button works** with accessibility on, in window
  coordinates: `GetAccessibleAtPoint(x, y, WINDOW)` on the Brave frame gave
  the button "Send" (role `push button`, or its `static` text leaf, name
  "Send"), about 10 ms per probe, 70-170 ms with the retry below. Lucy's
  Rust probe (`atspi` 0.30.0) read "Send" and "Play" correctly 16 times out
  of 16.
- **Screen coordinates don't work**, as expected: the frame's extents are
  `0,0` in both SCREEN and WINDOW types while Hyprland has the window at
  `1727,38`; the same point in screen coordinates hits nothing. Subtracting
  Hyprland's `at` is right for Brave (no client-side decoration offset; the
  page starts 137 px down, below Brave's own toolbar, which AT-SPI includes).
- **Chromium answers a hit-test into web content asynchronously.** A hit
  test that lands on the document, or on a leaf, returns that same object;
  right after a hit on the page's document, the next hit-test elsewhere in
  the page returned the document again (stale) until it was asked a second
  time. Lucy's probe re-asks when an object returns itself (2 retries, 40 ms
  apart); one retry already fixed every case seen.
- **Chromium descends in one step:** the frame's `GetAccessibleAtPoint`
  returned the button directly; the probe still descends level by level for
  GTK and Qt.
- **Accessibility off:** without `--force-renderer-accessibility` the Brave
  frame is on the bus (name = window title) but has one child and nothing
  under it; a hit-test returns nothing. Querying it over AT-SPI for 15 s did
  not turn accessibility on. `ACCESSIBILITY_ENABLED=1` in Brave's
  environment did not turn it on either. (14: "a browser on the bus with an
  empty page tree" = the frame of a Brave/Chrome app whose descendants hold
  no `document web`.)
- **The `brave://accessibility` switch does not survive a restart.**
  Turning on "Native accessibility API support" and "Web accessibility" on
  that page (in the throwaway profile, through the page's own checkboxes)
  filled the tree at once; after quitting Brave and starting it again on the
  same profile, every switch on the page was off and the tree was empty
  again.
- **No auto-off seen after 30 s.** Turned on through `brave://accessibility`,
  accessibility stayed on (page switches on after a reload, hit-test still
  read "Send") after 100 s with no AT-SPI use; turned on by
  `--force-renderer-accessibility`, it stayed on after 70 s idle.
- **Matching a window to its AT-SPI app:** the AT-SPI application's bus
  name's Unix PID (`org.freedesktop.DBus.GetConnectionUnixProcessID` on the
  a11y bus) equals Hyprland's `pid` for the window (Brave's browser process;
  Ghostty too). The frame's name equals Hyprland's `title`; the active frame
  has state `Active`.
- **Terminals:** Ghostty (GTK4) has a frame on the bus; a hit-test gives a
  nameless `panel` with no text, so the probe returns nothing (and Lucy asks).

### Spike: Still open

- **Chromium's 30 s auto-disable** applies to accessibility Chromium turned
  on by itself when it saw an assistive technology (`IsEnabled` true, or an
  AT's calls); that path can't be tested without setting `org.a11y.Status
  IsEnabled`, which this spike was not allowed to change. What was tested
  (the page switch and the flag) didn't turn off. Recheck with Orca or
  `IsEnabled` on, by hand.
- Whether a page that `brave://accessibility` turned on keeps it across a new
  tab or navigation was not tested (it stayed on across a reload of that
  page and across tab switches in the same run).
- GTK3 and Qt apps weren't hit-tested (none was running with a window); the
  CSD shadow offset for GTK3 is still unmeasured.
- The `document web` object is named after the page title, so a click on
  empty page space reads as the title: a title holding a risky word asks.

2026-11-18: built in slice 3 (`lucy-v0-2-slice-3`). `atspi` is pinned at `=0.30.0` (it brings a second zbus, 5.x). The probe runs only when `describe` is plain, and has 3 s on `Adapters.clock`; a barge-in cuts it off. It returns nothing, so the click asks, for: Xwayland windows, a window with no single frame titled like it, a point in a top or overlay layer surface, or any showing popup, menu or tooltip in the app (a popup's AT-SPI coordinates are its own surface's, so it isn't hit-tested). The Confirmation is spoken from `describe` only; screen text is never spoken. Hand checks left: in Brave with accessibility on, a click blandly described on a "Send" button asks first; a click on a GTK3 and a Brave context menu asks.
