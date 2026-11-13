//! Silero VAD on the pure-Rust tract runtime through the `ort` API.
//!
//! The bundled model is Silero's If-less opset 18 export
//! (`silero_vad_op18_ifless.onnx` from silero-vad v6.2.3, MIT): tract can't
//! run the `If` nodes of the standard exports. Ticket 26 checks it against
//! the standard model.

use ort::session::Session;
use ort::value::Tensor;

use super::{Error, Vad, SAMPLE_RATE};

const MODEL: &[u8] = include_bytes!("../../models/silero_vad_op18_ifless.onnx");

/// Silero judges 32 ms at a time...
const CHUNK_LEN: usize = 512;
/// ...with the end of the previous chunk in front of it.
const CONTEXT_LEN: usize = 64;
/// Its recurrent state, `[2, 1, 128]`.
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
        let sample_rate = Tensor::from_array(([0usize; 0], vec![i64::from(SAMPLE_RATE)]))?;
        let outputs = self.session.run(ort::inputs![
            "input" => input,
            "state" => state,
            "sr" => sample_rate,
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
        // A failed run hears silence; tract running a model it loaded
        // doesn't fail on well-formed input.
        self.run(chunk).unwrap_or_else(|e| {
            eprintln!("lucyd: Silero VAD failed: {e}");
            0.0
        })
    }
}
