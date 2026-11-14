//! The AEC bench (wayfinder 23, tickets 27 and 28). Records Lucy's TTS through the real
//! speakers and mic, then runs the recording through no canceller, lucyd's
//! `sonora` and the C++ `webrtc-audio-processing`, and through lucyd's VAD
//! and Barge-in gate. See README.md.

mod gate;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use gate::{BargeInEvent, Gate};
use lucy_config::BargeIn;
use lucyd::audio::{EchoCanceller, Silero, Sonora, Vad};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

pub const SAMPLE_RATE: u32 = 16_000;
const FRAME: usize = 160;
/// Silence recorded before Lucy speaks: the room's noise floor.
const PRE_ROLL: Duration = Duration::from_secs(2);
/// Recorded after she stops: the echo tail.
const POST_ROLL: Duration = Duration::from_secs(2);
/// When the talk scene asks the user to speak, from the start of playback.
const PROMPTS: [f32; 3] = [8.0, 17.0, 26.0];
/// How long after a prompt counts as the user's speech.
const TALK_WINDOW: f32 = 6.5;
/// The reference is moved this much earlier than the timestamps say, so a
/// timestamp error can't put the echo before its reference.
const LEAD_MS: f32 = 40.0;

const TEXT: &str = "Good morning. Here is your news briefing for today. \
The government has announced a new plan for railway investment in the north, \
with work on the first lines expected to begin next spring. \
In science, astronomers have found water vapour in the atmosphere of a planet \
more than a hundred light years away. \
Markets were steady overnight, with the pound rising slightly against the dollar. \
In sport, the national team won their opening match by two goals to one, \
after a late penalty in the second half. \
And finally, the weather. Expect a cloudy start, with some sunny spells this \
afternoon, and a top temperature of fourteen degrees. \
That's the briefing. Would you like me to open any of these stories?";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["record", setup, scene @ ("silent" | "talk")] => record(setup, scene),
        ["analyse", dirs @ ..] if !dirs.is_empty() => {
            for dir in dirs {
                analyse(Path::new(dir));
            }
        }
        ["synth", user] => synth(Path::new(user)),
        ["delay", dir] => delay(Path::new(dir)),
        ["erl", dirs @ ..] => dirs.iter().for_each(|d| erl(Path::new(d))),
        ["levels", dirs @ ..] => dirs.iter().for_each(|d| levels(Path::new(d))),
        _ => {
            eprintln!("usage: aec_bench record <setup> silent|talk\n       aec_bench analyse <run-dir>...\n       aec_bench levels <run-dir>...\n       aec_bench synth <user-speech.wav>");
            std::process::exit(2);
        }
    }
}

/// Where the echo sits relative to the reference: the lag (in ms, echo later
/// is positive) that best correlates the two signals' 1 ms log-energy
/// envelopes.
fn delay(dir: &Path) {
    let (mic, _) = read_wav(&dir.join("mic.wav"));
    let (reference, _) = read_wav(&dir.join("ref.wav"));
    let env = |s: &[f32]| -> Vec<f64> {
        let e: Vec<f64> = s.chunks(16).map(|c| db(mean_square(c))).collect();
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        e.iter().map(|x| x - mean).collect()
    };
    let (m, r) = (env(&mic), env(&reference));
    let n = m.len().min(r.len());
    let mut best = Vec::new();
    for lag in -500i64..1500 {
        let mut sum = 0.0;
        for i in 0..n as i64 {
            let j = i + lag;
            if j >= 0 && (j as usize) < n {
                sum += r[i as usize] * m[j as usize];
            }
        }
        best.push((sum, lag));
    }
    best.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (score, lag) in best.iter().take(5) {
        println!("lag {lag} ms  score {score:.0}");
    }
}

