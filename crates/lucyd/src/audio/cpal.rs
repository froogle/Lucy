use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{AudioIo, Direction, Error, DEFAULT, SAMPLE_RATE};

/// The sound card, through cpal.
pub struct Cpal;

impl AudioIo for Cpal {
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error> {
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

    fn is_pipewire(&self) -> bool {
        cpal::default_host().id() == cpal::HostId::PipeWire
    }
}

struct StopOnDrop(mpsc::Sender<()>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

pub(crate) fn is_device_lost(e: &cpal::Error) -> bool {
    matches!(
        e.kind(),
        cpal::ErrorKind::StreamInvalidated
            | cpal::ErrorKind::DeviceNotAvailable
            | cpal::ErrorKind::HostUnavailable
    )
}

fn is_named(device: &cpal::Device, name: &str) -> bool {
    device.id().is_ok_and(|id| id.id() == name)
}

fn open(
    mic: &str,
    mut ring: rtrb::Producer<f32>,
    lost: mpsc::Sender<()>,
) -> Result<cpal::Stream, Error> {
    let device = find(Direction::Input, mic).map_err(Error)?;
    let mono_16k = cpal::StreamConfig {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device
        .build_input_stream(
            mono_16k,
            move |data: &[f32], _| push_until_full(&mut ring, data),
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

fn push_until_full(ring: &mut rtrb::Producer<f32>, data: &[f32]) {
    for &sample in data {
        if ring.push(sample).is_err() {
            break;
        }
    }
}

pub(crate) fn find(direction: Direction, name: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    let (what, devices, default) = match direction {
        Direction::Input => (
            "microphone",
            host.input_devices(),
            host.default_input_device(),
        ),
        Direction::Output => (
            "speaker",
            host.output_devices(),
            host.default_output_device(),
        ),
    };
    if name != DEFAULT {
        let pinned = devices
            .map_err(|e| format!("can't list {what}s: {e}"))?
            .find(|device| is_named(device, name));
        match pinned {
            Some(device) => return Ok(device),
            None => eprintln!("lucyd: {what} {name} not found; using the default"),
        }
    }
    default.ok_or_else(|| format!("there is no {what}"))
}

#[cfg(test)]
mod tests {
    use cpal::ErrorKind;

    use super::is_device_lost;

    fn lost(kind: ErrorKind) -> bool {
        is_device_lost(&cpal::Error::new(kind))
    }

    #[test]
    fn a_stream_that_lost_its_device_counts_as_lost() {
        assert!(lost(ErrorKind::StreamInvalidated));
        assert!(lost(ErrorKind::DeviceNotAvailable));
        assert!(lost(ErrorKind::HostUnavailable));
    }

    #[test]
    fn a_default_stream_following_a_new_device_is_not_lost() {
        assert!(!lost(ErrorKind::DeviceChanged));
    }
}
