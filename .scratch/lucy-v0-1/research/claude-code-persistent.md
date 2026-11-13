# Claude Code (and Codex) as a long-running thinking Provider

Ticket: `issues/03-claude-code-persistent.md`. Researched 2026-11-12 against the official Claude Code docs (code.claude.com), the Codex docs (learn.chatgpt.com, formerly developers.openai.com/codex), `claude --help` (Claude Code 2.1.322), `codex --help` / `codex app-server generate-json-schema` (codex-cli 0.167.2), and a handful of headless timing runs on this machine.

## Recommendation

**Claude Code:** keep one `claude -p` process alive per Session in streaming input mode, launched with a Lucy-owned, isolated configuration:

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --include-partial-messages --no-session-persistence \
  --strict-mcp-config --mcp-config '<lucy mcp json>' \
  --setting-sources "" --disable-slash-commands \
  --tools "" \
  --permission-mode dontAsk \
  --allowedTools "mcp__lucy__*" \
  --append-system-prompt-file <lucy persona> \
  --model <user choice>
```

- Write one `{"type":"user","message":{"role":"user","content":"…"}}` line per utterance; read events until the `result` line.
- Feed `stream_event` deltas whose `event.delta.type == "text_delta"` into a sentence chunker for early TTS.
- Lucy's actions live on Lucy's own MCP server. Lucy enforces spoken confirmation **inside its own tool handlers**: a risky tool returns "needs confirmation" and Claude asks the user. Lucy should not lean on Claude Code's permission system for this. Pre-approve `mcp__lucy__*` and let `dontAsk` deny everything else.
- Pre-spawn the process when the Wake word fires, and in parallel with STT, so the 1–2 s startup is hidden.

Do **not** use `--bare`. It is the "recommended" scripted mode, but it never reads OAuth credentials, so it cannot use the Max subscription (see below).

**Codex:** drive `codex app-server` (JSON-RPC over stdio) with one thread per Session. Use `codex exec --json` only as the simple spawn-per-turn fallback, because it streams completed items, not text deltas. Register Lucy's MCP server through `-c mcp_servers.lucy.*` overrides, so Lucy doesn't edit the user's `config.toml`.

## Claude Code in detail

### Keeping one process alive across turns

- Streaming input mode is documented as the **preferred** way to use the Agent SDK: "It allows the agent to operate as a long lived process that takes in user input, handles interruptions, surfaces permission requests, and handles session management." ([streaming-vs-single-mode](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode))
- On the CLI this is `--input-format stream-json` (needs `-p`) together with `--output-format stream-json --verbose` ([cli-reference](https://code.claude.com/docs/en/cli-reference)). The stdin line shape is the SDK's `SDKUserMessage`: `{"type":"user","message":{"role":"user","content":<string or content blocks>}}` (same page; verified locally).
- Verified locally: one process served 3 sequential turns. It emits a `system/init` at the start of **each** turn and one `result` per turn. The process exits when stdin closes.
- `--replay-user-messages` echoes stdin messages back, which Lucy can use to acknowledge them. `--session-id <uuid>` pins the id. `--no-session-persistence` keeps transcripts off disk (`claude --help`).
- Docs recommend driving the CLI as a subprocess from languages other than Python/TS: "To drive the same agent loop from a language other than Python or TypeScript, run the CLI as a subprocess with the `-p` flag" ([agent-sdk overview](https://code.claude.com/docs/en/agent-sdk/overview)). So Rust → subprocess is the supported path. There is no Rust SDK.
- **Interrupt (barge-in):** the SDK's `interrupt()` only works in streaming input mode. It rides on the CLI's stdin "control protocol" (`interrupt` control request; `cancel_queued: true` is supported when `system/init.capabilities` contains `interrupt_cancel_queued_v1`) ([typescript reference](https://code.claude.com/docs/en/agent-sdk/typescript)). The public docs **do not specify the control-request wire format**. The fallback is SIGINT, which ends the turn cleanly. SIGTERM exits with code 143 and leaves the turn unfinished ([headless](https://code.claude.com/docs/en/headless)).
- Resuming after a crash or restart: `--resume <session-id>` or `--continue` ([headless](https://code.claude.com/docs/en/headless)). This only works if persistence stays on, which is a tradeoff against `--no-session-persistence`.

### Time to first token, cold vs warm (measured)

Each figure is send → first `text_delta`, from an empty, non-git directory. These are single runs, so treat them as indicative only.

| Setup | Turn 1 (includes process spawn) | Turn 2 (warm) | Turn 3 (warm) |
|---|---|---|---|
| Default config (Opus, ~8 user/claude.ai MCP servers, plugins, hooks) | 2.85 s (init event at 1.95 s) | 0.62 s | 1.33 s |
| Isolated config, `--model sonnet` | 3.14 s | 1.37 s | 1.18 s |
| Isolated config, `--model haiku` | 1.79 s | 1.12 s | 1.52 s |
| One-shot `claude -p "…"`, wall clock, default config | 4.43 s total | — | — |
| One-shot, isolated config | 2.10 s total | — | — |

("Isolated" = `--strict-mcp-config --setting-sources "" --tools "" --disable-slash-commands`.)

What the numbers show:
- Process start plus config discovery costs roughly **1–2 s**. The user's own MCP servers, plugins and hooks double it: the default run loaded 135 tools and connected Gmail, Dropbox, Atlassian and others. Isolation halves the one-shot wall time.
- Warm turns land at **~0.6–1.5 s** to first token. That is essentially API latency, and model choice affects it less than run-to-run noise.
- A persistent process saves the spawn and init cost on every turn after the first, and keeps prompt caching hot (`cache_read_input_tokens` grew per turn while `cache_creation` fell to ~0).

### Registering Lucy's tools (custom MCP server)

- `--mcp-config <file-or-json>` loads servers. `--strict-mcp-config` ignores every other MCP source, so the user's own servers and claude.ai connectors stay out ([cli-reference](https://code.claude.com/docs/en/cli-reference)).
- With `-p`, Claude Code waits for pending `--mcp-config` servers before the first turn, up to `MCP_TIMEOUT` (default 30 s). Entries that fail validation are skipped **silently** when stderr is captured and are reported only in `system/init.mcp_server_errors`, so Lucy should check that field ([headless](https://code.claude.com/docs/en/headless)).
- `system/init.mcp_servers[].status` reports connection state. Lucy should check that `lucy` is `connected`.
- Tool names appear to Claude as `mcp__<server>__<tool>`.
- Without `--strict-mcp-config` and without `--bare`, a `-p` run in a folder also connects that folder's `.mcp.json` and runs its hooks with no trust prompt ([headless](https://code.claude.com/docs/en/headless)). Lucy should run Claude from a fixed Lucy-owned working directory (e.g. `~/.local/share/lucy/work`).

### Headless permission handling

- From v2.1.302, `claude -p` starts in **Manual** mode when nothing sets a mode. A call that needs approval is denied unless a `PermissionRequest` hook or `--permission-prompt-tool` approves it ([headless](https://code.claude.com/docs/en/headless)).
- `--allowedTools` uses permission-rule syntax (`Bash(git diff *)`, `mcp__lucy__*`). `--tools ""` removes all built-in tools but does not affect MCP tools ([cli-reference](https://code.claude.com/docs/en/cli-reference)).
- `--permission-mode dontAsk` denies anything that would prompt. Allow rules still run ([headless](https://code.claude.com/docs/en/headless)).
- `--permission-prompts none` makes the run deny instead of waiting on a host, and removes `AskUserQuestion` (`claude --help`, v2.1.259+).
- `--permission-prompt-tool mcp__<server>__<tool>` routes each would-be prompt to an MCP tool. That tool must return a single text block containing `{"behavior":"allow"}` (optionally with `updatedInput`) or `{"behavior":"deny","message":"…"}`, and **no `structuredContent`** ([errors](https://code.claude.com/docs/en/errors#permission-prompt-tool-returned-an-invalid-result)). It cannot approve MCP tools marked `requiresUserInteraction` ([cli-reference](https://code.claude.com/docs/en/cli-reference)).
- The SDK's `canUseTool` callback is the richer equivalent, but it travels over the same undocumented control protocol.
- **For Lucy:** if raw shell becomes opt-in (map Notes), enable it by adding `Bash` to `--tools` and setting `--permission-prompt-tool mcp__lucy__approve`. Lucy's approve tool can then ask aloud before answering. With no shell, `dontAsk` plus `mcp__lucy__*` is enough, and spoken confirmation lives in Lucy's tool handlers.

### Streaming partial text for early TTS

- `--include-partial-messages` (needs `-p` and `--output-format stream-json`) emits `stream_event` lines that wrap raw Messages-API events. Text deltas are `select(.type=="stream_event" and .event.delta.type=="text_delta") | .event.delta.text` ([headless](https://code.claude.com/docs/en/headless)).
- Event types seen locally: `stream_event/{message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop}`, `assistant`, `result/success`, `system/{init,status,hook_started,hook_response,commands_changed}` and `rate_limit_event`.
- Text from subagents is not forwarded by default (`--forward-subagent-text`). Lucy should only speak messages whose `parent_tool_use_id` is null.

### How usage counts against a Max subscription

- Claude Code and claude.ai share one allowance: "all activity in both tools counts against the same limits". Pro and Max have a five-hour session limit and a weekly limit, and Max also has a separate weekly limit for one model family ([support article](https://support.claude.com/en/articles/11145838-using-claude-code-with-your-pro-or-max-plan)). The article doesn't mention `-p` or the SDK specifically. But the local `-p` run reported `apiKeySource: none` and a `rate_limit_event` with `five_hour` and `seven_day` utilisation windows, so headless turns draw on the same subscription windows.
- **`ANTHROPIC_API_KEY` overrides the subscription**: "Claude Code will use this API key for authentication instead of your Claude subscription" (same article). Lucy must scrub `ANTHROPIC_API_KEY` from the child environment unless the user chose API billing.
- `--bare` never reads OAuth or the keychain and needs `ANTHROPIC_API_KEY` or an `apiKeyHelper` ([headless](https://code.claude.com/docs/en/headless)). So **`--bare` is incompatible with subscription auth**, and the docs say it "will become the default for `-p` in a future release". That is a real future risk (see open questions).
- `total_cost_usd` in `result` is a client-side API-price estimate, and it is cumulative across the session. For subscribers it is "not relevant for billing purposes" ([costs](https://code.claude.com/docs/en/costs)).
- `rate_limit_event` (`SDKRateLimitEvent`: `status` is `allowed`, `allowed_warning` or `rejected`, plus `resetsAt` and `utilization`) is the signal Lucy can speak ("you're near your limit"). Running out surfaces as "You've hit your session/weekly limit" ([typescript reference](https://code.claude.com/docs/en/agent-sdk/typescript), [costs](https://code.claude.com/docs/en/costs)).
- A persistent process re-sends a growing context each turn. Prompt caching makes that cheap in latency, but it still counts tokens against the window. Ending the process (or `/compact`) when the Session ends keeps that bounded.

### Policy caution

The Agent SDK overview says: "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK." ([agent-sdk overview](https://code.claude.com/docs/en/agent-sdk/overview))

Lucy never handles a login. It runs the user's own installed and logged-in `claude` binary, which is the documented "run the CLI as a subprocess" path. Whether that counts as "offering claude.ai login or rate limits" is not settled by the docs. That is a product and legal question for the ADR rather than a technical one.

## Codex in detail (briefer)

### `codex exec` (spawn per turn)
- `--json` gives JSONL events: `thread.started`, `turn.started`, `turn.completed` (with `usage`: input, cached input, output and reasoning tokens), `turn.failed`, `item.*` and `error`. The docs don't mention partial text deltas, so the agent message arrives as a completed item ([non-interactive](https://learn.chatgpt.com/docs/non-interactive-mode)). That is poor for early TTS.
- Default sandbox is read-only. `--sandbox workspace-write` allows edits. `--full-auto` is gone; `--approve-for-me` routes approvals to automatic review (same page; `codex exec --help`).
- `codex exec resume <id>` / `--last` continues a thread. `--ephemeral` skips persisting the session. `--skip-git-repo-check` is required outside a git repo. `--ignore-user-config` drops `config.toml` but keeps auth (`codex exec --help`).
- Auth: "`codex exec` reuses saved CLI authentication by default". `CODEX_API_KEY` switches to API billing ([non-interactive](https://learn.chatgpt.com/docs/non-interactive-mode)).

### `codex app-server` (persistent; recommended)
- JSON-RPC 2.0 (no `jsonrpc` field) over newline-delimited stdio. Handshake: `initialize` (with `clientInfo`), then an `initialized` notification ([app-server](https://learn.chatgpt.com/docs/app-server)). `codex --help` still labels the subcommand "[experimental]".
- `thread/start` takes `cwd`, `model`, `sandbox`, `approvalPolicy`, `ephemeral`, `baseInstructions`, `developerInstructions`, and `config` overrides. Then `turn/start` per utterance, `turn/steer` to add input mid-turn, and `turn/interrupt` for barge-in (same page; schema via `codex app-server generate-json-schema`).
- Streaming: `turn/started`, `item/started`, **`item/agentMessage/delta`**, `item/completed` and `turn/completed`. The delta event is what early TTS needs.
- Approvals arrive as server→client requests: `item/commandExecution/requestApproval` (decisions include `accept`, `acceptForSession` and `acceptWithExecpolicyAmendment`), `item/fileChange/requestApproval`, `item/permissions/requestApproval` and `mcpServer/elicitation/request`. Lucy answers them in-process, which is cleaner than Claude's MCP-tool indirection.
- Rate limits: `account/rateLimits/read`, plus the `account/rateLimits/updated` notification (`usedPercent`, `windowDurationMins`, `resetsAt`).
- Tools: Lucy can use an MCP server via config, or experimental `dynamicTools` on `thread/start`. The latter answers `item/tool/call` requests in-process, which means no separate MCP server for Codex. But it needs `capabilities.experimentalApi` and is marked experimental.
- Measured locally: `initialize` took 0.48 s and `thread/start` 0.66 s. **Turn timings could not be measured**, because this machine's Codex ChatGPT token has expired (`401 invalid_jwt`, "Please log out and sign in again"). `codex login status` still reports "Logged in using ChatGPT", so Lucy's Provider health check must make a real call such as `account/rateLimits/read`, not trust `login status`.

### Registering Lucy's MCP server with Codex
- Stdio server keys under `[mcp_servers.<name>]` are `command`, `args`, `env`, `env_vars`, `cwd`, `startup_timeout_sec` (default 10), `tool_timeout_sec` (default 60), `required`, `enabled_tools` / `disabled_tools`, `default_tools_approval_mode` (`auto`, `prompt`, `writes` or `approve`) and `tools.<tool>.approval_mode` ([Codex MCP](https://learn.chatgpt.com/docs/extend/mcp)).
- These are config keys, so Lucy can pass them per invocation with `-c mcp_servers.lucy.command="…"` (`-c` takes any dotted config path, per `codex --help`), or put them in a `-p <profile>` file under `$CODEX_HOME`. Either way the user's `config.toml` is untouched. `codex mcp add` would edit it permanently.
- Set `required = true` so a broken Lucy server fails `thread/start` loudly.

### How usage counts against a ChatGPT plan
- "Local messages and cloud chats share your plan's usage allowance." Plus has five-hour windows (estimates vary widely by model). "Pro plans currently have no five-hour limit". "Weekly limits may also apply". After the limits, purchased credits or an API key continue at API rates ([pricing](https://learn.chatgpt.com/docs/pricing)).
- The docs warn against ChatGPT-managed auth on untrusted CI runners. A local desktop assistant on the user's own machine is the normal case ([non-interactive](https://learn.chatgpt.com/docs/non-interactive-mode)).

## Open questions surfaced

1. **`--bare` becoming the `-p` default.** It would drop subscription auth for headless runs. Lucy should pin or feature-detect the Claude Code version and watch the changelog. Is `claude setup-token` (a long-lived subscription token) a supported way to keep subscription auth under bare mode? The docs consulted don't say.
2. **Policy:** does an open-source app that drives the user's own logged-in `claude` CLI fall under "third party developers … offer claude.ai login or rate limits"? This needs an explicit answer, or a cautious stance recorded in ADR 0001.
3. **Barge-in on Claude:** implement the undocumented stream-json control protocol (`interrupt`) in Rust, or rely on SIGINT plus respawn? Needs a small prototype, ideally folded into ticket 07, the voice-loop prototype.
4. **Codex turn latency** is still unmeasured, pending a fresh `codex login` on this machine.
