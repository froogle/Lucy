# 14: Offer to turn on browser accessibility

**What to build:** When the probe finds a browser on the AT-SPI bus with an empty page tree (its accessibility is off), Lucy says "I can't read the page in {browser}, so I'll ask before each click. Want me to open {browser}'s accessibility settings?". On a yes she opens `brave://accessibility` (or `chrome://accessibility`) in a new tab of that browser. She offers at most once per Session, and the click itself still needs its Confirmation. She never turns accessibility on herself.

Spec: [v0.2](../spec.md), Screen access (Browser accessibility). ADR 0008. Story 54.

**Blocked by:** 10

**Seams:** owns the browser-accessibility offer. Uses the probe (owned by 10).

**Status:** resolved

**Notes:**

- Settled in a grilling session on 2026-11-17 (the user's choice: check and prompt when needed, rather than at setup) and split out of 10 the same day.
- Read 10's spike findings first. If the settings-page switch doesn't survive a restart, or Chromium turns accessibility off again when idle, the spoken offer also names a lasting fix.

**Acceptance:**

- [ ] In the harness: an empty browser tree gets the offer once per Session, and the click still asks; a yes opens the settings page in that browser through the `Launcher`
- [ ] A non-browser app that returns nothing gets no offer
- [ ] Once, by hand on Omarchy with Brave's accessibility off: the offer is made and opens the page

## Comments

2026-11-18: built on `ticket-14-offer-browser-accessibility`. The `brave://accessibility` switch doesn't survive a restart (10's spike), so the offer names the lasting fix. Wording, for Brave (Chrome and Chromium swap the name and `chrome-flags.conf` / `chromium-flags.conf`): "I can't read the page in Brave, so I'll ask before each click. Its accessibility switch turns off when Brave restarts; the force-renderer-accessibility flag in brave-flags.conf keeps it on. Want me to open Brave's accessibility settings?". The probe's answer carries `accessibility_off: "brave" | "chrome" | "chromium"`; lucyd maps it to its own program (`brave`, `google-chrome-stable`, `chromium`) and page, so a Plugin never names what lucyd runs. The Omarchy Plugin says a window is a browser by its Hyprland class (`brave-*`, which includes Omarchy's Brave web apps, `google-chrome`, `chromium`) and calls the tree empty when a walk of the frame (at most 4000 nodes, `MOST_SEARCHED`) finds no `document web`; a tree too big to walk gets no offer. A no to the offer is answered "Okay, I won't." before the click's Confirmation.

2026-11-18: built in slice 4 (`lucy-v0-2-slice-4`). New `Launcher::open_in_browser`; the probe reports `accessibility_off` as one of brave, chrome or chromium, and lucyd maps it to its own fixed program and page. A browser is known by its Hyprland class. Running out of the 4000-element search budget means "can't tell", so no offer. The offer is made only for clicks whose `describe` is plain, since those are the only ones probed. Hand checks left: with Brave's accessibility off, the offer is made and opens the page. Also check that `brave brave://accessibility` (and `google-chrome-stable chrome://accessibility`) really opens the page in a new tab, since browsers may refuse internal pages from the command line.
