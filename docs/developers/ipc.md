# The public socket

Everything outside lucyd talks to it through one Unix socket: `lucy`, the Indicator and the Omarchy bar panel are all clients ([ADR 0006](../adr/0006-single-daemon-public-socket-contract.md)). You can write your own: a widget for another bar, a status light, a keyboard shortcut.

## The basics

- **Where:** `$XDG_RUNTIME_DIR/lucy/lucyd.sock`.
- **What:** [JSON-RPC 2.0](https://www.jsonrpc.org/specification), one JSON object per line, in both directions.
- **First:** every connection starts with `hello`. Anything before it is refused.
- **The contract:** [`crates/lucy-ipc/schema/lucy-ipc.schema.json`](../../crates/lucy-ipc/schema/lucy-ipc.schema.json) is a JSON Schema of every method (`x-methods`), event (`x-events`) and type, generated from the Rust types in `crates/lucy-ipc`. Within a major protocol version, fields are only ever added, so ignore fields you don't know.
- **Never carried:** transcripts, what Lucy says, tool arguments, secrets and screenshots. The socket says *what state* Lucy is in, never *what* was said.

## A session with the socket

Here is a real exchange: say hello, ask for the state, then subscribe to changes.

```
→ {"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":1,"client":"my-widget"}}
← {"id":1,"jsonrpc":"2.0","result":{"protocol":1,"server":"lucyd 0.1.0"}}

→ {"jsonrpc":"2.0","id":2,"method":"status"}
← {"id":2,"jsonrpc":"2.0","result":{"building":false,"looking":false,"session_id":null,"state":"idle","working":false}}

→ {"jsonrpc":"2.0","id":3,"method":"subscribe","params":{"topics":["state"]}}
← {"id":3,"jsonrpc":"2.0","result":{}}
← {"jsonrpc":"2.0","method":"state","params":{"building":false,"looking":false,"session_id":null,"state":"idle","working":false}}
```

From the shell:

```sh
socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/lucy/lucyd.sock"
```

Or let `lucy` do the connecting:

```sh
lucy events                    # every event, as NDJSON
lucy status --json --follow    # a snapshot line each time something changes
```

`lucy status --json` is what the bar panel reads. It includes the settings the panel offers and their options, and works while lucyd is stopped (with the state reported from systemd). If you're writing a bar widget, reading it is often simpler than speaking the socket yourself.

## Methods

| Method | What it does |
|---|---|
| `hello` | `{protocol, client, role?}`. Starts the connection. Fails with `-32001` if lucyd doesn't speak your major version. |
| `status` | A snapshot: `state`, `session_id`, and the `building`, `looking` and `working` marks. |
| `subscribe` | `{topics: [...]}`: `state`, `level` and `plugins`. The current state is sent straight after subscribing to `state`. |
| `session.start` | Opens a Session, as the Wake word would. `-32003` if one is open. |
| `session.end` | Ends the open Session. `-32004` if none is. |
| `plugins.list` | Every Plugin, with its status and author. |
| `plugins.set_enabled` | Turns a Plugin on or off. |
| `approvals.list` | The approved Plugins. |
| `approvals.revoke` | Withdraws one Approval. (There is no method to grant one: Approvals are only spoken.) |
| `providers.status` | Each tier's Provider and whether it's usable. |
| `provider.check` | Checks one Provider in `config.toml`, by name. |
| `input.answer`, `input.cancel` | Answer or cancel the open Input dialog (only for the client holding the `input` role). |
| `test.mic_level`, `test.wake_word`, `test.voice_preview` | Setup's tests: the microphone level, a Wake word test, and Lucy speaking a sentence. Refused while a Session is open. |

## Events

| Event | When |
|---|---|
| `state` | The state or a mark changes. Same shape as `status`. |
| `level` | About 30 a second while Lucy speaks: her voice's level, for animating. |
| `plugins.changed` | A Plugin was added, approved, changed, enabled or disabled. Call `plugins.list` again. |
| `input.open`, `input.close` | The Input dialog opens or closes (only to the `input` role holder). |

### States

| `state` | Meaning |
|---|---|
| `idle` | No Session is open. |
| `listening` | A Session is open, waiting for the user to speak. |
| `thinking` | A Turn is in progress and nothing is playing. |
| `speaking` | Lucy's speech is queued or playing. |
| `confirming` | A tool call waits for the user's spoken yes. |
| `typing` | The Input dialog is open. |
| `error` | Something went wrong. Transient: the next state follows straight after. |

The marks: `looking` (a screen tool is capturing the screen), `building` (a Plugin build is running) and `working` (a Hand-off is running). Each can be set with or without a Session.

## Roles

A client can claim a role in `hello`:

- **`indicator`**: the client draws Lucy's state on screen. While no client holds it, lucyd shows notifications instead.
- **`input`**: the client draws the Input dialog. The first client to claim it holds it; while none does, lucyd opens `lucy input` in a terminal. The holder gets `input.open` with the fields to show, sends `input.answer` or `input.cancel`, and gets `input.close`. `lucy input --json` holds the role for you and relays these as JSON lines on its stdin and stdout; that's how the bar panel does it.

## Using Rust

`crates/lucy-ipc` has a blocking `Client` with typed methods; `lucy` and the Indicator use it. Clients must not link lucyd's audio or ML crates (`scripts/check-client-deps.sh` checks).

## Changing the protocol

New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Regenerate the committed schema with:

```sh
LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc
```

A test fails if the committed schema drifts from the types. Never remove or rename a field within a protocol version.
