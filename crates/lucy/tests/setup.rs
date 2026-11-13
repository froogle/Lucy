//! `lucy setup`: first-run setup, driven through the real binary.

mod support;

use support::{read, stderr, stdout, Home};

const COMMENTED: &str = "# My Lucy, tuned by hand.\n\n\
[session]\n\
silence_timeout_secs = 45 # long enough for me\n\n\
[tiers]\n\
fast = \"claude-fast\" # everyday talk\n";

/// The tools of a user with `claude`, Piper and Superwhisper.
fn tools(home: &Home) {
    home.fake("claude", "exit 0");
    // Imports piper.http_server.
    home.fake("python3", "exit 0");
    home.fake("superwhisper", "exit 0");
    home.fake("systemctl", "[ \"$2\" = cat ] && exit 1; exit 0");
    home.fake(
        "pactl",
        "case \"$3\" in \
         sources) printf '1\\talsa_input.usb-Yeti\\tPipeWire\\n2\\talsa_output.hdmi.monitor\\tPipeWire\\n';; \
         sinks) printf '1\\talsa_output.hdmi\\tPipeWire\\n';; esac",
    );
}

#[test]
fn setup_writes_each_choice_keeping_comments_and_enables_the_service() {
    let home = Home::with_lucyd();
    tools(&home);
    std::fs::write(home.config_path(), COMMENTED).unwrap();

    let output = home.lucy(
        &["setup"],
        &[
            "",       // fast tier: Claude
            "",       // its recommended model
            "",       // advanced tier: Claude
            "sonnet", // a model of my own
            "2",      // Superwhisper
            "y",      // despite its warnings
            "2",      // the Yeti microphone
            "",       // keep it
            "",       // the default speaker
            "",       // heard Lucy on it
            "3",      // a non-commercial voice
            "y",      // despite its licence
            "",       // keep it
            "",       // test the Wake word
            "",       // start Lucy at login
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let shown = stdout(&output);
    let config = read(&home.config_path());
    assert!(config.starts_with("# My Lucy, tuned by hand."), "{config}");
    assert!(config.contains("45 # long enough for me"), "{config}");
    assert!(config.contains("# everyday talk"), "{config}");
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.session.silence_timeout_secs, 45.0);
    assert_eq!(parsed.tiers.fast, "claude-fast");
    assert_eq!(parsed.tiers.advanced, "claude-advanced");
    assert_eq!(
        parsed.providers["claude-fast"].model.as_deref(),
        Some("haiku")
    );
    assert_eq!(
        parsed.providers["claude-advanced"].model.as_deref(),
        Some("sonnet")
    );
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Superwhisper);
    for warning in lucy_config::SttKind::Superwhisper.warnings() {
        assert!(shown.contains(warning), "{warning} not shown:\n{shown}");
    }
    assert_eq!(parsed.audio.mic, "alsa_input.usb-Yeti");
    assert!(!shown.contains("hdmi.monitor"), "{shown}");
    assert_eq!(parsed.audio.speaker, "default");
    assert_eq!(parsed.piper.voice, "en_US-lessac-medium");
    assert!(shown.contains("non-commercial"), "{shown}");

    let units = home.dir.path().join("config/systemd/user");
    let lucyd_unit = read(&units.join("lucyd.service"));
    let lucyd = std::path::Path::new(env!("CARGO_BIN_EXE_lucy")).with_file_name("lucyd");
    assert!(
        lucyd_unit.contains(&format!("ExecStart={}", lucyd.display())),
        "{lucyd_unit}"
    );
    assert!(lucyd_unit.contains("WantedBy=graphical-session.target"));
    assert!(units.join("lucy-indicator.service").is_file());
    let calls = home.calls("systemctl");
    assert!(
        calls.contains(&"--user daemon-reload".to_owned()),
        "{calls:?}"
    );
    assert!(
        calls.contains(&"--user enable --now lucyd.service lucy-indicator.service".to_owned()),
        "{calls:?}"
    );
}

#[test]
fn setup_writes_only_what_it_asks_about() {
    let home = Home::with_lucyd();
    tools(&home);

    // Every default, and no service.
    let mut answers = [""; 14];
    answers[13] = "n";
    let output = home.lucy(&["setup"], &answers);

    assert!(output.status.success(), "{}", stderr(&output));
    let config: toml_edit::DocumentMut = read(&home.config_path()).parse().unwrap();
    let tables: Vec<&str> = config.iter().map(|(name, _)| name).collect();
    for table in &tables {
        assert!(
            ["providers", "tiers", "stt", "whisper", "piper", "audio"].contains(table),
            "setup wrote [{table}], which is file-only"
        );
    }
    assert!(
        !home.calls("systemctl").iter().any(|c| c.contains("enable")),
        "{}",
        stdout(&output)
    );
}

#[test]
fn declining_piper_means_text_only_replies_and_no_voice_to_pick() {
    let home = Home::with_lucyd();
    home.fake("claude", "exit 0");
    home.fake("systemctl", "exit 0");

    let output = home.lucy(&["setup"], &["n"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let shown = stdout(&output);
    assert!(shown.contains("Install Piper"), "{shown}");
    assert!(shown.contains("as text"), "{shown}");
    assert!(!shown.contains("en_GB-cori-medium"), "{shown}");
}

#[test]
fn piper_is_installed_after_a_yes_with_uv_when_there_is_no_distro_package() {
    let home = Home::with_lucyd();
    home.fake("claude", "exit 0");
    home.fake("systemctl", "exit 0");
    // python3 imports Piper once uv has installed it.
    let installed = home.bin().join("installed");
    home.fake("python3", &format!("[ -e '{}' ]", installed.display()));
    home.fake("uv", &format!(": > '{}'", installed.display()));
    home.fake("pipx", "exit 1");

    let output = home.lucy(&["setup", "--os-release", "/nonexistent"], &["y"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(home.calls("uv"), ["tool install piper-tts[http]"]);
    assert!(home.calls("pipx").is_empty());
    assert!(
        stdout(&output).contains("Piper is installed"),
        "{}",
        stdout(&output)
    );
}
