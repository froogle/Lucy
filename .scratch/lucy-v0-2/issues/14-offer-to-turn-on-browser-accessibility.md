# 14: Offer to turn on browser accessibility

**What to build:** When the probe finds a browser on the AT-SPI bus with an empty page tree (its accessibility is off), Lucy says "I can't read the page in {browser}, so I'll ask before each click. Want me to open {browser}'s accessibility settings?". On a yes she opens `brave://accessibility` (or `chrome://accessibility`) in a new tab of that browser. She offers at most once per Session, and the click itself still needs its Confirmation. She never turns accessibility on herself.

Spec: [v0.2](../spec.md), Screen access (Browser accessibility). ADR 0008. Story 54.

**Blocked by:** 10

**Seams:** owns the browser-accessibility offer. Uses the probe (owned by 10).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-17 (the user's choice: check and prompt when needed, rather than at setup) and split out of 10 the same day.
- Read 10's spike findings first. If the settings-page switch doesn't survive a restart, or Chromium turns accessibility off again when idle, the spoken offer also names a lasting fix.

**Acceptance:**

- [ ] In the harness: an empty browser tree gets the offer once per Session, and the click still asks; a yes opens the settings page in that browser through the `Launcher`
- [ ] A non-browser app that returns nothing gets no offer
- [ ] Once, by hand on Omarchy with Brave's accessibility off: the offer is made and opens the page

## Comments
