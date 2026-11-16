use std::time::Duration;

use tokio::sync::oneshot;

use super::Runner;

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
    pub score: f32,
}

pub enum SetupTestError {
    SessionOpen,
    Busy,
    NoMicrophone,
    WakeWordOff,
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

    fn mic_level(&mut self) -> Result<f32, SetupTestError> {
        let hearing = self
            .adapters
            .hearing
            .as_mut()
            .ok_or(SetupTestError::NoMicrophone)?;
        let settings = lucy_config::load(&self.files.config_path).unwrap_or_default();
        hearing.configure(&settings.audio, &settings.endpointing, &settings.barge_in);
        Ok(hearing.take_level())
    }

    fn forget_earlier_wake_words(&mut self) {
        while self.adapters.wake_word.try_recv().is_ok() {}
    }

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
        self.forget_earlier_wake_words();
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
                    Some(command) => {
                        if let Some(started) = self.while_busy(command) {
                            let _ = reply.send(Err(SetupTestError::Interrupted));
                            self.run_session(Some(started)).await;
                            return;
                        }
                    }
                    None => return,
                },
                ignored_speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = ignored_speech.is_some();
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
