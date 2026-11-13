//! `lucy`: a thin client of lucyd's public socket.

use std::io::Write;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use lucy_ipc::{Client, State, Topic};

#[derive(Parser)]
#[command(version, about = "Talk to Lucy's daemon, lucyd")]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print a snapshot of lucyd's state.
    Status,
    /// Print lucyd's raw event stream as NDJSON, for widget authors.
    Events,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Status => status(),
        Command::Events => events(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(lucy_ipc::Error::NotRunning) => {
            eprintln!("lucyd isn't running. Start it with: systemctl --user start lucyd");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("lucy: {e}");
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
