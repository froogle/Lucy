use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PACKAGE: &str = "kokoro-onnx==0.6.1";

const PYTHONS: [&str; 4] = ["python3.13", "python3.12", "python3.11", "python3.10"];

const UV_PYTHON: &str = "3.13";

fn venv(dir: &Path) -> PathBuf {
    dir.join("venv")
}

pub fn installed(dir: &Path) -> bool {
    Command::new(venv(dir).join("bin/python"))
        .args(["-c", "import kokoro_onnx"])
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn install_plan(dir: &Path, has: impl Fn(&str) -> bool) -> Vec<Vec<Vec<String>>> {
    let venv = venv(dir).display().to_string();
    let python = format!("{venv}/bin/python");
    let pip = format!("{venv}/bin/pip");
    let mut plan = Vec::new();
    if has("uv") {
        plan.push(vec![
            words(&["uv", "venv", "--clear", "--python", UV_PYTHON, &venv]),
            words(&["uv", "pip", "install", "--python", &python, PACKAGE]),
        ]);
    }
    if let Some(found) = PYTHONS.into_iter().find(|python| has(python)) {
        plan.push(vec![
            words(&[found, "-m", "venv", "--clear", &venv]),
            words(&[&pip, "install", PACKAGE]),
        ]);
    }
    plan
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has<'a>(programs: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        |program| programs.contains(&program)
    }

    #[test]
    fn uv_makes_lucys_own_venv_on_a_python_kokoro_supports_then_installs_into_it() {
        let plan = install_plan(Path::new("/data/lucy/kokoro"), has(&["uv", "python3.12"]));

        assert_eq!(
            plan,
            [
                vec![
                    words(&[
                        "uv",
                        "venv",
                        "--clear",
                        "--python",
                        "3.13",
                        "/data/lucy/kokoro/venv"
                    ]),
                    words(&[
                        "uv",
                        "pip",
                        "install",
                        "--python",
                        "/data/lucy/kokoro/venv/bin/python",
                        "kokoro-onnx==0.6.1"
                    ]),
                ],
                vec![
                    words(&[
                        "python3.12",
                        "-m",
                        "venv",
                        "--clear",
                        "/data/lucy/kokoro/venv"
                    ]),
                    words(&[
                        "/data/lucy/kokoro/venv/bin/pip",
                        "install",
                        "kokoro-onnx==0.6.1"
                    ]),
                ],
            ]
        );
    }

    #[test]
    fn without_uv_or_a_python_kokoro_supports_there_is_no_way_to_install_it() {
        let plan = install_plan(Path::new("/data/lucy/kokoro"), has(&["python3", "pipx"]));

        assert!(plan.is_empty());
    }
}
