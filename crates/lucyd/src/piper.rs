//! The Piper TTS Provider (ADR 0003). Piper is GPL-3.0, so Lucy never links
//! it: she runs the user-installed upstream `piper.http_server`, one per
//! daemon, and sends it text sentence by sentence over localhost.
//!
//! A supervisor thread finds a Python that can import `piper`, downloads the
//! voice with its model card on first run, then keeps the server running on a
//! free port bound to 127.0.0.1, restarting it whenever it exits.

use std::io::Read;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::adapters::{Speaker, SpeakerEvent, Tts};
use crate::playback::{self, Pcm, Reference};

/// The default voice: its whole training chain is CC BY 4.0.
pub const DEFAULT_VOICE: &str = "en_GB-cori-medium";

/// How long a sentence waits for the server to come up before it fails.
const START_WAIT: Duration = Duration::from_secs(30);
/// How long the server gets to load the voice and start listening.
const READY_WAIT: Duration = Duration::from_secs(60);
/// The pause before restarting a server that exited.
const RESTART_DELAY: Duration = Duration::from_secs(1);
/// Seconds of 16 kHz reference audio kept for the echo canceller.
const REFERENCE_SECS: usize = 2;

pub struct Piper {
    shared: Arc<Shared>,
    reference: Reference,
    reference_out: Mutex<Option<rtrb::Consumer<f32>>>,
}

struct Shared {
    server: Mutex<Server>,
    changed: Condvar,
    stopping: AtomicBool,
    /// Where voices are kept; the server finds other voices here.
    voices_dir: PathBuf,
    /// The interpreter that runs Piper, once found.
    python: Mutex<Option<PathBuf>>,
}

#[derive(Clone)]
enum Server {
    Starting,
    Ready {
        port: u16,
        sample_rate: u32,
    },
    /// Piper can't run at all, for this reason.
    Unavailable(String),
}

impl Piper {
    /// Starts supervising Piper with `voice`, kept in `voices_dir`. `python`
    /// is the interpreter to use, or None to find one that can import Piper.
    pub fn start(python: Option<PathBuf>, voices_dir: PathBuf, voice: &str) -> Piper {
        let shared = Arc::new(Shared {
            server: Mutex::new(Server::Starting),
            changed: Condvar::new(),
            stopping: AtomicBool::new(false),
            voices_dir,
            python: Mutex::new(None),
        });
        let supervisor = shared.clone();
        let voice = voice.to_owned();
        std::thread::spawn(move || supervise(&supervisor, python, &voice));
        let (reference, reference_out) =
            rtrb::RingBuffer::new(playback::REFERENCE_RATE as usize * REFERENCE_SECS);
        Piper {
            shared,
            reference: Arc::new(Mutex::new(reference)),
            reference_out: Mutex::new(Some(reference_out)),
        }
    }

    /// The voice's native sample rate, once the voice is ready.
    pub fn sample_rate(&self) -> Option<u32> {
        match *self.shared.server.lock().unwrap() {
            Server::Ready { sample_rate, .. } => Some(sample_rate),
            _ => None,
        }
    }

    /// What Lucy actually played, at 16 kHz: the echo canceller's render
    /// reference. Can be taken once.
    pub fn take_reference(&self) -> Option<rtrb::Consumer<f32>> {
        self.reference_out.lock().unwrap().take()
    }

    /// Synthesises one sentence.
    pub fn synthesise(&self, text: &str) -> Result<Pcm, String> {
        self.shared.synthesise(text, None)
    }
}

impl Drop for Piper {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
    }
}

impl Tts for Piper {
    fn open(&self, speaker: &str, events: mpsc::UnboundedSender<SpeakerEvent>) -> Box<dyn Speaker> {
        let shared = self.shared.clone();
        playback::speaker(
            Box::new(move |text| shared.synthesise(text, None)),
            speaker,
            events,
            self.reference.clone(),
        )
    }

    fn open_in_voice(
        &self,
        speaker: &str,
        voice: &str,
        events: mpsc::UnboundedSender<SpeakerEvent>,
    ) -> Box<dyn Speaker> {
        let shared = self.shared.clone();
        let voice = voice.to_owned();
        playback::speaker(
            Box::new(move |text| shared.synthesise(text, Some(&voice))),
            speaker,
            events,
            self.reference.clone(),
        )
    }
}

impl Shared {
    fn set(&self, server: Server) {
        *self.server.lock().unwrap() = server;
        self.changed.notify_all();
    }

