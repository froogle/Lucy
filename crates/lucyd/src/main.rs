use std::error::Error;
use std::process::ExitCode;
use std::sync::Arc;

use lucyd::adapters::Adapters;
use lucyd::audio::{self, Cpal, Silero};
use lucyd::whisper::Whisper;

fn main() -> ExitCode {
    let config_path = lucy_config::default_path();
    // Read again at each Session start. Only Whisper's settings are used
    // from this first read, since its model stays loaded.
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
    // Ticket 09 feeds `take_reference()` to the echo canceller.
    adapters.tts = Box::new(lucyd::piper::Piper::start(
        None,
        lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share")
            .join("lucy")
            .join("voices"),
        lucyd::piper::DEFAULT_VOICE,
    ));
    adapters.notifier =
        std::sync::Arc::new(lucyd::notify_send::NotifySend::new("notify-send".into()));
    adapters.launcher = std::sync::Arc::new(lucyd::launcher::SystemLauncher);
    adapters.applications = lucyd::desktop_entries::xdg_dirs();
    if let Err(e) = hear(&config, &mut adapters) {
        eprintln!("lucyd: {e}; Lucy can't hear");
    }
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

/// Loads Whisper once and cuts the microphone into utterances for it.
fn hear(config: &lucy_config::Config, adapters: &mut Adapters) -> Result<(), Box<dyn Error>> {
    let models = lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share")
        .join("lucy")
        .join("models");
    let whisper = Whisper::load(&config.whisper, &models)?;
    let vad = Silero::load()?;
    let (hearing, speech) = audio::listen(
        Box::new(Cpal),
        &config.audio,
        Box::new(vad),
        &config.endpointing,
    )?;
    adapters.stt = Arc::new(whisper);
    adapters.speech = speech;
    adapters.hearing = Some(hearing);
    Ok(())
}
