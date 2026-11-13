//! The tests setup runs inside lucyd, outside any Session: how loud the
//! microphone is, and whether the Wake word is heard and with what score.

use std::time::Duration;

use tokio::sync::oneshot;

use super::{Command, EndError, PreviewError, Runner};

/// A `test.mic_level` or `test.wake_word` request.
pub enum SetupTest {
    /// Answered with the loudest RMS since the last one.
    MicLevel(oneshot::Sender<Result<f32, SetupTestError>>),
    WakeWord {
        timeout: Duration,
        reply: oneshot::Sender<Result<WakeWordHeard, SetupTestError>>,
    },
}

pub struct WakeWordHeard {
    pub heard: bool,
    /// The best score the engine gave during the test.
    pub score: f32,
}

pub enum SetupTestError {
    SessionOpen,
    /// A Voice preview or another setup test is running.
    Busy,
    NoMicrophone,
    /// The wake-word engine couldn't be loaded.
    WakeWordOff,
    /// A Session opened and cut the test short.
    Interrupted,
}

impl SetupTest {
    pub(super) fn refuse(self, error: SetupTestError) {
        match self {
            SetupTest::MicLevel(reply) => {
                let _ = reply.send(Err(error));
            }
            SetupTest::WakeWord { reply, .. } => {
                let _ = reply.send(Err(error));
            }
        }
    }
}

impl Runner {
    pub(super) async fn setup_test(&mut self, test: SetupTest) {
        match test {
            SetupTest::MicLevel(reply) => {
                let _ = reply.send(self.mic_level());
            }
            SetupTest::WakeWord { timeout, reply } => self.wake_word_test(timeout, reply).await,
        }
    }

    /// Switches to the microphone config names now, as a Session start
    /// would, and takes its level.
    fn mic_level(&mut self) -> Result<f32, SetupTestError> {
        let hearing = self
            .adapters
            .hearing
            .as_mut()
            .ok_or(SetupTestError::NoMicrophone)?;
        let settings = lucy_config::load(&self.files.config_path).unwrap_or_default();
        hearing.configure(&settings.audio, &settings.endpointing);
        Ok(hearing.take_level())
    }

    /// Listens for the Wake word until it is heard or `timeout` passes,
    /// without opening a Session. A Session that opens meanwhile ends the
    /// test and runs.
    async fn wake_word_test(
        &mut self,
        timeout: Duration,
        reply: oneshot::Sender<Result<WakeWordHeard, SetupTestError>>,
    ) {
        match &self.adapters.hearing {
            None => {
                let _ = reply.send(Err(SetupTestError::NoMicrophone));
                return;
            }
            Some(hearing) if !hearing.has_wake_word() => {
                let _ = reply.send(Err(SetupTestError::WakeWordOff));
                return;
            }
            Some(hearing) => hearing.take_wake_score(),
        };
        // Only what is heard from now on counts.
        while self.adapters.wake_word.try_recv().is_ok() {}
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let heard = loop {
            tokio::select! {
                biased;
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => break true,
                    None => self.wake_open = false,
                },
                () = &mut deadline => break false,
                command = self.commands.recv() => match command {
                    Some(Command::Start(started)) => {
                        let _ = reply.send(Err(SetupTestError::Interrupted));
                        self.run_session(Some(started)).await;
                        return;
                    }
                    Some(Command::End(end) | Command::EndAfterSpeech(end)) => {
                        let _ = end.send(Err(EndError));
                    }
                    // Dropped unanswered: there is no Session to ask in.
                    Some(Command::Ask(_)) => {}
                    Some(Command::VoicePreview(preview)) => {
                        let _ = preview.reply.send(Err(PreviewError::Busy));
                    }
                    Some(Command::SetupTest(other)) => other.refuse(SetupTestError::Busy),
                    None => return,
                },
                // Speech outside a Session is ignored.
                speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = speech.is_some();
                }
            }
        };
        let score = self
            .adapters
            .hearing
            .as_ref()
            .map_or(0.0, |hearing| hearing.take_wake_score());
        let _ = reply.send(Ok(WakeWordHeard { heard, score }));
    }
}
