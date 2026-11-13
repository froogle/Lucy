//! Real capture through cpal, which tries PipeWire, then PulseAudio, then
//! ALSA. It asks for 16 kHz so the sound server does the resampling.

use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{AudioIo, Error, SAMPLE_RATE};

/// The sound card, through cpal.
pub struct Cpal;

impl AudioIo for Cpal {
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error> {
        // A cpal stream may not move between threads, so one thread owns it
        // until the handle (the sender) is dropped.
        let (opened_tx, opened) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let mic = mic.to_owned();
        std::thread::Builder::new()
            .name("lucyd-capture".to_owned())
            .spawn(move || match open(&mic, ring) {
                Ok(stream) => {
                    let _ = opened_tx.send(Ok(()));
                    let _ = stopped.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = opened_tx.send(Err(e));
                }
            })
            .map_err(|e| Error(format!("can't start capture: {e}")))?;
        opened
            .recv()
            .map_err(|_| Error("capture stopped while opening".to_owned()))??;
        Ok(Box::new(stop))
    }
}

fn open(mic: &str, mut ring: rtrb::Producer<f32>) -> Result<cpal::Stream, Error> {
    let host = cpal::default_host();
    let device = find(&host, mic)?;
    // Mono 16 kHz whatever the device records; the sound server converts.
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device
        .build_input_stream(
            config,
            // Only pushes into the ring; when the ring is full the rest of
            // the buffer is dropped.
            move |data: &[f32], _| {
                for &sample in data {
                    if ring.push(sample).is_err() {
                        break;
                    }
                }
            },
            |e| eprintln!("lucyd: microphone error: {e}"),
            None,
        )
        .map_err(|e| Error(format!("can't open the microphone: {e}")))?;
    stream
        .play()
        .map_err(|e| Error(format!("can't start the microphone: {e}")))?;
    Ok(stream)
}

/// The default input, or the one whose node name is pinned. A pinned
/// microphone that isn't there falls back to the default.
fn find(host: &cpal::Host, mic: &str) -> Result<cpal::Device, Error> {
    if mic != "default" {
        let pinned = host
            .input_devices()
            .map_err(|e| Error(format!("can't list microphones: {e}")))?
            .find(|device| device.id().is_ok_and(|id| id.id() == mic));
        match pinned {
            Some(device) => return Ok(device),
            None => eprintln!("lucyd: microphone {mic} not found; using the default"),
        }
    }
    host.default_input_device()
        .ok_or_else(|| Error("there is no microphone".to_owned()))
}
