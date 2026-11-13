//! The Persona: the user's `persona.md`, which goes into every Session's
//! instructions. The user edits it by hand, or asks Lucy to change it.

use std::io;
use std::path::{Path, PathBuf};

/// Written when the user has no `persona.md`. Nothing in it is personal.
const DEFAULT: &str = "\
# Lucy

You are Lucy, a voice assistant on the user's desktop. Everything you write is spoken aloud.

## Manner

Friendly and jovial, with a touch of sass. Never flippant about serious news: when the news is \
grave, be plain and kind.

## Speaking

Answer in one to three short spoken sentences. No markdown, no lists, no emoji. Before calling \
a tool that may be slow, first say \"Let me check.\"

## The user

You don't know what to call the user yet.
";

/// The opening Turn of the user's first Session.
pub const INTRODUCTION: &str = "[This is the user's first Session with you. Introduce yourself \
briefly and ask what to call them. When they tell you, add it to your Persona with \
update_persona.]";

#[derive(Clone)]
pub struct Persona {
    path: PathBuf,
}

impl Persona {
    pub fn new(path: PathBuf) -> Persona {
        Persona { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The Persona's text, writing the default first if there is none.
    /// `created` is true when it had to be written: the user's first Session.
    pub fn load(&self) -> io::Result<Loaded> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Loaded {
                text,
                created: false,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.save(DEFAULT)?;
                Ok(Loaded {
                    text: DEFAULT.to_owned(),
                    created: true,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Replaces the Persona with `text`.
    pub fn save(&self, text: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, text)
    }
}

pub struct Loaded {
    pub text: String,
    pub created: bool,
}

impl Loaded {
    /// The default, for when `persona.md` can't be read or written.
    pub fn fallback() -> Loaded {
        Loaded {
            text: DEFAULT.to_owned(),
            created: false,
        }
    }
}
