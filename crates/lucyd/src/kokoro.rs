//! The Kokoro TTS Provider (ADR 0013). Kokoro-82M is Apache 2.0, but
//! `kokoro-onnx` speaks through espeak-ng, which is GPL, so it runs out of
//! process (ADR 0003): Lucy's own small server, embedded here, runs in the
//! venv `lucy setup` makes, as a [`LocalServer`](crate::local_server::LocalServer).
//!
//! Finding Kokoro means finding that venv, then downloading the model and
//! voices, checked against pinned hashes, on first use.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::local_server::Server;

const SERVER: &str = include_str!("kokoro/server.py");

const LOOPBACK_ONLY: &str = "127.0.0.1";

const RELEASE: &str =
    "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0";

const MODEL: Download = Download {
    file: "kokoro-v1.0.fp16.onnx",
    sha256: "c1610a859f3bdea01107e73e50100685af38fff88f5cd8e5c56df109ec880204",
};

const VOICES: Download = Download {
    file: "voices-v1.0.bin",
    sha256: "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
};

const FETCH: &str = "\
import hashlib, sys, urllib.request
url, part, sha256 = sys.argv[1:]
digest = hashlib.sha256()
with urllib.request.urlopen(url, timeout=30) as response, open(part, 'wb') as out:
    while chunk := response.read(1 << 20):
        digest.update(chunk)
        out.write(chunk)
if digest.hexdigest() != sha256:
    sys.exit(f'{url} is not the file Lucy expects')
";

struct Download {
    file: &'static str,
    sha256: &'static str,
}

/// Kokoro, installed with its model in one dir.
pub struct Kokoro {
    dir: PathBuf,
}

impl Kokoro {
    /// Kokoro in `dir`: the venv `lucy setup` makes is `dir/venv`.
    pub fn new(dir: PathBuf) -> Kokoro {
        Kokoro { dir }
    }

    fn python(&self) -> PathBuf {
        self.dir.join("venv/bin/python")
    }

    fn server(&self) -> PathBuf {
        self.dir.join("server.py")
    }
}

impl Server for Kokoro {
    type Found = PathBuf;

    const NAME: &'static str = "Kokoro";

    fn find(&self, voice: &str) -> Result<PathBuf, String> {
        check_name(voice)?;
        let python = self.python();
        if !imports_kokoro(&python) {
            return Err("Kokoro isn't installed, so replies are shown as text".into());
        }
        for download in [MODEL, VOICES] {
            fetch(&python, &self.dir, &download)?;
        }
        std::fs::write(self.server(), SERVER)
            .map_err(|e| format!("can't write {}: {e}", self.server().display()))?;
        Ok(python)
    }

    fn command(&self, python: &PathBuf, voice: &str, port: u16) -> Command {
        let mut command = Command::new(python);
        command
            .arg(self.server())
            .args(["--host", LOOPBACK_ONLY, "--port"])
            .arg(port.to_string())
            .arg("--model")
            .arg(self.dir.join(MODEL.file))
            .arg("--voices")
            .arg(self.dir.join(VOICES.file))
            .args(["--voice", voice])
            .current_dir(&self.dir)
            .env_remove("PYTHONPATH");
        command
    }

    fn fetch(&self, _python: &PathBuf, voice: &str) -> Result<(), String> {
        check_name(voice)
    }
}

fn check_name(voice: &str) -> Result<(), String> {
    let plain = !voice.is_empty() && voice.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    match plain {
        true => Ok(()),
        false => Err(format!("{voice:?} isn't a Kokoro voice name")),
    }
}

