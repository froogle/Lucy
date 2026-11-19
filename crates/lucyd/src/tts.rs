//! Lucy's TTS Providers behind one [`Tts`] (ADR 0013). Each Provider is an
//! [`Engine`] that turns a sentence into samples; playback, levels and the
//! echo canceller's reference are shared by all of them.

use std::sync::{Arc, Mutex};

use lucy_config::{TtsKind, Voice};
use tokio::sync::mpsc;

use crate::adapters::{Speaker, SpeakerEvent, Tts};
use crate::playback::{self, Pcm, Reference};
use crate::sync::Locked;

const REFERENCE_SECS: usize = 2;

/// One TTS Provider's engine: turns a sentence into samples in a voice.
pub trait Engine: Send + Sync {
    /// Starts the engine in `voice`, unless it is running or starting.
    fn start(&self, voice: &str);

    /// The samples of `text` said in `voice`.
    ///
    /// # Errors
    ///
    /// Why it couldn't be said, such as the engine not being installed.
    fn synthesise(&self, text: &str, voice: &str) -> Result<Pcm, String>;
}

/// Every TTS Provider's engine, one for each [`TtsKind`].
pub struct Engines {
    piper: Arc<dyn Engine>,
    kokoro: Arc<dyn Engine>,
    reference: Reference,
    reference_out: Mutex<Option<rtrb::Consumer<f32>>>,
}

impl Engines {
    /// Each TTS Provider's engine, none of them started yet.
    pub fn new(piper: Arc<dyn Engine>, kokoro: Arc<dyn Engine>) -> Engines {
        let (reference, reference_out) =
            rtrb::RingBuffer::new(playback::REFERENCE_RATE as usize * REFERENCE_SECS);
        Engines {
            piper,
            kokoro,
            reference: Arc::new(Mutex::new(reference)),
            reference_out: Mutex::new(Some(reference_out)),
        }
    }

    /// Starts the engine for `voice` now, so the first Session needn't wait.
    pub fn start(&self, voice: &Voice) {
        self.engine(voice.kind).start(&voice.name);
    }

    /// What Lucy actually played, at 16 kHz: the echo canceller's render
    /// reference. Can be taken once.
    pub fn take_reference(&self) -> Option<rtrb::Consumer<f32>> {
        self.reference_out.locked().take()
    }

    fn engine(&self, kind: TtsKind) -> &Arc<dyn Engine> {
        match kind {
            TtsKind::Piper => &self.piper,
            TtsKind::Kokoro => &self.kokoro,
        }
    }
}

impl Tts for Engines {
    fn open(
        &self,
        speaker: &str,
        voice: &Voice,
        events: mpsc::UnboundedSender<SpeakerEvent>,
    ) -> Box<dyn Speaker> {
        let engine = self.engine(voice.kind).clone();
        engine.start(&voice.name);
        let name = voice.name.clone();
        playback::speaker(
            Box::new(move |text| engine.synthesise(text, &name)),
            speaker,
            events,
            self.reference.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorded {
        started: Mutex<Vec<String>>,
    }

    impl Engine for Recorded {
        fn start(&self, voice: &str) {
            self.started.locked().push(voice.to_owned());
        }

        fn synthesise(&self, _text: &str, _voice: &str) -> Result<Pcm, String> {
            Err("not under test".into())
        }
    }

    fn voice(kind: TtsKind, name: &str) -> Voice {
        Voice {
            kind,
            name: name.to_owned(),
        }
    }

    #[test]
    fn each_voice_starts_only_its_own_providers_engine() {
        let (piper, kokoro) = (Arc::new(Recorded::default()), Arc::new(Recorded::default()));
        let engines = Engines::new(piper.clone(), kokoro.clone());

        engines.start(&voice(TtsKind::Kokoro, "bf_emma"));
        let (events, _) = mpsc::unbounded_channel();
        drop(engines.open("default", &voice(TtsKind::Kokoro, "bm_george"), events));

        assert_eq!(*kokoro.started.locked(), ["bf_emma", "bm_george"]);
        assert!(piper.started.locked().is_empty());

        let (events, _) = mpsc::unbounded_channel();
        drop(engines.open(
            "default",
            &voice(TtsKind::Piper, "en_GB-cori-medium"),
            events,
        ));

        assert_eq!(*piper.started.locked(), ["en_GB-cori-medium"]);
    }
}

#[cfg(test)]
pub(crate) mod live {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    use super::*;

    pub(crate) fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub(crate) fn only_loopback_reaches(port: u16) {
        let lan_ip = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
            .and_then(|s| s.local_addr())
            .map(|a| a.ip());
        if let Some(ip) = lan_ip
            .ok()
            .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        {
            assert!(
                TcpStream::connect((ip, port)).is_err(),
                "only loopback should reach it, but {ip} does"
            );
        }
    }

    pub(crate) fn speaks_with_levels(engine: Arc<dyn Engine>, voice: Voice) {
        let other: Arc<dyn Engine> = Arc::new(Unused);
        let engines = match voice.kind {
            TtsKind::Piper => Engines::new(engine, other),
            TtsKind::Kokoro => Engines::new(other, engine),
        };
        let reference = engines.take_reference().unwrap();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let mut speaker = engines.open("default", &voice, events_tx);

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
        assert!(
            levels > 30,
            "{levels} levels, not roughly 30 a second over a couple of seconds of speech"
        );
        assert!(
            reference.slots() > 16_000,
            "{} reference samples",
            reference.slots()
        );
    }

    struct Unused;

    impl Engine for Unused {
        fn start(&self, _voice: &str) {}

        fn synthesise(&self, _text: &str, _voice: &str) -> Result<Pcm, String> {
            panic!("the other Provider spoke")
        }
    }
}
