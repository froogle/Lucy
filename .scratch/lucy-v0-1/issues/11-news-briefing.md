# What exactly is the news briefing: sources, length, tone, greeting?

Type: grilling
Status: resolved
Blocked by: none

## Question

Specify the news briefing end to end: which sources (configurable defaults), how many pages are opened visibly, summary length and style, Lucy's persona and greeting ("Morning Pete…"), and what counts as success for v0.1.

## Answer

Grilled with the user on 2026-11-13. Research: `research/news-site-access.md`. Glossary: added **Persona** and **Source**.

- **Trigger**: on request only ("Hey Lucy, what's the news?"), chosen by normal selection. Unprompted morning briefings stay in the Proactive behaviour fog.
- **Input is official feeds only.** No site's terms clearly allow automated reading of its front page. Fox, the Guardian, Al Jazeera and the NYT ban bots or scraping, Reuters, AP and Sky block a plain GET, and several sites block user-triggered AI fetchers by name. So the shipped `news` Plugin (plain code, no model calls) has a `briefing` tool that fetches the official RSS/Atom feeds of the Sources in rotation, all at once, and returns each item's headline, description, link, Source and position. It never fetches article pages, and there is no `article` tool in v0.1.
- **The fast tier is the brain.** It decides the big stories (leading, and appearing across Sources) and speaks the top 1–2 in **one or two sentences** in the user's Persona: "Morning Pete. Well, the midterms didn't go too well for the Republicans, and Russell Brand got sentenced." Feed text is treated as untrusted; risky actions are still guarded by runtime Confirmation (ADR 0005).
- **Pages opened**: after speaking, the model uses `open_url` to open the front page of each Source a spoken story came from. It also opens the front pages of the user's preferred Sources that cannot be summarised (see below). "Show me that Russell Brand story" opens that item's link. "Don't open anything" is respected.
- **Source catalogue**: the Plugin ships a vetted catalogue listing each Source's official feed and an "AI summary allowed" flag taken from its terms.
  - Default rotation: **CNN, NPR, Sky News**. Sky's terms were unverified and are re-checked before release.
  - The Guardian and the NYT forbid AI use of their content, and the BBC's clause is arguable. Reuters and AP have no public feed and are avoided.
  - The catalogue's flags are re-checked before each release, alongside the Anthropic and OpenAI terms.
- **Sources that can't be read**: Lucy summarises only from Sources whose feed exists and whose terms allow AI summaries. For any other Source the user names or prefers, she says so plainly and cheerfully ("I'm not allowed to read that one for you, their rules not mine") and offers to open the page. That offer is an ordinary conversational question, not a Confirmation. The user's liked-but-forbidden Sources get their front page opened beside the summary.
- **Swapping**: "I'm not into Fox, try something else" drops Fox, and the model picks a replacement from the catalogue that fits what the user likes and says what it changed. A site the user names is added if it is in the catalogue and allowed; otherwise she checks it for an official feed, or gives the honest limit above.
- **Detailed roundup** on request: "give me a detailed summary" is about two minutes, built from the feed descriptions across the rotation. "Tell me more about X" opens that article in the browser. Barge-in works throughout.
- **Preferences** are owned by the Plugin, in `~/.local/state/lucy/plugins/news/preferences.toml`: Sources in rotation, liked-but-forbidden Sources to open, Sources to avoid, topics to show more or less of, and the default count and length. A `set_preference` tool sets them during conversation, with no Confirmation, and Lucy always says what changed. General Memory may take them over later.
- **Persona**: the default is friendly, jovial, a bit jokey with a touch of sass, and never flippant about serious news. It lives in an editable `$XDG_CONFIG_HOME/lucy/persona.md` that is included in every Session's prompt. The user edits the file or tells Lucy ("be a bit more sarcastic"); she rewrites it and reads the change back, with no Approval needed because the Persona is personality, not access.
- **First run**: the very first Session opens with Lucy introducing herself ("Hey, I'm Lucy… what should I call you?") and storing the name in `persona.md`. This is not in the TUI. Only the briefing opens with a time-of-day greeting by name.
- **v0.1 success** on a fresh install with the default Sources:
  1. "What's the news?" gives a greeting by name plus a gist of one or two sentences, with first audio within 2.5 s (a "Let me check" counts).
  2. The front pages of the Sources she spoke from open.
  3. "Show me that story" opens the article.
  4. A Source swap is acknowledged and still holds in the next Session.
  5. The detailed roundup works and Barge-in can cut it short.
  6. A site that forbids AI gets the honest refusal and an offer to open the page.
