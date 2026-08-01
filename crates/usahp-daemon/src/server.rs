use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use futures_util::SinkExt;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, info, warn};
use usahp_core::ServerMessage;

use crate::broker::BrokerCommand;

pub async fn serve(
    listener: TcpListener,
    broker: mpsc::Sender<BrokerCommand>,
    queue_capacity: usize,
) -> Result<()> {
    info!(address = %listener.local_addr()?, "WebSocket server listening");
    loop {
        let (stream, peer) = listener.accept().await?;
        let broker = broker.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, peer, broker, queue_capacity).await {
                warn!(%peer, %error, "client connection ended with error");
            }
        });
    }
}

async fn handle_client(
    stream: TcpStream,
    peer: SocketAddr,
    broker: mpsc::Sender<BrokerCommand>,
    queue_capacity: usize,
) -> Result<()> {
    let mut websocket = accept_async(stream)
        .await
        .context("WebSocket handshake failed")?;
    let (sender, mut receiver) = mpsc::channel::<Arc<ServerMessage>>(queue_capacity);
    let (reply, registered) = oneshot::channel();
    broker
        .send(BrokerCommand::Register { sender, reply })
        .await
        .context("broker stopped")?;
    let client_id = registered.await.context("broker rejected registration")?;
    debug!(client_id, %peer, "client connected");

    while let Some(message) = receiver.recv().await {
        let json = serde_json::to_string(&*message)?;
        if websocket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }

    let _ = broker.send(BrokerCommand::Unregister(client_id)).await;
    debug!(client_id, %peer, "client disconnected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::connect_async;
    use usahp_core::{Action, InputKind, Mapping, ServerMessage};

    use super::*;
    use crate::broker::{self, PhysicalEvent};

    #[tokio::test]
    async fn websocket_sends_snapshot_then_events() {
        let mapping = Mapping {
            id: "physical".into(),
            switch_id: "switch_1".into(),
            input: InputKind::Keyboard,
            code: "Space".into(),
            device: None,
        };
        let broker = broker::spawn(vec![mapping]);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(listener, broker.clone(), 8));

        let (socket, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let (_, mut incoming) = socket.split();
        let hello = timeout(Duration::from_secs(1), incoming.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(hello.to_text().unwrap()).unwrap(),
            ServerMessage::Hello(_)
        ));

        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "physical".into(),
                action: Action::Pressed,
            }))
            .await
            .unwrap();
        let event = timeout(Duration::from_secs(1), incoming.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(event.to_text().unwrap()).unwrap(),
            ServerMessage::SwitchEvent(event) if event.sequence == 1
        ));

        task.abort();
    }
}
