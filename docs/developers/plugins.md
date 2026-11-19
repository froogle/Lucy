# How Plugins work

A **Plugin** is a self-contained ability Lucy can use. Technically, it is:

- a folder named after the Plugin,
- holding a **Manifest**, `manifest.toml`, that describes what the Plugin handles and what it needs access to,
- and an executable that is an **MCP server over stdio**.

Lucy writes Plugins herself (Authoring), and people write them by hand. Both are the same thing to lucyd, and both need the user's Approval. To write one, see [Writing a Plugin](writing-a-plugin.md); this page explains the machinery.

The decisions behind this design are in ADRs [0002](../adr/0002-plugins-over-mcp-delegates-over-zed-acp.md) (MCP), [0004](../adr/0004-manifest-file-hash-pinned-approvals.md) (Manifest and Approvals), [0005](../adr/0005-proxied-plugin-tools-runtime-owned-confirmation.md) (proxied tools and Confirmation), [0008](../adr/0008-screen-access-through-the-thinking-provider.md) (screen tools) and [0011](../adr/0011-a-plugin-marks-a-secret-field-in-the-elicitations-meta.md) (secret fields).

## Where Plugins live

| Folder | What | Approval |
|---|---|---|
| `/usr/share/lucy/plugins/<name>/` | Shipped with Lucy, installed by root. | Approved automatically. |
| `~/.local/share/lucy/plugins/<name>/` | Installed by the user, or written by Lucy. | Needs the user's spoken Approval. |

A user Plugin with the same name as a shipped one replaces it. Folders whose names start with `.` are ignored (Lucy builds in `.build/`).

Lucy's own state about Plugins is in `~/.local/state/lucy/`, where no Plugin can write:

- `approvals.toml`: each approved Plugin's hash and the tool input schemas it had when approved.
- `plugins.toml`: which Plugins the user turned off, and which awaiting ones Lucy has mentioned.
- `versions/<name>/`: the current and previous approved versions, for rollback.
- `plugins/<name>/`: each Plugin's private, writable state folder.

## The life of a Plugin

### 1. Discovery

At the start of every Session, lucyd scans both folders and reads each Manifest. It never runs an unapproved Plugin to do this: everything it needs to decide and to ask for Approval is in the Manifest.

Each Plugin ends up as one of:

- **Not for this desktop**: its `requires` isn't met (an environment variable or command is missing). Silently left out. This is how the Omarchy Plugin stays out of the way on other desktops.
- **Broken**: the Manifest doesn't parse or validate, its executable can't be read, or it lists different tools from the ones it offers. Left out; the model is told once, so Lucy can mention it.
- **Awaiting Approval**: no Approval matches its current hash. The model is told once per version, and offers to go through it with the user.
- **Approved and working**: offered to the model.

### 2. Approval

An Approval pins a **SHA-256 hash of the Manifest plus the executable** that `command[0]` names. Any change to either, by anyone, needs a fresh Approval. Rolling back to the previously approved version doesn't.

Only those two files are hashed. If your executable is a script that loads other files from its folder, changing those doesn't trigger a fresh Approval. Prefer a single compiled binary.

The Approval is spoken. When the user agrees to go through a Plugin, the model calls the Built-in tool `approve_plugin`, and lucyd (not the model):

1. starts the Plugin in its sandbox, lists its tools over MCP, and checks the list matches the Manifest's `[[tools]]` exactly;
2. reads the Manifest out: what it handles, what it can read, write, reach, run and use, and which actions it asks about first. For a changed Plugin, it reads out only what changed, and shows the full diff in a notification;
3. hears the yes itself, against `[confirmation] yes_phrases`;
4. records the hash, the Manifest text, and each tool's **input schema as listed now**.

From then on, the model sees the input schemas saved at Approval, not whatever the Plugin lists later.

### 3. Offering tools to the model

