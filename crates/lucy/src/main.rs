//! `lucy`: a thin client of lucyd's public socket. With no arguments it
//! shows Lucy's Providers and Approvals; `lucy setup` sets her up. Both are
//! full-screen; every other command prints plain lines.

use std::io::{IsTerminal, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use lucy::ui::{Console, Ui};
use lucy::{home, setup, Env};
use lucy_ipc::{Client, State, Topic};
use ratatui::DefaultTerminal;

#[derive(Parser)]
#[command(version, about = "Talk to Lucy's daemon, lucyd")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// First-run setup: Providers, speech, voice, devices, the Wake word
    /// and the service.
    Setup {
        /// Where to read the distro from, for installing Piper.
        #[arg(long, hide = true, default_value = "/etc/os-release")]
        os_release: PathBuf,
    },
    /// Print a snapshot of lucyd's state.
    Status,
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
        None => return home_page(),
        Some(Command::Setup { os_release }) => return setup_page(&os_release),
        Some(Command::Status) => status(),
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

fn home_page() -> ExitCode {
    let mut client = match connect() {
        Ok(client) => client,
        Err(e) => return failed(e),
    };
    let mut terminal = match full_screen("lucy") {
        Ok(terminal) => terminal,
        Err(code) => return code,
    };
    let result = home::run(&mut client, &mut terminal, &mut Console);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => failed(e),
    }
}

fn setup_page(os_release: &Path) -> ExitCode {
    let terminal = match full_screen("lucy setup") {
        Ok(terminal) => terminal,
        Err(code) => return code,
    };
    let env = Env::of_process(os_release);
    let mut keys = Console;
    let mut ui = Ui::new(terminal, &mut keys, "Lucy setup");
    let result = setup::run(&mut ui, &env);
    ratatui::restore();
    // What was said and chosen stays in the terminal's scrollback.
    for line in &ui.screen.lines {
        println!("{line}");
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lucy: setup: {e}");
            ExitCode::FAILURE
        }
    }
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
        State::Error => "Error",
    };
    println!("State: {state}");
    if let Some(session_id) = status.session_id {
        println!("Session: {session_id}");
    }
    Ok(())
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
