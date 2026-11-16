//! Each section of `lucy`, opened from the list: what it writes to
//! `config.toml`, when that applies, and the parts that need lucyd.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use std::path::{Path, PathBuf};

use lucy::menu::Open;
use ratatui::crossterm::event::KeyCode;
use support::{read, Home, Script};

const COMMENTED: &str = "# My Lucy, tuned by hand.\n\n\
[session]\n\
silence_timeout_secs = 45 # long enough for me\n\n\
[tiers]\n\
fast = \"claude-fast\" # everyday talk\n";

const THINKING: usize = 0;
const SPEECH: usize = 1;
const VOICE: usize = 2;
const AUDIO: usize = 3;
const WAKE_WORD: usize = 4;
const APPROVALS: usize = 6;
const SERVICE: usize = 7;

fn open(index: usize) -> Script {
    Script::new().down(index).enter()
}

fn leave(script: Script) -> Script {
    script.press(KeyCode::Esc).typed("q")
}

fn configured(home: &Home) {
    std::fs::write(home.config_path(), COMMENTED).unwrap();
}

fn approve(home: &Home, plugin: &str) {
    let path = home.state().join("approvals.toml");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(&format!(
        "[plugins.{plugin}]\nhash = \"{plugin}-hash\"\nschemas = {{}}\n"
    ));
    std::fs::write(path, text).unwrap();
}

#[test]
fn thinking_shows_each_tiers_provider_with_its_health_and_runs_a_real_check() {
    let home = Home::with_lucyd();
    configured(&home);

    let tui = home.tui(Open::Lucy, leave(open(THINKING).down(1).typed("c")));

    for shown in [
        "Fast tier: claude-fast, Claude, haiku",
        "Advanced tier: claude-advanced, Claude, opus",
    ] {
        assert!(tui.shown.contains(shown), "{shown} missing:\n{}", tui.shown);
    }
    // The inert lucyd has no Thinking Provider, for the status and the check.
    let problem = "This lucyd has no Thinking Provider.";
    assert!(
        tui.shown
            .contains(&format!("claude-fast, Claude, haiku. {problem}")),
        "{}",
        tui.shown
    );
    assert!(
        tui.shown
            .contains(&format!("Real check of claude-advanced: {problem}")),
        "{}",
        tui.shown
    );
}

#[test]
fn picking_a_tiers_provider_writes_it_at_once_keeping_comments() {
    let home = Home::with_lucyd();
    home.fake("claude", "exit 0");
    configured(&home);

    let keys = open(THINKING)
        .down(1)
        .enter() // the advanced tier
        .enter() // Claude
        .typed("sonnet")
        .enter(); // a model of my own
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    let config = read(&home.config_path());
    assert!(config.starts_with("# My Lucy, tuned by hand."), "{config}");
    assert!(config.contains("45 # long enough for me"), "{config}");
    assert!(config.contains("# everyday talk"), "{config}");
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.tiers.advanced, "claude-advanced");
    assert_eq!(
        parsed.providers["claude-advanced"].model.as_deref(),
        Some("sonnet")
    );
    assert!(
        tui.shown.contains("Applies at the next Session."),
        "{}",
        tui.shown
    );
    assert!(!tui.shown.contains("Restart lucyd now?"), "{}", tui.shown);
}

#[test]
fn without_claude_or_codex_a_tier_is_left_as_it_was() {
    let home = Home::with_lucyd();
    configured(&home);

    let tui = home.tui(Open::Lucy, leave(open(THINKING).enter()));

    assert!(
        tui.shown
            .contains("No thinking CLI for the fast tier is installed: install claude or codex"),
        "{}",
        tui.shown
    );
    assert_eq!(read(&home.config_path()), COMMENTED);
}

#[test]
fn superwhisper_is_chosen_only_after_its_warnings() {
    let home = Home::without_lucyd();
    home.fake("superwhisper", "exit 0");
    configured(&home);

    let keys = open(SPEECH).enter().down(1).enter().typed("y");
    let tui = home.tui(Open::Lucy, leave(keys));

    for warning in lucy_config::SttKind::Superwhisper.warnings() {
        assert!(
            tui.shown.contains(warning),
            "{warning} not shown:\n{}",
            tui.shown
        );
    }
    assert!(
        tui.shown.contains("Use Superwhisper anyway?"),
        "{}",
        tui.shown
    );
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Superwhisper);
}

