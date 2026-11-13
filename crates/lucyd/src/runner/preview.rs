//! The Voice preview: Lucy says a sentence through her real TTS path,
//! outside any Session. It publishes no state and no levels, and a Session
//! that opens cuts it short.

use tokio::sync::{mpsc, oneshot};

use super::{Runner, SessionId, StartError};
use crate::adapters::{Sound, SpeakerEvent};

/// A `test.voice_preview` request.
pub struct VoicePreview {
    pub text: String,
    /// The voice to say it in, if not the running one.
    pub voice: Option<String>,
    /// Answered once playback has finished.
    pub reply: oneshot::Sender<Result<(), PreviewError>>,
}

pub enum PreviewError {
    SessionOpen,
    /// Another Voice preview is playing.
    Busy,
    /// A Session opened and cut it short.
    Interrupted,
    /// It couldn't be spoken; the text says why.
    Failed(String),
}

/// How a Voice preview ended.
enum Ending {
    Spoken(Result<(), PreviewError>),
    /// A Session is opening: by `session.start`, or by the Wake word (None).
    Interrupted(Option<oneshot::Sender<Result<SessionId, StartError>>>),
    /// lucyd is stopping.
    Stopped,
}

impl Runner {
    /// Says `preview` on the configured speaker between the Start and
    /// Closing sounds. If a Session opens first, the preview stops at once
    /// and the Session runs.
    pub(super) async fn preview(&mut self, preview: VoicePreview) {
        let settings = lucy_config::load(&self.files.config_path).unwrap_or_default();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let voice = preview.voice.as_ref().unwrap_or(&settings.piper.voice);
        let mut speaker = self
            .adapters
            .tts
            .open(&settings.audio.speaker, voice, events_tx);
        speaker.sound(Sound::Start);
        speaker.speak(preview.text);
        let ending = loop {
            tokio::select! {
                biased;
                Some(event) = events.recv() => match event {
                    SpeakerEvent::Played => break Ending::Spoken(Ok(())),
                    SpeakerEvent::Failed { reason, .. } => {
                        break Ending::Spoken(Err(PreviewError::Failed(reason)))
                    }
                    SpeakerEvent::Level(_) => {}
                    SpeakerEvent::DeviceLost => {
                        let lost = "the speaker went away".to_owned();
                        break Ending::Spoken(Err(PreviewError::Failed(lost)));
                    }
                },
                command = self.commands.recv() => match command {
                    Some(command) => {
                        if let Some(started) = self.while_busy(command) {
                            break Ending::Interrupted(Some(started));
                        }
                    }
                    None => break Ending::Stopped,
                },
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => break Ending::Interrupted(None),
                    None => self.wake_open = false,
                },
                // Speech outside a Session is ignored.
                speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = speech.is_some();
                }
            }
        };
        match ending {
            Ending::Spoken(outcome) => {
                // Queued only now: a Speaker dropped with a cue last plays
                // everything out, and an interrupted preview stops at once.
                speaker.sound(Sound::Closing);
                let _ = preview.reply.send(outcome);
            }
            Ending::Interrupted(started) => {
                drop(speaker);
                let _ = preview.reply.send(Err(PreviewError::Interrupted));
                self.run_session(started).await;
            }
            Ending::Stopped => {}
        }
    }
}
