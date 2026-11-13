# News site access for the news briefing Plugin

Researched 2026-11-13. Method: one curl each of the front page and `/robots.txt` (browser UA), terms pages fetched and grepped for scraping/automated/AI/non-commercial clauses, robots parsed with Python `urllib.robotparser` for `Lucy/0.1`. Not legal advice; quotes are verbatim excerpts.

## Cross-cutting findings

- **Every site's robots.txt allows `/` for `*` and for `Lucy/0.1` except Reuters** (`User-agent: *` / `Disallow: /`).
- **Almost every site explicitly blocks AI agents** in robots.txt (GPTBot, ClaudeBot, Claude-User, ChatGPT-User, Perplexity-User, Google-Extended, etc.). Several block *user-triggered* AI fetchers (Claude-User, ChatGPT-User, Perplexity-User), which is the closest analogue to what Lucy does. Sending a novel UA to dodge those groups would be against the spirit of the project rule.
- **Several terms forbid using content with AI regardless of commercial status** (Guardian, NYT incl. "grounding ... or as part of retrieval-augmented generation", Fox for training, NPR for training, BBC for "develop or train AI"). Lucy summarising headlines with an LLM is closest to NYT's "grounding/RAG" prohibition and Guardian's "(c) with any ... AI technologies to generate any data or content".
- **All except Reuters/AP have a working official RSS feed** (HTTP 200 today).

## Per site

