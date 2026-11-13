use std::error::Error;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use lucyd::adapters::Adapters;
use lucyd::audio::{self, Cpal, LiveKit, Silero, WakeWord};
use lucyd::superwhisper::Superwhisper;
use lucyd::whisper::Whisper;

fn main() -> ExitCode {
    let config_path = lucy_config::default_path();
    // Read again at each Session start. Only Whisper's settings, and
    // whether it is needed, are used from this first read, since its model
    // stays loaded.
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
    let work = lucy_config::xdg_dir("XDG_STATE_HOME", ".local/state")
        .join("lucy")
        .join("work");
    adapters.thinking = Arc::new(lucyd::by_kind::ByKind {
        claude: Arc::new(lucyd::claude::Claude::new("claude".into(), work.clone())),
        codex: Arc::new(lucyd::codex::Codex::new("codex".into(), work)),
    });
    // Ticket 09 feeds `take_reference()` to the echo canceller.
    adapters.tts = Box::new(lucyd::piper::Piper::start(
        None,
        lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share")
            .join("lucy")
            .join("voices"),
        &config.piper.voice,
    ));
    adapters.notifier =
        std::sync::Arc::new(lucyd::notify_send::NotifySend::new("notify-send".into()));
    adapters.launcher = std::sync::Arc::new(lucyd::launcher::SystemLauncher);
    // Utterances go beside the socket, in the user's runtime dir.
    let utterances = socket_path.parent().expect("the socket is in a dir");
    adapters.superwhisper = Arc::new(Superwhisper::new(
        "superwhisper".into(),
        utterances.to_owned(),
    ));
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
        plugin_paths: lucyd::plugins::Paths::from_env(),
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

/// Loads Whisper and the wake word once, listens for the Wake word and cuts
/// the microphone into utterances for STT. Whisper is optional while `[stt]`
/// chooses Superwhisper.
fn hear(config: &lucy_config::Config, adapters: &mut Adapters) -> Result<(), Box<dyn Error>> {
    let models = lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share")
        .join("lucy")
        .join("models");
    let whisper = match Whisper::load(&config.whisper, &models) {
        Ok(whisper) => Some(whisper),
        Err(e) if config.stt.kind == lucy_config::SttKind::Superwhisper => {
            eprintln!("lucyd: {e}; only Superwhisper can transcribe");
            None
        }
        Err(e) => return Err(e.into()),
    };
    let vad = Silero::load()?;
    let wake_word = match wake_word(&config.wake_word, &models) {
        Ok(wake_word) => Some(Box::new(wake_word) as Box<dyn WakeWord>),
        Err(e) => {
            eprintln!("lucyd: {e}; the Wake word is off");
            None
        }
    };
    let ears = audio::listen(
        Box::new(Cpal),
        &config.audio,
        Box::new(vad),
        wake_word,
        &config.endpointing,
    )?;
    if let Some(whisper) = whisper {
        adapters.stt = Arc::new(whisper);
    }
    adapters.speech = ears.speech;
    adapters.wake_word = ears.wake_word;
    adapters.speaking = ears.speaking;
    adapters.hearing = Some(ears.hearing);
    Ok(())
}

/// The configured wake-word model, or the bundled one, installed under
/// `models/wake`.
fn wake_word(config: &lucy_config::WakeWord, models: &Path) -> Result<LiveKit, Box<dyn Error>> {
    let model = match &config.model {
        Some(model) => model.clone(),
        None => audio::install_bundled_wake_model(&models.join("wake"))?,
    };
    Ok(LiveKit::load(&model, config.threshold)?)
}