The thinking model never talks to a Plugin directly. It sees only Lucy's MCP server, which lists each working Plugin's tools as **`<plugin>__<tool>`** (so Plugin names can't contain `__`). The tool's description is built from the Manifest:

```
<handles>. For example: "<example 1>"; "<example 2>"
```

That's all the model has to choose a tool by, so `handles` and `examples` matter. Write `handles` the way the user would put the request, not the way the code does it.

Screen tools are left out when the user has `[screen] enabled = false`.

### 4. A call

When the model calls `news__briefing`, lucyd:

1. **Reads the Plugin's credentials** from the keyring, if this is the first call in the Session.
2. **Asks for screen access** if the tool is a screen tool and the user hasn't said yes since lucyd started.
3. **Asks for a Confirmation** if the tool has `confirm = true`, speaking the `confirm_prompt`. Anything but a clear yes returns `declined` to the model and the Plugin is never called.
4. **Asks for any missing credentials** in an Input dialog.
5. **Starts the Plugin** if it isn't running yet in this Session (see [the sandbox](#the-sandbox)), and waits up to 10 seconds for it to start.
6. **Forwards the call** over MCP, handling any [mid-call questions](#mid-call-questions).
7. **Returns the result** to the model unchanged, text and images alike.

A Plugin keeps running for the rest of the Session once started, so it can hold state in memory between calls. It is stopped when the Session ends. Keep anything that should last longer in `LUCY_PLUGIN_STATE_DIR`.

**Barge-in.** If the user cuts in during a call to a tool without `confirm`, lucyd cancels the call (an MCP cancellation). A confirmed call runs to completion, since the user said yes to it, and its result is passed to the model on the next Turn.

### 5. Managing

The model has a Built-in `manage_plugin` tool, and `lucy` has Plugins and Approvals sections:

- **disable / enable**: kept in `plugins.toml`. Files and Approval are kept.
- **rollback**: back to the previous approved version, only for Plugins Lucy wrote (`author = "lucy"`). A human-written Plugin's files are its author's.
- **remove**: deletes the folder and the Approval, after a Confirmation. Shipped Plugins can't be removed, only disabled.
- **revoke** (in `lucy` only): withdraws the Approval, so the Plugin awaits a fresh one.

## The sandbox

Each Plugin runs under [Landlock](https://docs.kernel.org/userspace-api/landlock.html). If the kernel doesn't enforce Landlock, the Plugin doesn't start.

| | Read and execute | Write |
|---|---|---|
| System | `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc` | |
| Always | its own folder | its private state folder (`LUCY_PLUGIN_STATE_DIR`) |
| Declared | `[access] read` | `[access] write` |

Notes:

- A declared path may start with `~/` or `$NAME`, expanded from the Plugin's environment. A path is skipped if the variable is unset, empty, relative or contains `..`, or if the path doesn't exist when the Plugin starts. A declared path with a `..` component makes the Manifest invalid.
- `/tmp`, `/dev/shm` and `/proc` aren't available unless declared.
- **Network access** (`[access] network`) is declared and read out at Approval, but **not enforced** in v0.1.
- **Commands** (`[access] commands`) are declared and read out. Anything in the system folders can run, so this is a statement of honesty that the user approves, not a wall. Keep it accurate; the shipped Plugins test that it matches what they run.
- **Unix sockets** aren't confined by the Landlock version lucyd uses, so a Plugin can connect to sockets it can find.

### Environment

A Plugin starts with an empty environment plus only:

- `PATH`, `HOME` and `LANG`, from lucyd's environment;
- `LUCY_PLUGIN_STATE_DIR`, its private state folder;
- each variable named in `requires.env`;
- each credential named in `[access] credentials`, during real calls (see below).

Its working directory is its own folder, and its stderr goes to lucyd's log (`journalctl --user -u lucyd`).

## Credentials

A Plugin that needs a secret (an API key, a password) names it in `[access] credentials`, as an environment variable name:

```toml
[access]
credentials = ["LINEAR_API_KEY"]
```

The user enters it with `lucy credentials <plugin>`, or Lucy asks for it in an Input dialog the first time a call needs it. It is stored in the Secret Service keyring. lucyd passes it as an environment variable when it starts the Plugin for a call.

**When lucyd starts a Plugin only to list its tools (at Approval and the start-of-Session check), credentials aren't set.** Read them inside tool calls, never at start-up, or listing will fail.

Credential names must be uppercase environment variable names, and can't be `PATH`, `HOME`, `LANG`, or start with `LUCY_` or `LD_`.

## Mid-call questions

A Plugin can ask the user something in the middle of a call. lucyd speaks the 2026-07-28 MCP protocol to Plugins first, which supports **`input_required`** results: the tool returns an `InputRequiredResult` holding form elicitations instead of a final result, lucyd asks the user, and calls the tool again with `input_responses` and the `request_state` the Plugin gave. A call may ask up to 10 times.

How the question is asked depends on its form:

- **One plain string field**: Lucy speaks the `message` and the user's spoken reply is the answer.
- **Anything else** (several fields, a number, a `uri`, an `enum` choice, a secret): Lucy opens the Input dialog with a box per field.

To ask for a secret, name one of your declared credentials as a field, and list it in the elicitation's `_meta` under `lucy/secret` ([ADR 0011](../adr/0011-a-plugin-marks-a-secret-field-in-the-elicitations-meta.md)):

```json
{"mode": "form", "message": "Your LINEAR_API_KEY?",
 "requestedSchema": {"type": "object", "properties": {"LINEAR_API_KEY": {"type": "string"}}},
 "_meta": {"lucy/secret": ["LINEAR_API_KEY"]}}
```

What the user types goes to the keyring. The Plugin receives the string `"provided"` in its place, and gets the value itself as an environment variable the next time it is started. A dialog that is cancelled or times out comes back as the `cancel` action, with `_meta {"lucy/result": "cancelled" | "timed_out"}`.

Older Plugins that don't speak the 2026-07-28 protocol still work, through the usual `initialize` handshake; they just can't ask questions.

## Confirmations

Mark a tool `confirm = true` when its action is risky or can't be undone: sending, buying, deleting, force-quitting. Before each call, Lucy speaks:

- the tool's `confirm_prompt`, with `{argument}` placeholders filled from the call's arguments: `"Shall I force-quit that window ({target})?"`, or
- without one, `"Shall I use the <plugin> Plugin to <handles>?"`.

Lucy hears the answer herself. The model only gets `declined` when the user doesn't say yes, so it can't talk its way past the check.

Don't mark every tool `confirm`. Users stop listening to questions they're asked all the time, and Lucy reads out at Approval which actions ask first.

## Screen tools

A tool marked `screen = "look" | "click" | "key" | "type" | "scroll"` is a screen tool ([ADR 0008](../adr/0008-screen-access-through-the-thinking-provider.md)). The first call of any screen tool after lucyd starts asks the user once: "I need to look at and use your screen. That lasts until Lucy restarts. Okay?" A yes covers every screen tool, in every Plugin, until lucyd restarts. While a Turn uses one, the Indicator shows a "looking" mark.

A `look` tool returns the screenshot as MCP image content, which lucyd passes to the model untouched. The Omarchy Plugin (`crates/lucy-omarchy`) is the reference: it scales screenshots to the Claude models' size limit so coordinates stay true, maps clicks back through the last look, and refuses to type into a password field.

## Authoring: how Lucy writes Plugins

When nothing fits a request, the model calls `request_plugin`. lucyd starts a **Delegate** (the advanced tier's CLI) in a Landlock sandbox that sees only its own workspace and has the network for fetching dependencies, never the user's data. The Delegate starts from `crates/lucyd/plugin-template/`, writes the Plugin and its tests, and calls `submit_plugin`. lucyd then runs the workspace's `build.sh` and `test.sh`, sandboxed. If they pass, the Plugin is installed into `~/.local/share/lucy/plugins/` with `author = "lucy"`, and Lucy asks for its Approval.

A build has a budget (`[authoring] budget_secs`, 20 minutes by default) and outlives the Session that started it. See `docs/agents/working-in-lucy/authoring-and-hand-off.md` for the details.

## Manifest reference

```toml
name = "photos"                  # must equal the folder name; no "__"
version = "0.1.0"
author = "human"                 # "human" or "lucy"
summary = "Sorts the user's photos into folders by date"   # one line
command = ["./photos", "--quiet"]   # path inside the folder, then arguments

# Optional: only load where these exist. Left out silently otherwise.
requires = { env = ["WAYLAND_DISPLAY"], commands = ["exiftool"] }

[[tools]]
name = "sort"                    # the MCP tool name the server lists
handles = "sorting photos into folders by the date they were taken"   # one line
examples = ["sort my holiday photos", "tidy up my Pictures folder"]     # at most 3
confirm = true                   # ask the user before each call
confirm_prompt = "Shall I move the photos in {folder} into dated folders?"
# screen = "look"                # look | click | key | type | scroll

[access]
read = ["~/Pictures"]            # paths it reads
write = ["~/Pictures"]           # paths it writes
network = []                     # hosts it reaches (declared, not enforced)
credentials = []                 # env var names of secrets, kept in the keyring
commands = ["exiftool"]          # programs it runs
```

### Top level

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | The Plugin's name. Must match its folder's name and contain no `__`. The model sees its tools as `<name>__<tool>`. |
| `version` | yes | Free-form; shown to the user. Approval goes by hash, not version. |
| `author` | yes | `"human"` or `"lucy"`. Lucy rolls back and fixes only her own. |
| `summary` | yes | One line: what the Plugin is for, as Lucy would say it. |
| `command` | yes | The executable, as a path inside the Plugin's folder (`./photos`, `./target/release/photos`), then any arguments. `command[0]` is what the Approval hashes. |
| `requires.env` | no | Environment variables that must be set in lucyd's environment. They're passed to the Plugin, and read out at Approval. Can't start with `LUCY_` or `LD_`. |
| `requires.commands` | no | Bare command names that must be on `PATH`. |

### `[[tools]]`

One entry per tool the MCP server lists, with the same names. The set must match exactly, or the Plugin is broken.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | The MCP tool's name. |
| `handles` | yes | One line: the requests it handles, as the user would put them. Read out at Approval, and the main thing the model chooses by. |
| `examples` | no | Up to 3 things the user might say. |
| `confirm` | no | `true` to ask the user before each call. Default `false`. |
| `confirm_prompt` | no | What Lucy says to confirm. `{name}` is replaced by that argument's value. |
| `screen` | no | Marks a screen tool: `look`, `click`, `key`, `type` or `scroll`. |

The tool's input schema comes from the MCP server's tool listing, not the Manifest. Describe each property well: the model reads those descriptions.

### `[access]`

Everything here is read out to the user at Approval.

| Key | Meaning | Enforced |
|---|---|---|
| `read` | Paths it reads. `~/` and `$VAR` are expanded. | Yes, by Landlock. |
| `write` | Paths it writes. | Yes, by Landlock. |
| `network` | Hosts it reaches. | Not in v0.1. |
| `credentials` | Secrets it needs, as environment variable names. | Only these are passed, and only for calls. |
| `commands` | Programs it runs. | Declared only. |

Unknown keys anywhere in the Manifest are an error.