#[test]
fn whispers_model_applies_after_a_restart_which_is_offered_while_lucyd_runs() {
    let home = Home::with_lucyd();
    home.fake("systemctl", "exit 0");
    configured(&home);

    // Whisper's model: base.en, then yes to the restart.
    let keys = open(SPEECH).down(1).enter().down(2).enter().typed("y");
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(
        lucy_config::load(&home.config_path())
            .unwrap()
            .whisper
            .model
            .as_deref(),
        Some("base.en")
    );
    assert!(
        tui.shown.contains("Applies after restarting lucyd."),
        "{}",
        tui.shown
    );
    assert!(tui.shown.contains("Restart lucyd now?"), "{}", tui.shown);
    assert!(
        home.calls("systemctl")
            .contains(&"--user restart lucyd.service".to_owned()),
        "{:?}",
        home.calls("systemctl")
    );
    assert!(tui.shown.contains("lucyd is running."), "{}", tui.shown);
}

#[test]
fn with_lucyd_stopped_a_change_still_writes_and_no_restart_is_offered() {
    let home = Home::without_lucyd();
    configured(&home);

    let keys = open(SPEECH).down(1).enter().down(1).enter();
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(
        lucy_config::load(&home.config_path())
            .unwrap()
            .whisper
            .model
            .as_deref(),
        Some("small.en")
    );
    assert!(
        tui.shown.contains("Applies after restarting lucyd."),
        "{}",
        tui.shown
    );
    assert!(!tui.shown.contains("Restart lucyd now?"), "{}", tui.shown);
}

#[test]
fn declining_piper_means_text_only_replies_and_no_voice_to_pick() {
    let home = Home::with_lucyd();
    configured(&home);

    let tui = home.tui(Open::Lucy, leave(open(VOICE).enter().typed("n")));

    assert!(tui.shown.contains("Install Piper now?"), "{}", tui.shown);
    assert!(tui.shown.contains("as text"), "{}", tui.shown);
    assert!(!tui.shown.contains("en_GB-cori-medium"), "{}", tui.shown);
}

#[test]
fn piper_is_installed_after_a_yes_with_uv_then_lucyd_restarts_to_use_it() {
    let home = Home::with_lucyd();
    home.fake("systemctl", "exit 0");
    // python3 imports Piper once uv has installed it.
    let installed = home.bin().join("installed");
    home.fake("python3", &format!("[ -e '{}' ]", installed.display()));
    home.fake("uv", &format!(": > '{}'", installed.display()));
    home.fake("pipx", "exit 1");
    configured(&home);
    let env = lucy::Env {
        os_release: "/nonexistent".into(),
        ..home.env()
    };

    // Yes to the install and to the restart; then the voice row is there.
    let keys = open(VOICE).enter().typed("yy").press(KeyCode::Up);
    let store = support::FakeStore::default();
    let tui = home.tui_with(&env, &store, Open::Lucy, leave(keys));

    assert_eq!(home.calls("uv"), ["tool install piper-tts[http]"]);
    assert!(home.calls("pipx").is_empty());
    assert!(tui.shown.contains("Piper is installed"), "{}", tui.shown);
    assert!(
        home.calls("systemctl")
            .contains(&"--user restart lucyd.service".to_owned()),
        "{:?}",
        home.calls("systemctl")
    );
    assert!(
        tui.shown.contains("Voice: en_GB-cori-medium"),
        "{}",
        tui.shown
    );
}

#[test]
fn a_non_commercial_voice_shows_its_licence_and_is_kept_after_its_preview() {
    let home = Home::with_lucyd();
    home.fake("python3", "exit 0");
    configured(&home);

    // The third voice, yes despite its licence, keep it after hearing it.
    let keys = open(VOICE).enter().down(2).enter().typed("y").enter();
    let tui = home.tui(Open::Lucy, leave(keys));

    assert!(tui.shown.contains("non-commercial"), "{}", tui.shown);
    assert!(tui.shown.contains("Keep this voice?"), "{}", tui.shown);
    assert_eq!(
        lucy_config::load(&home.config_path()).unwrap().piper.voice,
        "en_US-lessac-medium"
    );
    assert!(
        tui.shown.contains("Applies at the next Session."),
        "{}",
        tui.shown
    );
}

