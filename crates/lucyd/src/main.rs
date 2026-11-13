use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let config_path = lucy_config::default_path();
    let config = match lucy_config::load(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("lucyd: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(socket_path) = lucy_ipc::default_socket_path() else {
        eprintln!("lucyd: XDG_RUNTIME_DIR is not set");
        return ExitCode::FAILURE;
    };

    // `lucy` is installed beside `lucyd`.
    let lucy_program = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("lucy"),
        Err(e) => {
            eprintln!("lucyd: can't find own executable: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Ticket 12 picks the Provider from config.
    let mut adapters = lucyd::adapters::Adapters::inert();
    adapters.thinking = Box::new(lucyd::claude::Claude::new(
        "claude".into(),
        state_dir().join("work"),
    ));
    let daemon = match lucyd::spawn(lucyd::Options {
        socket_path,
        config,
        lucy_program,
        adapters,
    }) {
        Ok(daemon) => daemon,
        Err(e) => {
            eprintln!("lucyd: can't start: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("lucyd: listening on {}", daemon.socket_path().display());
    if let Err(e) = daemon.wait_for_shutdown_signal() {
        eprintln!("lucyd: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `$XDG_STATE_HOME/lucy`, falling back to `~/.local/state`.
fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("state")
        });
    base.join("lucy")
}
