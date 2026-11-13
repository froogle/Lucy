use std::process::ExitCode;

fn main() -> ExitCode {
    let config_path = lucy_config::default_path();
    // Read again at each Session start; the launcher is set once, here.
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
    let mut adapters = lucyd::adapters::Adapters::inert();
    adapters.thinking = std::sync::Arc::new(lucyd::claude::Claude::new(
        "claude".into(),
        lucy_config::xdg_dir("XDG_STATE_HOME", ".local/state")
            .join("lucy")
            .join("work"),
    ));
    adapters.launcher = std::sync::Arc::new(lucyd::launcher::SystemLauncher::new(
        config.desktop.launcher.clone(),
    ));
    adapters.applications = lucyd::desktop_entries::xdg_dirs();
    let daemon = match lucyd::spawn(lucyd::Options {
        socket_path,
        config_path,
        persona_path: lucy_config::xdg_dir("XDG_CONFIG_HOME", ".config")
            .join("lucy")
            .join("persona.md"),
        lucy_program,
        adapters,
        plugins: lucyd::plugins::Paths::from_env(),
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