const BARELY: &str = "Lucy can barely hear you on this mic, so talking over her may not work. \
                      Move it closer, turn it up in your sound settings, or pick another mic.";

#[test]
fn a_mic_lucy_can_barely_hear_is_flagged_and_not_kept_by_default() {
    // About −60 dBFS: too quiet even for Lucy's boost.
    let home = Home::with_mic(0.001);
    configured(&home);

    // The mic; Enter at "Keep this microphone?" picks No, so the mic again,
    // then keep it with a "y".
    let keys = open(AUDIO).enters(3).enter().typed("y");
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    assert!(tui.shown.contains(BARELY), "{}", tui.shown);
    assert_eq!(
        tui.shown.matches("Keep this microphone?").count(),
        2,
        "{}",
        tui.shown
    );
}

#[test]
fn a_mic_at_a_normal_level_is_kept_by_default_with_no_warning() {
    // About −20 dBFS.
    let home = Home::with_mic(0.1);
    home.fake_pactl(
        &[("alsa_input.usb-Yeti", "Yeti Analog Stereo")],
        &[],
        ("", ""),
    );
    configured(&home);

    let keys = open(AUDIO).enter().down(1).enter().enter();
    let tui = home.tui(Open::Lucy, leave(keys));

    assert!(!tui.shown.contains("barely hear"), "{}", tui.shown);
    assert!(tui.shown.contains("the meter should move"), "{}", tui.shown);
    assert_eq!(
        lucy_config::load(&home.config_path()).unwrap().audio.mic,
        "alsa_input.usb-Yeti"
    );
}

#[test]
fn audio_devices_shows_descriptions_and_writes_the_chosen_node_name() {
    let home = Home::with_lucyd();
    home.fake_pactl(
        &[
            ("alsa_input.usb-B", "USB Mic Mono"),
            ("alsa_input.usb-A", "USB Mic Mono"),
        ],
        &[("alsa_output.usb-Pebble", "Pebble V3 Analog Stereo")],
        ("alsa_input.usb-A", "alsa_output.usb-Pebble"),
    );
    std::fs::write(
        home.config_path(),
        "[audio]\nspeaker = \"alsa_output.usb-Gone\"\n",
    )
    .unwrap();

    // The speaker row; the second choice, Pebble; heard; then back.
    let keys = open(AUDIO).down(1).enter().down(1).enter().enter();
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    for label in [
        "Microphone: Default (USB Mic Mono (1))",
        "Speaker: alsa_output.usb-Gone (not connected)",
        "Default (Pebble V3 Analog Stereo)",
        "Pebble V3 Analog Stereo",
        "Speaker: Pebble V3 Analog Stereo",
    ] {
        assert!(tui.shown.contains(label), "{label}:\n{}", tui.shown);
    }
    assert_eq!(
        lucy_config::load(&home.config_path())
            .unwrap()
            .audio
            .speaker,
        "alsa_output.usb-Pebble"
    );
}

#[test]
fn sections_that_need_lucyd_open_while_it_is_stopped_and_say_so() {
    let home = Home::without_lucyd();
    configured(&home);

    for (section, keys) in [
        (THINKING, Script::new().typed("c")),
        (AUDIO, Script::new().enters(2)),
        (WAKE_WORD, Script::new().enter()),
        (APPROVALS, Script::new()),
    ] {
        let tui = home.tui(Open::Lucy, leave(open(section).chain(keys)));

        assert_eq!(tui.result, Ok(()), "{}", tui.shown);
        assert!(
            tui.shown.contains("lucyd isn't running. s starts it."),
            "{}",
            tui.shown
        );
    }
    // Settings only written to the file still work.
    assert_eq!(
        lucy_config::load(&home.config_path()).unwrap().audio.mic,
        "default"
    );
}

fn fifo(path: &Path) -> PathBuf {
    let made = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success());
    path.to_owned()
}

