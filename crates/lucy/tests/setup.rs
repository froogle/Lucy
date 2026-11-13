//! `lucy setup`: first-run setup, full-screen, pressed by scripted keys.

mod support;

use support::{read, stderr, Home, Script};

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

    let keys = Script::new()
        .enter() // fast tier: Claude
        .enter() // its recommended model
        .enter() // advanced tier: Claude
        .typed("sonnet")
        .enter() // a model of my own
        .down(1)
        .enter() // Superwhisper
        .typed("y") // despite its warnings
        .down(1)
        .enter() // the Yeti microphone
        .enter() // keep it
        .enter() // the default speaker
        .enter() // heard Lucy on it
        .down(2)
        .enter() // a non-commercial voice
        .typed("y") // despite its licence
        .enter() // keep it
        .enter() // test the Wake word
        .enter(); // start Lucy at login
    let setup = home.setup(&home.env(), keys);

    assert_eq!(setup.result, Ok(()), "{}", setup.shown);
    let shown = setup.shown;
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
    assert!(shown.contains("Use Superwhisper anyway?"), "{shown}");
    assert_eq!(parsed.audio.mic, "alsa_input.usb-Yeti");
    assert!(shown.contains("alsa_input.usb-Yeti"), "{shown}");
    assert!(!shown.contains("hdmi.monitor"), "{shown}");
    assert_eq!(parsed.audio.speaker, "default");
    assert_eq!(parsed.piper.voice, "en_US-lessac-medium");
    assert!(shown.contains("non-commercial"), "{shown}");
    // The transcript keeps each answer.
    assert!(
        setup
            .lines
            .contains(&"> Model, or any the CLI accepts: sonnet".to_owned()),
        "{:?}",
        setup.lines
    );

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

    // Every suggestion, and no service.
    let setup = home.setup(&home.env(), Script::new().enters(13).typed("n"));

    assert_eq!(setup.result, Ok(()), "{}", setup.shown);
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
        setup.shown
    );
}

#[test]
fn declining_piper_means_text_only_replies_and_no_voice_to_pick() {
    let home = Home::with_lucyd();
    home.fake("claude", "exit 0");
    home.fake("systemctl", "exit 0");

    // No: then tiers and models, Whisper's model, mic, keep, speaker,
    // heard, the Wake word and the service.
    let setup = home.setup(&home.env(), Script::new().typed("n").enters(11));

    assert_eq!(setup.result, Ok(()), "{}", setup.shown);
    let shown = setup.shown;
    assert!(shown.contains("Install Piper"), "{shown}");
    assert!(shown.contains("as text"), "{shown}");
    assert!(!shown.contains("en_GB-cori-medium"), "{shown}");
}

#[test]
fn without_claude_or_codex_setup_says_each_tier_is_left_as_it_was() {
    let home = Home::with_lucyd();
    home.fake("systemctl", "exit 0");

    // No Piper: then Whisper's model, mic, keep, speaker, heard, the Wake
    // word and the service.
    let setup = home.setup(&home.env(), Script::new().typed("n").enters(7));

    assert_eq!(setup.result, Ok(()), "{}", setup.shown);
    let shown = setup.shown;
    for tier in ["fast", "advanced"] {
        assert!(
            shown.contains(&format!(
                "No thinking CLI for the {tier} tier is installed: install claude or codex"
            )),
            "{shown}"
        );
    }
    // Setup carries on past the tiers, and writes none.
    assert!(shown.contains("Whisper's model"), "{shown}");
    assert!(!read(&home.config_path()).contains("[tiers]"));
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
    let env = lucy::Env {
        os_release: "/nonexistent".into(),
        ..home.env()
    };

    // Yes: then the rest as suggested, a voice among them.
    let setup = home.setup(&env, Script::new().typed("y").enters(13));

    assert_eq!(setup.result, Ok(()), "{}", setup.shown);
    assert_eq!(home.calls("uv"), ["tool install piper-tts[http]"]);
    assert!(home.calls("pipx").is_empty());
    assert!(
        setup.shown.contains("Piper is installed"),
        "{}",
        setup.shown
    );
    assert!(setup.shown.contains("Lucy's voice:"), "{}", setup.shown);
}

#[test]
fn ctrl_c_leaves_setup_at_once() {
    let home = Home::with_lucyd();
    tools(&home);

    let setup = home.setup(&home.env(), Script::new().enter().ctrl_c());

    assert_eq!(setup.result, Err("cancelled".to_owned()));
    assert!(!home.config_path().exists());
}

#[test]
fn setup_needs_a_terminal() {
    let home = Home::with_lucyd();

    let output = home.lucy(&["setup"], &[]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("needs a terminal"),
        "{}",
        stderr(&output)
    );
}
