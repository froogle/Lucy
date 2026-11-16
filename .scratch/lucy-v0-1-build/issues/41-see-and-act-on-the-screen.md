# 41: See and act on the screen

**What to build:** Lucy can open YouTube and search for something, but then she stops: "now play the third video" does nothing, because she can't see what's on screen or click it. Lucy's core needs to look at the screen when a request calls for it, work out what the user means ("the third video", "that link", "the play button"), and act on it: click, follow a link, scroll, type into a field. Then she looks again to check it worked. This is a general ability, not something only YouTube or the browser can use.

Still to settle:
- How she sees: screenshots read by a vision-capable Provider; the accessibility tree (AT-SPI); driving the browser directly (for example over the Chrome DevTools Protocol); or a mix. The browser and accessibility routes give exact targets, and screenshots cover everything else.
- How she acts: fake input on Wayland (`ydotool`, `wtype`, or Hyprland's own dispatchers) versus browser or accessibility actions.
- What's core and what's per desktop: capturing the screen and sending input differ between Hyprland, GNOME and KDE. The core may own the look–act–check loop while each desktop Plugin (ticket 35 for Omarchy) provides the primitives.
- Privacy: screenshots can show anything, including passwords and messages. When does she look, does the user know she's looking, and what reaches the model or the logs.
- Which actions need Confirmation, and how a click on the wrong thing gets caught or undone.
- How it fits with ticket 34: when the next step needs typed input, she should ask through the dialog rather than guess.
- Scope: the v0.1 spec lists vision Providers as out of scope (for photo search only), so this probably comes after v0.1.

Details to be settled in a grilling session.

**Status:** resolved (moved to lucy-v0-2 06–07 (from [06](../../lucy-v0-2/issues/06-look-at-the-screen.md)))

## Comments

2026-11-16: raised by the user while widening ticket 35. Example: open YouTube, search, then "play the third video".

2026-11-16: settled in a grilling session and moved to v0.2 as lucy-v0-2 06–07 (from [06](../../lucy-v0-2/issues/06-look-at-the-screen.md)), spec [v0.2](../../lucy-v0-2/spec.md), ADR 0008. The questions above are answered there.
