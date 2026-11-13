//! Lucy's configuration: types, defaults and loading.
//!
//! lucyd runs with no config file at all; every setting has a default here.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The whole of `config.toml`. Settings arrive with the tickets that use them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub session: Session,
    pub desktop: Desktop,
    /// `[providers.<name>]`: the Thinking Providers the tiers can name.
    pub providers: BTreeMap<String, Provider>,
    pub tiers: Tiers,
    pub audio: Audio,
    pub endpointing: Endpointing,
    pub stt: Stt,
    pub whisper: Whisper,
    pub wake_word: WakeWord,
    pub confirmation: Confirmation,
    pub barge_in: BargeIn,
}

impl Default for Config {
    fn default() -> Self {
        let claude = |tier| Provider {
            kind: Kind::Claude,
            model: Some(recommended_model(Kind::Claude, tier).to_owned()),
            allow_api_key: false,
        };
        Config {
            session: Session::default(),
            desktop: Desktop::default(),
            providers: BTreeMap::from([
                ("claude-fast".to_owned(), claude(Tier::Fast)),
                ("claude-advanced".to_owned(), claude(Tier::Advanced)),
            ]),
            tiers: Tiers::default(),
            audio: Audio::default(),
            endpointing: Endpointing::default(),
            stt: Stt::default(),
            whisper: Whisper::default(),
            wake_word: WakeWord::default(),
            confirmation: Confirmation::default(),
            barge_in: BargeIn::default(),
        }
    }
}

impl Config {
    /// The Provider `tier` names, or why there isn't one, ready to speak.
    pub fn provider(&self, tier: Tier) -> Result<&Provider, String> {
        let name = self.tiers.name(tier);
        self.providers.get(name).ok_or_else(|| {
            format!(
                "The {tier} tier names a Provider called {name}, but config.toml has no \
                 providers.{name} section. Add one, or point the tier at another Provider."
            )
        })
    }
}

/// `[providers.<name>]`: one Thinking Provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Provider {
    pub kind: Kind,
    /// Passed to the CLI as it is; `None` means the CLI's own default.
    #[serde(default)]
    pub model: Option<String>,
    /// Turns the subscription guard off, so the CLI may bill an API key.
    #[serde(default)]
    pub allow_api_key: bool,
}

/// Which CLI a Thinking Provider drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    /// What Lucy calls it when she speaks of it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Claude => "Claude",
            Kind::Codex => "Codex",
        }
    }
}

/// A Model tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Advanced,
}

impl From<Kind> for lucy_ipc::ProviderKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Claude => lucy_ipc::ProviderKind::Claude,
            Kind::Codex => lucy_ipc::ProviderKind::Codex,
        }
    }
}

impl From<Tier> for lucy_ipc::Tier {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Fast => lucy_ipc::Tier::Fast,
            Tier::Advanced => lucy_ipc::Tier::Advanced,
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tier::Fast => "fast",
            Tier::Advanced => "advanced",
        })
    }
}

/// `[tiers]`: the Provider each Model tier uses, by name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tiers {
    pub fast: String,
    pub advanced: String,
}

impl Tiers {
    pub fn name(&self, tier: Tier) -> &str {
        match tier {
            Tier::Fast => &self.fast,
            Tier::Advanced => &self.advanced,
        }
    }
}

impl Default for Tiers {
    fn default() -> Self {
        Tiers {
            fast: "claude-fast".to_owned(),
            advanced: "claude-advanced".to_owned(),
        }
    }
}

/// One row of the shipped model recommendations, updated each release.
/// Setup offers these; any model the CLI accepts may be configured instead.
pub struct Recommendation {
    pub kind: Kind,
    pub tier: Tier,
    pub model: &'static str,
    pub why: &'static str,
}

pub const RECOMMENDATIONS: &[Recommendation] = &[
    Recommendation {
        kind: Kind::Claude,
        tier: Tier::Fast,
        model: "haiku",
        why: "quick spoken replies for everyday talk",
    },
    Recommendation {
        kind: Kind::Claude,
        tier: Tier::Advanced,
        model: "opus",
        why: "research and writing Plugins",
    },
    Recommendation {
        kind: Kind::Codex,
        tier: Tier::Fast,
        model: "gpt-6-luna",
        why: "quick spoken replies for everyday talk",
    },
    Recommendation {
        kind: Kind::Codex,
        tier: Tier::Advanced,
        model: "gpt-6.1-sol",
        why: "research and writing Plugins",
    },
];

