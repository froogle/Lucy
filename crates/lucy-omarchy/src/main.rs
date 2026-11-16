//! The Omarchy Plugin executable: an MCP stdio server lucyd starts in its
//! sandbox. It needs `HYPRLAND_INSTANCE_SIGNATURE` and `XDG_RUNTIME_DIR`,
//! which its Manifest requires.

use std::process::ExitCode;

use lucy_omarchy::{Desktop, Omarchy};
use rmcp::ServiceExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("omarchy: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), String> {
    let desktop = Desktop::from_env()?;
    let service = Omarchy::new(desktop)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| format!("MCP didn't start: {e}"))?;
    let _ = service.waiting().await;
    Ok(())
}
