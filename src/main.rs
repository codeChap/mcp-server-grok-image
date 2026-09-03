mod config;
mod grok;
mod headshot;
mod image_io;
mod params;
mod server;
mod styles;

use config::load_config;
use rmcp::{ServiceExt, transport::stdio};
use server::GrokImageServer;
use std::path::PathBuf;
use styles::build_styles;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cfg = load_config()?;
    let save_dir = PathBuf::from(&cfg.save_dir);
    let styles = build_styles(&cfg.styles);
    info!(
        save_dir = %save_dir.display(),
        style_count = styles.len(),
        "Starting mcp-server-grok-image"
    );

    let server = GrokImageServer::new(cfg.api_key, save_dir, styles);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