/// The recommended model for `kind` in `tier`.
pub fn recommended_model(kind: Kind, tier: Tier) -> &'static str {
    RECOMMENDATIONS
        .iter()
        .find(|r| r.kind == kind && r.tier == tier)
        .map(|r| r.model)
        .expect("every kind has a recommendation for every tier")
}

/// `[audio]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// `"default"`, or the PipeWire node name of a pinned microphone.
    pub mic: String,
    /// `"default"`, or the PipeWire node name of a pinned speaker.
    pub speaker: String,
    /// What cancels Lucy's own voice from the microphone.
    pub echo_cancellation: EchoCancellation,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            mic: "default".to_owned(),
            speaker: "default".to_owned(),
            echo_cancellation: EchoCancellation::default(),
        }
    }
}

/// What cancels Lucy's own voice from the microphone.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EchoCancellation {
    /// Lucy's own echo canceller, with what she plays as its reference.
    #[default]
    Lucy,
    /// PipeWire's echo-cancel module, which the user has set up and pointed
    /// `mic` at. Lucy's own is off.
    #[serde(rename = "pipewire")]
    PipeWire,
}

/// `[barge_in]`: how the user cuts in while Lucy speaks. The gate values
/// are starting points to tune.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct BargeIn {
    pub mode: BargeInMode,
    /// Lets speech barge in on PulseAudio or ALSA, where it is off by
    /// default because the echo canceller works less well there.
    pub force: bool,
    /// Speech probability that counts while Lucy speaks.
    pub threshold: f32,
    /// How long speech must stay above `threshold`.
    pub sustain_ms: u64,
    /// The stricter threshold while the echo canceller hasn't converged.
    pub unconverged_threshold: f32,
    /// How long speech must stay above `unconverged_threshold`.
    pub unconverged_sustain_ms: u64,
    /// The echo canceller has converged once its echo return loss
    /// enhancement reaches this.
    pub converged_erle_db: f32,
    /// The echo canceller counts as unconverged for this much of Lucy's
    /// speech at the start of each Session.
    pub warm_up_ms: u64,
    /// How long after Lucy stops speaking her echo may still be heard.
    pub tail_ms: u64,
}

impl Default for BargeIn {
    fn default() -> Self {
        BargeIn {
            mode: BargeInMode::default(),
            force: false,
            threshold: 0.7,
            sustain_ms: 300,
            unconverged_threshold: 0.85,
            unconverged_sustain_ms: 500,
            converged_erle_db: 10.0,
            warm_up_ms: 2000,
            tail_ms: 300,
        }
    }
}

/// What can cut Lucy off.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BargeInMode {
    /// Sustained speech, or the Wake word.
    #[default]
    Speech,
    /// Only the Wake word: for when her own voice would cut her off.
    WakeWord,
}

/// `[endpointing]`: how the VAD cuts utterances from the microphone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Endpointing {
    /// Speech probability at or above which speech starts.
    pub start_threshold: f32,
    /// Speech probability below which speech has stopped.
    pub end_threshold: f32,
    /// Speech must last this long before it counts as an utterance.
    pub start_ms: u64,
    /// Audio kept from before the speech started.
    pub pre_roll_ms: u64,
    /// Silence that ends an utterance.
    pub end_silence_ms: u64,
    /// The longest utterance; longer speech is cut here.
    pub max_utterance_secs: f64,
}

impl Default for Endpointing {
    fn default() -> Self {
        Endpointing {
            start_threshold: 0.5,
            end_threshold: 0.35,
            start_ms: 100,
            pre_roll_ms: 500,
            end_silence_ms: 700,
            max_utterance_secs: 30.0,
        }
    }
}

/// `[stt]`: which STT Provider turns utterances into text.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Stt {
    pub kind: SttKind,
}

/// An STT Provider.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttKind {
    /// whisper.cpp inside lucyd, set up by `[whisper]`.
    #[default]
    Whisper,
    /// The user's own Superwhisper, through its `superwhisper` CLI.
    Superwhisper,
}

impl SttKind {
    /// The STT Providers setup offers. Superwhisper is offered only when an
    /// executable `superwhisper` is on `path` (a `$PATH` value).
    pub fn options(path: Option<&OsStr>) -> Vec<SttKind> {
        let superwhisper = path.is_some_and(|path| {
            std::env::split_paths(path).any(|dir| is_executable(&dir.join("superwhisper")))
        });
        let mut options = vec![SttKind::Whisper];
        if superwhisper {
            options.push(SttKind::Superwhisper);
        }
        options
    }

