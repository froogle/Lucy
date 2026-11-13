//! The `superwhisper` STT Provider, against a fake `superwhisper` CLI.

mod harness;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::adapters::{SpeechSegment, Stt};
use lucyd::codex::Codex;
use lucyd::superwhisper::Superwhisper;

const CHOSEN: &str = "[stt]\nkind = \"superwhisper\"\n";

/// A fake `superwhisper` in `dir`. It logs each call to `calls`, keeps the
/// file it is asked to transcribe as `heard.wav`, prints `started` for
/// `transcribe`, and for `read` prints `previous` twice, then `result`.
fn fake_superwhisper(dir: &Path, started: &str, result: &str) -> PathBuf {
    let d = dir.display();
    std::fs::write(dir.join("started"), started).unwrap();
    std::fs::write(dir.join("result"), result).unwrap();
    std::fs::write(
        dir.join("previous"),
        r#"{"session_id":6,"raw":"an older dictation","text":"An older dictation."}"#,
    )
    .unwrap();
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> '{d}/calls'\n\
         case \"$1\" in\n\
         transcribe) cp \"$2\" '{d}/heard.wav'; cat '{d}/started' ;;\n\
         read) n=$(cat '{d}/reads' 2>/dev/null || echo 0); n=$((n + 1)); echo $n > '{d}/reads'\n\
           if [ $n -le 2 ]; then cat '{d}/previous'; else cat '{d}/result'; fi ;;\n\
         *) exit 1 ;;\n\
         esac\n"
    );
    let program = dir.join("superwhisper");
    // Written from a child process, so no fd of ours leaks into it.
    let written = Command::new("/bin/sh")
        .args([
            "-c",
            "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
            "sh",
        ])
        .arg(script)
        .arg(&program)
        .status()
        .unwrap();
    assert!(written.success());
    program
}

const STARTED: &str = r#"{"ok":true,"session_id":7}"#;
const RESULT: &str =
    r#"{"session_id":7,"raw":" what time is it ","text":"What time is it?","delivery":"none"}"#;

fn calls(dir: &Path) -> Vec<String> {
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    calls.lines().map(str::to_owned).collect()
}

/// Superwhisper for an utterance in `dir`, with its fake CLI there too.
fn superwhisper(dir: &Path, started: &str, result: &str) -> Superwhisper {
    let program = fake_superwhisper(dir, started, result);
    Superwhisper::new(program, dir.to_owned())
}

fn second_of_speech() -> SpeechSegment {
    SpeechSegment {
        samples: vec![0.25; 16_000],
    }
}

#[test]
fn an_utterance_is_written_as_16_khz_mono_and_its_own_result_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let stt = superwhisper(dir.path(), STARTED, RESULT);

    let text = stt.transcribe(second_of_speech()).unwrap();

    // The raw transcript, not the cleanup model's rewrite, and not the older
    // dictation `read` showed first.
    assert_eq!(text, "what time is it");
    let wav = hound::WavReader::open(dir.path().join("heard.wav")).unwrap();
    assert_eq!(wav.spec().channels, 1);
    assert_eq!(wav.spec().sample_rate, 16_000);
    assert_eq!(wav.duration(), 16_000);
    let calls = calls(dir.path());
    assert!(calls[0].starts_with("transcribe "), "{calls:?}");
    assert!(calls[1..].iter().all(|call| call == "read"), "{calls:?}");
    assert_eq!(calls.len(), 4, "{calls:?}");
}

#[test]
fn the_utterance_file_is_removed_once_transcribed() {
    let dir = tempfile::tempdir().unwrap();
    let stt = superwhisper(dir.path(), STARTED, RESULT);

    stt.transcribe(second_of_speech()).unwrap();

    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("superwhisper-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_refused_utterance_is_an_stt_failure() {
    let dir = tempfile::tempdir().unwrap();
    let stt = superwhisper(dir.path(), r#"{"ok":false,"error":"busy"}"#, RESULT);

    let err = stt.transcribe(second_of_speech()).unwrap_err();

    assert!(err.0.contains("busy"), "{err:?}");
}

#[test]
fn a_failed_transcription_is_an_stt_failure() {
    let dir = tempfile::tempdir().unwrap();
    let failed = r#"{"session_id":7,"error":"audio must be mono 16kHz, at least 0.1 seconds"}"#;
    let stt = superwhisper(dir.path(), STARTED, failed);

    let err = stt.transcribe(second_of_speech()).unwrap_err();

    assert!(err.0.contains("mono 16kHz"), "{err:?}");
}

#[test]
fn superwhisper_missing_is_an_stt_failure() {
    let dir = tempfile::tempdir().unwrap();
    let stt = Superwhisper::new(dir.path().join("superwhisper"), dir.path().to_owned());

    let err = stt.transcribe(second_of_speech()).unwrap_err();

    assert!(err.0.contains("can't run"), "{err:?}");
}

#[test]
fn chosen_in_config_superwhisper_hears_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let lucyd =
        Harness::with_superwhisper(CHOSEN, Arc::new(superwhisper(dir.path(), STARTED, RESULT)));
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    // What the fake Whisper would have heard; Superwhisper hears it instead.
    lucyd.say("not this");

    assert_eq!(lucyd.thinking.next_turn(), "what time is it");
    assert!(
        calls(dir.path())
            .iter()
            .all(|call| call.starts_with("transcribe ") || call == "read"),
        "never record, start or stop"
    );
}

#[test]
fn unchosen_superwhisper_is_never_run() {
    let dir = tempfile::tempdir().unwrap();
    let lucyd = Harness::with_superwhisper("", Arc::new(superwhisper(dir.path(), STARTED, RESULT)));
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    lucyd.say("hello");

    assert_eq!(lucyd.thinking.next_turn(), "hello");
    assert!(calls(dir.path()).is_empty());
}

#[test]
fn a_superwhisper_failure_takes_the_stt_error_path() {
    let dir = tempfile::tempdir().unwrap();
    let stt = Superwhisper::new(dir.path().join("superwhisper"), dir.path().to_owned());
    let lucyd = Harness::with_superwhisper(CHOSEN, Arc::new(stt));
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    lucyd.say("hello");

    assert_eq!(lucyd.tts.next_sentence(), "Sorry, I didn't catch that.");
    assert_eq!(next_state(&mut events).state, State::Error);
    lucyd.thinking.assert_no_turn();
}

#[test]
fn superwhisper_hears_a_session_on_codex_and_its_reply_is_spoken() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let config = format!(
        "{CHOSEN}\n[providers.chatgpt]\nkind = \"codex\"\nmodel = \"gpt-6-luna\"\n\n\
         [tiers]\nfast = \"chatgpt\"\nadvanced = \"chatgpt\"\n"
    );
    let fake_codex = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/fake-codex");
    let lucyd = Harness::with_superwhisper_and_thinking(
        &config,
        Arc::new(superwhisper(dir.path(), STARTED, RESULT)),
        Arc::new(Codex::new(fake_codex, work.path().to_owned())),
    );
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    lucyd.say("not this");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Got it, I'll remember the word kiwi."
    );
    let sent = std::fs::read_to_string(work.path().join("stdin")).unwrap();
    let turn: serde_json::Value = sent
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|message| message["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["input"][0]["text"], "what time is it");
}
