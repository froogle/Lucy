//! `lucy`: the list of sections, each opened as its own page, and the walk
//! through them that first run and `lucy setup` take. Full-screen, pressed
//! by scripted keys.

mod support;

use lucy::menu::Open;
use ratatui::crossterm::event::KeyCode;
use support::{read, stderr, Home, Script};

const SECTIONS: [&str; 8] = [
    "Thinking",
    "Speech-to-text",
    "Voice",
    "Audio devices",
    "Wake word",
    "Plugins",
    "Approvals",
    "Service",
];

fn configured(home: &Home) {
    std::fs::write(home.config_path(), "# Mine.\n").unwrap();
}

#[test]
fn lucy_opens_the_list_of_sections_under_lucyds_status() {
    let home = Home::with_lucyd();
    configured(&home);

    let tui = home.tui(Open::Lucy, Script::new().typed("q"));

    assert_eq!(tui.result, Ok(()));
    assert!(
        tui.screen.contains("lucyd is running. No Session is open."),
        "{}",
        tui.screen
    );
    let at: Vec<usize> = SECTIONS
        .iter()
        .map(|s| {
            tui.screen
                .find(s)
                .unwrap_or_else(|| panic!("{s}:\n{}", tui.screen))
        })
        .collect();
    assert!(at.is_sorted(), "{}", tui.screen);
}

#[test]
fn the_status_line_says_when_lucyd_is_stopped_or_a_session_is_open() {
    let stopped = Home::without_lucyd();
    configured(&stopped);
    let open = Home::with_lucyd();
    configured(&open);
    let mut client = open.env().connect().unwrap();
    client.start_session().unwrap();

    let stopped = stopped.tui(Open::Lucy, Script::new().typed("q"));
    let open = open.tui(Open::Lucy, Script::new().typed("q"));

    assert!(
        stopped.screen.contains("lucyd isn't running."),
        "{}",
        stopped.screen
    );
    assert!(
        open.screen.contains("A Session is open."),
        "{}",
        open.screen
    );
}

#[test]
fn enter_opens_a_section_as_its_own_page_and_esc_goes_back_to_the_list() {
    let home = Home::with_lucyd();
    configured(&home);

    let keys = Script::new()
        .enter() // Thinking
        .press(KeyCode::Esc)
        .down(6)
        .enter() // Approvals
        .press(KeyCode::Esc)
        .typed("q");
    let tui = home.tui(Open::Lucy, keys);

    assert_eq!(tui.result, Ok(()));
    assert!(tui.shown.contains("Fast tier"), "{}", tui.shown);
    assert!(tui.shown.contains("granted only by voice"), "{}", tui.shown);
    for section in SECTIONS {
        assert!(
            tui.screen.contains(section),
            "back on the list:\n{}",
            tui.screen
        );
    }
}

#[test]
fn j_and_k_move_and_q_or_ctrl_c_quit_from_inside_a_section() {
    let home = Home::with_lucyd();
    configured(&home);

    let q = home.tui(Open::Lucy, Script::new().typed("jjk").enter().typed("q"));
    let ctrl_c = home.tui(Open::Lucy, Script::new().enter().ctrl_c());

    assert_eq!(q.result, Ok(()));
    assert!(q.shown.contains("Speech-to-text:"), "{}", q.shown);
    assert_eq!(ctrl_c.result, Ok(()));
}

#[test]
fn lucy_needs_a_terminal_rather_than_writing_escape_codes_to_a_pipe() {
    let home = Home::with_lucyd();

    for args in [&[][..], &["setup"], &["credentials", "news"]] {
        let output = home.lucy(args, &[]);

        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            stderr(&output).contains("needs a terminal"),
            "{}",
            stderr(&output)
        );
    }
}

fn tools(home: &Home) {
    home.fake("claude", "exit 0");
    // Imports piper.http_server.
    home.fake("python3", "exit 0");
    home.fake("superwhisper", "exit 0");
    home.fake(
        "systemctl",
        "case \"$2\" in cat|is-enabled) exit 1;; esac; exit 0",
    );
    home.fake(
        "pactl",
        "case \"$3\" in \
         sources) printf '1\\talsa_input.usb-Yeti\\tPipeWire\\n2\\talsa_output.hdmi.monitor\\tPipeWire\\n';; \
         sinks) printf '1\\talsa_output.hdmi\\tPipeWire\\n';; esac",
    );
}

