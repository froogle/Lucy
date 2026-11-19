//! The Piper TTS Provider (ADR 0003). Piper is GPL-3.0, so Lucy never links
//! it: she runs the user-installed upstream `piper.http_server` as a
//! [`LocalServer`](crate::local_server::LocalServer).
//!
//! Finding Piper means finding a Python that can import `piper`, then
//! downloading the voice with its model card on first use.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::local_server::Server;

pub const DEFAULT_VOICE: &str = "en_GB-cori-medium";

const LOOPBACK_ONLY: &str = "127.0.0.1";

/// Piper, keeping its voices in one dir.
pub struct Piper {
    voices_dir: PathBuf,
    python: Option<PathBuf>,
}

impl Piper {
    /// Piper with its voices in `voices_dir`, run by the first Python found
    /// that can import it.
    pub fn new(voices_dir: PathBuf) -> Piper {
        Piper {
            voices_dir,
            python: None,
        }
    }
}

impl Server for Piper {
    type Found = PathBuf;

    const NAME: &'static str = "Piper";

    fn find(&self, voice: &str) -> Result<PathBuf, String> {
        let python = self
            .python
            .clone()
            .or_else(|| find_python(&python_candidates()))
            .ok_or_else(|| "Piper isn't installed, so replies are shown as text".to_owned())?;
        ensure_voice(&python, &self.voices_dir, voice)?;
        Ok(python)
    }

    fn command(&self, python: &PathBuf, voice: &str, port: u16) -> Command {
        server_command(python, &self.voices_dir.join(format!("{voice}.onnx")), port)
    }

    fn fetch(&self, python: &PathBuf, voice: &str) -> Result<(), String> {
        ensure_voice(python, &self.voices_dir, voice).map(drop)
    }
}

fn server_command(python: &Path, model: &Path, port: u16) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-m", "piper.http_server", "--host", LOOPBACK_ONLY, "--port"])
        .arg(port.to_string())
        .arg("-m")
        .arg(model)
        .env_remove("PYTHONPATH");
    if let Some(dir) = model.parent() {
        command.current_dir(dir);
    }
    command
}

fn python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let script = std::env::split_paths(&path)
        .map(|dir| dir.join("piper"))
        .find(|piper| piper.is_file());
    if let Some(interpreter) = script.and_then(|script| shebang(&script)) {
        candidates.push(interpreter);
    }
    candidates.push("python3".into());
    candidates
}

fn shebang(script: &Path) -> Option<PathBuf> {
    let mut head = [0; 256];
    let n = std::fs::File::open(script).ok()?.read(&mut head).ok()?;
    let line = std::str::from_utf8(&head[..n]).ok()?.lines().next()?;
    let interpreter = line.strip_prefix("#!")?.split_whitespace().next()?;
    Some(interpreter.into())
}

fn find_python(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|python| imports_piper(python))
        .cloned()
}

