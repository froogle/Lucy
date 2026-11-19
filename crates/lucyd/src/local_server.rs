//! A TTS engine that runs as a supervised server on localhost (ADR 0003,
//! ADR 0013). The engine supplies a [`Server`]: how to find it, start it and
//! fetch a voice. This module does the rest once for every such engine:
//! starting it on first use, restarting it whenever it exits, and sending it
//! one sentence at a time.
//!
//! The protocol is Piper's: `POST /synthesize` with `{"text": .., "voice": ..}`
//! answers a 16-bit WAV. `voice` is sent only when it isn't the voice the
//! server started in.

use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::playback::Pcm;
use crate::sync::{Locked, POISONED};
use crate::tts::Engine;

const FIND_WAIT: Duration = Duration::from_secs(600);
const START_WAIT: Duration = Duration::from_secs(30);
const READY_WAIT: Duration = Duration::from_secs(60);
const RESTART_DELAY: Duration = Duration::from_secs(1);

/// What one engine needs to run as a [`LocalServer`].
pub trait Server: Send + Sync + 'static {
    /// What finding the engine yields, such as its interpreter.
    type Found: Send + Sync + 'static;

    /// The engine's name, as Lucy speaks of it.
    const NAME: &'static str;

    /// Finds the engine and fetches `voice` if it must.
    ///
    /// # Errors
    ///
    /// Why the engine can't speak, ready to log and show.
    fn find(&self, voice: &str) -> Result<Self::Found, String>;

    /// The command that serves `voice` on `127.0.0.1:port`.
    fn command(&self, found: &Self::Found, voice: &str, port: u16) -> Command;

    /// Fetches `voice` before the first sentence in it, if it must.
    ///
    /// # Errors
    ///
    /// Why the voice can't be used.
    fn fetch(&self, found: &Self::Found, voice: &str) -> Result<(), String>;
}

/// A [`Server`] run and supervised by lucyd, started on first use.
pub struct LocalServer<S: Server> {
    shared: Arc<Shared<S>>,
}

struct Shared<S: Server> {
    server: S,
    state: Mutex<State>,
    changed: Condvar,
    stopping: AtomicBool,
    started: Mutex<Option<Started<S::Found>>>,
}

struct Started<F> {
    found: Arc<F>,
    voice: String,
}

#[derive(Clone)]
enum State {
    Idle,
    Finding,
    Starting,
    Ready { port: u16 },
    Unavailable(String),
}

impl<S: Server> LocalServer<S> {
    /// Supervises `server`, which starts at the first `Engine::start`.
    pub fn new(server: S) -> LocalServer<S> {
        LocalServer {
            shared: Arc::new(Shared {
                server,
                state: Mutex::new(State::Idle),
                changed: Condvar::new(),
                stopping: AtomicBool::new(false),
                started: Mutex::new(None),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn port(&self) -> Option<u16> {
        match *self.shared.state.locked() {
            State::Ready { port } => Some(port),
            _ => None,
        }
    }
}

impl<S: Server> Drop for LocalServer<S> {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
    }
}

impl<S: Server> Engine for LocalServer<S> {
    fn start(&self, voice: &str) {
        let mut state = self.shared.state.locked();
        if matches!(*state, State::Idle | State::Unavailable(_)) {
            *state = State::Finding;
            let shared = self.shared.clone();
            let voice = voice.to_owned();
            std::thread::spawn(move || supervise(&shared, &voice));
        }
    }

    fn synthesise(&self, text: &str, voice: &str) -> Result<Pcm, String> {
        self.shared.synthesise(text, voice)
    }
}

impl<S: Server> Shared<S> {
    fn set(&self, state: State) {
        *self.state.locked() = state;
        self.changed.notify_all();
    }

    fn synthesise(&self, text: &str, voice: &str) -> Result<Pcm, String> {
        let name = S::NAME;
        let state = self.state.locked();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, FIND_WAIT, |s| matches!(s, State::Finding))
            .expect(POISONED);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, START_WAIT, |s| matches!(s, State::Starting))
            .expect(POISONED);
        let port = match state.clone() {
            State::Ready { port } => port,
            State::Idle | State::Finding | State::Starting => {
                return Err(format!("{name} is still starting"))
            }
            State::Unavailable(reason) => return Err(reason),
        };
        drop(state);
        let mut request = serde_json::json!({ "text": text });
        if let Some(found) = self.switching_to(voice) {
            self.server.fetch(&found, voice)?;
            request["voice"] = voice.into();
        }
        let wav = ureq::post(format!("http://127.0.0.1:{port}/synthesize"))
            .header("Content-Type", "application/json")
            .send(request.to_string())
            .map_err(|e| format!("{name}: {e}"))?
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("{name}: {e}"))?;
        decode_wav(name, &wav)
    }

    fn switching_to(&self, voice: &str) -> Option<Arc<S::Found>> {
        let started = self.started.locked();
        let started = started.as_ref()?;
        (started.voice != voice).then(|| started.found.clone())
    }
}

fn supervise<S: Server>(shared: &Shared<S>, voice: &str) {
    let found = match shared.server.find(voice) {
        Ok(found) => Arc::new(found),
        Err(reason) => {
            eprintln!("lucyd: {reason}");
            shared.set(State::Unavailable(reason));
            return;
        }
    };
    *shared.started.locked() = Some(Started {
        found: found.clone(),
        voice: voice.to_owned(),
    });
    while !shared.stopping.load(Ordering::SeqCst) {
        shared.set(State::Starting);
        match run(shared, &found, voice) {
            Ok(()) => return,
            Err(e) => eprintln!("lucyd: {} server {e}; restarting it", S::NAME),
        }
        std::thread::sleep(RESTART_DELAY);
    }
}

fn run<S: Server>(shared: &Shared<S>, found: &S::Found, voice: &str) -> Result<(), String> {
    let port = free_port().map_err(|e| format!("found no free port: {e}"))?;
    let mut command = shared.server.command(found, voice, port);
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
    let watched = watch(shared, &mut child, port);
    let _ = child.kill();
    let _ = child.wait();
    watched
}

fn watch<S: Server>(shared: &Shared<S>, child: &mut Child, port: u16) -> Result<(), String> {
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
                shared.set(State::Ready { port });
            } else if started.elapsed() > READY_WAIT {
                return Err("never started listening".into());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

fn decode_wav(name: &str, wav: &[u8]) -> Result<Pcm, String> {
    let bad = |e| format!("{name} sent a bad WAV: {e}");
    let reader = hound::WavReader::new(wav).map_err(bad)?;
    let sample_rate = reader.spec().sample_rate;
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|s| f32::from(s) / 32768.0))
        .collect::<Result<_, _>>()
        .map_err(bad)?;
    Ok(Pcm {
        sample_rate,
        samples,
    })
}