#[test]
fn s_starts_lucyd_from_a_section_that_needs_it() {
    let home = Home::without_lucyd();
    let started = fifo(&home.bin().join("started"));
    let up = fifo(&home.bin().join("up"));
    home.fake(
        "systemctl",
        &format!(
            "[ \"$2\" = start ] && {{ echo > '{}'; read _ < '{}'; }}; exit 0",
            started.display(),
            up.display()
        ),
    );
    configured(&home);

    let (tui, _lucyd) = std::thread::scope(|scope| {
        let systemd = scope.spawn(|| {
            std::fs::read(&started).unwrap();
            let lucyd = home.spawn_lucyd();
            std::fs::write(&up, "\n").unwrap();
            lucyd
        });
        let tui = home.tui(Open::Lucy, leave(open(APPROVALS).typed("s")));
        (tui, systemd.join().unwrap())
    });

    assert!(tui.shown.contains("lucyd is running."), "{}", tui.shown);
    assert!(tui.shown.contains("Lucy holds none."), "{}", tui.shown);
    assert!(
        tui.screen.contains("lucyd is running. No Session is open."),
        "{}",
        tui.screen
    );
}

#[test]
fn the_service_section_enables_disables_starts_stops_and_restarts_lucyd() {
    let home = Home::with_lucyd();
    let enabled = home.bin().join("enabled");
    home.fake(
        "systemctl",
        &format!(
            "case \"$2\" in \
             is-enabled) [ -e '{0}' ];; \
             enable) : > '{0}';; \
             disable) rm -f '{0}';; \
             esac",
            enabled.display()
        ),
    );
    configured(&home);

    let keys = open(SERVICE)
        .enter()
        .typed("y")
        .enter()
        .typed("n")
        .typed("sxr");
    let tui = home.tui(Open::Lucy, leave(keys));

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    let calls = home.calls("systemctl");
    for call in [
        "--user enable --now lucyd.service lucy-indicator.service",
        "--user disable lucyd.service lucy-indicator.service",
        "--user start lucyd.service",
        "--user stop lucyd.service",
        "--user restart lucyd.service",
    ] {
        assert!(
            calls.contains(&call.to_owned()),
            "{call} not run: {calls:?}"
        );
    }
    assert!(
        tui.shown.contains("Start Lucy whenever you log in: yes"),
        "{}",
        tui.shown
    );
    assert!(
        tui.shown.contains("Start Lucy whenever you log in: no"),
        "{}",
        tui.shown
    );
}

fn enable(home: &Home, bin: &Path, systemctl: &str) -> support::Tui {
    home.fake("systemctl", systemctl);
    configured(home);
    let env = lucy::Env {
        bin: bin.to_owned(),
        ..home.env()
    };
    let keys = leave(open(SERVICE).enter().enter());
    home.tui_with(&env, &support::FakeStore::default(), Open::Lucy, keys)
}

const UNKNOWN_TO_SYSTEMD: &str = "[ \"$2\" != is-enabled ] && [ \"$2\" != cat ]";
const ALREADY_ENABLED: &str = "exit 0";

fn omarchy(home: &Home) -> PathBuf {
    let omarchy = home.dir.path().join("config/omarchy");
    std::fs::create_dir_all(&omarchy).unwrap();
    omarchy
}

fn bar_panel(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("manifest.json"), "{}").unwrap();
    dir.to_owned()
}

fn linked(omarchy: &Path) -> PathBuf {
    std::fs::canonicalize(omarchy.join("plugins/lucy-panel")).unwrap()
}

#[test]
fn enabling_lucyd_links_the_bar_panel_shipped_beside_lucy() {
    let home = Home::with_lucyd();
    let omarchy = omarchy(&home);
    let bin = home.dir.path().join("usr/bin");
    let panel = bar_panel(&bin.join("omarchy-plugin"));

    let tui = enable(&home, &bin, UNKNOWN_TO_SYSTEMD);

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    assert!(
        tui.shown.contains("Lucy starts whenever you log in."),
        "{}",
        tui.shown
    );
    assert_eq!(linked(&omarchy), std::fs::canonicalize(panel).unwrap());
    assert!(home
        .dir
        .path()
        .join("config/systemd/user/lucyd.service")
        .exists());
}

