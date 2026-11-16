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

#[derive(Default)]
struct Answers {
    clients: Value,
    active: Value,
    dispatch: String,
    requests: Vec<String>,
}

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

    pub fn set(&self, address: &str, field: &str, value: Value) {
        let mut answers = self.answers.lock().unwrap();
        for window in answers.clients.as_array_mut().unwrap() {
            if window["address"] == address {
                window[field] = value.clone();
            }
        }
    }

    pub fn answer_dispatches_with(&self, answer: &str) {
        self.answers.lock().unwrap().dispatch = answer.into();
    }

    pub fn dispatched(&self) -> Vec<String> {
        let answers = self.answers.lock().unwrap();
        answers
            .requests
            .iter()
            .filter_map(|r| r.strip_prefix("dispatch ").map(str::to_owned))
            .collect()
    }
}

pub struct Plugin {
    runtime: tokio::runtime::Runtime,
    client: RunningService<RoleClient, ()>,
    pub hyprland: Hyprland,
    dir: tempfile::TempDir,
}

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
    pub fn start() -> Plugin {
        let dir = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let hyprland = Hyprland::serve(&dir.path().join(".socket.sock"));
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let fake = |command: &str, body: &str| {
            write_executable(
                &bin.join(command),
                &format!(
                    "#!/bin/sh\ncd '{}'\necho \"{command} $*\" >> ran\n{body}",
                    dir.path().display()
                ),
            );
        };
        fake(
            "omarchy-hyprland-workspace-layout-current",
            "read layout < layout\necho \"$layout\"\n",
        );
        fake(
            "omarchy-hyprland-workspace-layout-toggle",
            "echo \"toggle $*\" >> toggles\n\
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
        );
        fake(
            "omarchy-theme-list",
            "printf 'Catppuccin\\nRose Pine\\nTokyo Night\\n'\n",
        );
        fake(
            "omarchy-theme-current",
            "read theme < theme\necho \"$theme\"\n",
        );
        fake("omarchy-theme-set", "echo \"$1\" > theme\n");
        std::fs::write(dir.path().join("theme"), "Catppuccin\n").unwrap();
        fake("omarchy-toggle-bar", "");
        fake("omarchy-system-lock", "");
        fake("omarchy-toggle-idle", "");
        fake(
            "omarchy-toggle-nightlight",
            "read on < nightlight\n\
             echo \"{\\\"enabled\\\":$on,\\\"temperature\\\":4000}\"\n",
        );
        std::fs::write(dir.path().join("nightlight"), "false\n").unwrap();
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

    pub fn set_layout(&self, layout: &str) {
        std::fs::write(self.dir.path().join("layout"), format!("{layout}\n")).unwrap();
    }

    pub fn toggles(&self) -> Vec<String> {
        let log: PathBuf = self.dir.path().join("toggles");
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn ran(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("ran"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn theme(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("theme"))
            .unwrap()
            .trim()
            .to_owned()
    }

    pub fn set_night_light(&self, on: bool) {
        std::fs::write(self.dir.path().join("nightlight"), format!("{on}\n")).unwrap();
    }

    pub fn layout(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("layout"))
            .unwrap()
            .trim()
            .to_owned()
    }

    pub fn tool_names(&self) -> Vec<String> {
        let tools = self.runtime.block_on(self.client.list_all_tools()).unwrap();
        tools
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

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

    pub fn call(&self, tool: &str, arguments: Value) -> Value {
        let result = self.call_result(tool, arguments);
        let text = text(&result);
        assert_eq!(result.is_error, Some(false), "{text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    pub fn refused(&self, tool: &str, arguments: Value) -> String {
        let result = self.call_result(tool, arguments);
        assert_eq!(result.is_error, Some(true), "{}", text(&result));
        text(&result)
    }
}

pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}
