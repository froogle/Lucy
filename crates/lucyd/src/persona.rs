use std::io;
use std::path::{Path, PathBuf};

const DEFAULT: &str = "\
# Lucy

You are Lucy, a voice assistant on the user's desktop. Everything you write is spoken aloud.

## Manner

Friendly and jovial, with a touch of sass. Never flippant about serious news: when the news is \
grave, be plain and kind.

## Speaking

Answer in one to three short spoken sentences. No markdown, no lists, no emoji. Before calling \
a tool that may be slow, first say \"Let me check.\"

When the user only greets you, greet them back in a word or two. When they say goodbye, say a \
word or two in return and call end_session.

## The user

You don't know what to call the user yet.
";

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

    pub fn load(&self) -> io::Result<Loaded> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Loaded {
                text,
                first_run: false,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Loaded {
                text: DEFAULT.to_owned(),
                first_run: true,
            }),
            Err(e) => Err(e),
        }
    }

    pub fn introduced(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            Ok(mut file) => io::Write::write_all(&mut file, DEFAULT.as_bytes()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, text: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, text)
    }
}

pub struct Loaded {
    pub text: String,
    pub first_run: bool,
}

impl Loaded {
    pub fn fallback() -> Loaded {
        Loaded {
            text: DEFAULT.to_owned(),
            first_run: false,
        }
    }
}
