//! Lucy's configuration: types, defaults and loading.
//!
//! lucyd runs with no config file at all; every setting has a default here.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The whole of `config.toml`. Settings arrive with the tickets that use them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub session: Session,
    /// `[providers.<name>]`: the Thinking Providers the tiers can name.
    pub providers: BTreeMap<String, Provider>,
    pub tiers: Tiers,
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
            providers: BTreeMap::from([
                ("claude-fast".to_owned(), claude(Tier::Fast)),
                ("claude-advanced".to_owned(), claude(Tier::Advanced)),
            ]),
            tiers: Tiers::default(),
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
}

/// A Model tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Advanced,
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
];

/// The recommended model for `kind` in `tier`.
pub fn recommended_model(kind: Kind, tier: Tier) -> &'static str {
    RECOMMENDATIONS
        .iter()
        .find(|r| r.kind == kind && r.tier == tier)
        .map(|r| r.model)
        .expect("every kind has a recommendation for every tier")
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
