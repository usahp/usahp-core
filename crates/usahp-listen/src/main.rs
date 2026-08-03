use anyhow::{Context, Result};
use clap::Parser;
use futures_util::StreamExt;
use tokio_tungstenite::connect_async;
use usahp_core::ServerMessage;

#[derive(Debug, Parser)]
#[command(
    name = "usahp-listen",
    version,
    about = "Print events from a USAHP daemon"
)]
struct Args {
    /// Local USAHP WebSocket URL.
    #[arg(default_value = "ws://127.0.0.1:7312")]
    url: String,

    /// Print messages as compact JSON instead of readable text.
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let (socket, _) = connect_async(&args.url)
        .await
        .with_context(|| format!("could not connect to {}", args.url))?;
    println!("connected to {}", args.url);
    let (_, mut incoming) = socket.split();

    while let Some(message) = incoming.next().await {
        let message = message.context("WebSocket receive failed")?;
        if !message.is_text() {
            continue;
        }
        let parsed: ServerMessage = serde_json::from_str(message.to_text()?)
            .context("server sent an invalid USAHP message")?;
        if args.json {
            println!("{}", serde_json::to_string(&parsed)?);
            continue;
        }
        match parsed {
            ServerMessage::Hello(hello) => {
                println!("protocol {}", hello.protocol_version);
                for switch in hello.switches {
                    println!("  {}: {:?}", switch.switch_id, switch.state);
                }
            }
            ServerMessage::SwitchEvent(event) => println!(
                "#{:<6} +{:>10}us  {} {:?}  {}",
                event.sequence,
                event.monotonic_us,
                event.switch_id,
                event.action,
                event
                    .confidence
                    .map(|c| format!("{c:.1}%"))
                    .unwrap_or_else(|| "—".into())
            ),
            ServerMessage::HandshakeResponse(resp) => {
                println!("handshake: {resp:?}")
            }
            ServerMessage::SessionRevoked(rev) => {
                println!("session {} revoked: {:?}", rev.session_id, rev.reason)
            }
        }
    }
    Ok(())
}
