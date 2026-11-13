# 17: News briefing Plugin

**What to build:** "Hey Lucy, what's the news?" works as the shipped, hand-written reference Plugin. The `news` Plugin (plain code, no model calls) has a `briefing` tool. It fetches the official RSS/Atom feeds of the Sources in rotation concurrently and returns headline, description, link, Source and position. It never fetches article pages. A `set_preference` tool needs no Confirmation. A vetted Source catalogue carries each Source's feed and an AI-summary-allowed flag; the default rotation is CNN, NPR and Sky News. The fast tier speaks a time-of-day greeting by name plus the top one or two stories, then uses `open_url` for the front pages it drew from and for the user's liked-but-forbidden Sources.

Spec: News Plugin; Persona. Wayfinder 11 lists the success checks.

**Blocked by:** 10, 14, 16

**Status:** ready-for-agent

- [ ] Plugin ships with a Manifest and is approved at setup
- [ ] Feed text is treated as untrusted input
- [ ] Forbidden or feedless Sources get an honest "their rules, not mine" plus an ordinary offer to open the page
- [ ] Source swaps ("I'm not into Fox") choose a fitting replacement, say what changed, and persist in the Plugin's state-dir preferences
- [ ] "Show me that story" opens the item's link; "don't open anything" is respected
- [ ] Detailed roundup (~2 min) built from feed descriptions only
- [ ] Tested through its MCP interface against recorded feed fixtures (no live network); doubles as the template's fixtures example