    /// Synthesises `text` in `voice`, fetched first if need be, or in the
    /// running voice.
    fn synthesise(&self, text: &str, voice: Option<&str>) -> Result<Pcm, String> {
        let server = self.server.lock().unwrap();
        let (server, _) = self
            .changed
            .wait_timeout_while(server, START_WAIT, |s| matches!(s, Server::Starting))
            .unwrap();
        let port = match server.clone() {
            Server::Ready { port, .. } => port,
            Server::Starting => return Err("Piper is still starting".into()),
            Server::Unavailable(reason) => return Err(reason),
        };
        drop(server);
        let mut request = serde_json::json!({ "text": text });
        if let Some(voice) = voice {
            let python = self.python.lock().unwrap().clone();
            let python = python.ok_or("Piper isn't installed")?;
            ensure_voice(&python, &self.voices_dir, voice)?;
            request["voice"] = voice.into();
        }
        let wav = ureq::post(format!("http://127.0.0.1:{port}/synthesize"))
            .header("Content-Type", "application/json")
            .send(request.to_string())
            .map_err(|e| format!("Piper: {e}"))?
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("Piper: {e}"))?;
        decode_wav(&wav)
    }
}

/// Runs until lucyd stops: prepares Piper, then keeps its server running.
fn supervise(shared: &Shared, python: Option<PathBuf>, voice: &str) {
    let voices_dir = &shared.voices_dir;
    let prepared = python
        .or_else(|| find_python(&python_candidates()))
        .ok_or_else(|| "Piper isn't installed, so replies are shown as text".to_owned())
        .and_then(|python| {
            let model = ensure_voice(&python, voices_dir, voice)?;
            let sample_rate = voice_sample_rate(&model)?;
            Ok((python, model, sample_rate))
        });
    let (python, model, sample_rate) = match prepared {
        Ok(prepared) => {
            *shared.python.lock().unwrap() = Some(prepared.0.clone());
            prepared
        }
        Err(reason) => {
            eprintln!("lucyd: {reason}");
            shared.set(Server::Unavailable(reason));
            return;
        }
    };
    while !shared.stopping.load(Ordering::SeqCst) {
        shared.set(Server::Starting);
        match run_server(shared, &python, &model, sample_rate) {
            Ok(()) => return,
            Err(e) => eprintln!("lucyd: Piper server {e}; restarting it"),
        }
        std::thread::sleep(RESTART_DELAY);
    }
}

/// Starts one server and watches it. Ok once lucyd is stopping; Err when the
/// server failed or exited.
fn run_server(
    shared: &Shared,
    python: &Path,
    model: &Path,
    sample_rate: u32,
) -> Result<(), String> {
    let port = free_port().map_err(|e| format!("found no free port: {e}"))?;
    let mut command = server_command(python, model, port);
    // SAFETY: prctl is async-signal-safe and touches no memory of ours.
    // The server dies with this thread, which outlives it unless lucyd exits.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start: {e}"))?;
    let watched = watch(shared, &mut child, port, sample_rate);
    let _ = child.kill();
    let _ = child.wait();
    watched
}

fn watch(shared: &Shared, child: &mut Child, port: u16, sample_rate: u32) -> Result<(), String> {
    let started = Instant::now();
    let mut ready = false;
    loop {
        if shared.stopping.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("exited ({status})"));
        }
        if !ready {
            if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
                ready = true;
                shared.set(Server::Ready { port, sample_rate });
            } else if started.elapsed() > READY_WAIT {
                return Err("never started listening".into());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `python -m piper.http_server` on `port`. Piper defaults to 0.0.0.0, so the
/// host is always given. A `PYTHONPATH` meant for another Python can break
/// Piper's imports, so it is cleared.
fn server_command(python: &Path, model: &Path, port: u16) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-m", "piper.http_server", "--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .arg("-m")
        .arg(model)
        .env_remove("PYTHONPATH");
    if let Some(dir) = model.parent() {
        command.current_dir(dir);
    }
    command
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

/// Interpreters that might have Piper: the one behind a `piper` script on
/// `PATH` (as `uv tool` and `pipx` install it), then `python3`. A `piper`
/// binary alone proves nothing: on Arch, `extra/piper` is a mouse tool.
fn python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let script = std::env::split_paths(&path)
        .map(|dir| dir.join("piper"))
        .find(|piper| piper.is_file());
    if let Some(interpreter) = script.and_then(|script| shebang(&script)) {
        candidates.push(interpreter);
    }
    candidates.push("python3".into());
    candidates
}

/// The interpreter named on a script's `#!` line.
fn shebang(script: &Path) -> Option<PathBuf> {
    let mut head = [0; 256];
    let n = std::fs::File::open(script).ok()?.read(&mut head).ok()?;
    let line = std::str::from_utf8(&head[..n]).ok()?.lines().next()?;
    let interpreter = line.strip_prefix("#!")?.split_whitespace().next()?;
    Some(interpreter.into())
}

