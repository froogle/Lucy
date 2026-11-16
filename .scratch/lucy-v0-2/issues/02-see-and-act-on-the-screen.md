# 02: See and act on the screen

**What to build:** Lucy can look at the screen when a request needs it, work out what the user means ("the third video"), click, type, press keys or scroll, then look again to check. The Omarchy Plugin provides the screen tools. The Session model runs the loop. Lucy's core passes images through to the model, asks for a spoken yes before the first look in each Session, shows a "looking" mark, confirms clicks whose description matches a fixed list of risky words, and refuses typing into password fields.

Spec: [v0.2](../spec.md), Screen access. ADR 0008.

**Blocked by:** lucy-v0-1-build 24 (v0.1 acceptance), lucy-v0-1-build 34 (Input dialog), lucy-v0-1-build 35 (Omarchy Plugin)

**Seams:** owns the Manifest mark for screen tools, image passthrough in proxied results, the `[screen]` config section, the "looking" `state` flag and the `Screen` trait inside the Omarchy Plugin. Uses the Omarchy Plugin (owned by lucy-v0-1-build 35) and `ask_typed` (owned by lucy-v0-1-build 34).

**Status:** ready-for-agent

**Acceptance:**

- [ ] The Omarchy Plugin offers `screenshot_for_model`, `click {x, y, describe}`, `type`, `key` and `scroll`, using Wayland virtual-pointer and virtual-keyboard (no `ydotool`)
- [ ] An image from a Plugin tool reaches the model, and never logs, state files, notifications or the public socket
- [ ] The first look in a Session needs a Confirmation; a decline holds for the Session; later looks show the "looking" flag
- [ ] A `click` whose `describe` matches `[screen] risky_words` needs a Confirmation spoken from `describe`; a custom list works
- [ ] `type` into a password field is refused, pointing to `ask_typed`
- [ ] `[screen] enabled = false` leaves the screen tools out
- [ ] Once, by hand on Omarchy: open YouTube, search, "play the third video" plays it

## Comments

Moved from lucy-v0-1-build ticket 41 after the 2026-11-16 grilling session.
