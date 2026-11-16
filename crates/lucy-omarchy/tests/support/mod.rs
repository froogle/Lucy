//! Runs the Omarchy Plugin's MCP server in-process, talking to it through a
//! real MCP client as lucyd would. Hyprland is a real Unix socket served
//! here, which records each request and answers queries from what the test
//! set; the `omarchy-*` commands are fake executables on the Plugin's
//! `PATH`. Nothing reaches the real desktop.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lucy_omarchy::{Desktop, Omarchy};
use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, RunningService};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

/// What the fake Hyprland answers, and every request it was sent.
#[derive(Default)]
struct Answers {
    clients: Value,
    active: Value,
    dispatch: String,
    requests: Vec<String>,
}

/// A fake Hyprland serving its request socket.
pub struct Hyprland {
    answers: Arc<Mutex<Answers>>,
}

impl Hyprland {
    fn serve(socket: &Path) -> Hyprland {
        let answers = Arc::new(Mutex::new(Answers {
            clients: json!([]),
            active: json!({}),
            dispatch: "ok".into(),
            requests: Vec::new(),
        }));
        let listener = UnixListener::bind(socket).unwrap();
        let serving = answers.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = vec![0; 1 << 16];
                let read = stream.read(&mut request).unwrap();
                let request = String::from_utf8(request[..read].to_vec()).unwrap();
                let answer = {
                    let mut answers = serving.lock().unwrap();
                    answers.requests.push(request.clone());
                    match request.as_str() {
                        "j/clients" => answers.clients.to_string(),
                        "j/activewindow" => answers.active.to_string(),
                        dispatch if dispatch.starts_with("dispatch ") => answers.dispatch.clone(),
                        _ => "unknown request".into(),
                    }
                };
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        Hyprland { answers }
    }

    /// Sets the open windows, as `(address, class, title, workspace)`, and
    /// which one is focused.
    pub fn windows(&self, windows: &[(&str, &str, &str, &str)], focused: Option<&str>) {
        let clients: Vec<Value> = windows
            .iter()
            .map(|(address, class, title, workspace)| {
                json!({
                    "address": address,
                    "mapped": true,
                    "hidden": false,
                    "workspace": {"id": workspace.parse::<i64>().unwrap_or(-98), "name": workspace},
                    "floating": false,
                    "class": class,
                    "title": title,
                    "pinned": false,
                    "fullscreen": 0,
                    "pid": 4242,
                })
            })
            .collect();
        let mut answers = self.answers.lock().unwrap();
        answers.clients = Value::Array(clients);
        answers.active = match focused {
            Some(address) => windows
                .iter()
                .position(|w| w.0 == address)
                .map(|i| answers.clients[i].clone())
                .unwrap(),
            None => json!({}),
        };
    }

    /// Changes one field of the window at `address`.
    pub fn set(&self, address: &str, field: &str, value: Value) {
        let mut answers = self.answers.lock().unwrap();
        for window in answers.clients.as_array_mut().unwrap() {
            if window["address"] == address {
                window[field] = value.clone();
            }
        }
    }

    /// What Hyprland answers each dispatch with.
    pub fn answer_dispatches_with(&self, answer: &str) {
        self.answers.lock().unwrap().dispatch = answer.into();
    }

    /// Every dispatcher sent, in order.
    pub fn dispatched(&self) -> Vec<String> {
        let answers = self.answers.lock().unwrap();
        answers
            .requests
            .iter()
            .filter_map(|r| r.strip_prefix("dispatch ").map(str::to_owned))
            .collect()
    }
}

/// The Plugin, running and connected.
pub struct Plugin {
    runtime: tokio::runtime::Runtime,
    client: RunningService<RoleClient, ()>,
    pub hyprland: Hyprland,
    dir: tempfile::TempDir,
}

/// Writes an executable from a child process, so this one holds no write
/// fd another test's child could inherit ("Text file busy").
fn write_executable(path: &Path, script: &str) {
    let mut writer = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod +x \"$1\"", "sh"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    assert!(writer.wait().unwrap().success());
}

impl Plugin {
    /// The Plugin, with a fake Hyprland showing no windows and Omarchy's
    /// focused workspace tiling.
    pub fn start() -> Plugin {
        let dir = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let hyprland = Hyprland::serve(&dir.path().join(".socket.sock"));
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        write_executable(
            &bin.join("omarchy-hyprland-workspace-layout-current"),
            &format!(
                "#!/bin/sh\nread layout < '{}/layout'\necho \"$layout\"\n",
                dir.path().display()
            ),
        );
        write_executable(
            &bin.join("omarchy-hyprland-workspace-layout-toggle"),
            &format!(
                "#!/bin/sh\n\
                 cd '{}'\n\
                 echo \"toggle $*\" >> toggles\n\
                 read layout < layout\n\
                 case \"$1:$layout\" in\n\
                 back:dwindle) next=floating ;;\n\
                 back:floating) next=scrolling ;;\n\
                 back:*) next=dwindle ;;\n\
                 *:dwindle) next=scrolling ;;\n\
                 *:scrolling) next=floating ;;\n\
                 *) next=dwindle ;;\n\
                 esac\n\
                 echo $next > layout\n",
                dir.path().display()
            ),
        );
        std::fs::write(dir.path().join("layout"), "dwindle\n").unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let omarchy = Omarchy::new(Desktop {
            socket: dir.path().join(".socket.sock"),
            path: bin.into_os_string(),
        });
        let client = runtime.block_on(async {
            let (ours, theirs) = tokio::io::duplex(1 << 16);
            tokio::spawn(async move {
                let server = omarchy.serve(theirs).await.expect("the Plugin starts");
                let _ = server.waiting().await;
            });
            let lifecycle = ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: None,
            };
            serve_client_with_lifecycle((), ours, lifecycle)
                .await
                .expect("the client connects")
        });
        Plugin {
            runtime,
            client,
            hyprland,
            dir,
        }
    }

    /// Sets the focused workspace's layout as Omarchy reports it.
    pub fn set_layout(&self, layout: &str) {
        std::fs::write(self.dir.path().join("layout"), format!("{layout}\n")).unwrap();
    }

    /// Each run of Omarchy's layout toggle, as `toggle <args>`.
    pub fn toggles(&self) -> Vec<String> {
        let log: PathBuf = self.dir.path().join("toggles");
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn layout(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("layout"))
            .unwrap()
            .trim()
            .to_owned()
    }

    /// The names of the tools it lists.
    pub fn tool_names(&self) -> Vec<String> {
        let tools = self.runtime.block_on(self.client.list_all_tools()).unwrap();
        tools
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    /// Calls `tool` with `arguments`, a JSON object.
    pub fn call_result(&self, tool: &str, arguments: Value) -> CallToolResult {
        let Value::Object(arguments) = arguments else {
            panic!("tool arguments are an object");
        };
        let request = CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments);
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), self.client.call_tool(request))
                .await
                .expect("the call answers in time")
                .unwrap()
        })
    }

    /// Calls `tool`, which must succeed, and returns its JSON answer.
    pub fn call(&self, tool: &str, arguments: Value) -> Value {
        let result = self.call_result(tool, arguments);
        let text = text(&result);
        assert_eq!(result.is_error, Some(false), "{text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    /// Calls `tool`, which must fail, and returns what it said.
    pub fn refused(&self, tool: &str, arguments: Value) -> String {
        let result = self.call_result(tool, arguments);
        assert_eq!(result.is_error, Some(true), "{}", text(&result));
        text(&result)
    }
}

/// The text a tool answered with.
pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}