    /// What the user should know before choosing it, one warning each.
    pub fn warnings(self) -> &'static [&'static str] {
        match self {
            SttKind::Whisper => &[],
            SttKind::Superwhisper => &[
                "Superwhisper is slower: it adds about 250 ms between the end of \
                 what you say and Lucy hearing it.",
                "Superwhisper always runs what you say through its cleanup \
                 language model, which is part of that delay.",
                "Superwhisper saves everything you say to Lucy in its history, \
                 kept for as long as its history settings say.",
            ],
        }
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `[whisper]`: the `whisper` STT Provider.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Whisper {
    /// Run on the GPU through Vulkan. Unset means on the GPU if Vulkan
    /// finds one.
    pub gpu: Option<bool>,
    /// The model name, such as `small.en`, found as `ggml-<model>.bin` in
    /// `$XDG_DATA_HOME/lucy/models`. Unset means `small.en` on the GPU and
    /// `base.en` on the CPU.
    pub model: Option<String>,
}

/// `[wake_word]`: the one wake-word engine, which is not a Provider. Read
/// when lucyd starts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct WakeWord {
    /// A LiveKit WakeWord v2 `.onnx` model. Its file name names the wake
    /// phrase: `hey_lucy.onnx` listens for "hey lucy". Unset means the model
    /// bundled with lucyd.
    pub model: Option<PathBuf>,
    /// The score from 0 to 1 at which the Wake word counts as heard.
    pub threshold: f32,
}

impl Default for WakeWord {
    fn default() -> Self {
        WakeWord {
            model: None,
            threshold: 0.5,
        }
    }
}

/// `[session]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Seconds of silence in Listening before the Session ends.
    pub silence_timeout_secs: f64,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            silence_timeout_secs: 20.0,
        }
    }
}

/// `[confirmation]`: how Lucy hears the yes to a Confirmation or an
/// Approval.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Confirmation {
    /// What counts as a yes. The whole reply must be one of these, ignoring
    /// case and punctuation.
    pub yes_phrases: Vec<String>,
    /// Seconds of silence after the question before it counts as a no.
    pub timeout_secs: f64,
}

impl Default for Confirmation {
    fn default() -> Self {
        Confirmation {
            yes_phrases: ["yes", "yeah", "yep", "go ahead", "do it"]
                .map(str::to_owned)
                .to_vec(),
            timeout_secs: 8.0,
        }
    }
}

/// `[desktop]`
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Desktop {
    /// The command `launch_app` runs, with the app's desktop entry path
    /// appended. Unset, Lucy uses `uwsm app --` when uwsm is running and
    /// `gio launch` otherwise.
    pub launcher: Option<Vec<String>>,
}

/// `$XDG_CONFIG_HOME/lucy/config.toml`, falling back to `~/.config`.
pub fn default_path() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
        .join("lucy")
        .join("config.toml")
}

/// The XDG base directory in `var`, or `~/<fallback>` if it is unset or empty.
pub fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(fallback)
        })
}

/// Loads the config at `path`. A missing file means all defaults.
pub fn load(path: &Path) -> Result<Config, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(Error::Read(path.to_owned(), e)),
    };
    toml::from_str(&text).map_err(|e| Error::Parse(path.to_owned(), e))
}

/// Sets the value at `key` (the names of the tables it is in, then its own)
/// in the config file at `path`, keeping every comment and all other
/// formatting. A missing file or table is created.
pub fn set(path: &Path, key: &[&str], value: impl Into<toml_edit::Value>) -> Result<(), Error> {
    let (last, tables) = key.split_last().expect("a key to set");
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::Read(path.to_owned(), e)),
    };
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e| Error::Edit(path.to_owned(), e))?;
    let mut table = doc.as_table_mut();
    for name in tables {
        table = table
            .entry(name)
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .ok_or_else(|| Error::NotATable(path.to_owned(), (*name).to_owned()))?;
    }
    match table.get_mut(last).and_then(|item| item.as_value_mut()) {
        // Keep the comment and spacing around the old value.
        Some(old) => {
            let decor = old.decor().clone();
            *old = value.into();
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(last, toml_edit::value(value));
        }
    }
    write_atomically(path, &doc.to_string()).map_err(|e| Error::Write(path.to_owned(), e))
}

/// Writes beside `path` and renames over it, so lucyd never reads half a file.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".new");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

