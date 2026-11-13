//! Questions in the terminal, one a line. At the end of input every
//! question takes its default, so nothing waits forever.

use std::io::{self, BufRead, Write};

pub struct Ui {
    input: Box<dyn BufRead>,
    out: Box<dyn Write>,
}

impl Ui {
    pub fn terminal() -> Ui {
        Ui {
            input: Box::new(io::stdin().lock()),
            out: Box::new(io::stdout()),
        }
    }

    /// Prints a line.
    pub fn say(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", text.as_ref());
    }

    /// Prints `text` over the current line, for a meter.
    pub fn redraw(&mut self, text: &str) {
        let _ = write!(self.out, "\r{text}");
        let _ = self.out.flush();
    }

    /// The answer to `prompt`, trimmed; None at the end of input.
    pub fn ask(&mut self, prompt: &str) -> Option<String> {
        let _ = write!(self.out, "{prompt} ");
        let _ = self.out.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => {
                let _ = writeln!(self.out);
                None
            }
            Ok(_) => Some(line.trim().to_owned()),
        }
    }

    /// A yes or no, `default` if the answer is empty.
    pub fn yes(&mut self, prompt: &str, default: bool) -> bool {
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        loop {
            let answer = self.ask(&format!("{prompt} {hint}")).unwrap_or_default();
            match answer.to_lowercase().as_str() {
                "" => return default,
                "y" | "yes" => return true,
                "n" | "no" => return false,
                _ => self.say("Please answer y or n."),
            }
        }
    }

    /// The index of one of `options`, listed under `title`; the first if the
    /// answer is empty.
    pub fn choose(&mut self, title: &str, options: &[String]) -> usize {
        self.say(title);
        for (i, option) in options.iter().enumerate() {
            self.say(format!("  {}) {option}", i + 1));
        }
        loop {
            let Some(answer) = self.ask("Choose [1]:") else {
                return 0;
            };
            if answer.is_empty() {
                return 0;
            }
            match answer.parse::<usize>() {
                Ok(n) if (1..=options.len()).contains(&n) => return n - 1,
                _ => self.say(format!("Please choose 1 to {}.", options.len())),
            }
        }
    }
}