#[test]
fn enabling_a_lucyd_built_from_a_checkout_links_the_checkouts_bar_panel() {
    let home = Home::with_lucyd();
    let omarchy = omarchy(&home);
    let checkout = home.dir.path().join("lucy");
    let panel = bar_panel(&checkout.join("omarchy-plugin"));
    let bin = checkout.join("target/debug");
    std::fs::create_dir_all(&bin).unwrap();

    let tui = enable(&home, &bin, UNKNOWN_TO_SYSTEMD);

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    assert_eq!(linked(&omarchy), std::fs::canonicalize(panel).unwrap());
}

#[test]
fn enabling_lucyd_while_it_is_enabled_refreshes_the_link() {
    let home = Home::with_lucyd();
    let omarchy = omarchy(&home);
    std::fs::create_dir_all(omarchy.join("plugins")).unwrap();
    std::os::unix::fs::symlink(
        home.dir.path().join("a-worktree-since-removed"),
        omarchy.join("plugins/lucy-panel"),
    )
    .unwrap();
    let bin = home.dir.path().join("usr/bin");
    let panel = bar_panel(&bin.join("omarchy-plugin"));

    let tui = enable(&home, &bin, ALREADY_ENABLED);

    assert!(
        tui.shown.contains("Start Lucy whenever you log in: yes"),
        "{}",
        tui.shown
    );
    assert!(
        home.calls("systemctl")
            .contains(&"--user enable --now lucyd.service lucy-indicator.service".to_owned()),
        "{:?}",
        home.calls("systemctl")
    );
    assert_eq!(linked(&omarchy), std::fs::canonicalize(panel).unwrap());
}

#[test]
fn with_no_bar_panel_to_find_enabling_lucyd_still_works_and_says_so() {
    let home = Home::with_lucyd();
    let omarchy = omarchy(&home);
    let bin = home.dir.path().join("usr/bin");
    std::fs::create_dir_all(&bin).unwrap();

    let tui = enable(&home, &bin, UNKNOWN_TO_SYSTEMD);

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    assert!(
        tui.shown.contains("Lucy starts whenever you log in."),
        "{}",
        tui.shown
    );
    assert!(
        tui.shown.contains("Couldn't link the bar panel"),
        "{}",
        tui.shown
    );
    assert!(!omarchy.join("plugins/lucy-panel").exists());
}

#[test]
fn without_omarchy_enabling_lucyd_links_no_bar_panel() {
    let home = Home::with_lucyd();
    let bin = home.dir.path().join("usr/bin");
    bar_panel(&bin.join("omarchy-plugin"));

    let tui = enable(&home, &bin, UNKNOWN_TO_SYSTEMD);

    assert_eq!(tui.result, Ok(()), "{}", tui.shown);
    assert!(!tui.shown.contains("bar panel"), "{}", tui.shown);
    assert!(!home.dir.path().join("config/omarchy").exists());
}

#[test]
fn approvals_are_listed_and_one_is_revoked_after_a_yes() {
    let home = Home::with_lucyd();
    approve(&home, "news");
    approve(&home, "notes");
    configured(&home);

    let keys = open(APPROVALS)
        .down(1)
        .typed("ry") // revoke notes
        .press(KeyCode::Up)
        .enter()
        .typed("n"); // not news
    let tui = home.tui(Open::Lucy, leave(keys));

    assert!(tui.shown.contains("Not revoked."), "{}", tui.shown);
    let left: Vec<String> = home
        .env()
        .connect()
        .unwrap()
        .approvals()
        .unwrap()
        .approvals
        .into_iter()
        .map(|a| a.plugin)
        .collect();
    assert_eq!(left, ["news"]);
}

#[test]
fn approvals_are_never_granted_here_only_by_voice() {
    let home = Home::with_lucyd();
    configured(&home);

    let keys = Script::new().typed("a1gy");
    let tui = home.tui(Open::Lucy, leave(open(APPROVALS).chain(keys)));
    let shown = tui.shown.to_lowercase();

    assert!(shown.contains("granted only by voice"), "{shown}");
    assert!(!shown.contains("approve"), "{shown}");
}