fn imports_piper(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import piper"])
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn ensure_voice(python: &Path, dir: &Path, voice: &str) -> Result<PathBuf, String> {
    let plain = voice
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !plain || model_card_url(voice).is_none() {
        return Err(format!("{voice:?} isn't a Piper voice name"));
    }
    let model = dir.join(format!("{voice}.onnx"));
    let card = dir.join(format!("{voice}.MODEL_CARD"));
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let config = model.with_extension("onnx.json");
    if !model.is_file() || !config.is_file() {
        eprintln!("lucyd: downloading the Piper voice {voice}");
        run_python(
            python,
            &["-m", "piper.download_voices", "--data-dir"],
            &[dir.as_os_str(), voice.as_ref()],
        )?;
    }
    if !card.is_file() {
        let url = model_card_url(voice).ok_or(format!("{voice:?} isn't a Piper voice name"))?;
        let part = card.with_extension("MODEL_CARD.part");
        run_python(
            python,
            &[
                "-c",
                "import sys, urllib.request; urllib.request.urlretrieve(sys.argv[1], sys.argv[2])",
            ],
            &[url.as_ref(), part.as_os_str()],
        )?;
        std::fs::rename(&part, &card).map_err(|e| e.to_string())?;
    }
    Ok(model)
}

fn run_python(python: &Path, args: &[&str], more: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(python)
        .args(args)
        .args(more)
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run {}: {e}", python.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().unwrap_or("no output");
    Err(format!("downloading the voice failed: {last}"))
}

fn model_card_url(voice: &str) -> Option<String> {
    let mut parts = voice.splitn(3, '-');
    let (lang, name, quality) = (parts.next()?, parts.next()?, parts.next()?);
    let family = lang.split_once('_')?.0;
    Some(format!(
        "https://huggingface.co/rhasspy/piper-voices/resolve/main/\
         {family}/{lang}/{name}/{quality}/MODEL_CARD"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::local_server::LocalServer;
    use crate::tts::{self, Engine};

    #[test]
    fn the_server_binds_localhost_with_pythonpath_cleared_and_the_voice_preloaded() {
        let command = server_command(
            Path::new("/venv/bin/python"),
            Path::new("/voices/en_GB-cori-medium.onnx"),
            5123,
        );

        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "-m",
                "piper.http_server",
                "--host",
                "127.0.0.1",
                "--port",
                "5123",
                "-m",
                "/voices/en_GB-cori-medium.onnx"
            ]
        );
        let pythonpath = command.get_envs().find(|(name, _)| *name == "PYTHONPATH");
        assert_eq!(pythonpath, Some(("PYTHONPATH".as_ref(), None)));
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let written = Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
                "sh",
            ])
            .arg(format!("#!/bin/sh\n{body}\n"))
            .arg(&path)
            .status()
            .unwrap();
        assert!(written.success());
        path
    }

    #[test]
    fn piper_is_the_first_python_that_can_import_its_module() {
        let dir = tempfile::tempdir().unwrap();
        let without = script(dir.path(), "without", "exit 1");
        let with = script(
            dir.path(),
            "with",
            r#"[ "$1" = -c ] && [ "$2" = "import piper" ]"#,
        );
        let missing = dir.path().join("missing");

        assert_eq!(
            find_python(&[missing, without.clone(), with.clone()]),
            Some(with)
        );
        assert_eq!(find_python(&[without]), None);
    }

    #[test]
    fn a_piper_script_names_its_interpreter_on_the_shebang_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piper");
        std::fs::write(
            &path,
            "#!/home/me/.local/share/uv/tools/piper-tts/bin/python\n",
        )
        .unwrap();

        assert_eq!(
            shebang(&path),
            Some("/home/me/.local/share/uv/tools/piper-tts/bin/python".into())
        );
    }

    #[test]
    fn the_model_card_comes_from_the_voice_folder_on_piper_voices() {
        assert_eq!(
            model_card_url("en_GB-cori-medium").as_deref(),
            Some(
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_GB/cori/medium/MODEL_CARD"
            )
        );
        assert_eq!(model_card_url("cori"), None);
    }

    fn live_python() -> PathBuf {
        std::env::var_os("LUCY_PIPER_PYTHON")
            .map(PathBuf::from)
            .or_else(|| find_python(&python_candidates()))
            .expect("set LUCY_PIPER_PYTHON to a Python with piper-tts installed")
    }

    fn live_piper(voices: &Path) -> LocalServer<Piper> {
        LocalServer::new(Piper {
            voices_dir: voices.to_owned(),
            python: Some(live_python()),
        })
    }

    #[test]
    #[ignore = "needs piper-tts and the network"]
    fn live_piper_downloads_the_voice_speaks_and_survives_a_crash() {
        let voices = tempfile::tempdir().unwrap();
        let piper = live_piper(voices.path());
        piper.start(DEFAULT_VOICE);

        let pcm = piper.synthesise("Hello there.", DEFAULT_VOICE).unwrap();
        assert_eq!(pcm.sample_rate, 22050);
        assert!(pcm.samples.len() > 22050 / 4, "{}", pcm.samples.len());
        for file in [
            "en_GB-cori-medium.onnx",
            "en_GB-cori-medium.onnx.json",
            "en_GB-cori-medium.MODEL_CARD",
        ] {
            assert!(voices.path().join(file).is_file(), "{file} missing");
        }

        let port = piper.port().expect("ready after speaking");
        tts::live::only_loopback_reaches(port);
        let server_killed = Command::new("pkill")
            .args([
                "-f",
                &format!("piper.http_server --host 127.0.0.1 --port {port}"),
            ])
            .status()
            .unwrap();
        assert!(server_killed.success());
        tts::live::wait_for("a new server", || {
            piper.port().is_some_and(|new| new != port)
        });
        assert!(piper.synthesise("Still here.", DEFAULT_VOICE).is_ok());
    }

    #[test]
    #[ignore = "needs piper-tts, the network and speakers"]
    fn live_piper_speaks_through_the_speakers_with_levels() {
        let voices = tempfile::tempdir().unwrap();
        let piper = live_piper(voices.path());

        tts::live::speaks_with_levels(
            Arc::new(piper),
            lucy_config::Voice {
                kind: lucy_config::TtsKind::Piper,
                name: DEFAULT_VOICE.to_owned(),
            },
        );
    }
}
