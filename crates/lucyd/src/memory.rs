use std::io;
use std::path::{Path, PathBuf};

use crate::limits;
use crate::persona::Persona;

const HEADING: &str = "## What the user asked you to remember";

#[derive(Clone)]
pub struct Memory {
    path: PathBuf,
}

impl Memory {
    pub fn beside(persona: &Persona) -> Memory {
        Memory {
            path: persona.path().with_file_name("memory.md"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn section(&self) -> io::Result<Option<String>> {
        let text = self.read()?;
        let text = text.trim();
        Ok((!text.is_empty()).then(|| format!("{HEADING}\n\n{text}")))
    }

    pub fn remember(&self, fact: &str) -> io::Result<String> {
        let date =
            limits::today().ok_or_else(|| io::Error::other("today's local date is unknown"))?;
        let bullet = format!("- {date}: {fact}\n");
        let existing = self.read()?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        io::Write::write_all(&mut file, format!("{separator}{bullet}").as_bytes())?;
        Ok(bullet)
    }

    pub fn forget(&self, fact: &str) -> io::Result<Forgot> {
        let text = self.read()?;
        let kept: String = text
            .split_inclusive('\n')
            .filter(|line| fact_of(line) != Some(fact))
            .collect();
        if kept.len() == text.len() {
            return Ok(Forgot::NoMatch { memory: text });
        }
        std::fs::write(&self.path, kept)?;
        Ok(Forgot::Removed)
    }

    fn read(&self) -> io::Result<String> {
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            read => read,
        }
    }
}

pub enum Forgot {
    Removed,
    NoMatch { memory: String },
}

fn fact_of(line: &str) -> Option<&str> {
    let bullet = line.trim_end().strip_prefix("- ")?;
    match bullet.split_once(": ") {
        Some((date, fact)) if is_date(date) => Some(fact),
        _ => Some(bullet),
    }
}

fn is_date(text: &str) -> bool {
    text.len() == 10 && text.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}