#[derive(Debug)]
pub enum Error {
    Read(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    Edit(PathBuf, toml_edit::TomlError),
    /// A key on the way to the one being set isn't a table.
    NotATable(PathBuf, String),
    Write(PathBuf, io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(path, e) => write!(f, "can't read {}: {e}", path.display()),
            Error::Parse(path, e) => write!(f, "can't parse {}: {e}", path.display()),
            Error::Edit(path, e) => write!(f, "can't parse {}: {e}", path.display()),
            Error::NotATable(path, key) => {
                write!(f, "can't edit {}: {key} isn't a table", path.display())
            }
            Error::Write(path, e) => write!(f, "can't write {}: {e}", path.display()),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Config {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        load(&path).unwrap()
    }

    #[test]
    fn by_default_each_tier_names_a_claude_provider_with_its_recommended_model() {
        let config = Config::default();

        let fast = config.provider(Tier::Fast).unwrap();
        assert_eq!(fast.kind, Kind::Claude);
        assert_eq!(fast.model.as_deref(), Some("haiku"));
        assert!(!fast.allow_api_key);
        let advanced = config.provider(Tier::Advanced).unwrap();
        assert_eq!(advanced.model.as_deref(), Some("opus"));
    }

    #[test]
    fn a_tier_resolves_to_the_named_provider_with_any_model() {
        let config = parse(
            r#"
            [providers.quick]
            kind = "claude"
            model = "claude-some-future-model"
            allow_api_key = true

            [tiers]
            fast = "quick"
            "#,
        );

        let fast = config.provider(Tier::Fast).unwrap();
        assert_eq!(fast.model.as_deref(), Some("claude-some-future-model"));
        assert!(fast.allow_api_key);
    }

    #[test]
    fn either_tier_can_name_a_codex_provider() {
        let config = parse(
            r#"
            [providers.chatgpt]
            kind = "codex"

            [tiers]
            fast = "chatgpt"
            advanced = "chatgpt"
            "#,
        );

        for tier in [Tier::Fast, Tier::Advanced] {
            let provider = config.provider(tier).unwrap();
            assert_eq!(provider.kind, Kind::Codex);
            assert_eq!(provider.model, None);
        }
        assert_eq!(Kind::Codex.name(), "Codex");
    }

    #[test]
    fn codex_has_a_recommended_model_for_each_tier() {
        assert_eq!(recommended_model(Kind::Codex, Tier::Fast), "gpt-6-luna");
        assert_eq!(
            recommended_model(Kind::Codex, Tier::Advanced),
            "gpt-6.1-sol"
        );
    }

    #[test]
    fn a_tier_naming_a_missing_provider_says_which() {
        let config = parse("[tiers]\nfast = \"nope\"\n");

        let err = config.provider(Tier::Fast).unwrap_err();
        assert!(err.contains("nope") && err.contains("fast"), "{err}");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn silence_timeout_defaults_to_twenty_seconds_and_can_be_set() {
        assert_eq!(Config::default().session.silence_timeout_secs, 20.0);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[session]\nsilence_timeout_secs = 45\n").unwrap();
        assert_eq!(load(&path).unwrap().session.silence_timeout_secs, 45.0);
    }

    #[test]
    fn yes_phrases_default_to_the_common_ones_and_can_be_replaced() {
        let defaults = Config::default().confirmation;
        assert_eq!(
            defaults.yes_phrases,
            ["yes", "yeah", "yep", "go ahead", "do it"]
        );
        assert_eq!(defaults.timeout_secs, 8.0);

        let config = parse("[confirmation]\nyes_phrases = [\"go for it\"]\n");

        assert_eq!(config.confirmation.yes_phrases, ["go for it"]);
        assert_eq!(config.confirmation.timeout_secs, 8.0);
    }

    #[test]
    fn the_launcher_is_chosen_at_runtime_unless_config_names_one() {
        assert_eq!(Config::default().desktop.launcher, None);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[desktop]\nlauncher = [\"dex\"]\n").unwrap();
        assert_eq!(
            load(&path).unwrap().desktop.launcher,
            Some(vec!["dex".to_owned()])
        );
    }

    #[test]
    fn devices_and_endpointing_can_be_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[audio]\nmic = \"alsa_input.usb-Blue_Yeti\"\n\
             speaker = \"alsa_output.hdmi\"\n\
             [endpointing]\nend_silence_ms = 900\n",
        )
        .unwrap();

        let config = load(&path).unwrap();

        assert_eq!(config.audio.mic, "alsa_input.usb-Blue_Yeti");
        assert_eq!(config.audio.speaker, "alsa_output.hdmi");
        assert_eq!(config.endpointing.end_silence_ms, 900);
        assert_eq!(config.endpointing.pre_roll_ms, 500);
    }

    #[test]
    fn barge_in_is_by_speech_with_the_spec_gate_unless_config_says_otherwise() {
        let defaults = Config::default();
        assert_eq!(defaults.barge_in.mode, BargeInMode::Speech);
        assert!(!defaults.barge_in.force);
        assert_eq!(defaults.barge_in.threshold, 0.7);
        assert_eq!(defaults.barge_in.sustain_ms, 300);
        assert_eq!(defaults.barge_in.unconverged_threshold, 0.85);
        assert_eq!(defaults.barge_in.unconverged_sustain_ms, 500);
        assert_eq!(defaults.barge_in.converged_erle_db, 10.0);
        assert_eq!(defaults.barge_in.warm_up_ms, 2000);
        assert_eq!(defaults.barge_in.tail_ms, 300);
        assert_eq!(defaults.audio.echo_cancellation, EchoCancellation::Lucy);

        let config = parse(
            "[barge_in]\nmode = \"wake_word\"\nforce = true\n\
             [audio]\necho_cancellation = \"pipewire\"\n",
        );

        assert_eq!(config.barge_in.mode, BargeInMode::WakeWord);
        assert!(config.barge_in.force);
        assert_eq!(config.audio.echo_cancellation, EchoCancellation::PipeWire);
    }

    #[test]
    fn the_wake_word_model_is_bundled_unless_config_names_one() {
        assert_eq!(Config::default().wake_word.model, None);

        let config = parse("[wake_word]\nmodel = \"/models/computer.onnx\"\n");

        assert_eq!(
            config.wake_word.model.as_deref(),
            Some(Path::new("/models/computer.onnx"))
        );
        assert_eq!(config.wake_word.threshold, 0.5);
    }

    #[test]
    fn stt_is_whisper_unless_config_chooses_superwhisper() {
        assert_eq!(Config::default().stt.kind, SttKind::Whisper);

        let config = parse("[stt]\nkind = \"superwhisper\"\n");

        assert_eq!(config.stt.kind, SttKind::Superwhisper);
    }

    #[test]
    fn superwhisper_is_offered_only_when_it_is_installed() {
        let empty = tempfile::tempdir().unwrap();
        let installed = tempfile::tempdir().unwrap();
        let program = installed.path().join("superwhisper");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let path = |dirs: &[&Path]| std::env::join_paths(dirs).unwrap();
        let options = |dirs: &[&Path]| SttKind::options(Some(&path(dirs)));

        assert_eq!(options(&[empty.path()]), [SttKind::Whisper]);
        // Not executable, so not installed.
        assert_eq!(options(&[installed.path()]), [SttKind::Whisper]);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            options(&[empty.path(), installed.path()]),
            [SttKind::Whisper, SttKind::Superwhisper]
        );
        assert_eq!(SttKind::options(None), [SttKind::Whisper]);
    }

