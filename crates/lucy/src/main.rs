//! `lucy`: a thin client of lucyd's public socket. With no arguments it is
//! a full-screen list of sections to set Lucy up with; `lucy setup` walks
//! them in order. Every other command prints plain lines, but for the JSON
//! the bar panel reads: `status --json`, `set` and `unset`.

use std::io::{IsTerminal, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use lucy::menu::{self, Open};
use lucy::panel::{self, Devices, Snapshot};
use lucy::settings::{self, Change};
use lucy::systemd::{self, Verb};
use lucy::ui::{Console, Ui};
use lucy::Env;
use lucy_ipc::{Client, State, Topic};
use ratatui::DefaultTerminal;

const OS_RELEASE: &str = "/etc/os-release";

#[derive(Parser)]
#[command(version, about = "Talk to Lucy's daemon, lucyd")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Walk every section in order: Providers, speech, voice, devices, the
    /// Wake word, Plugins, Approvals and the service.
    Setup {
        /// Where to read the distro from, for installing Piper.
        #[arg(long, hide = true, default_value = OS_RELEASE)]
        os_release: PathBuf,
    },
    /// Enter the credentials a Plugin declares, such as an API key, in the
    /// Plugins section. They go into your keyring, never to Lucy's model.
    Credentials {
        /// The Plugin's name.
        plugin: String,
    },
    /// Print a snapshot of lucyd's state.
    Status {
        /// As one line of JSON, with the settings the bar panel offers and
        /// their options. lucyd needn't be running.
        #[arg(long)]
        json: bool,
        /// Keep running, printing a snapshot each time one changes.
        #[arg(long, requires = "json")]
        follow: bool,
    },
    /// Set one setting in config.toml, keeping its comments, and print as
    /// JSON when lucyd takes it up. lucyd needn't be running.
    Set {
        /// Its dotted path, such as tiers.fast or audio.mic; or stt, for
        /// superwhisper or one of Whisper's models.
        key: String,
        value: String,
    },
    /// Remove one setting from config.toml, so its default applies, and
    /// print as JSON when lucyd takes that up.
    Unset {
        /// Its dotted path, such as whisper.model.
        key: String,
    },
    /// Start, stop or restart lucyd.service, as the bar panel's switch and
    /// Restart button do.
    Service {
        #[arg(value_enum)]
        verb: Verb,
    },
    /// Print lucyd's raw event stream as NDJSON, for client authors.
    Events,
    /// Open a Session, as if the Wake word had been heard.
    Listen,
    /// End the open Session.
    End,
    /// Have Lucy say `text` outside any Session (a Voice preview), and
    /// wait until she has.
    #[command(hide = true)]
    Say { text: String },
    /// Show the box Lucy puts on screen for you to type in. lucyd opens it
    /// in a terminal while nothing else draws it.
    #[command(hide = true)]
    Input {
        /// lucyd's socket; defaults to `$XDG_RUNTIME_DIR/lucy/lucyd.sock`.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Bridge stdio to lucyd's private MCP socket. A model process that
    /// lucyd spawns runs this as its MCP server.
    #[command(hide = true)]
    McpRelay {
        /// The per-spawn token lucyd issued.
        token: String,
        /// The MCP socket; defaults to `$XDG_RUNTIME_DIR/lucy/mcp.sock`.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => return lucy(Open::Lucy, Path::new(OS_RELEASE)),
        Some(Command::Setup { os_release }) => return lucy(Open::Setup, &os_release),
        Some(Command::Credentials { plugin }) => {
            return lucy(Open::Credentials(plugin), Path::new(OS_RELEASE))
        }
        Some(Command::Status {
            json: false,
            follow: _,
        }) => status(),
        Some(Command::Status { json: true, follow }) => return snapshot(follow),
        Some(Command::Set { key, value }) => return write(settings::parse_set(&key, value)),
        Some(Command::Unset { key }) => return write(settings::parse_unset(&key)),
        Some(Command::Service { verb }) => return service(verb),
        Some(Command::Input { socket }) => return input(socket),
        Some(Command::Events) => events(),
        Some(Command::Listen) => connect().and_then(|mut client| client.start_session().map(drop)),
        Some(Command::End) => connect().and_then(|mut client| client.end_session()),
        Some(Command::Say { text }) => connect().and_then(|mut client| client.voice_preview(&text)),
        Some(Command::McpRelay { token, socket }) => return mcp_relay(&token, socket),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => failed(e),
    }
}

fn failed(e: lucy_ipc::Error) -> ExitCode {
    match e {
        lucy_ipc::Error::NotRunning => {
            eprintln!("lucyd isn't running. Start it with: systemctl --user start lucyd")
        }
        e => eprintln!("lucy: {e}"),
    }
    ExitCode::FAILURE
}

/// The full screen, or why there can't be one: a pipe would get escape
/// codes, not a page.
fn full_screen(page: &str) -> Result<DefaultTerminal, ExitCode> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!(
            "lucy: {page} is a full-screen page and needs a terminal. \
             For a plain snapshot of lucyd, run: lucy status"
        );
        return Err(ExitCode::FAILURE);
    }
    // Also restores the terminal if lucy panics.
    ratatui::try_init().map_err(|e| {
        eprintln!("lucy: can't take over the terminal: {e}");
        ExitCode::FAILURE
    })
}

