# Lucy sees the screen through screenshots read by the Thinking Provider, and a fixed risk list, not the model, decides which clicks need a Confirmation

To act on what is on screen ("play the third video"), Lucy needs to see it and drive input. The desktop Plugin (the Omarchy Plugin first) provides the screen tools: a screenshot returned as an MCP image, a click at a point, typing, keys and scrolling. Input goes through Wayland's virtual-pointer and virtual-keyboard protocols. The Session model runs the look–act–check loop itself through ordinary tool calls. There is no separate vision Provider, because the Thinking Provider (`claude`) already reads images. Lucy's core adds only two things: image content passing through proxied tool results, and the privacy and Confirmation rules below.

Privacy. The first look in each Session needs a spoken Confirmation ("I need to look at your screen. Okay?"), because the screenshot leaves the machine for the Thinking Provider. Later looks in that Session show a "looking" mark on the Indicator. Images are never logged, written to disk or put on the public socket, and they go when the Session transcript is deleted. A config switch turns screen access off.

Clicks. The runtime can't know what a button does, and screen content is untrusted input that can carry a prompt injection. So `click` takes a required `describe` ("the Play button on the third video"). The runtime asks for a Confirmation when that text, or the text under the point read through AT-SPI where it is available, matches a fixed, configurable list of risky words (send, buy, pay, delete, submit, post, confirm). The match is made locally. The model never decides whether a click is risky. Typing into a password field is refused: secrets go through the Input dialog to the tool that needs them, never through the screen.

## Considered Options

- Accessibility tree (AT-SPI) only: exact targets, but Chromium and Electron need accessibility turned on and many apps expose little. Kept as a later addition for exact targets and for the risk check.
- Chrome DevTools Protocol: exact for the browser, but the browser must run with a debugging port that any local program can drive.
- A separate vision Provider: not needed while the Thinking Provider reads images.
- No Confirmation for screen actions, or a Confirmation for every click: the first trusts the model with risk, the second makes the feature unusable.
- `ydotool`: needs a root daemon with `uinput` access.
