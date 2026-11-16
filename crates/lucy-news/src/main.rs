//! The news Plugin executable: an MCP stdio server lucyd starts in its
//! sandbox. Its private state dir is in `LUCY_PLUGIN_STATE_DIR`.

use std::process::ExitCode;
use std::sync::Arc;

use lucy_news::{local_hour, Http, News};
use rmcp::ServiceExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("news: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), String> {
    let state_dir = std::env::var_os("LUCY_PLUGIN_STATE_DIR")
        .expect("lucyd sets LUCY_PLUGIN_STATE_DIR")
        .into();
    let news = News::new(Arc::new(Http::new()), state_dir, local_hour);
    let service = news
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| format!("MCP didn't start: {e}"))?;
    let _ = service.waiting().await;
    Ok(())
}
