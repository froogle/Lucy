# 07: Act on the screen

**What to build:** After a search on YouTube, "play the third video" works. The Omarchy Plugin gains `click {x, y, describe, button?}`, `type {text}`, `key {combo}` and `scroll {x, y, direction, amount}`, using Wayland's virtual-pointer and virtual-keyboard protocols (never `ydotool`). The model looks, acts and looks again to check. Before forwarding `click`, Lucy's core matches `describe` against `[screen] risky_words` (send, buy, pay, delete, submit, post, confirm). A match needs a Confirmation spoken from `describe`, and the match is never the model's call. `type` into a password field is refused, pointing the model to `ask_typed` or the Plugin's credential path.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Stories 29–31, 37–41.

**Blocked by:** 06

**Seams:** owns `[screen] risky_words`. Extends the `Screen` trait (owned by 06) with pointer and keyboard.

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- Coordinates are in screenshot pixels. Password fields are detected by AT-SPI role where it's available; full AT-SPI targeting is out of scope.

**Acceptance:**

- [ ] The four tools are tested through the Plugin's MCP interface with a fake `Screen`
- [ ] In the harness: a risky `describe` asks for a Confirmation spoken from it; a plain one doesn't; a custom `risky_words` list works
- [ ] `type` into a password field is refused with the pointer to `ask_typed`
- [ ] Once, by hand on Omarchy: open YouTube, search, "play the third video" plays it; a click described as "Send" asks first

## Comments
