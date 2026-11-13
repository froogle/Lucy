//! The lucyd harness: starts lucyd in-process and observes it only through
//! the real public IPC, using the `lucy-ipc` client. Later tickets extend it
//! with fake adapters.

#![allow(dead_code)]

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use lucy_config::Config;
use lucy_ipc::Client;

pub struct Harness {
    // Dropped first, so lucyd stops before its runtime dir goes away.
    daemon: lucyd::Daemon,
    _dir: tempfile::TempDir,
}

impl Harness {
    /// Starts lucyd with default config and a socket in a fresh runtime dir.
    pub fn start() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("lucy").join("lucyd.sock");
        let daemon = lucyd::spawn(lucyd::Options {
            socket_path,
            config: Config::default(),
        })
        .expect("lucyd starts");
        Harness { daemon, _dir: dir }
    }

    pub fn socket_path(&self) -> &Path {
        self.daemon.socket_path()
    }

    /// A client that has completed the `hello` handshake.
    pub fn client(&self) -> Client {
        let client = Client::connect(self.socket_path(), "harness", None).expect("hello succeeds");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client
    }

    /// Sends one raw line on a fresh connection and returns the reply line,
    /// for checking the wire format without the client in the way.
    pub fn raw_exchange(&self, line: &str) -> serde_json::Value {
        use std::io::{BufRead, BufReader, Write};
        let mut stream = UnixStream::connect(self.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(stream, "{line}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }
}
