use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

use super::{Error, Vad};

const MODEL: &[u8] = include_bytes!("../../models/silero_vad_16k.onnx");

const CHUNK_LEN: usize = 512;
const CONTEXT_LEN: usize = 64;
const STATE_LEN: usize = 2 * 128;

pub struct Silero {
    session: Session,
    state: Vec<f32>,
    context: Vec<f32>,
}

impl Silero {
    pub fn load() -> Result<Silero, Error> {
        ort::set_api(ort_tract::api());
        let session = Session::builder()
            .and_then(|builder| builder.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|builder| builder.commit_from_memory(MODEL))
            .map_err(|e| Error(format!("can't load the Silero VAD: {e}")))?;
        Ok(Silero {
            session,
            state: vec![0.0; STATE_LEN],
            context: vec![0.0; CONTEXT_LEN],
        })
    }

    fn run(&mut self, chunk: &[f32]) -> ort::Result<f32> {
        let mut input = Vec::with_capacity(CONTEXT_LEN + CHUNK_LEN);
        input.extend_from_slice(&self.context);
        input.extend_from_slice(chunk);
        let input = Tensor::from_array(([1, CONTEXT_LEN + CHUNK_LEN], input))?;
        let state = Tensor::from_array(([2, 1, 128], self.state.clone()))?;
        let outputs = self.session.run(ort::inputs![
            "input" => input,
            "state" => state,
        ])?;
        let (_, probability) = outputs["output"].try_extract_tensor::<f32>()?;
        let probability = probability[0];
        let (_, state) = outputs["stateN"].try_extract_tensor::<f32>()?;
        self.state.copy_from_slice(state);
        self.context
            .copy_from_slice(&chunk[CHUNK_LEN - CONTEXT_LEN..]);
        Ok(probability)
    }
}

impl Vad for Silero {
    fn chunk_len(&self) -> usize {
        CHUNK_LEN
    }

    fn speech_probability(&mut self, chunk: &[f32]) -> f32 {
        const SILENCE: f32 = 0.0;
        self.run(chunk).unwrap_or_else(|e| {
            eprintln!("lucyd: Silero VAD failed: {e}");
            SILENCE
        })
    }
}