/// A self-check with no speakers: a simulated room (delay, a few
/// reflections, noise) and, for the talk run, `user` at each prompt.
fn synth(user: &Path) {
    let (speech, rate) = speech();
    let reference = resample(&speech, rate, SAMPLE_RATE);
    let (user, user_rate) = read_wav(user);
    let user = resample(&user, user_rate, SAMPLE_RATE);
    let pre = (PRE_ROLL.as_secs_f32() * SAMPLE_RATE as f32) as usize;
    let len = pre + reference.len() + 2 * SAMPLE_RATE as usize;
    let mut played = vec![0.0; pre];
    played.extend_from_slice(&reference);
    played.resize(len, 0.0);
    let taps = [(400, 0.4), (720, 0.15), (1300, 0.06)];
    let mut seed = 1u32;
    for scene in ["silent", "talk"] {
        let dir = PathBuf::from(format!("runs/synth-{scene}"));
        fs::create_dir_all(&dir).unwrap();
        let mut mic: Vec<f32> = (0..len)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.002;
                let echo: f32 = taps
                    .iter()
                    .filter(|(d, _)| i >= *d)
                    .map(|(d, g)| g * played[i - d])
                    .sum();
                echo + noise
            })
            .collect();
        let mut prompts = Vec::new();
        if scene == "talk" {
            for p in PROMPTS {
                let at = pre as f32 / SAMPLE_RATE as f32 + p;
                prompts.push(format!("{at:.3}"));
                let start = (at * SAMPLE_RATE as f32) as usize + 8000;
                for (i, s) in user.iter().enumerate().take(3 * SAMPLE_RATE as usize) {
                    if let Some(m) = mic.get_mut(start + i) {
                        *m += s;
                    }
                }
            }
        }
        let lead = (LEAD_MS / 1000.0 * SAMPLE_RATE as f32) as usize;
        let mut ref_on_mic = played[lead..].to_vec();
        ref_on_mic.resize(len, 0.0);
        write_wav(&dir.join("mic.wav"), &mic);
        write_wav(&dir.join("ref.wav"), &ref_on_mic);
        fs::write(
            dir.join("meta.txt"),
            format!("setup=synth\nscene={scene}\nprompts={}\n", prompts.join(",")),
        )
        .unwrap();
        analyse(&dir);
    }
}

// ---------------------------------------------------------------- record

fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
    base.join("lucy")
}

/// Lucy's voice reading `TEXT`, as mono f32 at the voice's rate.
fn speech() -> (Vec<f32>, u32) {
    let cache = Path::new("runs/speech.wav");
    if !cache.exists() {
        fs::create_dir_all("runs").unwrap();
        let voice = data_dir().join("voices/en_GB-cori-medium.onnx");
        let mut piper = Command::new("piper")
            .env_remove("PYTHONPATH")
            .arg("-m")
            .arg(&voice)
            .arg("-f")
            .arg(cache)
            .stdin(Stdio::piped())
            .spawn()
            .expect("piper on PATH");
        piper
            .stdin
            .take()
            .unwrap()
            .write_all(TEXT.as_bytes())
            .unwrap();
        assert!(piper.wait().unwrap().success(), "piper failed");
    }
    read_wav(cache)
}