### BBC News
- Front page https://www.bbc.com/news: 200, readable HTML (234 links). robots.txt 200: `/` allowed for `*`; full blocks for GPTBot, ClaudeBot, Claude-SearchBot, ChatGPT-User, Perplexity-User, Google-Extended, Scrapy, etc. `Lucy/0.1` allowed.
- Terms (https://www.bbc.co.uk/usingthebbc/terms/can-i-share-things-from-the-bbc) list as not shareable/usable: "Anything plucked from our services to develop or train artificial intelligence or to do computer analysis". Main terms page (https://www.bbc.co.uk/usingthebbc/terms/) is JS-rendered; full scraping clause not extracted.
- Feed: https://feeds.bbci.co.uk/news/rss.xml (200). BBC feeds help page https://www.bbc.co.uk/news/10628494 describes using them in news readers / on your own site.

### CNN
- Front https://edition.cnn.com: 200, readable HTML (6 MB). robots 200: `/` allowed for `*`; blocks ~100 agents incl. ClaudeBot, Claude-User, ChatGPT-User, Perplexity-User, MistralAI-user, Scrapy. `Lucy/0.1` allowed.
- Terms https://www.cnn.com/terms: "You are only authorized to access and display Content for your non-commercial, personal, use on compatible devices ..."; copying "limited to viewing or downloading a single copy for non-commercial, personal, entertainment use only". No explicit scraping ban found in the grep beyond anti-inflation bots.
- Feed: http://rss.cnn.com/rss/edition.rss (200; legacy, not updated reliably in recent years).

### Fox News
- Front https://www.foxnews.com: 200, readable HTML. robots 200: `/` allowed; no full-site AI blocks found. `Lucy/0.1` allowed.
- Terms https://www.foxnews.com/terms-of-use: "Fox furnishes the Fox Services for your personal, non-commercial use and enjoyment." and "you may not ... copy, data mine, scrape or in any way extract or use the Content ... for the purpose of training any artificial intelligence algorithm ..." and "UNAUTHORIZED USE, COPYING, ... SCRAPING, ... OF ANY CONTENT IS STRICTLY PROHIBITED."
- Feed: https://moxie.foxnews.com/google-publisher/latest.xml (200); Fox links "RSS" in its footer.

### The Guardian
- Front https://www.theguardian.com/international: 200, readable HTML. robots 200: `/` allowed for `*`; blocks ClaudeBot, Claude-User, PerplexityBot, Amazonbot, CCBot, etc. `Lucy/0.1` allowed by robots.
- Terms https://www.theguardian.com/help/terms-of-service: "you shall also not use, or facilitate, authorise or permit the use of, any (i) 'robot', 'bot', 'spider', 'scraper', 'crawler' or (ii) other automated device, program ... (whether for data gathering, mining, collection, reading, scraping, extraction or other purposes)" [other than as expressly set out / approved in writing]; content may not be used "(c) with any machine learning and/or artificial intelligence technologies to generate any data or content"; "These terms and conditions shall prevail in all respects over our robots.txt". Personal: "Your use ... is for your own personal and non-commercial use only."
- Feed: https://www.theguardian.com/international/rss (200). Official API: Open Platform https://open-platform.theguardian.com (key required; `content.guardianapis.com` returned 401 without key). Note the AI clause still applies to RSS content under the site terms.

### Reuters
- Front https://www.reuters.com: **401** (bot wall). robots 200: **`User-agent: *` `Disallow: /`**; `Lucy/0.1` disallowed. Terms page https://www.reuters.com/info-pages/terms-of-use/ also 401, not read.
- Feed: no public RSS (Reuters discontinued it in 2020); only sitemaps listed in robots. Commercial API (Reuters Connect) only.

### AP News
- Front https://apnews.com: **403** (bot wall). robots 200: `/` allowed for `*` but `Disallow: /*.rss` and `Disallow: /api/v2/feed/`; blocks Claude-User, ClaudeBot, GPTBot, Perplexity-User, etc. Terms https://apnews.com/termsofservice: 403, not read. (https://www.ap.org/terms-and-conditions/: "For personal, non-commercial use ONLY, You may make a single printed copy of any item of Content".)
- Feed: no official public RSS; feed paths disallowed in robots. Commercial AP Media API only.

### NPR
- Front https://www.npr.org: 200, readable HTML. robots 200: `/` allowed; blocks ClaudeBot, Claude-User, ChatGPT-User, PerplexityUser, Google-Extended, etc. `Lucy/0.1` allowed.
- Terms https://www.npr.org/about-npr/179876898/terms-of-use: "You may not use ... any content ... to build or train large language models, machine learning tools, or artificial intelligence (AI) systems without the prior written consent of NPR."; reuse conditions include "(e) you do not scrape any content to replicate or republish the content".
- Feed: https://feeds.npr.org/1001/rss.xml (200).

### Sky News
- Front https://news.sky.com: **403** to a plain GET (bot wall). robots 200: `/` allowed for `*`; blocks ClaudeBot, GPTBot, Google-Extended, PerplexityBot, Scrapy, news-please; comment "Disallow news aggregators except on RSS." `Lucy/0.1` allowed by robots.
- Terms: could not locate a current terms URL (news.sky.com/terms and /info/terms 404). Unverified.
- Feed: https://feeds.skynews.com/feeds/rss/home.xml (200).

### Al Jazeera
- Front https://www.aljazeera.com: 200, readable HTML. robots 200: `/` allowed; blocks GPTBot, ClaudeBot, Claude-Web, ChatGPT-User, PerplexityBot, anthropic-ai. `Lucy/0.1` allowed.
- Terms https://www.aljazeera.com/terms-and-conditions: "In accessing the Service you agree that you do so only for your own personal, non-commercial use." and you may not use "(i) any 'spider', 'scraper', 'bot' or other automated technology, mechanism or process in order to access, copy, view or record any portion of the Service ...".
- Feed: https://www.aljazeera.com/xml/rss/all.xml (200).

### New York Times
- Front https://www.nytimes.com: 200, readable HTML. robots 200: `/` allowed for `*`; blocks Claude-User, ClaudeBot, ChatGPT-User, Perplexity-User, Google-Extended and many more. `Lucy/0.1` allowed by robots.
- Terms https://help.nytimes.com/hc/en-us/articles/115014893428-Terms-of-Service: "The contents of the Services ... are intended for your personal, non-commercial use."; prohibits accessing the Services "using automated means" and using Content for "development or operation of a machine learning or artificial intelligence (AI) system (including any use of the Content for training, fine tuning, or grounding the machine learning or AI system or as part of retrieval-augmented generation)". Terms define Services to include RSS feeds.
- Feed: https://rss.nytimes.com/services/xml/rss/nyt/HomePage.xml (200). https://www.nytimes.com/rss: "NYTimes.com RSS feeds for personal use in a news reader or as part of a non-commercial blog." Official API: https://developer.nytimes.com (Top Stories API, key required; its own terms).

## Summary table

| site | terms allow personal automated read? | robots allows `/` for generic UA? | official feed? | verdict |
|---|---|---|---|---|
| BBC News | Unclear; AI use ("develop or train AI or computer analysis") excluded | Yes | Yes, RSS | Feed only (titles+links); avoid LLM summary of body |
| CNN | Personal non-commercial only; no explicit scraping ban found | Yes | Legacy RSS | Feed only (front page OK per robots, feed is cleaner) |
| Fox News | No: "SCRAPING ... STRICTLY PROHIBITED" | Yes | Yes, RSS | Feed only |
| The Guardian | No: bans bots/scrapers and AI use; terms override robots | Yes | RSS + Open Platform API | Feed/API only; AI summary is against terms, needs written approval |
| Reuters | Not readable (401) | **No** (`Disallow: /`) | No public | Avoid |
| AP News | Not readable (403) | Yes, but feeds disallowed | No public | Avoid |
| NPR | Personal OK; no scraping to republish; no AI training | Yes | Yes, RSS | Feed only |
| Sky News | Unverified (terms not found) | Yes; aggregators "RSS only" | Yes, RSS | Feed only (front page 403) |
| Al Jazeera | No: bans bots/scrapers | Yes | Yes, RSS | Feed only |
| NYT | No: bans automated access and AI grounding/RAG | Yes | RSS (personal news reader) + API | Feed for headlines in a reader is OK; LLM summary conflicts with terms; avoid |

No site's terms clearly permit automated front-page scraping, so "OK front page" applies to none. Safest design: read official RSS feeds (titles + links, which is the explicit "personal news reader" use), speak headlines verbatim, then open the front page in the user's browser. LLM summarisation of article content conflicts with Guardian, NYT and arguably BBC terms.
