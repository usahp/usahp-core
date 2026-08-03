use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;
use usahp_core::Config;

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
    let config = Config::load(&args.config)
        .with_context(|| format!("failed to load {}", args.config.display()))?;
    usahp_daemon::input::validate(&config.mappings)?;

    let mut broker_mappings = config.mappings.clone();
    if config.simulator.stdin {
        broker_mappings.extend(usahp_daemon::simulator::mappings_for(&config.mappings));
    }
    let capture = usahp_daemon::input::CaptureControl::new_enabled();
    let broker = usahp_daemon::broker::spawn(broker_mappings, capture.clone());
    usahp_daemon::input::spawn(&config.mappings, broker.clone(), capture)?;
    if config.simulator.stdin {
        usahp_daemon::simulator::spawn_stdin(broker.clone(), &config.mappings);
    }

    #[cfg(target_os = "macos")]
    {
        usahp_daemon::focus_watcher::spawn(broker.clone());
    }

    let listener = TcpListener::bind(config.server.address())
        .await
        .context("could not bind loopback WebSocket server")?;
    info!(config = %args.config.display(), "USAHP daemon started");

    tokio::select! {
        result = usahp_daemon::server::serve(listener, broker, config.server.client_queue_capacity) => result,
        result = tokio::signal::ctrl_c() => {
            result.context("could not listen for Ctrl+C")?;
            info!("shutdown requested");
            Ok(())
        }
    }
}
