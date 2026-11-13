//! Real capture through cpal.

use std::time::{Duration, Instant};

use lucyd::audio::{AudioIo, Cpal};

#[test]
#[ignore = "needs a microphone"]
fn the_default_microphone_delivers_16_khz_audio() {
    let (ring, samples) = rtrb::RingBuffer::new(16_000);
    let _capture = Cpal.capture("default", ring).expect("the microphone opens");

    // Half a second of audio arrives in about half a second.
    let start = Instant::now();
    while samples.slots() < 8_000 {
        assert!(start.elapsed() < Duration::from_secs(2), "too few samples");
        // sleep-ok: poll loop with a deadline; real capture runs in real time
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        start.elapsed() >= Duration::from_millis(400),
        "too many samples"
    );
}
