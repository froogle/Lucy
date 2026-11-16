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

    pub fn judge(
        &mut self,
        probability: f32,
        len: usize,
        erle_db: Option<f32>,
        echo_heard: Option<bool>,
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
        let (threshold, sustain) = if self.converged(erle_db, echo_heard, own_canceller) {
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

    pub fn converged(
        &self,
        erle_db: Option<f32>,
        echo_heard: Option<bool>,
        own_canceller: bool,
    ) -> bool {
        let nothing_to_cancel = echo_heard == Some(false);
        self.warmed >= ms_to_samples(self.config.warm_up_ms)
            && (!own_canceller
                || erle_db.is_some_and(|erle| erle >= self.config.converged_erle_db)
                || nothing_to_cancel)
    }

    pub fn on(&self) -> bool {
        self.on
    }

    fn unduck(&mut self) -> Option<BargeInEvent> {
        self.speech_run = 0;
        std::mem::replace(&mut self.ducked, false).then_some(BargeInEvent::Unduck)
    }
}
