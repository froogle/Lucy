# Writing a Plugin

This guide builds a small Plugin from start to finish: **notes**, which saves things the user dictates and reads them back. Along the way it covers the Manifest, tools, state, Confirmations, tests, installing and Approval.

Read [How Plugins work](plugins.md) alongside it for the full reference.

## What you're building

A Plugin is a folder holding a `manifest.toml` and an executable that speaks MCP over stdio. When it's done, the notes Plugin will look like this:

```
~/.local/share/lucy/plugins/notes/
├── manifest.toml
└── notes
```

and the user will be able to say "take a note: buy milk", "what are my notes?" and "clear my notes".

Any language with an MCP server library works. This guide uses Rust and [`rmcp`](https://crates.io/crates/rmcp), starting from the same template Lucy uses when she writes Plugins herself, because it comes with a test harness.

## 1. Start from the template

```sh
cp -r path/to/lucy/crates/lucyd/plugin-template ~/src/notes
cd ~/src/notes
mv Cargo.toml.template Cargo.toml
mv manifest.toml.template manifest.toml
sed -i 's/{{name}}/notes/g' Cargo.toml manifest.toml
```

You now have:

| File | What it is |
|---|---|
| `manifest.toml` | The Manifest. |
| `Cargo.toml` | A standalone crate (it has its own `[workspace]`), with the library named `plugin`. |
| `src/main.rs` | Starts the MCP server on stdio, with the state folder from `LUCY_PLUGIN_STATE_DIR`. You won't need to change it. |
| `src/lib.rs` | The MCP server: the tools and what they do. |
| `tests/plugin.rs` | Your tests. |
| `tests/support/mod.rs` | The test harness: runs your server in-process behind a real MCP client, and answers web requests from recorded files. |
| `build.sh`, `test.sh` | What Lucy's builder runs; handy for you too. |

## 2. Write the Manifest

The Manifest is what Lucy reads to choose your Plugin and what she reads out to the user for Approval. Write it first: it's the Plugin's contract.

```toml
name = "notes"
version = "0.1.0"
author = "human"
summary = "Keeps short notes the user dictates, and reads them back"
command = ["./notes"]

[[tools]]
name = "add_note"
handles = "taking a note or remembering something for later"
examples = ["take a note: buy milk", "remind me to call the dentist"]

[[tools]]
name = "read_notes"
handles = "reading back the notes you've taken"
examples = ["what are my notes?", "read me my notes"]

[[tools]]
name = "clear_notes"
handles = "deleting all your notes"
examples = ["clear my notes", "delete all my notes"]
confirm = true
confirm_prompt = "Shall I delete all your notes? They can't be got back."
```

Some things to get right:

- **`name` must match the folder name** it will be installed in.
- **`command[0]` is a path inside the folder.** The template's `./target/release/notes` works if you install the whole crate; this guide installs just the binary, so it's `./notes`.
- **`handles` is written as the user would ask**, because it's what the model chooses tools by, and what Lucy reads out at Approval: "The notes Plugin needs your Approval. It handles taking a note or remembering something for later, reading back the notes you've taken and deleting all your notes."
- **`clear_notes` has `confirm = true`**, because it can't be undone. Lucy asks the user before every call, and the model can't get round it.
- **There's no `[access]` section**, because the Plugin only writes to its own private state folder, which every Plugin gets. Lucy will tell the user "It needs no access to your files, the network or other programs."

## 3. Write the tools

Open `src/lib.rs`. Keep the `Web` trait, `NoWeb` and the `Plugin` struct with its `new` (the template's `main.rs` and test harness use them); this Plugin just never calls the web. Add the notes logic to `impl Plugin`:

```rust
impl Plugin {
    pub fn new(web: Arc<dyn Web>, state_dir: PathBuf) -> Plugin {
        Plugin { web, state_dir }
    }

    fn notes_file(&self) -> PathBuf {
        self.state_dir.join("notes.txt")
    }

    fn add(&self, text: &str) -> Result<String, String> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.notes_file())
            .map_err(|e| format!("can't open the notes: {e}"))?;
        writeln!(file, "{text}").map_err(|e| format!("can't save the note: {e}"))?;
        Ok("saved".to_owned())
    }

    fn read(&self) -> String {
        match std::fs::read_to_string(self.notes_file()) {
            Ok(notes) if !notes.trim().is_empty() => notes,
            _ => "no notes yet".to_owned(),
        }
    }

    fn clear(&self) -> Result<String, String> {
        match std::fs::remove_file(self.notes_file()) {
            Ok(()) => Ok("cleared".to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("cleared".to_owned()),
            Err(e) => Err(format!("can't clear the notes: {e}")),
        }
    }
}
```

The notes live in `state_dir`, which is `LUCY_PLUGIN_STATE_DIR`: a private folder only this Plugin can write, kept between Sessions. It's the only place a Plugin can write without declaring `[access] write`.

Then replace the template's `example_tool` and the `ServerHandler` impl. `list_tools` must list exactly the tools the Manifest names, or Lucy treats the Plugin as broken:

```rust
fn tool(name: &str, description: &str, schema: Value) -> Tool {
    let Value::Object(schema) = schema else {
        unreachable!("a schema is an object")
    };
    Tool::new(name.to_owned(), description.to_owned(), Arc::new(schema))
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}})
}

impl ServerHandler for Plugin {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![
            tool(
                "add_note",
                "Saves one note",
                json!({
                    "type": "object",
                    "properties": {"text": {"type": "string", "description": "The note, in the user's words"}},
                    "required": ["text"],
                }),
            ),
            tool("read_notes", "Returns every saved note, one per line", no_arguments()),
            tool("clear_notes", "Deletes every saved note", no_arguments()),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let outcome = match &*request.name {
            "add_note" => {
                let text = request
                    .arguments
                    .as_ref()
                    .and_then(|arguments| arguments.get("text"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| ErrorData::invalid_params("text is required", None))?;
                self.add(text)
            }
            "read_notes" => Ok(self.read()),
            "clear_notes" => self.clear(),
            name => return Err(ErrorData::invalid_params(format!("no tool {name}"), None)),
        };
        let result = match outcome {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        };
        Ok(result.into())
    }
}
```

A few points:

- **The model sees the input schema, so describe each property.** It doesn't see the MCP tool's own description: Lucy builds that from the Manifest's `handles` and `examples`.
- **Lucy saves the input schemas at Approval.** If you change a schema, the binary changes, so the user approves again and the new schema is saved then.
- **Return text the model can turn into a spoken answer.** Short and factual is best; Lucy's model does the talking. Return `CallToolResult::error` for a failure the model should explain to the user, and an MCP error for a bad request.

## 4. Test it

Replace `tests/plugin.rs` with tests for what the user will ask:

```rust
mod support;

use serde_json::json;
use support::Plugin;

#[test]
fn it_lists_the_tools_its_manifest_lists() {
    let plugin = Plugin::start();
    assert_eq!(plugin.tool_names(), ["add_note", "read_notes", "clear_notes"]);
}

#[test]
fn a_saved_note_is_read_back() {
    let plugin = Plugin::start();
    plugin.call("add_note", json!({"text": "buy milk"}));
    assert_eq!(plugin.call("read_notes", json!({})), "buy milk\n");
}

#[test]
fn notes_last_until_the_next_session() {
    let plugin = Plugin::start();
    plugin.call("add_note", json!({"text": "call the dentist"}));
    let next_session = plugin.restart();
    assert_eq!(next_session.call("read_notes", json!({})), "call the dentist\n");
}

#[test]
fn clearing_leaves_no_notes() {
    let plugin = Plugin::start();
    plugin.call("add_note", json!({"text": "buy milk"}));
    plugin.call("clear_notes", json!({}));
    assert_eq!(plugin.call("read_notes", json!({})), "no notes yet");
}
```

The harness gives each test a fresh state folder; `restart` starts a new run with the same folder, as the next Session would. Run them:

```sh
cargo test --release
```

## 5. Install it

Build it, and copy the binary and the Manifest into Lucy's Plugins folder:

```sh
cargo build --release
mkdir -p ~/.local/share/lucy/plugins/notes
install -m755 target/release/notes ~/.local/share/lucy/plugins/notes/notes
install -m644 manifest.toml ~/.local/share/lucy/plugins/notes/manifest.toml
```

(If `CARGO_TARGET_DIR` is set in your environment, the binary is under that folder instead of `target/`.)

## 6. Approve it

Say "Hey Lucy". She mentions that the notes Plugin is awaiting your Approval and offers to go through it. Say yes, listen to what she reads out, and say "yes" again.

Behind the scenes, lucyd starts your Plugin in its sandbox, checks the tools it lists match the Manifest, reads the Manifest out, hears your yes, and records the hash of the Manifest and binary with the tools' input schemas.

Now try it:

- "Take a note: buy milk."
- "What are my notes?"
- "Clear my notes." Lucy asks "Shall I delete all your notes? They can't be got back." before calling it.

## 7. Change it

Every time you install a new binary or Manifest, the hash changes and Lucy asks for Approval again, reading out only what changed. That's on purpose: it's what stops anything (including a Plugin, or Lucy herself) changing a Plugin behind your back.

While developing, run your tests rather than going through Lucy for every change, and install when you have something worth trying.

## Debugging

- **Lucy says the Plugin is broken.** She says why at the start of the Session. The usual reasons are a Manifest that doesn't parse (an unknown key, a multi-line `handles`, more than 3 examples), a folder name that doesn't match `name`, or tools that don't match the Manifest's.
- **Lucy never mentions it.** Check the folder is directly in `~/.local/share/lucy/plugins/`, and that its `requires` (if any) is met in lucyd's environment. lucyd runs as a systemd user service, so its environment is the systemd user environment, not your shell's.
- **The Plugin's own output.** Its stderr goes to lucyd's log: `journalctl --user -u lucyd -f`. Write diagnostics there, never to stdout (that's the MCP channel).
- **A permission error.** The sandbox blocked a path you didn't declare. Add it to `[access] read` or `write`.
- **It works in tests but fails at start-up in Lucy.** Remember that the Plugin is started with no credentials when Lucy only lists its tools. Read credentials in tool calls, not at start-up.

## Going further

### Reaching the network

Keep every web request behind the template's `Web` trait, and give the real implementation an HTTP client (the news Plugin uses `ureq` with `rustls`). In tests, the harness answers requests from recorded files: the body of `https://example.com/feed.xml` is `tests/fixtures/web/example.com/feed.xml`, and a site's home page is `index.html` in its host's folder. An unrecorded URL fails, as an unreachable site would, so tests never touch the network.

List the hosts in the Manifest:

```toml
[access]
network = ["api.example.com"]
```

`crates/lucy-news` is the reference for a network Plugin.

### Reading and writing the user's files

Declare the paths, and Landlock lets the Plugin reach them and nothing else:

```toml
[access]
read = ["~/Pictures"]
write = ["~/Pictures/Sorted"]
```

### Secrets

Declare credentials by environment variable name, and read them from the environment in tool calls:

```toml
[access]
credentials = ["WEATHER_API_KEY"]
```

The user enters them with `lucy credentials <plugin>`, or Lucy asks for them the first time a call needs one. See [Credentials](plugins.md#credentials).

### Asking the user something mid-call

Return an `InputRequiredResult` with a form elicitation, and Lucy asks the user, then calls your tool again with their answers. One plain string field is asked out loud; anything else opens the Input dialog. See [Mid-call questions](plugins.md#mid-call-questions), and the `ask` tools in `crates/lucyd/examples/fixture-plugin.rs` for working code.

### Only on some desktops

`requires` keeps a Plugin out of the way where it can't work:

```toml
requires = { env = ["HYPRLAND_INSTANCE_SIGNATURE"], commands = ["hyprctl"] }
```

### Seeing and using the screen

Mark screen tools with `screen = "look"` (or `click`, `key`, `type`, `scroll`). Lucy asks the user once for screen access, and passes your screenshots (MCP image content) to the model. `crates/lucy-omarchy` is the reference.

### Other languages

Any MCP stdio server works, as long as `command[0]` is an executable inside the Plugin's folder. Bear in mind:

- **The Approval only hashes the Manifest and `command[0]`.** A script that imports other files from its folder can change without a fresh Approval. A single self-contained executable is safest.
- **The sandbox only lets the Plugin read the system folders and its own folder.** An interpreter and its libraries under `/usr` are fine; a virtualenv or `node_modules` must be inside the Plugin's folder.
- **Mid-call questions need the 2026-07-28 MCP protocol** (`input_required`). Plugins on older protocol versions work, but can't ask questions.
