# Thinking runs through subscription CLIs, not pay-per-use APIs

Lucy's default Providers drive locally installed, subscription-authenticated CLIs (`claude -p`, `codex exec`) and local speech, rather than calling vendor APIs directly. Users already pay flat subscriptions for these tools, and pay-per-use voice and model APIs would put a running meter on an always-available assistant. Every Provider is pluggable and configured through a TUI, so users with API keys or other tools can swap them in.

## Considered Options

- OpenAI Realtime voice API for the conversation: the most natural voice, but billed per minute and not covered by a ChatGPT subscription. Rejected as the default; it can still be added as an optional Provider.

## Consequences

- Anthropic's terms are ambiguous about third-party tools driving a user's own `claude` login; OpenAI's explicitly allow the Codex app-server route. We accept this because Lucy is self-installed software: each user runs it on their own machine with their own account, and Lucy never handles, bundles or proxies credentials, and is single-user. See `.scratch/lucy-v0-1/research/subscription-terms.md`.

