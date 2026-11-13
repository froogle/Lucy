//! The Plugin executable: an MCP stdio server lucyd starts in its sandbox,
//! with its private state folder in `LUCY_PLUGIN_STATE_DIR`.

use std::sync::Arc;

use rmcp::ServiceExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let state_dir = std::env::var_os("LUCY_PLUGIN_STATE_DIR")
        .expect("lucyd sets LUCY_PLUGIN_STATE_DIR")
        .into();
    let plugin = plugin::Plugin::new(Arc::new(plugin::NoWeb), state_dir);
    let service = plugin
        .serve(rmcp::transport::stdio())
        .await
        .expect("MCP starts");
    let _ = service.waiting().await;
}
