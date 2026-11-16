use std::collections::VecDeque;

use lucy_config::Endpointing;

use super::{Vad, SAMPLE_RATE};
use crate::adapters::SpeechSegment;

pub struct Endpointer {
    vad: Box<dyn Vad>,
    start_threshold: f32,
    end_threshold: f32,
    start_len: usize,
    pre_roll_len: usize,
    end_silence_len: usize,
    max_len: usize,
    partial_chunk: Vec<f32>,
    pre_roll: VecDeque<f32>,
    state: State,
}

enum State {
    Quiet,
    Starting {
        audio: Vec<f32>,
    },
    Speaking {
        audio: Vec<f32>,
        trailing_silence: usize,
    },
}

impl Endpointer {
    pub fn new(vad: Box<dyn Vad>, config: &Endpointing) -> Self {
        let mut endpointer = Endpointer {
            vad,
            start_threshold: 0.0,
            end_threshold: 0.0,
            start_len: 0,
            pre_roll_len: 0,
            end_silence_len: 0,
            max_len: 0,
            partial_chunk: Vec::new(),
            pre_roll: VecDeque::new(),
            state: State::Quiet,
        };
        endpointer.configure(config);
        endpointer
    }

    pub fn configure(&mut self, config: &Endpointing) {
        let samples = |ms: u64| (ms * u64::from(SAMPLE_RATE) / 1000) as usize;
        self.start_threshold = config.start_threshold;
        self.end_threshold = config.end_threshold;
        self.start_len = samples(config.start_ms);
        self.pre_roll_len = samples(config.pre_roll_ms);
        self.end_silence_len = samples(config.end_silence_ms);
        self.max_len = (config.max_utterance_secs * f64::from(SAMPLE_RATE)) as usize;
        self.pre_roll.clear();
        self.state = State::Quiet;
    }

    pub fn speaking(&self) -> bool {
        matches!(self.state, State::Speaking { .. })
    }

    pub fn push(
        &mut self,
        frame: &[f32],
        mut on_judged: impl FnMut(f32, usize),
    ) -> Option<SpeechSegment> {
        self.partial_chunk.extend_from_slice(frame);
        let chunk_len = self.vad.chunk_len();
        let mut ended = None;
        while self.partial_chunk.len() >= chunk_len {
            let chunk: Vec<f32> = self.partial_chunk.drain(..chunk_len).collect();
            let probability = self.vad.speech_probability(&chunk);
            on_judged(probability, chunk_len);
            ended = self.judge(chunk, probability).or(ended);
        }
        ended
    }

    fn judge(&mut self, chunk: Vec<f32>, probability: f32) -> Option<SpeechSegment> {
        let speech = probability >= self.start_threshold;
        match std::mem::replace(&mut self.state, State::Quiet) {
            State::Quiet if speech => self.starting(chunk),
            State::Quiet => self.keep_as_pre_roll(&chunk),
            State::Starting { mut audio } if speech => {
                audio.extend(chunk);
                self.starting(audio);
            }
            State::Starting { audio } => {
                self.keep_as_pre_roll(&audio);
                self.keep_as_pre_roll(&chunk);
            }
            State::Speaking {
                mut audio,
                mut trailing_silence,
            } => {
                if probability < self.end_threshold {
                    trailing_silence += chunk.len();
                } else {
                    trailing_silence = 0;
                }
                audio.extend(chunk);
                if trailing_silence >= self.end_silence_len
                    || self.pre_roll.len() + audio.len() >= self.max_len
                {
                    return Some(self.end(audio));
                }
                self.state = State::Speaking {
                    audio,
                    trailing_silence,
                };
            }
        }
        None
    }

    fn starting(&mut self, audio: Vec<f32>) {
        self.state = if audio.len() >= self.start_len {
            State::Speaking {
                audio,
                trailing_silence: 0,
            }
        } else {
            State::Starting { audio }
        };
    }

    fn keep_as_pre_roll(&mut self, audio: &[f32]) {
        self.pre_roll.extend(audio);
        let excess = self.pre_roll.len().saturating_sub(self.pre_roll_len);
        self.pre_roll.drain(..excess);
    }

    fn end(&mut self, audio: Vec<f32>) -> SpeechSegment {
        let mut samples: Vec<f32> = self.pre_roll.drain(..).collect();
        samples.extend(audio);
        samples.truncate(self.max_len);
        SpeechSegment { samples }
    }
}
