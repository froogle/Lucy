# Do Anthropic's and OpenAI's terms allow an open-source tool to drive a user's own logged-in Claude Code and Codex CLIs?

Type: research
Status: resolved
Blocked by: none

## Question

ADR 0001 makes Lucy's default thinking Providers drive the user's own subscription-authenticated `claude` and `codex` CLIs. The Claude Agent SDK docs say third-party developers may not "offer claude.ai login or rate limits" without approval. Establish from the current primary sources (Anthropic Consumer Terms, Usage Policy, Claude Code / Agent SDK docs and legal pages; OpenAI Terms of Use and Codex docs) what is and isn't permitted for: (a) a user personally scripting their own CLI, (b) a published open-source project whose users bring their own login, (c) anything Lucy must avoid (bundling credentials, proxying, multi-user use). Quote the exact clauses with links and dates; flag ambiguity rather than resolving it. This is input to a decision, not legal advice.

## Answer

- **OpenAI/Codex: sanctioned, with conditions.** OpenAI documents "ChatGPT plan usage in your open-source app", including Codex app-server. The Sign in with ChatGPT Terms (eff. 2026-11-04) require that requests are local and single-user, the app is free, the plan is not exposed as a general API, and nobody else's speech is served on the user's plan.
- **Anthropic/Claude Code: unresolved.** Anthropic allows "an end user signing in to the unmodified Claude Code binary with their own Claude subscription" and documents scripted `-p` / `setup-token`. It also says third-party products "should use API key authentication" and must not "route requests through … plan credentials on behalf of their users" or "offer claude.ai login or rate limits" without approval. Lucy sits between those sentences, so ask Anthropic.
- **Never, for either vendor:** handle or bundle credentials, proxy or expose an API, serve multiple users from one subscription, modify or spoof the CLI, or use their branding.
- Not legal advice. [findings](../research/subscription-terms.md)