fn imports_kokoro(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import kokoro_onnx"])
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn fetch(python: &Path, dir: &Path, download: &Download) -> Result<(), String> {
    let file = dir.join(download.file);
    if file.is_file() {
        return Ok(());
    }
    eprintln!("lucyd: downloading Kokoro's {}", download.file);
    let part = dir.join(format!("{}.part", download.file));
    let url = format!("{RELEASE}/{}", download.file);
    let output = Command::new(python)
        .args(["-c", FETCH])
        .args([OsStr::new(&url), part.as_os_str(), download.sha256.as_ref()])
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run {}: {e}", python.display()))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&part);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr.lines().last().unwrap_or("no output");
        return Err(format!(
            "downloading Kokoro's {} failed: {last}",
            download.file
        ));
    }
    std::fs::rename(&part, &file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::local_server::LocalServer;
    use crate::tts::{self, Engine};

    #[test]
    fn the_server_binds_localhost_with_pythonpath_cleared_in_the_voice_asked_for() {
        let kokoro = Kokoro::new("/data/lucy/kokoro".into());

        let command = kokoro.command(&"/data/lucy/kokoro/venv/bin/python".into(), "bf_emma", 5123);

        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "/data/lucy/kokoro/server.py",
                "--host",
                "127.0.0.1",
                "--port",
                "5123",
                "--model",
                "/data/lucy/kokoro/kokoro-v1.0.fp16.onnx",
                "--voices",
                "/data/lucy/kokoro/voices-v1.0.bin",
                "--voice",
                "bf_emma"
            ]
        );
        let pythonpath = command.get_envs().find(|(name, _)| *name == "PYTHONPATH");
        assert_eq!(pythonpath, Some(("PYTHONPATH".as_ref(), None)));
    }

    #[test]
    fn without_its_venv_kokoro_isnt_installed_and_nothing_is_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let kokoro = Kokoro::new(dir.path().to_owned());

        let found = kokoro.find("bf_emma");

        assert_eq!(
            found,
            Err("Kokoro isn't installed, so replies are shown as text".to_owned())
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_voice_name_that_could_be_a_path_or_flag_is_refused() {
        let kokoro = Kokoro::new("/data/lucy/kokoro".into());
        let python = PathBuf::from("/data/lucy/kokoro/venv/bin/python");

        for voice in ["../bf_emma", "--help", "bf emma", ""] {
            assert!(kokoro.fetch(&python, voice).is_err(), "{voice:?}");
        }
        assert!(kokoro.fetch(&python, "bm_george").is_ok());
    }

    fn live_kokoro() -> (tempfile::TempDir, LocalServer<Kokoro>) {
        let venv = std::env::var_os("LUCY_KOKORO_VENV")
            .map(PathBuf::from)
            .expect("set LUCY_KOKORO_VENV to a venv with kokoro-onnx installed");
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(venv, dir.path().join("venv")).unwrap();
        let kokoro = LocalServer::new(Kokoro::new(dir.path().to_owned()));
        (dir, kokoro)
    }

    #[test]
    #[ignore = "needs a kokoro-onnx venv and the network"]
    fn live_kokoro_downloads_its_model_speaks_switches_voice_and_survives_a_crash() {
        let (dir, kokoro) = live_kokoro();
        kokoro.start("bf_emma");

        let pcm = kokoro.synthesise("Hello there.", "bf_emma").unwrap();
        assert_eq!(pcm.sample_rate, 24_000);
        assert!(pcm.samples.len() > 24_000 / 4, "{}", pcm.samples.len());
        for download in [MODEL, VOICES] {
            assert!(
                dir.path().join(download.file).is_file(),
                "{} missing",
                download.file
            );
        }
        assert!(kokoro
            .synthesise("And in another voice.", "bm_george")
            .is_ok());
        assert!(kokoro.synthesise("Nobody.", "xx_nobody").is_err());

        let port = kokoro.port().expect("ready after speaking");
        tts::live::only_loopback_reaches(port);
        let server_killed = Command::new("pkill")
            .args(["-f", &format!("server.py --host 127.0.0.1 --port {port}")])
            .status()
            .unwrap();
        assert!(server_killed.success());
        tts::live::wait_for("a new server", || {
            kokoro.port().is_some_and(|new| new != port)
        });
        assert!(kokoro.synthesise("Still here.", "bf_emma").is_ok());
    }

    #[test]
    #[ignore = "needs a kokoro-onnx venv, the network and speakers"]
    fn live_kokoro_speaks_through_the_speakers_with_levels_and_an_echo_reference() {
        let (_dir, kokoro) = live_kokoro();

        tts::live::speaks_with_levels(
            Arc::new(kokoro),
            lucy_config::Voice {
                kind: lucy_config::TtsKind::Kokoro,
                name: "bf_emma".to_owned(),
            },
        );
    }
}
