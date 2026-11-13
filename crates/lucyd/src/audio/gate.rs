//! The Barge-in gate: whether the user, not Lucy's echo, is talking over
//! her. It judges each VAD chunk while she speaks and for a short tail
//! after, with a stricter bar while the echo canceller hasn't converged.

use lucy_config::BargeIn;

use super::SAMPLE_RATE;

/// What the gate asks of playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BargeInEvent {
    /// The user may be cutting in: turn Lucy down.
    Duck,
    /// They weren't: turn her back up.
    Unduck,
    /// They are: cut her off.
    BargeIn,
}

#[derive(Default)]
pub(super) struct Gate {
    config: BargeIn,
    /// Speech may barge in this Session.
    on: bool,
    lucy_speaking: bool,
    /// Samples since Lucy stopped speaking, until the tail is over.
    since_stopped: Option<usize>,
    /// Samples heard while Lucy spoke this Session.
    warmed: usize,
    /// Samples of the current run of speech above the threshold.
    run: usize,
    ducked: bool,
    /// The user has cut in since Lucy last started speaking.
    cut: bool,
}

/// `ms` as 16 kHz samples.
fn samples(ms: u64) -> usize {
    (ms * u64::from(SAMPLE_RATE) / 1000) as usize
}

impl Gate {
    /// Starts a Session's gate with `config`, letting speech barge in if
    /// `on`.
    pub fn configure(&mut self, config: &BargeIn, on: bool) {
        *self = Gate {
            config: config.clone(),
            on,
            ..Gate::default()
        };
    }

    pub fn lucy_speaking(&mut self, speaking: bool) {
        if speaking && !self.lucy_speaking {
            self.since_stopped = None;
            self.cut = false;
        }
        if !speaking && self.lucy_speaking {
            self.since_stopped = Some(0);
        }
        self.lucy_speaking = speaking;
    }

    /// Lucy is speaking, or stopped too recently for her echo to have died
    /// away.
    pub fn window_open(&self) -> bool {
        self.lucy_speaking
            || self
                .since_stopped
                .is_some_and(|since| since < samples(self.config.tail_ms))
    }

    /// The user has cut in, by speech or the Wake word, since Lucy last
    /// started speaking.
    pub fn is_cut(&self) -> bool {
        self.cut
    }

    /// The Wake word cut Lucy off.
    pub fn wake_word(&mut self) {
        self.cut = true;
        self.ducked = false;
        self.run = 0;
    }

    /// Judges the next VAD chunk of `len` samples, `probability` speech.
    /// `erle_db` is how much echo the echo canceller removes, if it is
    /// Lucy's own and can tell.
    pub fn judge(
        &mut self,
        probability: f32,
        len: usize,
        erle_db: Option<f32>,
        own_canceller: bool,
    ) -> Option<BargeInEvent> {
        if self.lucy_speaking {
            self.warmed += len;
        } else if let Some(since) = &mut self.since_stopped {
            *since += len;
        }
        if !self.on || self.cut || !self.window_open() {
            return self.unduck();
        }
        let converged = self.warmed >= samples(self.config.warm_up_ms)
            && (!own_canceller
                || erle_db.is_some_and(|erle| erle >= self.config.converged_erle_db));
        let (threshold, sustain) = if converged {
            (self.config.threshold, self.config.sustain_ms)
        } else {
            (
                self.config.unconverged_threshold,
                self.config.unconverged_sustain_ms,
            )
        };
        if probability < threshold {
            return self.unduck();
        }
        self.run += len;
        if self.run >= samples(sustain) {
            self.cut = true;
            self.ducked = false;
            self.run = 0;
            return Some(BargeInEvent::BargeIn);
        }
        (!std::mem::replace(&mut self.ducked, true)).then_some(BargeInEvent::Duck)
    }

    fn unduck(&mut self) -> Option<BargeInEvent> {
        self.run = 0;
        std::mem::replace(&mut self.ducked, false).then_some(BargeInEvent::Unduck)
    }
}
