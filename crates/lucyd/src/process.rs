use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, ExitStatus};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::adapters::Exit;
use crate::sync::Locked;

const EXIT_WAIT: Duration = Duration::from_secs(1);

const EXIT_POLL: Duration = Duration::from_millis(10);

const STDERR_LINES: usize = 20;

pub enum Process {
    Starting,
    Running(Child),
    Ended,
}

pub fn exit(process: &Mutex<Process>, stderr: Option<StderrTail>) -> Exit {
    Exit {
        status: exit_status(process),
        stderr: stderr.map(StderrTail::lines).unwrap_or_default(),
    }
}

fn exit_status(process: &Mutex<Process>) -> Option<ExitStatus> {
    let deadline = Instant::now() + EXIT_WAIT;
    loop {
        match &mut *process.locked() {
            Process::Running(child) => {
                if let Ok(Some(status)) = child.try_wait() {
                    return Some(status);
                }
            }
            Process::Starting | Process::Ended => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(EXIT_POLL);
    }
}

pub struct StderrTail {
    kept: Arc<Mutex<VecDeque<String>>>,
    closed: std_mpsc::Receiver<()>,
}

impl StderrTail {
    pub fn follow(stderr: ChildStderr) -> StderrTail {
        let kept = Arc::new(Mutex::new(VecDeque::new()));
        let (closed_tx, closed) = std_mpsc::channel();
        let reader_kept = kept.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut kept = reader_kept.locked();
                if kept.len() == STDERR_LINES {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
            let _ = closed_tx.send(());
        });
        StderrTail { kept, closed }
    }

    fn lines(self) -> Vec<String> {
        let _held_open_by_a_grandchild = self.closed.recv_timeout(EXIT_WAIT);
        self.kept.locked().iter().cloned().collect()
    }
}