    #[test]
    fn superwhisper_warns_of_its_latency_cleanup_model_and_history() {
        assert!(SttKind::Whisper.warnings().is_empty());

        let warnings = SttKind::Superwhisper.warnings().join(" ");

        for topic in ["250 ms", "cleanup", "history"] {
            assert!(warnings.contains(topic), "{warnings}");
        }
    }

    #[test]
    fn malformed_file_is_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is = = not toml").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("config.toml"), "{err}");
    }

    const COMMENTED: &str = r#"# Lucy's config, edited by hand.

[session]
# Long enough for me to think.
silence_timeout_secs = 45   # seconds

[tiers]
fast = "claude-fast" # everyday talk
advanced = "claude-advanced"
"#;

    #[test]
    fn writing_a_key_keeps_every_comment_and_changes_only_that_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, COMMENTED).unwrap();

        set(&path, &["tiers", "fast"], "mine").unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            COMMENTED.replace(r#"fast = "claude-fast""#, r#"fast = "mine""#)
        );
    }

    #[test]
    fn writing_creates_the_file_and_its_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lucy").join("config.toml");

        set(&path, &["providers", "mine", "kind"], "claude").unwrap();
        set(&path, &["providers", "mine", "allow_api_key"], true).unwrap();

        assert!(load(&path).unwrap().providers["mine"].allow_api_key);
    }
}
