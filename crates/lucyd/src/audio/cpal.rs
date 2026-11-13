//! Real capture through cpal, which tries PipeWire, then PulseAudio, then
//! ALSA. It asks for 16 kHz so the sound server does the resampling.

use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{AudioIo, Direction, Error, SAMPLE_RATE};

/// The sound card, through cpal.
pub struct Cpal;

impl AudioIo for Cpal {
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error> {
        // A cpal stream may not move between threads, so one thread owns it
        // until the handle is dropped or the device goes away. Dropping the
        // stream drops the ring.
        let (opened_tx, opened) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let lost = stop.clone();
        let mic = mic.to_owned();
        std::thread::Builder::new()
            .name("lucyd-capture".to_owned())
            .spawn(move || match open(&mic, ring, lost) {
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
        Ok(Box::new(StopOnDrop(stop)))
    }

    fn has_device(&self, direction: Direction, name: &str) -> bool {
        let host = cpal::default_host();
        let devices = match direction {
            Direction::Input => host.input_devices(),
            Direction::Output => host.output_devices(),
        };
        devices.is_ok_and(|mut devices| devices.any(|device| is_named(&device, name)))
    }
}

/// Stops capture when dropped.
struct StopOnDrop(mpsc::Sender<()>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// Whether a stream error means its device has gone away.
pub(crate) fn is_device_lost(e: &cpal::Error) -> bool {
    matches!(
        e.kind(),
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable
    )
}

fn is_named(device: &cpal::Device, name: &str) -> bool {
    device.id().is_ok_and(|id| id.id() == name)
}

/// The default output, or the one whose node name is pinned. A pinned
/// speaker that isn't there falls back to the default.
pub(crate) fn find_output(name: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if name != "default" {
        let pinned = host
            .output_devices()
            .map_err(|e| format!("can't list speakers: {e}"))?
            .find(|device| is_named(device, name));
        match pinned {
            Some(device) => return Ok(device),
            None => eprintln!("lucyd: speaker {name} not found; using the default"),
        }
    }
    host.default_output_device()
        .ok_or_else(|| "there is no speaker".to_owned())
}

/// `lost` hears once the device goes away.
fn open(
    mic: &str,
    mut ring: rtrb::Producer<f32>,
    lost: mpsc::Sender<()>,
) -> Result<cpal::Stream, Error> {
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
            move |e| {
                eprintln!("lucyd: microphone error: {e}");
                if is_device_lost(&e) {
                    let _ = lost.send(());
                }
            },
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
            .find(|device| is_named(device, mic));
        match pinned {
            Some(device) => return Ok(device),
            None => eprintln!("lucyd: microphone {mic} not found; using the default"),
        }
    }
    host.default_input_device()
        .ok_or_else(|| Error("there is no microphone".to_owned()))
}
