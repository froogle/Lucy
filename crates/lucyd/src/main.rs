use std::process::ExitCode;

fn main() -> ExitCode {
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
    let mut adapters = lucyd::adapters::Adapters::inert();
    adapters.thinking = std::sync::Arc::new(lucyd::claude::Claude::new(
        "claude".into(),
        lucy_config::xdg_dir("XDG_STATE_HOME", ".local/state")
            .join("lucy")
            .join("work"),
    ));
    let daemon = match lucyd::spawn(lucyd::Options {
        socket_path,
        config_path: lucy_config::default_path(),
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
