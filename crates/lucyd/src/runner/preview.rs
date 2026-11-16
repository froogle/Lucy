use tokio::sync::{mpsc, oneshot};

use super::{Runner, SessionId, StartError};
use crate::adapters::{Sound, SpeakerEvent};

/// A `test.voice_preview` request.
pub struct VoicePreview {
    pub text: String,
    pub voice: Option<String>,
    pub reply: oneshot::Sender<Result<(), PreviewError>>,
}

pub enum PreviewError {
    SessionOpen,
    Busy,
    Interrupted,
    Failed(String),
}

enum Ending {
    Spoken(Result<(), PreviewError>),
    Interrupted(Option<oneshot::Sender<Result<SessionId, StartError>>>),
    Stopped,
}

impl Runner {
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
                ignored_speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = ignored_speech.is_some();
                }
            }
        };
        match ending {
            Ending::Spoken(outcome) => {
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