fn record(setup: &str, scene: &str) {
    let (speech, speech_rate) = speech();
    let dir = PathBuf::from(format!("runs/{setup}-{scene}"));
    fs::create_dir_all(&dir).unwrap();
    let host = cpal::default_host();
    println!("host: {:?}", host.id());

    // Capture exactly as lucyd does: mono 16 kHz, the server converts.
    let mic = Arc::new(Mutex::new(Vec::<f32>::new()));
    let first_capture: Arc<Mutex<Option<(Instant, Option<cpal::StreamInstant>)>>> =
        Arc::default();
    let input = host.default_input_device().expect("a microphone");
    let input_stream = {
        let mic = mic.clone();
        let first = first_capture.clone();
        input
            .build_input_stream(
                cpal::StreamConfig {
                    channels: 1,
                    sample_rate: SAMPLE_RATE,
                    buffer_size: cpal::BufferSize::Default,
                },
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let mut mic = mic.lock().unwrap();
                    if mic.is_empty() {
                        *first.lock().unwrap() =
                            Some((Instant::now(), Some(info.timestamp().capture)));
                    }
                    mic.extend_from_slice(data);
                },
                |e| eprintln!("mic error: {e}"),
                None,
            )
            .unwrap()
    };

    // Playback as lucyd does: mono at the voice's rate.
    let played = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let first_playback: Arc<Mutex<Option<(Instant, Option<cpal::StreamInstant>)>>> =
        Arc::default();
    let output = host.default_output_device().expect("a speaker");
    let output_stream = {
        let speech = speech.clone();
        let played = played.clone();
        let done = done.clone();
        let first = first_playback.clone();
        output
            .build_output_stream(
                cpal::StreamConfig {
                    channels: 1,
                    sample_rate: speech_rate,
                    buffer_size: cpal::BufferSize::Default,
                },
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let at = played.load(Ordering::Relaxed);
                    if at == 0 {
                        *first.lock().unwrap() =
                            Some((Instant::now(), Some(info.timestamp().playback)));
                    }
                    for (i, out) in data.iter_mut().enumerate() {
                        *out = speech.get(at + i).copied().unwrap_or(0.0);
                    }
                    played.store(at + data.len(), Ordering::Relaxed);
                    if at + data.len() >= speech.len() {
                        done.store(true, Ordering::Relaxed);
                    }
                },
                |e| eprintln!("speaker error: {e}"),
                None,
            )
            .unwrap()
    };
    output_stream.pause().ok();

    println!(
        "mic: {}\nspeaker: {}",
        input.id().map(|id| id.id().to_owned()).unwrap_or_default(),
        output.id().map(|id| id.id().to_owned()).unwrap_or_default()
    );
    if scene == "talk" {
        println!("When TALK NOW shows, talk over Lucy for 2-3 seconds, as if cutting her off.");
    } else {
        println!("Stay quiet the whole time.");
    }
    input_stream.play().unwrap();
    std::thread::sleep(PRE_ROLL);
    output_stream.play().unwrap();
    let started = Instant::now();
    let mut prompts_at = Vec::new();
    let mut next = 0;
    while !done.load(Ordering::Relaxed) {
        if scene == "talk"
            && next < PROMPTS.len()
            && started.elapsed().as_secs_f32() >= PROMPTS[next]
        {
            println!("\n   >>> TALK NOW <<<");
            prompts_at.push(Instant::now());
            next += 1;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(POST_ROLL);
    drop(output_stream);
    drop(input_stream);

    let mic = mic.lock().unwrap().clone();
    let (cap_instant, cap_stream) = first_capture.lock().unwrap().unwrap();
    let (play_instant, play_stream) = first_playback.lock().unwrap().unwrap();
    // Where the first played sample lands on the mic's timeline.
    let by_instant = play_instant.duration_since(cap_instant).as_secs_f32();
    let by_stream = match (cap_stream, play_stream) {
        (Some(c), Some(p)) => Some(p.duration_since(c).as_secs_f32()),
        _ => None,
    };
    let offset = match by_stream {
        Some(s) if (s - by_instant).abs() < 0.5 => s,
        _ => by_instant,
    };
    let lead = offset - LEAD_MS / 1000.0;
    let reference_16k = resample(&speech, speech_rate, SAMPLE_RATE);
    let mut reference = vec![0.0; (lead.max(0.0) * SAMPLE_RATE as f32) as usize];
    reference.extend(reference_16k);
    reference.resize(mic.len(), 0.0);

    write_wav(&dir.join("mic.wav"), &mic);
    write_wav(&dir.join("ref.wav"), &reference);
    let prompts: Vec<String> = prompts_at
        .iter()
        .map(|p| format!("{:.3}", p.duration_since(cap_instant).as_secs_f32()))
        .collect();
    let volume = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    fs::write(
        dir.join("meta.txt"),
        format!(
            "setup={setup}\nscene={scene}\noffset_by_instant={by_instant:.4}\noffset_by_stream={}\noffset_used={offset:.4}\nprompts={}\nvolume={volume}\n",
            by_stream.map(|s| format!("{s:.4}")).unwrap_or_default(),
            prompts.join(","),
        ),
    )
    .unwrap();
    println!("\nsaved {}", dir.display());
    analyse(&dir);
}

// --------------------------------------------------------------- analyse

/// One echo canceller under test.
trait Canceller {
    fn name(&self) -> &'static str;
    /// Cancels `reference`'s echo from `mic`, one 10 ms frame each.
    fn process(&mut self, reference: &[f32], mic: &mut [f32]);
    fn erle_db(&self) -> Option<f32>;
    fn own(&self) -> bool {
        true
    }
    /// Ticket 27's measurement: lucyd's `Sonora` only.
    fn echo_heard(&self) -> Option<bool> {
        None
    }
    /// lucyd hands its canceller the reference only while Lucy plays.
    fn lucy_speaking(&mut self, _speaking: bool) {}
    /// Ticket 28's pre-gain: lucyd's `Sonora` only.
    fn gain_db(&self) -> Option<f32> {
        None
    }
}

struct NoCanceller;
impl Canceller for NoCanceller {
    fn name(&self) -> &'static str {
        "none"
    }
    fn process(&mut self, _: &[f32], _: &mut [f32]) {}
    fn erle_db(&self) -> Option<f32> {
        None
    }
    fn own(&self) -> bool {
        false
    }
}

