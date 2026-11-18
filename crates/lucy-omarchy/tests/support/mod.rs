#![allow(dead_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lucy_omarchy::{
    Acted, Button, Captured, Combo, Desktop, Direction, Element, Image, InWindow, Omarchy, Page,
    Paged, Point, Probed, Region, Screen, Wayland,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, RunningService};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

#[derive(Default)]
struct Answers {
    clients: Value,
    active: Value,
    monitors: Value,
    layers: Value,
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
            monitors: json!([]),
            layers: json!({}),
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
                        "j/monitors" => answers.monitors.to_string(),
                        "j/layers" => answers.layers.to_string(),
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
            .enumerate()
            .map(|(focus, (address, class, title, workspace))| {
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
                    "xwayland": false,
                    "visible": true,
                    "focusHistoryID": focus,
                    "at": [0, 0],
                    "size": [800, 600],
                    "monitor": 0,
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

    pub fn monitors(&self, monitors: &[Monitor]) {
        let monitors: Vec<Value> = monitors
            .iter()
            .enumerate()
            .map(|(id, monitor)| {
                json!({
                    "id": id,
                    "name": monitor.name,
                    "x": monitor.at.0,
                    "y": monitor.at.1,
                    "width": monitor.mode.0,
                    "height": monitor.mode.1,
                    "scale": monitor.scale,
                    "transform": monitor.transform,
                    "focused": monitor.focused,
                })
            })
            .collect();
        self.answers.lock().unwrap().monitors = Value::Array(monitors);
    }

    pub fn layer(&self, monitor: &str, level: u8, at: (i32, i32), size: (i32, i32)) {
        let mut answers = self.answers.lock().unwrap();
        let surfaces = &mut answers.layers[monitor]["levels"][level.to_string()];
        if surfaces.is_null() {
            *surfaces = json!([]);
        }
        let surfaces = surfaces.as_array_mut().unwrap();
        surfaces.push(json!({
            "address": format!("0x{}", surfaces.len() + 1),
            "x": at.0,
            "y": at.1,
            "w": size.0,
            "h": size.1,
            "namespace": "a layer",
            "pid": 999,
        }));
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

pub struct Monitor {
    pub name: &'static str,
    pub at: (i32, i32),
    pub mode: (u32, u32),
    pub scale: f64,
    pub transform: u8,
    pub focused: bool,
}

pub const PNG: &[u8] = b"\x89PNG fake screen";

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Click(Point, Button),
    Scroll(Point, Direction, u32),
    Key(Combo),
    Type(String),
}

#[derive(Clone, Default)]
pub struct FakeScreen {
    captures: Arc<Mutex<Vec<(Region, f64)>>>,
    acts: Arc<Mutex<Vec<Act>>>,
    failure: Arc<Mutex<Option<String>>>,
    under: Arc<Mutex<Option<Element>>>,
    probed: Arc<Mutex<Vec<InWindow>>>,
    empty_page: Arc<AtomicBool>,
}

impl FakeScreen {
    pub fn empty_page(&self) {
        self.empty_page.store(true, Ordering::SeqCst);
    }

    pub fn show(&self, under: Element) {
        *self.under.lock().unwrap() = Some(under);
    }

    pub fn probed(&self) -> Vec<InWindow> {
        self.probed.lock().unwrap().clone()
    }

    pub fn captures(&self) -> Vec<(Region, f64)> {
        self.captures.lock().unwrap().clone()
    }

    pub fn acts(&self) -> Vec<Act> {
        self.acts.lock().unwrap().clone()
    }

    fn act(&self, act: Act) -> Acted<'_> {
        let failure = self.failure.lock().unwrap().clone();
        if failure.is_none() {
            self.acts.lock().unwrap().push(act);
        }
        Box::pin(async move { failure.map_or(Ok(()), Err) })
    }

    pub fn fail(&self, reason: &str) {
        *self.failure.lock().unwrap() = Some(reason.into());
    }
}

impl Screen for FakeScreen {
    fn click(&self, at: Point, button: Button) -> Acted<'_> {
        self.act(Act::Click(at, button))
    }

    fn scroll(&self, at: Point, direction: Direction, notches: u32) -> Acted<'_> {
        self.act(Act::Scroll(at, direction, notches))
    }

    fn key(&self, combo: Combo) -> Acted<'_> {
        self.act(Act::Key(combo))
    }

    fn type_text(&self, text: String) -> Acted<'_> {
        self.act(Act::Type(text))
    }

    fn text_at(&self, at: InWindow) -> Probed<'_> {
        self.probed.lock().unwrap().push(at);
        let under = self.under.lock().unwrap().clone();
        Box::pin(async move { Ok(under) })
    }

    fn page(&self, _window: InWindow) -> Paged<'_> {
        let page = if self.empty_page.load(Ordering::SeqCst) {
            Page::Empty
        } else {
            Page::Filled
        };
        Box::pin(async move { Ok(page) })
    }

    fn capture(&self, region: Region, scale: f64) -> Captured<'_> {
        self.captures.lock().unwrap().push((region, scale));
        let failure = self.failure.lock().unwrap().clone();
        Box::pin(async move {
            match failure {
                Some(reason) => Err(reason),
                None => Ok(Image {
                    bytes: PNG.to_vec(),
                    mime_type: "image/png".into(),
                }),
            }
        })
    }
}

pub struct Plugin {
    runtime: tokio::runtime::Runtime,
    client: RunningService<RoleClient, ()>,
    pub hyprland: Hyprland,
    pub screen: FakeScreen,
    dir: tempfile::TempDir,
}

enum Capturing {
    Fake,
    Grim,
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
        Plugin::capturing(Capturing::Fake)
    }

    pub fn with_grim() -> Plugin {
        Plugin::capturing(Capturing::Grim)
    }

    fn capturing(capturing: Capturing) -> Plugin {
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
        fake("grim", "printf 'grim png'\n");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let desktop = Desktop {
            socket: dir.path().join(".socket.sock"),
            path: bin.into_os_string(),
        };
        let screen = FakeScreen::default();
        let omarchy = match capturing {
            Capturing::Fake => Omarchy::with_screen(desktop, Arc::new(screen.clone())),
            Capturing::Grim => {
                let wayland = Wayland::new(desktop.path.clone());
                Omarchy::with_screen(desktop, Arc::new(wayland))
            }
        };
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
            screen,
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

    pub fn required(&self, tool: &str) -> Vec<String> {
        let tools = self.runtime.block_on(self.client.list_all_tools()).unwrap();
        let tool = tools.into_iter().find(|t| t.name == tool).unwrap();
        tool.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
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