fn every_suggestion() -> Script {
    Script::new()
        .enters(7) // Thinking: each tier, its CLI and model, then Next
        .enters(6) // Speech-to-text: Whisper; its model, restarting lucyd; Next
        .enters(4) // Voice: the first, heard and kept; Next
        .enters(7) // Audio devices: the default mic, kept; the default speaker, heard; Next
        .enters(2) // Wake word: the test; Next
        .enter() // Plugins: Next
        .enter() // Approvals: Next
        .enters(3) // Service: start at login, yes; Done
}

#[test]
fn with_no_config_lucy_walks_every_section_in_order_then_shows_the_list() {
    let home = Home::with_lucyd();
    tools(&home);

    let tui = home.tui(Open::Lucy, every_suggestion().typed("q"));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    let nexts: Vec<usize> = SECTIONS[1..]
        .iter()
        .map(|s| {
            tui.shown
                .find(&format!("Next: {s}"))
                .unwrap_or_else(|| panic!("{s}"))
        })
        .collect();
    assert!(nexts.is_sorted(), "{}", tui.shown);
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.tiers.fast, "claude-fast");
    assert_eq!(parsed.tiers.advanced, "claude-advanced");
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Whisper);
    assert_eq!(parsed.piper.voice, "en_GB-cori-medium");
    assert_eq!(parsed.audio.mic, "default");
    assert!(
        home.calls("systemctl")
            .contains(&"--user enable --now lucyd.service lucy-indicator.service".to_owned()),
        "{:?}",
        home.calls("systemctl")
    );
    for section in SECTIONS {
        assert!(
            tui.screen.contains(section),
            "ends on the list:\n{}",
            tui.screen
        );
    }
}

#[test]
fn the_walk_writes_only_the_settings_it_shows() {
    let home = Home::with_lucyd();
    tools(&home);

    let tui = home.tui(Open::Lucy, every_suggestion().typed("q"));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    let config: toml_edit::DocumentMut = read(&home.config_path()).parse().unwrap();
    for (table, _) in config.iter() {
        assert!(
            ["providers", "tiers", "stt", "whisper", "piper", "audio"].contains(&table),
            "the walk wrote [{table}], which is file-only"
        );
    }
}

#[test]
fn esc_leaves_the_walk_for_the_list_keeping_what_was_picked() {
    let home = Home::without_lucyd();
    home.fake("claude", "exit 0");

    let keys = Script::new()
        .enters(3) // the fast tier's CLI and model
        .press(KeyCode::Esc)
        .typed("q");
    let left = home.tui(Open::Lucy, keys);
    let again = home.tui(Open::Lucy, Script::new().typed("q"));

    assert_eq!(left.result, Ok(()), "{}", left.shown);
    assert_eq!(
        lucy_config::load(&home.config_path()).unwrap().tiers.fast,
        "claude-fast"
    );
    assert!(!left.shown.contains("Next: Voice"), "{}", left.shown);
    for section in SECTIONS {
        assert!(left.screen.contains(section), "{}", left.screen);
        assert!(again.screen.contains(section), "{}", again.screen);
    }
    assert!(
        !again.shown.contains("Next:"),
        "straight to the list:\n{}",
        again.shown
    );
}

#[test]
fn a_walk_left_before_any_pick_still_means_lucy_opens_at_the_list_next_time() {
    let home = Home::without_lucyd();

    home.tui(Open::Lucy, Script::new().press(KeyCode::Esc).typed("q"));
    let again = home.tui(Open::Lucy, Script::new().typed("q"));

    assert!(home.config_path().exists());
    assert!(!again.shown.contains("Next:"), "{}", again.shown);
}

#[test]
fn lucy_setup_walks_the_sections_again_on_purpose() {
    let home = Home::without_lucyd();
    configured(&home);

    let tui = home.tui(
        Open::Setup,
        Script::new().down(2).enter().press(KeyCode::Esc).typed("q"),
    );

    assert!(tui.shown.contains("Next: Speech-to-text"), "{}", tui.shown);
    assert!(tui.shown.contains("Next: Voice"), "{}", tui.shown);
    assert_eq!(read(&home.config_path()), "# Mine.\n");
}
