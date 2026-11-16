//! The Omarchy Plugin executable: an MCP stdio server lucyd starts in its
//! sandbox, given `HYPRLAND_INSTANCE_SIGNATURE` and `XDG_RUNTIME_DIR`,
//! which its Manifest requires.

use lucy_omarchy::{Desktop, Omarchy};
use rmcp::ServiceExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let desktop = match Desktop::from_env() {
        Ok(desktop) => desktop,
        Err(e) => {
            eprintln!("omarchy: {e}");
            std::process::exit(1);
        }
    };
    let service = Omarchy::new(desktop)
        .serve(rmcp::transport::stdio())
        .await
        .expect("MCP starts");
    let _ = service.waiting().await;
}