/// lucyd's own `Sonora`, through its `EchoCanceller` seam.
struct LucydSonora {
    reference: rtrb::Producer<f32>,
    sonora: Sonora,
    speaking: bool,
    pending: Vec<f32>,
}
impl Canceller for LucydSonora {
    fn name(&self) -> &'static str {
        "sonora"
    }
    fn process(&mut self, reference: &[f32], mic: &mut [f32]) {
        if self.speaking || std::env::var_os("ALWAYS_REF").is_some() {
            self.pending.extend_from_slice(reference);
        }
        // REF_BURST frames handed over at a time, REF_LATE frames after they
        // played, as lucyd's playback tick does.
        let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<usize>().ok());
        let (burst, late) = (env("REF_BURST").unwrap_or(1), env("REF_LATE").unwrap_or(0));
        let due = (burst + late) * FRAME;
        if self.pending.len() >= due || (!self.speaking && !self.pending.is_empty()) {
            let n = if self.speaking { burst * FRAME } else { self.pending.len() };
            for s in self.pending.drain(..n.min(self.pending.len())) {
                self.reference.push(s).unwrap();
            }
        }
        self.sonora.cancel(mic);
    }
    fn erle_db(&self) -> Option<f32> {
        self.sonora.erle_db()
    }
    fn echo_heard(&self) -> Option<bool> {
        self.sonora.echo_heard()
    }
    fn gain_db(&self) -> Option<f32> {
        Some(self.sonora.gain_db())
    }
    fn lucy_speaking(&mut self, speaking: bool) {
        self.speaking = speaking;
    }
}

/// The C++ library, configured like lucyd's `Sonora`: AEC3 only.
struct Cpp {
    apm: webrtc_audio_processing::Processor,
    frames: usize,
    erle_db: Option<f32>,
}
impl Canceller for Cpp {
    fn name(&self) -> &'static str {
        "webrtc"
    }
    fn process(&mut self, reference: &[f32], mic: &mut [f32]) {
        self.apm.analyze_render_frame([reference]).unwrap();
        self.apm.process_capture_frame([mic]).unwrap();
        self.frames += 1;
        if self.frames.is_multiple_of(50) {
            self.erle_db = self
                .apm
                .get_stats()
                .echo_return_loss_enhancement
                .map(|e| e as f32);
        }
    }
    fn erle_db(&self) -> Option<f32> {
        self.erle_db
    }
}

