//! DreamForge server binary entry point.

use dreamforge_server::config::ServerConfig;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dreamforge_server=info,tower_http=info".into()),
        )
        .init();

    let config = ServerConfig::from_env();
    tracing::info!(
        host = %config.host,
        port = %config.port,
        model = %config.model,
        workspace = %config.workspace,
        "starting DreamForge server"
    );

    if let Err(e) = dreamforge_server::serve(config).await {
        tracing::error!("server failed: {e}");
        std::process::exit(1);
    }
}
