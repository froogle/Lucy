use std::ffi::OsStr;

use tokio::process::Command;

pub(crate) async fn run(path: &OsStr, command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .env("PATH", path)
        .output()
        .await
        .map_err(|e| format!("{command} can't run: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