fn cancellers() -> Vec<Box<dyn Canceller>> {
    let (reference, consumer) = rtrb::RingBuffer::new(SAMPLE_RATE as usize);
    let apm = webrtc_audio_processing::Processor::new(SAMPLE_RATE).unwrap();
    apm.set_config(webrtc_audio_processing::Config {
        echo_canceller: Some(Default::default()),
        ..Default::default()
    });
    vec![
        Box::new(NoCanceller),
        Box::new(LucydSonora {
            reference,
            sonora: Sonora::new(consumer),
            speaking: false,
            pending: Vec::new(),
        }),
        Box::new(Cpp {
            apm,
            frames: 0,
            erle_db: None,
        }),
        Tuned::new("s-g0.1", |c, _| c.ep_strength.default_gain = 0.1),
        Tuned::new("s-g0.01", |c, _| c.ep_strength.default_gain = 0.01),
        Tuned::new("s-hmm", |_, e| {
            e.transparent_mode = sonora::config::TransparentModeType::Hmm
        }),
    ]
}

/// `sonora` set up like lucyd's, with one AEC3 tuning changed.
struct Tuned {
    name: &'static str,
    apm: sonora::AudioProcessing,
    scratch: Vec<f32>,
    frames: usize,
    erle_db: Option<f32>,
}
impl Tuned {
    fn new(
        name: &'static str,
        tune: impl FnOnce(
            &mut sonora::config::EchoCanceller3Config,
            &mut sonora::config::EchoCanceller,
        ),
    ) -> Box<dyn Canceller> {
        let mut aec3 = sonora::config::EchoCanceller3Config::default();
        let mut ec = sonora::config::EchoCanceller::default();
        tune(&mut aec3, &mut ec);
        let stream = sonora::StreamConfig::new(SAMPLE_RATE, 1);
        let apm = sonora::AudioProcessing::builder()
            .config(sonora::Config {
                echo_canceller: Some(ec),
                ..sonora::Config::default()
            })
            .echo_canceller3_config(aec3)
            .capture_config(stream)
            .render_config(stream)
            .build();
        Box::new(Tuned {
            name,
            apm,
            scratch: vec![0.0; FRAME],
            frames: 0,
            erle_db: None,
        })
    }
}
impl Canceller for Tuned {
    fn name(&self) -> &'static str {
        self.name
    }
    fn process(&mut self, reference: &[f32], mic: &mut [f32]) {
        let _ = self
            .apm
            .process_render_f32(&[reference], &mut [&mut self.scratch]);
        let _ = self.apm.set_stream_delay_ms(0);
        self.scratch.copy_from_slice(mic);
        let _ = self.apm.process_capture_f32(&[&self.scratch], &mut [mic]);
        self.frames += 1;
        if self.frames.is_multiple_of(50) {
            self.erle_db = self
                .apm
                .statistics()
                .echo_return_loss_enhancement
                .map(|e| e as f32);
        }
    }
    fn erle_db(&self) -> Option<f32> {
        self.erle_db
    }
}

fn db(power: f64) -> f64 {
    10.0 * power.max(1e-12).log10()
}