/// The first interpreter that can import the `piper` module.
fn find_python(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|python| imports_piper(python))
        .cloned()
}

fn imports_piper(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import piper"])
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Downloads `voice` and its model card into `dir` unless they are already
/// there, and returns the model's path. Piper's own downloader fetches the
/// model; the model card, which its licence asks to be kept, comes separately.
fn ensure_voice(python: &Path, dir: &Path, voice: &str) -> Result<PathBuf, String> {
    let plain = voice
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !plain || model_card_url(voice).is_none() {
        return Err(format!("{voice:?} isn't a Piper voice name"));
    }
    let model = dir.join(format!("{voice}.onnx"));
    let card = dir.join(format!("{voice}.MODEL_CARD"));
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let config = model.with_extension("onnx.json");
    if !model.is_file() || !config.is_file() {
        eprintln!("lucyd: downloading the Piper voice {voice}");
        run_python(
            python,
            &["-m", "piper.download_voices", "--data-dir"],
            &[dir.as_os_str(), voice.as_ref()],
        )?;
    }
    if !card.is_file() {
        let url = model_card_url(voice).ok_or(format!("{voice:?} isn't a Piper voice name"))?;
        let part = card.with_extension("MODEL_CARD.part");
        run_python(
            python,
            &[
                "-c",
                "import sys, urllib.request; urllib.request.urlretrieve(sys.argv[1], sys.argv[2])",
            ],
            &[url.as_ref(), part.as_os_str()],
        )?;
        std::fs::rename(&part, &card).map_err(|e| e.to_string())?;
    }
    Ok(model)
}

fn run_python(python: &Path, args: &[&str], more: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(python)
        .args(args)
        .args(more)
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run {}: {e}", python.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().unwrap_or("no output");
    Err(format!("downloading the voice failed: {last}"))
}

/// Where `piper-voices` keeps a voice's model card, from a name like
/// `en_GB-cori-medium`.
fn model_card_url(voice: &str) -> Option<String> {
    let mut parts = voice.splitn(3, '-');
    let (lang, name, quality) = (parts.next()?, parts.next()?, parts.next()?);
    let family = lang.split_once('_')?.0;
    Some(format!(
        "https://huggingface.co/rhasspy/piper-voices/resolve/main/\
         {family}/{lang}/{name}/{quality}/MODEL_CARD"
    ))
}

/// The voice's native rate, from the config beside the model.
fn voice_sample_rate(model: &Path) -> Result<u32, String> {
    let config = model.with_extension("onnx.json");
    let text = std::fs::read_to_string(&config)
        .map_err(|e| format!("can't read {}: {e}", config.display()))?;
    sample_rate_of(&text).ok_or(format!("{} has no audio.sample_rate", config.display()))
}

fn sample_rate_of(config: &str) -> Option<u32> {
    let config: serde_json::Value = serde_json::from_str(config).ok()?;
    config["audio"]["sample_rate"].as_u64()?.try_into().ok()
}

/// Piper's reply: a 16-bit mono WAV.
fn decode_wav(wav: &[u8]) -> Result<Pcm, String> {
    let reader = hound::WavReader::new(wav).map_err(|e| format!("Piper sent a bad WAV: {e}"))?;
    let sample_rate = reader.spec().sample_rate;
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|s| f32::from(s) / 32768.0))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Piper sent a bad WAV: {e}"))?;
    Ok(Pcm {
        sample_rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_binds_localhost_with_pythonpath_cleared_and_the_voice_preloaded() {
        let command = server_command(
            Path::new("/venv/bin/python"),
            Path::new("/voices/en_GB-cori-medium.onnx"),
            5123,
        );

        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "-m",
                "piper.http_server",
                "--host",
                "127.0.0.1",
                "--port",
                "5123",
                "-m",
                "/voices/en_GB-cori-medium.onnx"
            ]
        );
        let pythonpath = command.get_envs().find(|(name, _)| *name == "PYTHONPATH");
        assert_eq!(pythonpath, Some(("PYTHONPATH".as_ref(), None)));
    }

    /// A shell script running `body`, written by a child process so that
    /// no write fd here can leak into a child another test forks and make
    /// running it fail with "Text file busy".
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let written = Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
                "sh",
            ])
            .arg(format!("#!/bin/sh\n{body}\n"))
            .arg(&path)
            .status()
            .unwrap();
        assert!(written.success());
        path
    }

    #[test]
    fn piper_is_the_first_python_that_can_import_its_module() {
        let dir = tempfile::tempdir().unwrap();
        let without = script(dir.path(), "without", "exit 1");
        let with = script(
            dir.path(),
            "with",
            r#"[ "$1" = -c ] && [ "$2" = "import piper" ]"#,
        );
        let missing = dir.path().join("missing");

        assert_eq!(
            find_python(&[missing, without.clone(), with.clone()]),
            Some(with)
        );
        assert_eq!(find_python(&[without]), None);
    }

    #[test]
    fn a_piper_script_names_its_interpreter_on_the_shebang_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piper");
        std::fs::write(
            &path,
            "#!/home/me/.local/share/uv/tools/piper-tts/bin/python\n",
        )
        .unwrap();

        assert_eq!(
            shebang(&path),
            Some("/home/me/.local/share/uv/tools/piper-tts/bin/python".into())
        );
    }

    #[test]
    fn the_model_card_comes_from_the_voice_folder_on_piper_voices() {
        assert_eq!(
            model_card_url("en_GB-cori-medium").as_deref(),
            Some(
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_GB/cori/medium/MODEL_CARD"
            )
        );
        assert_eq!(model_card_url("cori"), None);
    }

    #[test]
    fn the_sample_rate_comes_from_the_voice_config() {
        let config = r#"{"audio": {"sample_rate": 22050, "quality": "medium"}, "num_speakers": 1}"#;

        assert_eq!(sample_rate_of(config), Some(22050));
        assert_eq!(sample_rate_of("{}"), None);
    }

    /// The prototype's Piper venv, for the live check.
    fn live_python() -> PathBuf {
        std::env::var_os("LUCY_PIPER_PYTHON")
            .map(PathBuf::from)
            .or_else(|| find_python(&python_candidates()))
            .expect("set LUCY_PIPER_PYTHON to a Python with piper-tts installed")
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Downloads the default voice, so it needs the network and Piper:
    /// `LUCY_PIPER_PYTHON=… cargo test -p lucyd --lib piper -- --ignored`.
    #[test]
    #[ignore = "needs piper-tts and the network"]
    fn live_piper_downloads_the_voice_speaks_and_survives_a_crash() {
        let voices = tempfile::tempdir().unwrap();
        let piper = Piper::start(Some(live_python()), voices.path().to_owned(), DEFAULT_VOICE);

        let pcm = piper.synthesise("Hello there.").unwrap();
        assert_eq!(pcm.sample_rate, 22050);
        assert_eq!(piper.sample_rate(), Some(22050));
        assert!(pcm.samples.len() > 22050 / 4, "{}", pcm.samples.len());
        for file in [
            "en_GB-cori-medium.onnx",
            "en_GB-cori-medium.onnx.json",
            "en_GB-cori-medium.MODEL_CARD",
        ] {
            assert!(voices.path().join(file).is_file(), "{file} missing");
        }

        // Only reachable on loopback.
        let Server::Ready { port, .. } = piper.shared.server.lock().unwrap().clone() else {
            panic!("not ready");
        };
        let lan_ip = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
            .and_then(|s| s.local_addr())
            .map(|a| a.ip());
        if let Some(ip) = lan_ip
            .ok()
            .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        {
            assert!(TcpStream::connect((ip, port)).is_err(), "reachable on {ip}");
        }

        // Kill the server: the supervisor starts another one.
        let killed = Command::new("pkill")
            .args([
                "-f",
                &format!("piper.http_server --host 127.0.0.1 --port {port}"),
            ])
            .status()
            .unwrap();
        assert!(killed.success());
        wait_for("a new server", || {
            matches!(*piper.shared.server.lock().unwrap(),
                Server::Ready { port: new, .. } if new != port)
        });
        assert!(piper.synthesise("Still here.").is_ok());
    }

    /// Plays two sentences on the default output device; listen for them.
    #[test]
    #[ignore = "needs piper-tts, the network and speakers"]
    fn live_piper_speaks_through_the_speakers_with_levels() {
        let voices = tempfile::tempdir().unwrap();
        let piper = Piper::start(Some(live_python()), voices.path().to_owned(), DEFAULT_VOICE);
        let reference = piper.take_reference().unwrap();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let mut speaker = piper.open("default", events_tx);

        speaker.speak("Hello, I'm Lucy.".into());
        speaker.speak("This is my voice.".into());

        let (mut played, mut levels) = (0, 0);
        while played < 2 {
            match events.blocking_recv().unwrap() {
                SpeakerEvent::Played => played += 1,
                SpeakerEvent::Level(rms) => {
                    assert!((0.0..=1.0).contains(&rms));
                    levels += 1;
                }
                SpeakerEvent::Failed { sentence, reason } => {
                    panic!("{sentence:?} failed: {reason}")
                }
                SpeakerEvent::DeviceLost => panic!("the speaker went away"),
            }
        }
        // Roughly 30 levels a second over a couple of seconds of speech.
        assert!(levels > 30, "{levels} levels");
        assert!(
            reference.slots() > 16_000,
            "{} reference samples",
            reference.slots()
        );
    }
}