fn lucy(open: Open, os_release: &Path) -> ExitCode {
    full_screen_page("lucy", open, &Env::of_process(os_release))
}

fn full_screen_page(page: &str, open: Open, env: &Env) -> ExitCode {
    let terminal = match full_screen(page) {
        Ok(terminal) => terminal,
        Err(code) => return code,
    };
    let mut keys = Console;
    let mut ui = Ui::new(terminal, &mut keys, "Lucy");
    let store = lucy_config::credentials::SecretService;
    let result = menu::run(&mut ui, env, &store, open);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lucy: {e}");
            ExitCode::FAILURE
        }
    }
}

fn input(socket: Option<PathBuf>) -> ExitCode {
    let mut env = Env::of_process(Path::new(OS_RELEASE));
    if socket.is_some() {
        env.socket = socket;
    }
    full_screen_page("lucy input", Open::Input, &env)
}

fn connect() -> Result<Client, lucy_ipc::Error> {
    // With no runtime dir there can be no socket, so lucyd isn't running.
    let path = lucy_ipc::default_socket_path().ok_or(lucy_ipc::Error::NotRunning)?;
    Client::connect(&path, "lucy", None)
}

fn status() -> Result<(), lucy_ipc::Error> {
    let status = connect()?.status()?;
    let state = match status.state {
        State::Idle => "Idle",
        State::Listening => "Listening",
        State::Thinking => "Thinking",
        State::Speaking => "Speaking",
        State::Confirming => "Confirming",
        State::Typing => "Typing",
        State::Error => "Error",
    };
    println!("State: {state}");
    if let Some(session_id) = status.session_id {
        println!("Session: {session_id}");
    }
    Ok(())
}

fn snapshot(follow: bool) -> ExitCode {
    let env = Env::of_process(Path::new(OS_RELEASE));
    let mut stdout = std::io::stdout().lock();
    if follow {
        let _reader_gone = panel::follow(&env, &mut stdout);
        return ExitCode::SUCCESS;
    }
    let status = env.connect().and_then(|mut client| client.status()).ok();
    let snapshot = Snapshot::of(&env, status.as_ref(), &Devices::list(&env));
    let _ = writeln!(stdout, "{}", snapshot.to_json());
    ExitCode::SUCCESS
}

fn service(verb: Verb) -> ExitCode {
    match systemd::run(&Env::of_process(Path::new(OS_RELEASE)), verb) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lucy: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write(changes: Result<Vec<Change>, String>) -> ExitCode {
    let written = changes.and_then(|changes| {
        settings::write(&lucy_config::default_path(), &changes).map_err(|e| e.to_string())
    });
    match written {
        Ok(applies) => {
            let answer = serde_json::json!({"applies": applies, "message": applies.to_string()});
            println!("{answer}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lucy: {e}");
            ExitCode::FAILURE
        }
    }
}

fn events() -> Result<(), lucy_ipc::Error> {
    let mut client = connect()?;
    client.subscribe(&[Topic::State])?;
    let mut stdout = std::io::stdout().lock();
    loop {
        let line = client.next_event_line()?;
        if writeln!(stdout, "{line}").is_err() {
            // The reader went away (e.g. `lucy events | head`).
            return Ok(());
        }
    }
}

/// Sends `token`, then pipes stdin to the socket and the socket to stdout
/// until lucyd hangs up.
fn mcp_relay(token: &str, socket: Option<PathBuf>) -> ExitCode {
    let Some(socket) = socket.or_else(lucy_ipc::default_mcp_socket_path) else {
        eprintln!("lucy: mcp-relay: XDG_RUNTIME_DIR is not set");
        return ExitCode::FAILURE;
    };
    match relay(token, &socket) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("lucy: mcp-relay: lucyd refused the token");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("lucy: mcp-relay: {}: {e}", socket.display());
            ExitCode::FAILURE
        }
    }
}

/// `Ok(false)` if lucyd refused the token.
fn relay(token: &str, socket: &std::path::Path) -> std::io::Result<bool> {
    let mut stream = UnixStream::connect(socket)?;
    writeln!(stream, "{token}")?;
    // Byte by byte, so nothing after the answer line is consumed.
    let mut answer = Vec::new();
    let mut byte = [0u8];
    while stream.read(&mut byte)? == 1 && byte[0] != b'\n' {
        answer.push(byte[0]);
    }
    if answer != b"ok" {
        return Ok(false);
    }
    let mut to_lucyd = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut to_lucyd);
        let _ = to_lucyd.shutdown(std::net::Shutdown::Write);
    });
    // By hand: io::copy from the socket to a stdout pipe stalled, holding
    // replies back.
    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(true);
        }
        stdout.write_all(&buf[..n])?;
        stdout.flush()?;
    }
}