fn analyse(dir: &Path) {
    let (mut mic, _) = read_wav(&dir.join("mic.wav"));
    if let Some(g) = std::env::var("MIC_GAIN_DB").ok().and_then(|g| g.parse::<f32>().ok()) {
        let k = 10f32.powf(g / 20.0);
        mic.iter_mut().for_each(|s| *s = (*s * k).clamp(-1.0, 1.0));
        println!("mic boosted by {g} dB");
    }
    let (reference, _) = read_wav(&dir.join("ref.wav"));
    let meta = fs::read_to_string(dir.join("meta.txt")).unwrap_or_default();
    let prompts: Vec<f32> = meta
        .lines()
        .find_map(|l| l.strip_prefix("prompts="))
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.parse().ok())
        .collect();
    let in_talk = |t: f32| prompts.iter().any(|&p| t >= p && t < p + TALK_WINDOW);
    let frames = mic.len().min(reference.len()) / FRAME;
    let t_of = |frame: usize| (frame * FRAME) as f32 / SAMPLE_RATE as f32;

    // Lucy's speech, from the reference: first and last loud sample.
    let loud = |s: &f32| s.abs() > 1e-3;
    let speech_start = reference.iter().position(loud).unwrap_or(0) / FRAME;
    let speech_end = reference.iter().rposition(loud).unwrap_or(0) / FRAME;
    let active: Vec<bool> = (0..frames)
        .map(|f| rms(&reference[f * FRAME..(f + 1) * FRAME]) > 0.003)
        .collect();
    let noise_floor = db(mean_square(&mic[..speech_start.saturating_sub(10) * FRAME]));

    println!("\n=== {}  ({})", dir.display(), meta.replace('\n', " "));
    println!(
        "speech {:.1}-{:.1} s, noise floor {:.1} dBFS",
        t_of(speech_start),
        t_of(speech_end),
        noise_floor
    );
    println!(
        "{:<7} {:>8} {:>8} {:>9} {:>9} {:>10} {:>8} {:>8} {:>9} {:>9} {:>14} {:>6}  {}",
        "aec",
        "ERLE2s+",
        "ERLE<2s",
        "resid dB",
        "over flr",
        "rep ERLE",
        "conv s",
        "maxP",
        ">=.7 ms",
        "false",
        "heard T/F/? %",
        "gain",
        "talk windows (barge-in latency)"
    );

    let barge_in = BargeIn::default();
    // Ticket 28: each canceller hears the run PASSES times (default 2), as
    // that many Sessions in one lucyd, so its gain carries over.
    let passes = std::env::var("PASSES").ok().and_then(|p| p.parse().ok()).unwrap_or(2);
    let mut cancellers = cancellers();
    if let Ok(only) = std::env::var("CANCELLERS") {
        let only: Vec<&str> = only.split(',').collect();
        cancellers.retain(|c| only.contains(&c.name()));
    }
    for pass in 1..=passes {
    println!("--- pass {pass}");
    for canceller in cancellers.iter_mut() {
        let mut vad = Silero::load().expect("silero");
        let chunk = vad.chunk_len();
        let mut gate = Gate::default();
        gate.configure(&barge_in, true);
        let mut out = Vec::with_capacity(frames * FRAME);
        let (mut in_early, mut out_early, mut in_late, mut out_late) = (0.0, 0.0, 0.0, 0.0);
        let mut reported = Vec::new();
        let mut converged_at = None;
        let mut pending = Vec::new();
        let mut max_p: f32 = 0.0;
        let mut run = 0usize;
        let mut longest_run = 0usize;
        let mut false_events = (0, 0); // (ducks, barge-ins) outside talk windows
        let mut talk_hits: Vec<Option<f32>> = vec![None; prompts.len()];
        let mut talk_onset: Vec<Option<f32>> = vec![None; prompts.len()];
        // Ticket 27: what echo_heard said over Lucy's speech, in frames.
        let mut heard = [0usize; 3];
        let mut trace = String::from("t,p,rep_erle,mic_db,ref_db,out_db\n");

        for f in 0..frames {
            let t = t_of(f);
            let r = &reference[f * FRAME..(f + 1) * FRAME];
            let mut m = mic[f * FRAME..(f + 1) * FRAME].to_vec();
            let heard_power = mean_square(&m);
            canceller.lucy_speaking(f >= speech_start && f <= speech_end);
            canceller.process(r, &mut m);
            if f >= speech_start && f <= speech_end {
                heard[match canceller.echo_heard() {
                    Some(true) => 0,
                    Some(false) => 1,
                    None => 2,
                }] += 1;
            }
            let left = mean_square(&m);
            if active[f] && !in_talk(t) {
                if f < speech_start + 200 {
                    in_early += heard_power;
                    out_early += left;
                } else {
                    in_late += heard_power;
                    out_late += left;
                }
            }
            if f >= speech_start && f <= speech_end {
                if let Some(e) = canceller.erle_db() {
                    reported.push(e);
                    if converged_at.is_none() && e >= barge_in.converged_erle_db {
                        converged_at = Some(t - t_of(speech_start));
                    }
                }
            }
            gate.lucy_speaking(f >= speech_start && f <= speech_end);
            out.extend_from_slice(&m);
            pending.extend_from_slice(&m);
            while pending.len() >= chunk {
                let c: Vec<f32> = pending.drain(..chunk).collect();
                let p = vad.speech_probability(&c);
                trace.push_str(&format!(
                    "{t:.2},{p:.3},{},{:.1},{:.1},{:.1}\n",
                    canceller.erle_db().map(|e| format!("{e:.1}")).unwrap_or_default(),
                    db(heard_power),
                    db(mean_square(r)),
                    db(mean_square(&c)),
                ));
                let talking = in_talk(t);
                let window = prompts.iter().position(|&s| t >= s && t < s + TALK_WINDOW);
                if gate.window_open() && !talking {
                    max_p = max_p.max(p);
                    if p >= barge_in.threshold {
                        run += chunk;
                        longest_run = longest_run.max(run);
                    } else {
                        run = 0;
                    }
                }
                if let Some(w) = window {
                    if p >= 0.5 && talk_onset[w].is_none() {
                        talk_onset[w] = Some(t);
                    }
                }
                match gate.judge(
                    p,
                    chunk,
                    canceller.erle_db(),
                    canceller.echo_heard(),
                    canceller.own(),
                ) {
                    Some(BargeInEvent::Duck) if !talking => false_events.0 += 1,
                    Some(BargeInEvent::BargeIn) => {
                        match window {
                            Some(w) if talk_hits[w].is_none() => {
                                talk_hits[w] = Some(t - talk_onset[w].unwrap_or(prompts[w]));
                            }
                            Some(_) => {}
                            None => false_events.1 += 1,
                        }
                        // Re-arm, so every false barge-in is counted.
                        gate.lucy_speaking(false);
                        gate.lucy_speaking(f >= speech_start && f <= speech_end);
                    }
                    _ => {}
                }
            }
        }
        reported.sort_by(f32::total_cmp);
        let median = reported.get(reported.len() / 2).copied();
        let active_late = (speech_start + 200..frames)
            .filter(|&f| active[f] && !in_talk(t_of(f)))
            .count()
            .max(1);
        let resid_db = db(out_late / (active_late * FRAME) as f64);
        let talk: Vec<String> = talk_hits
            .iter()
            .map(|h| match h {
                Some(l) => format!("{:.0}ms", l * 1000.0),
                None => "missed".to_owned(),
            })
            .collect();
        println!(
            "{:<7} {:>8.1} {:>8.1} {:>9.1} {:>9.1} {:>10} {:>8} {:>8.2} {:>9} {:>9} {:>14} {:>6}  {}",
            canceller.name(),
            db(in_late) - db(out_late),
            db(in_early) - db(out_early),
            resid_db,
            resid_db - noise_floor,
            median.map(|m| format!("{m:.1}")).unwrap_or("-".into()),
            converged_at
                .map(|c| format!("{c:.1}"))
                .unwrap_or("never".into()),
            max_p,
            longest_run * 1000 / SAMPLE_RATE as usize,
            format!("{}d/{}b", false_events.0, false_events.1),
            {
                let total = heard.iter().sum::<usize>().max(1);
                let pct = |n: usize| n * 100 / total;
                format!("{}/{}/{}", pct(heard[0]), pct(heard[1]), pct(heard[2]))
            },
            canceller.gain_db().map(|g| format!("{g:.1}")).unwrap_or("-".into()),
            talk.join(" ")
        );
        fs::write(dir.join(format!("trace-{}.csv", canceller.name())), &trace).unwrap();
        if canceller.name() != "none" {
            write_wav(&dir.join(format!("residual-{}.wav", canceller.name())), &out);
        }
    }
    }
}

