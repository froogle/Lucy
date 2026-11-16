# 24: v0.1 acceptance runs

**What to build:** Verify v0.1 on real hardware against the success criteria, with the user in the loop.

Spec: Solution; Testing Decisions (manual acceptance). Wayfinder tickets 11 and 20 hold the success checks.

**Blocked by:** 09, 17, 23, and wayfinder tickets 23–27 in `.scratch/lucy-v0-1/issues/`

**Status:** resolved

- [ ] News briefing, fresh install, default Sources: greeting plus gist with first audio ≤ 2.5 s; front pages open; "show me that story" works; a Source swap persists into the next Session; the roundup works and Barge-in cuts it short; a forbidden site gets the honest refusal
- [ ] Rainier honest limit: with only Google Photos, Lucy says she can't search it and opens Google Photos search, with no Plugin written
- [ ] Rainier Authoring: from a small local Takeout, Lucy writes, gets Approval for and uses a photo-folder Plugin that finds the photo
- [ ] Speech end → first audio ≤ 2.5 s measured on real hardware
- [ ] Barge-in without false interrupts on laptop speakers, desk speakers and headphones (or the `barge_in` default adjusted)
- [ ] Indicator on Hyprland; notification fallback on GNOME
- [ ] Real wake models from wayfinder 27 in place
- [ ] Anthropic and OpenAI terms, and the Source catalogue flags (including Sky News), re-checked

## Comments

2026-11-16: accepted by the user; resolved. The Rainier Authoring check can't pass on a machine where `claude` is a mise shim: every Authoring build dies at once there. That bug is v0.2 ticket 09.
