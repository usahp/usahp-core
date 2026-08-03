use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, info, warn};
use usahp_core::{ClientMessage, ServerMessage};

use crate::broker::BrokerCommand;

pub async fn serve(
    listener: TcpListener,
    broker: mpsc::Sender<BrokerCommand>,
    queue_capacity: usize,
) -> Result<()> {
    let (_running, receiver) = watch::channel(true);
    serve_until_stopped(listener, broker, queue_capacity, receiver).await
}

pub async fn serve_until_stopped(
    listener: TcpListener,
    broker: mpsc::Sender<BrokerCommand>,
    queue_capacity: usize,
    mut running: watch::Receiver<bool>,
) -> Result<()> {
    info!(address = %listener.local_addr()?, "WebSocket server listening");
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => Some(accepted?),
            changed = running.changed() => {
                if changed.is_err() || !*running.borrow() { None } else { continue }
            }
        };
        let Some((stream, peer)) = accepted else {
            break;
        };
        let broker = broker.clone();
        let client_running = running.clone();
        tokio::spawn(async move {
            if let Err(error) =
                handle_client(stream, peer, broker, queue_capacity, client_running).await
            {
                warn!(%peer, %error, "client connection ended with error");
            }
        });
    }
    Ok(())
}

async fn handle_client(
    stream: TcpStream,
    peer: SocketAddr,
    broker: mpsc::Sender<BrokerCommand>,
    queue_capacity: usize,
    mut running: watch::Receiver<bool>,
) -> Result<()> {
    let websocket = accept_async(stream)
        .await
        .context("WebSocket handshake failed")?;
    let (mut ws_sink, mut ws_stream) = websocket.split();
    let (sender, mut receiver) = mpsc::channel::<Arc<ServerMessage>>(queue_capacity);
    let (reply, registered) = oneshot::channel();
    broker
        .send(BrokerCommand::RegisterClient {
            sender,
            peer,
            reply,
        })
        .await
        .context("broker stopped")?;
    let client_id = registered.await.context("broker rejected registration")?;
    debug!(client_id, %peer, "client connected");

    loop {
        tokio::select! {
            changed = running.changed() => {
                if changed.is_err() || !*running.borrow() { break; }
            }
            // Daemon → client: forward broker messages.
            msg = receiver.recv() => {
                let Some(msg) = msg else { break; };
                let json = serde_json::to_string(&*msg)?;
                if ws_sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            // Client → daemon: read handshake / heartbeat.
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(m)) if m.is_text() => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(
                            m.to_text().unwrap_or(""),
                        ) {
                            match client_msg {
                                ClientMessage::Handshake(hs) => {
                                    let _ = broker
                                        .send(BrokerCommand::Handshake {
                                            client_id,
                                            handshake: hs,
                                        })
                                        .await;
                                }
                                ClientMessage::Heartbeat { session_id } => {
                                    let _ = broker
                                        .send(BrokerCommand::Heartbeat { client_id, session_id })
                                        .await;
                                }
                            }
                        }
                    }
                    Some(Ok(_)) => {} // binary / ping / pong — ignore
                    _ => break,        // error or connection closed
                }
            }
        }
    }

    let _ = broker.send(BrokerCommand::Unregister(client_id)).await;
    debug!(client_id, %peer, "client disconnected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::connect_async;
    use usahp_core::{
        Action, ClientMessage, Handshake, HandshakeRejectionReason, HandshakeResponse, InputKind,
        Mapping, PROTOCOL_VERSION, RequestedMode, ServerMessage,
    };

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
        let broker = broker::spawn(vec![mapping], crate::input::CaptureControl::default());
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
                confidence: Some(100.0),
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

    #[tokio::test]
    async fn websocket_managed_session_rejects_competitor_and_allows_reconnect() {
        let mapping = Mapping {
            id: "physical".into(),
            switch_id: "switch_1".into(),
            input: InputKind::Keyboard,
            code: "Space".into(),
            device: None,
        };
        let broker = broker::spawn(vec![mapping], crate::input::CaptureControl::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(listener, broker, 8));
        let (mut first, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let (mut second, _) = connect_async(format!("ws://{address}")).await.unwrap();
        first.next().await.unwrap().unwrap();
        second.next().await.unwrap().unwrap();

        let request = |app_id: &str| {
            ClientMessage::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION.into(),
                app_id: app_id.into(),
                requested_mode: RequestedMode::ExclusiveForeground,
                pid: None,
            })
        };
        first
            .send(Message::Text(
                serde_json::to_string(&request("org.first")).unwrap().into(),
            ))
            .await
            .unwrap();
        let accepted: ServerMessage =
            serde_json::from_str(first.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(
            accepted,
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted { .. })
        ));

        second
            .send(Message::Text(
                serde_json::to_string(&request("org.second"))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
        let busy: ServerMessage =
            serde_json::from_str(second.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(
            busy,
            ServerMessage::HandshakeResponse(HandshakeResponse::Rejected {
                reason: HandshakeRejectionReason::SessionBusy,
                ..
            })
        ));

        first.close(None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        second
            .send(Message::Text(
                serde_json::to_string(&request("org.second"))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
        let reaccepted: ServerMessage =
            serde_json::from_str(second.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(
            reaccepted,
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted { .. })
        ));
        task.abort();
    }

    #[tokio::test]
    async fn stopped_server_disconnects_clients_and_releases_listener() {
        let mapping = Mapping {
            id: "physical".into(),
            switch_id: "switch_1".into(),
            input: InputKind::Keyboard,
            code: "Space".into(),
            device: None,
        };
        let broker = broker::spawn(vec![mapping], crate::input::CaptureControl::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (running, receiver) = watch::channel(true);
        let task = tokio::spawn(serve_until_stopped(listener, broker, 8, receiver));
        let (mut socket, _) = connect_async(format!("ws://{address}")).await.unwrap();
        socket.next().await.unwrap().unwrap();

        running.send(false).unwrap();
        task.await.unwrap().unwrap();
        let closed = timeout(Duration::from_secs(1), socket.next())
            .await
            .unwrap();
        assert!(matches!(
            closed,
            None | Some(Err(_)) | Some(Ok(Message::Close(_)))
        ));
        TcpListener::bind(address).await.unwrap();
    }
}