/// The mic's 10 ms frame levels (RMS, dBFS): percentiles over the run, and
/// over the pre-roll, which is the room's noise.
fn levels(dir: &Path) {
    let (mic, _) = read_wav(&dir.join("mic.wav"));
    let level = |f: &[f32]| db(mean_square(f));
    let sorted = |frames: &[f32]| {
        let mut l: Vec<f64> = frames.chunks_exact(FRAME).map(level).collect();
        l.sort_by(f64::total_cmp);
        l
    };
    let at = |l: &[f64], p: f64| l[((l.len() - 1) as f64 * p) as usize];
    let all = sorted(&mic);
    let pre_roll = (PRE_ROLL.as_secs_f32() * SAMPLE_RATE as f32) as usize;
    let pre = sorted(&mic[..pre_roll.min(mic.len())]);
    println!(
        "{:<28} p10 {:6.1} p50 {:6.1} p90 {:6.1} p99 {:6.1} max {:6.1} | pre-roll p50 {:6.1} p99 {:6.1}",
        dir.display(),
        at(&all, 0.1),
        at(&all, 0.5),
        at(&all, 0.9),
        at(&all, 0.99),
        at(&all, 1.0),
        at(&pre, 0.5),
        at(&pre, 0.99),
    );
}

fn mean_square(s: &[f32]) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    s.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / s.len() as f64
}

