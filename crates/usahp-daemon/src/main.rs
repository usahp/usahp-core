use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "usahpd", version, about = "USAHP local switch-event broker")]
struct Args {
    /// TOML configuration file.
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "usahp=info".into()))
        .init();

    let args = Args::parse();
    info!(config = %args.config.display(), "USAHP daemon started");
    usahp_daemon::service::run_headless(args.config).await?;
    info!("shutdown requested");
    Ok(())
}
