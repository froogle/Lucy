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

/// What Lucy's own echo canceller says of her echo, for one chunk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Echo {
    /// Her canceller is off: the sound server cancels her echo, or nothing does.
    Elsewhere,
    Cancelled {
        erle_db: Option<f32>,
        reaches: Reaches,
    },
}

/// Whether Lucy's voice reaches the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reaches {
    Unknown,
    Yes,
    No,
}

impl From<Option<bool>> for Reaches {
    fn from(heard: Option<bool>) -> Reaches {
        match heard {
            None => Reaches::Unknown,
            Some(true) => Reaches::Yes,
            Some(false) => Reaches::No,
        }
    }
}

pub(super) struct Judged {
    pub event: Option<BargeInEvent>,
    pub converged: bool,
}

#[derive(Default)]
pub(super) struct Gate {
    config: BargeIn,
    on: bool,
    lucy_speaking: bool,
    since_stopped: Option<usize>,
    warmed: usize,
    speech_run: usize,
    ducked: bool,
    cut: bool,
}

fn ms_to_samples(ms: u64) -> usize {
    (ms * u64::from(SAMPLE_RATE) / 1000) as usize
}

impl Gate {
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

    pub fn window_open(&self) -> bool {
        self.lucy_speaking
            || self
                .since_stopped
                .is_some_and(|since| since < ms_to_samples(self.config.tail_ms))
    }

    pub fn is_cut(&self) -> bool {
        self.cut
    }

    pub fn cut_by_wake_word(&mut self) {
        self.cut = true;
        self.ducked = false;
        self.speech_run = 0;
    }

    pub fn judge(&mut self, probability: f32, len: usize, echo: Echo) -> Judged {
        if self.lucy_speaking {
            self.warmed += len;
        } else if let Some(since) = &mut self.since_stopped {
            *since += len;
        }
        let converged = self.converged(echo);
        let event = self.event(probability, len, converged);
        Judged { event, converged }
    }

    fn event(&mut self, probability: f32, len: usize, converged: bool) -> Option<BargeInEvent> {
        if !self.on || self.cut || !self.window_open() {
            return self.unduck();
        }
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
        self.speech_run += len;
        if self.speech_run >= ms_to_samples(sustain) {
            self.cut = true;
            self.ducked = false;
            self.speech_run = 0;
            return Some(BargeInEvent::BargeIn);
        }
        (!std::mem::replace(&mut self.ducked, true)).then_some(BargeInEvent::Duck)
    }

    fn converged(&self, echo: Echo) -> bool {
        let cancelled = match echo {
            Echo::Elsewhere => true,
            Echo::Cancelled { erle_db, reaches } => {
                reaches == Reaches::No
                    || erle_db.is_some_and(|erle| erle >= self.config.converged_erle_db)
            }
        };
        self.warmed >= ms_to_samples(self.config.warm_up_ms) && cancelled
    }

    pub fn on(&self) -> bool {
        self.on
    }

    fn unduck(&mut self) -> Option<BargeInEvent> {
        self.speech_run = 0;
        std::mem::replace(&mut self.ducked, false).then_some(BargeInEvent::Unduck)
    }
}