fn rms(s: &[f32]) -> f32 {
    mean_square(s).sqrt() as f32
}

// ------------------------------------------------------------- wav, rate

/// The same FFT resampler lucyd uses for its reference.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return samples.to_vec();
    }
    let mut r = Fft::<f32>::new(from as usize, to as usize, 1024, 1, FixedSync::Input).unwrap();
    let mut input = samples.to_vec();
    let chunk = r.input_frames_next();
    input.resize(input.len().div_ceil(chunk) * chunk, 0.0);
    let mut buf = vec![0.0; r.output_frames_max()];
    let mut out = Vec::new();
    for c in input.chunks(chunk) {
        let i = InterleavedSlice::new(c, 1, chunk).unwrap();
        let cap = buf.len();
        let mut o = InterleavedSlice::new_mut(&mut buf, 1, cap).unwrap();
        let (_, written) = r.process_into_buffer(&i, &mut o, None).unwrap();
        out.extend_from_slice(&buf[..written]);
    }
    out
}

fn read_wav(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = hound::WavReader::open(path).unwrap();
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / scale)
                .collect()
        }
    };
    let mono = samples
        .chunks(spec.channels as usize)
        .map(|c| c[0])
        .collect();
    (mono, spec.sample_rate)
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &s in samples {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

/// sonora's ERL and ERLE over a recording, once a second while Lucy speaks
/// (summarised as quartiles), to see whether ERL tells headphones from
/// speakers.
fn erl(dir: &Path) {
    let (mic, _) = read_wav(&dir.join("mic.wav"));
    let (reference, _) = read_wav(&dir.join("ref.wav"));
    let stream = sonora::StreamConfig::new(SAMPLE_RATE, 1);
    let mut apm = sonora::AudioProcessing::builder()
        .config(sonora::Config {
            echo_canceller: Some(Default::default()),
            ..sonora::Config::default()
        })
        .capture_config(stream)
        .render_config(stream)
        .build();
    let mut scratch = vec![0.0; FRAME];
    let mut out = vec![0.0; FRAME];
    let (mut erls, mut erles) = (Vec::new(), Vec::new());
    let mut series = String::new();
    for f in 0..mic.len().min(reference.len()) / FRAME {
        let r = &reference[f * FRAME..(f + 1) * FRAME];
        let _ = apm.process_render_f32(&[r], &mut [&mut scratch]);
        let _ = apm.set_stream_delay_ms(0);
        let _ = apm.process_capture_f32(&[&mic[f * FRAME..(f + 1) * FRAME]], &mut [&mut out]);
        if f % 100 == 99 && rms(r) > 0.003 {
            let s = apm.statistics();
            let erl = s.echo_return_loss.unwrap_or(f64::NAN);
            let erle = s.echo_return_loss_enhancement.unwrap_or(f64::NAN);
            series.push_str(&format!("{}:{erl:.0}/{erle:.0} ", f / 100));
            erls.push(erl);
            erles.push(erle);
        }
    }
    let q = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let at = |p: f64| v.get(((v.len() - 1) as f64 * p) as usize).copied().unwrap_or(f64::NAN);
        format!("min {:.1} q1 {:.1} median {:.1} q3 {:.1} max {:.1}", at(0.0), at(0.25), at(0.5), at(0.75), at(1.0))
    };
    println!("{}\n  ERL  {}\n  ERLE {}\n  s:ERL/ERLE {series}", dir.display(), q(&mut erls), q(&mut erles));
}
