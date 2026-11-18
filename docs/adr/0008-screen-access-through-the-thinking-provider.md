# Lucy sees the screen through screenshots read by the Thinking Provider, and a fixed risk list, not the model, decides which clicks need a Confirmation

To act on what is on screen ("play the third video"), Lucy needs to see it and drive input. The desktop Plugin (the Omarchy Plugin first) provides the screen tools: a screenshot returned as an MCP image, a click at a point, typing, keys and scrolling. Input goes through Wayland's virtual-pointer and virtual-keyboard protocols. The Session model runs the look–act–check loop itself through ordinary tool calls. There is no separate vision Provider, because the Thinking Provider (`claude`) already reads images. Lucy's core adds only two things: image content passing through proxied tool results, and the privacy and Confirmation rules below.

Privacy. The first look in each Session needs a spoken Confirmation ("I need to look at your screen. Okay?"), because the screenshot leaves the machine for the Thinking Provider. Later looks in that Session show a "looking" mark on the Indicator. Images are never logged, written to disk or put on the public socket, and they go when the Session transcript is deleted. A config switch turns screen access off.

Clicks. The runtime can't know what a button does, and screen content is untrusted input that can carry a prompt injection. So `click` takes a required `describe` ("the Play button on the third video"). The runtime asks for a Confirmation when that text, or the text under the point read through AT-SPI where it is available, matches a fixed, configurable list of risky words (send, buy, pay, delete, submit, post, confirm). The match is made locally. The model never decides whether a click is risky. Typing into a password field is refused: secrets go through the Input dialog to the tool that needs them, never through the screen.

_Amended 2026-11-17._ `describe` is the model's own words, so on its own the check catches mistakes, not a hostile model. The text under the point is therefore checked too, through a probe tool in the desktop Plugin that the core calls before each click whose `describe` is plain (a risky `describe` already asks) and never lists to the model. When the probe can't read the target (a terminal, a canvas app, a browser with accessibility off) the click needs a Confirmation: an unreadable target fails closed. `key` carries a `describe` and gets the same word check, because Enter can send as surely as a click. The password check lives in the desktop Plugin, which can see the focused element, and lets typing through when it can't tell, because the model never holds the user's secrets: the check prevents a mistake, not a leak. Lucy never turns a browser's accessibility on herself; when it is off she says so and offers to open the browser's own setting. That switch (`brave://accessibility`, `chrome://accessibility`) doesn't survive a restart of the browser (ticket 10's spike), so the offer also names the lasting fix: the `--force-renderer-accessibility` flag in the browser's flags file (`brave-flags.conf`, `chrome-flags.conf`, `chromium-flags.conf`). Screenshots exist on disk only inside the Session's `claude` transcript, which is deleted at Session end and swept at lucyd's next start after a crash.

## Consequences

- lucyd holds `rmcp`'s log to warnings whatever `RUST_LOG` asks, because its trace prints whole MCP messages, screenshots included, and images are never logged.

## Considered Options

- Accessibility tree (AT-SPI) only: exact targets, but Chromium and Electron need accessibility turned on and many apps expose little. Kept as a later addition for exact targets; used from v0.2 for the risk check and the password check.
- A `describe`-only risk check, with AT-SPI left for later: simpler, but the model would decide which clicks are risky in all but name.
- Chrome DevTools Protocol: exact for the browser, but the browser must run with a debugging port that any local program can drive.
- A separate vision Provider: not needed while the Thinking Provider reads images.
- No Confirmation for screen actions, or a Confirmation for every click: the first trusts the model with risk, the second makes the feature unusable.
- `ydotool`: needs a root daemon with `uinput` access.
