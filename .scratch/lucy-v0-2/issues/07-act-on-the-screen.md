# 07: Act on the screen

**What to build:** After a search on YouTube, "play the third video" works. The Omarchy Plugin gains `click {x, y, describe, button?}`, `type {text}`, `key {combo, describe}` and `scroll {x, y, direction, amount}`, using Wayland's virtual-pointer and virtual-keyboard protocols (never `ydotool`). The model looks, acts and looks again to check. Before forwarding `click` or `key`, Lucy's core matches `describe` against `[screen] risky_words` (send, buy, pay, delete, submit, post, confirm); a `type` whose text ends in a newline counts as a key press and is checked the same way. A match needs a Confirmation spoken from `describe`, and the match is never the model's call.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Stories 29–31, 37–39, 41, 52.

**Blocked by:** 06

**Seams:** owns `[screen] risky_words`. Uses the Manifest `screen` field (owned by 06): `click` and `key` tools must carry `describe`. Extends the `Screen` trait (owned by 06) with pointer and keyboard.

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- Amended 2026-11-17: `key` gained a required `describe` (every key, harmless ones included) and the risk check; a newline-ending `type` counts as a key. The password-field check and the AT-SPI text under a click moved to ticket 10, so this ticket needs no AT-SPI. Until 10 lands, `type` has no password check and the click check rests on `describe` alone.
- Coordinates are in screenshot pixels; the Plugin maps them back to real pixels (06 scales the screenshot).

**Acceptance:**

- [ ] The four tools are tested through the Plugin's MCP interface with a fake `Screen`
- [ ] In the harness: a risky `describe` on `click` or `key`, or a risky newline-ending `type`, asks for a Confirmation spoken from `describe`; a plain one doesn't; a custom `risky_words` list works
- [ ] Once, by hand on Omarchy: open YouTube, search, "play the third video" plays it; a click described as "Send" asks first

## Comments
