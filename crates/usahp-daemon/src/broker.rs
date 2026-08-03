use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tracing::{debug, info, warn};
use usahp_core::{
    Action, Handshake, HandshakeRejectionReason, HandshakeResponse, Hello, Mapping,
    PROTOCOL_VERSION, RequestedMode, ServerMessage, SessionRevocationReason, SessionRevoked,
    SwitchEvent, SwitchStateMachine, valid_app_id,
};
use uuid::Uuid;

use crate::input::CaptureControl;

pub const HEARTBEAT_INTERVAL_MS: u32 = 500;
pub const MISSED_HEARTBEAT_LIMIT: u32 = 3;
/// Escape hatch (Trigger A): if any switch is held continuously for this
/// duration during an active session, the session is force-revoked and
/// control returns to the OS. The user is never trapped.
pub const ESCAPE_HOLD_MS: u64 = 4000;

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalEvent {
    pub mapping_id: String,
    pub action: Action,
    pub confidence: Option<f32>,
}

#[derive(Debug)]
pub enum BrokerCommand {
    Input(PhysicalEvent),
    Register {
        sender: mpsc::Sender<Arc<ServerMessage>>,
        reply: oneshot::Sender<u64>,
    },
    Unregister(u64),
    Handshake {
        client_id: u64,
        handshake: Handshake,
    },
    Heartbeat {
        client_id: u64,
        session_id: String,
    },
    RevokeSession {
        reason: SessionRevocationReason,
    },
    FocusChanged {
        frontmost_pid: Option<u32>,
    },
}

struct Session {
    client_id: u64,
    session_id: String,
    pid: Option<u32>,
    last_heartbeat: Instant,
}

struct Runtime {
    state: SwitchStateMachine,
    capture: CaptureControl,
    started: Instant,
    sequence: u64,
    next_client: u64,
    clients: HashMap<u64, mpsc::Sender<Arc<ServerMessage>>>,
    session: Option<Session>,
    paused: bool,
    /// Per-switch press timestamps for escape-hatch monitoring. Entries
    /// exist while a switch is physically held; removed on release.
    escape_tracker: HashMap<String, Instant>,
}

pub fn spawn(mappings: Vec<Mapping>, capture: CaptureControl) -> mpsc::Sender<BrokerCommand> {
    let (sender, receiver) = mpsc::channel(1024);
    tokio::spawn(run(receiver, mappings, capture));
    sender
}

async fn run(
    mut receiver: mpsc::Receiver<BrokerCommand>,
    mappings: Vec<Mapping>,
    capture: CaptureControl,
) {
    let mut runtime = Runtime {
        state: SwitchStateMachine::new(&mappings),
        capture,
        started: Instant::now(),
        sequence: 0,
        next_client: 1,
        clients: HashMap::new(),
        session: None,
        paused: false,
        escape_tracker: HashMap::new(),
    };
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                match command {
                    BrokerCommand::Register { sender, reply } => runtime.register(sender, reply),
                    BrokerCommand::Unregister(client_id) => runtime.unregister(client_id).await,
                    BrokerCommand::Input(event) => runtime.input(event).await,
                    BrokerCommand::Handshake { client_id, handshake } => {
                        runtime.handshake(client_id, handshake).await;
                    }
                    BrokerCommand::Heartbeat { client_id, session_id } => {
                        if let Some(session) = runtime.session.as_mut()
                            && session.client_id == client_id
                            && session.session_id == session_id
                        {
                            session.last_heartbeat = Instant::now();
                        }
                    }
                    BrokerCommand::RevokeSession { reason } => runtime.revoke(reason).await,
                    BrokerCommand::FocusChanged { frontmost_pid } => {
                        let needs_revoke = runtime
                            .session
                            .as_ref()
                            .is_some_and(|session| {
                                session.pid.is_some_and(|pid| Some(pid) != frontmost_pid)
                            });
                        if needs_revoke {
                            runtime.revoke(SessionRevocationReason::FocusLost).await;
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                // Heartbeat timeout check.
                let timed_out = runtime.session.as_ref().is_some_and(|session| {
                    session.last_heartbeat.elapsed()
                        >= Duration::from_millis(
                            u64::from(HEARTBEAT_INTERVAL_MS) * u64::from(MISSED_HEARTBEAT_LIMIT),
                        )
                });
                if timed_out {
                    runtime.revoke(SessionRevocationReason::HeartbeatTimeout).await;
                    continue;
                }

                // Escape-hatch check (Trigger A): if any switch has been held
                // continuously for ESCAPE_HOLD_MS during an active session,
                // force-revoke. The user is never trapped.
                if runtime.session.is_some() {
                    let escape_switch = runtime
                        .escape_tracker
                        .iter()
                        .find_map(|(switch_id, pressed_at)| {
                            (pressed_at.elapsed()
                                >= Duration::from_millis(ESCAPE_HOLD_MS))
                            .then_some(switch_id.clone())
                        });
                    if let Some(switch_id) = escape_switch {
                        tracing::warn!(%switch_id, "escape hatch triggered — sustained hold exceeded {}ms", ESCAPE_HOLD_MS);
                        runtime.revoke(SessionRevocationReason::EscapeHatch).await;
                    }
                }
            }
        }
    }
}

impl Runtime {
    fn register(&mut self, sender: mpsc::Sender<Arc<ServerMessage>>, reply: oneshot::Sender<u64>) {
        let client_id = self.next_client;
        self.next_client += 1;
        let hello = Arc::new(ServerMessage::Hello(Hello {
            protocol_version: PROTOCOL_VERSION.into(),
            switches: self.state.snapshots(),
        }));
        if sender.try_send(hello).is_ok() {
            self.clients.insert(client_id, sender);
            let _ = reply.send(client_id);
        }
    }

    async fn unregister(&mut self, client_id: u64) {
        self.clients.remove(&client_id);
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.client_id == client_id)
        {
            info!(client_id, "managed session disconnected");
            self.revoke(SessionRevocationReason::ExplicitRevocation)
                .await;
        }
    }

    async fn input(&mut self, event: PhysicalEvent) {
        if self.paused || !self.capture.enabled() {
            return;
        }
        // Validate confidence at the boundary: reject NaN/Inf/out-of-range,
        // treating invalid values as None (unknown).
        let confidence = event.confidence.and_then(usahp_core::validate_confidence);
        match self
            .state
            .apply(&event.mapping_id, event.action, confidence)
        {
            Ok(Some(transition)) => {
                // Escape-hatch tracking: record logical press timestamps so
                // the ticker can detect sustained holds. Events still flow
                // through to clients unmodified — this is a parallel monitor.
                match transition.action {
                    Action::Pressed => {
                        self.escape_tracker
                            .insert(transition.switch_id.clone(), Instant::now());
                    }
                    Action::Released => {
                        self.escape_tracker.remove(&transition.switch_id);
                    }
                }

                let message = self.event(
                    transition.switch_id,
                    transition.action,
                    transition.confidence,
                );
                if let Some(client_id) = self.session.as_ref().map(|session| session.client_id) {
                    let failed = self
                        .clients
                        .get(&client_id)
                        .is_none_or(|sender| sender.try_send(message).is_err());
                    if failed {
                        self.clients.remove(&client_id);
                        warn!(client_id, "managed session queue overflowed");
                        self.revoke(SessionRevocationReason::QueueOverflow).await;
                    }
                } else {
                    self.broadcast(message);
                }
            }
            Ok(None) => {}
            Err(error) => debug!(%error, "ignored invalid or stale physical transition"),
        }
    }

    async fn handshake(&mut self, client_id: u64, handshake: Handshake) {
        let rejection = if handshake.protocol_version != PROTOCOL_VERSION {
            Some(HandshakeRejectionReason::ProtocolMismatch)
        } else if !valid_app_id(&handshake.app_id) {
            Some(HandshakeRejectionReason::InvalidAppId)
        } else if !matches!(handshake.requested_mode, RequestedMode::ExclusiveForeground) {
            Some(HandshakeRejectionReason::UnsupportedMode)
        } else if self.session.is_some() {
            Some(HandshakeRejectionReason::SessionBusy)
        } else {
            None
        };
        if let Some(reason) = rejection {
            self.send_response(
                client_id,
                HandshakeResponse::Rejected {
                    protocol_version: PROTOCOL_VERSION.into(),
                    reason,
                },
            );
            return;
        }

        if self.paused
            && let Err(error) = self.capture.resume().await
        {
            warn!(%error, "capture reacquisition failed");
            self.send_response(
                client_id,
                HandshakeResponse::Rejected {
                    protocol_version: PROTOCOL_VERSION.into(),
                    reason: HandshakeRejectionReason::CaptureUnavailable,
                },
            );
            return;
        }

        // The first managed session is a routing boundary: legacy held state is
        // released globally before exclusive events begin.
        if !self.paused {
            let releases = self.release_messages();
            for release in releases {
                self.broadcast(release);
            }
        }
        if !self.clients.contains_key(&client_id) {
            let _ = self.capture.pause().await;
            self.paused = true;
            return;
        }
        self.paused = false;
        // Clear any stale press timestamps from passive/broadcast mode so the
        // escape-hatch clock starts fresh with the new session.
        self.escape_tracker.clear();
        let session_id = Uuid::new_v4().to_string();
        self.session = Some(Session {
            client_id,
            session_id: session_id.clone(),
            pid: handshake.pid,
            last_heartbeat: Instant::now(),
        });
        let accepted = self.send_response(
            client_id,
            HandshakeResponse::Accepted {
                protocol_version: PROTOCOL_VERSION.into(),
                session_id,
                heartbeat_interval_ms: HEARTBEAT_INTERVAL_MS,
                missed_heartbeat_limit: MISSED_HEARTBEAT_LIMIT,
            },
        );
        if !accepted {
            self.revoke(SessionRevocationReason::QueueOverflow).await;
            return;
        }
        info!(client_id, "managed session accepted");
    }

    async fn revoke(&mut self, reason: SessionRevocationReason) {
        let Some(session) = self.session.take() else {
            return;
        };
        // Clear escape-hatch tracking on any revocation.
        self.escape_tracker.clear();
        // Disable callbacks before releasing broker state so no physical edge can
        // race into the new paused epoch.
        if let Err(error) = self.capture.pause().await {
            warn!(%error, "capture backend pause failed");
        }
        self.paused = true;
        let releases = self.release_messages();
        let mut delivery_failed = false;
        if let Some(sender) = self.clients.get(&session.client_id) {
            for release in releases {
                delivery_failed |= sender.try_send(release).is_err();
            }
            delivery_failed |= sender
                .try_send(Arc::new(ServerMessage::SessionRevoked(SessionRevoked {
                    protocol_version: PROTOCOL_VERSION.into(),
                    session_id: session.session_id,
                    reason,
                })))
                .is_err();
        }
        if delivery_failed {
            self.clients.remove(&session.client_id);
        }
        warn!(
            client_id = session.client_id,
            ?reason,
            "managed session revoked"
        );
    }

    fn release_messages(&mut self) -> Vec<Arc<ServerMessage>> {
        self.state
            .release_all()
            .into_iter()
            .map(|transition| {
                self.event(
                    transition.switch_id,
                    transition.action,
                    transition.confidence,
                )
            })
            .collect()
    }

    fn event(
        &mut self,
        switch_id: String,
        action: Action,
        confidence: Option<f32>,
    ) -> Arc<ServerMessage> {
        self.sequence += 1;
        Arc::new(ServerMessage::SwitchEvent(SwitchEvent {
            protocol_version: PROTOCOL_VERSION.into(),
            sequence: self.sequence,
            monotonic_us: self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
            switch_id,
            action,
            confidence,
        }))
    }

    fn send_response(&mut self, client_id: u64, response: HandshakeResponse) -> bool {
        let sent = self.clients.get(&client_id).is_some_and(|sender| {
            sender
                .try_send(Arc::new(ServerMessage::HandshakeResponse(response)))
                .is_ok()
        });
        if !sent {
            self.clients.remove(&client_id);
        }
        sent
    }

    fn broadcast(&mut self, message: Arc<ServerMessage>) {
        self.clients
            .retain(|client_id, sender| match sender.try_send(message.clone()) {
                Ok(()) => true,
                Err(error) => {
                    warn!(client_id, %error, "disconnecting slow or closed passive client");
                    false
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::error::TryRecvError;
    use usahp_core::{InputKind, SwitchState};

    use super::*;

    fn mapping(id: &str, switch_id: &str) -> Mapping {
        Mapping {
            id: id.into(),
            switch_id: switch_id.into(),
            input: InputKind::Keyboard,
            code: "Space".into(),
            device: None,
        }
    }

    fn handshake(app_id: &str) -> Handshake {
        Handshake {
            protocol_version: PROTOCOL_VERSION.into(),
            app_id: app_id.into(),
            requested_mode: RequestedMode::ExclusiveForeground,
            pid: None,
        }
    }

    fn handshake_with_pid(app_id: &str, pid: u32) -> Handshake {
        Handshake {
            protocol_version: PROTOCOL_VERSION.into(),
            app_id: app_id.into(),
            requested_mode: RequestedMode::ExclusiveForeground,
            pid: Some(pid),
        }
    }

    async fn register(
        broker: &mpsc::Sender<BrokerCommand>,
        capacity: usize,
    ) -> (u64, mpsc::Receiver<Arc<ServerMessage>>) {
        let (sender, receiver) = mpsc::channel(capacity);
        let (reply, result) = oneshot::channel();
        broker
            .send(BrokerCommand::Register { sender, reply })
            .await
            .unwrap();
        (result.await.unwrap(), receiver)
    }

    #[tokio::test]
    async fn passive_clients_receive_snapshot_and_ordered_events() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (_, mut first) = register(&broker, 4).await;
        let (_, mut second) = register(&broker, 4).await;
        first.recv().await.unwrap();
        second.recv().await.unwrap();
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        for receiver in [&mut first, &mut second] {
            assert!(
                matches!(&*receiver.recv().await.unwrap(), ServerMessage::SwitchEvent(e) if e.sequence == 1)
            );
        }
    }

    #[tokio::test]
    async fn handshake_is_exclusive_and_busy_is_rejected() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (first_id, mut first) = register(&broker, 8).await;
        let (second_id, mut second) = register(&broker, 8).await;
        first.recv().await.unwrap();
        second.recv().await.unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: first_id,
                handshake: handshake("org.first"),
            })
            .await
            .unwrap();
        let accepted = first.recv().await.unwrap();
        assert!(matches!(
            &*accepted,
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted {
                heartbeat_interval_ms: 500,
                missed_heartbeat_limit: 3,
                ..
            })
        ));
        broker
            .send(BrokerCommand::Handshake {
                client_id: second_id,
                handshake: handshake("org.second"),
            })
            .await
            .unwrap();
        assert!(matches!(
            &*second.recv().await.unwrap(),
            ServerMessage::HandshakeResponse(HandshakeResponse::Rejected {
                reason: HandshakeRejectionReason::SessionBusy,
                ..
            })
        ));
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        assert!(matches!(
            &*first.recv().await.unwrap(),
            ServerMessage::SwitchEvent(_)
        ));
        tokio::task::yield_now().await;
        assert!(matches!(second.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn rejects_invalid_version_id_and_mode() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (id, mut receiver) = register(&broker, 8).await;
        receiver.recv().await.unwrap();
        let cases = [
            (
                Handshake {
                    protocol_version: "0.1".into(),
                    ..handshake("app")
                },
                HandshakeRejectionReason::ProtocolMismatch,
            ),
            (handshake("bad id"), HandshakeRejectionReason::InvalidAppId),
            (
                Handshake {
                    requested_mode: RequestedMode::Unsupported("shared".into()),
                    ..handshake("app")
                },
                HandshakeRejectionReason::UnsupportedMode,
            ),
        ];
        for (request, expected) in cases {
            broker
                .send(BrokerCommand::Handshake {
                    client_id: id,
                    handshake: request,
                })
                .await
                .unwrap();
            assert!(
                matches!(&*receiver.recv().await.unwrap(), ServerMessage::HandshakeResponse(HandshakeResponse::Rejected { reason, .. }) if *reason == expected)
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_releases_held_state_and_pauses_until_reacquired() {
        let capture = CaptureControl::default();
        let broker = spawn(vec![mapping("a", "switch_1")], capture.clone());
        let (id, mut receiver) = register(&broker, 16).await;
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        receiver.recv().await.unwrap();
        tokio::time::advance(Duration::from_millis(1_600)).await;
        tokio::task::yield_now().await;
        assert!(
            matches!(&*receiver.recv().await.unwrap(), ServerMessage::SwitchEvent(e) if e.action == Action::Released)
        );
        assert!(matches!(
            &*receiver.recv().await.unwrap(),
            ServerMessage::SessionRevoked(SessionRevoked {
                reason: SessionRevocationReason::HeartbeatTimeout,
                ..
            })
        ));
        assert!(!capture.enabled());

        let (_, mut snapshot) = register(&broker, 4).await;
        assert!(
            matches!(&*snapshot.recv().await.unwrap(), ServerMessage::Hello(Hello { switches, .. }) if switches[0].state == SwitchState::Released)
        );
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Released,
                confidence: Some(0.0),
            }))
            .await
            .unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        assert!(matches!(
            &*receiver.recv().await.unwrap(),
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted { .. })
        ));
        assert!(capture.enabled());
    }

    #[tokio::test(start_paused = true)]
    async fn matching_session_heartbeat_extends_the_lease() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (id, mut receiver) = register(&broker, 8).await;
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        let session_id = match &*receiver.recv().await.unwrap() {
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted {
                session_id, ..
            }) => session_id.clone(),
            _ => panic!("expected acceptance"),
        };
        tokio::time::advance(Duration::from_millis(1_000)).await;
        broker
            .send(BrokerCommand::Heartbeat {
                client_id: id,
                session_id,
            })
            .await
            .unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1_000)).await;
        tokio::task::yield_now().await;
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn reacquisition_failure_has_typed_rejection() {
        let capture = CaptureControl::default();
        let broker = spawn(vec![mapping("a", "switch_1")], capture.clone());
        let (id, mut receiver) = register(&broker, 8).await;
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::RevokeSession {
                reason: SessionRevocationReason::ExplicitRevocation,
            })
            .await
            .unwrap();
        receiver.recv().await.unwrap();
        capture.fail_resume_for_test(true);
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        assert!(matches!(
            &*receiver.recv().await.unwrap(),
            ServerMessage::HandshakeResponse(HandshakeResponse::Rejected {
                reason: HandshakeRejectionReason::CaptureUnavailable,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn session_queue_overflow_revokes_and_pauses_capture() {
        let capture = CaptureControl::default();
        let broker = spawn(vec![mapping("a", "switch_1")], capture.clone());
        let (id, mut receiver) = register(&broker, 1).await;
        receiver.recv().await.unwrap();
        broker
            .send(BrokerCommand::Handshake {
                client_id: id,
                handshake: handshake("app"),
            })
            .await
            .unwrap();
        // Leave acceptance queued, then overflow it with input.
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert!(receiver.try_recv().is_ok());
        tokio::task::yield_now().await;
        assert!(!capture.enabled());
        assert!(matches!(
            receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn passive_queue_overflow_disconnects_the_slow_client() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (_, mut receiver) = register(&broker, 1).await;
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert!(receiver.try_recv().is_ok());
        assert!(matches!(
            receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn in_process_delivery_p95_is_below_twenty_milliseconds() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (_, mut receiver) = register(&broker, 8).await;
        receiver.recv().await.unwrap();
        let mut samples = Vec::new();
        for index in 0..100 {
            let action = if index % 2 == 0 {
                Action::Pressed
            } else {
                Action::Released
            };
            let started = std::time::Instant::now();
            broker
                .send(BrokerCommand::Input(PhysicalEvent {
                    mapping_id: "a".into(),
                    action,
                    confidence: Some(if action == Action::Pressed {
                        100.0
                    } else {
                        0.0
                    }),
                }))
                .await
                .unwrap();
            receiver.recv().await.unwrap();
            samples.push(started.elapsed());
        }
        samples.sort_unstable();
        assert!(
            samples[94] < Duration::from_millis(20),
            "p95={:?}",
            samples[94]
        );
    }

    #[tokio::test]
    async fn focus_changed_revokes_session_with_mismatched_pid() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        // Handshake with PID 1000
        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake_with_pid("com.test.app", 1000),
            })
            .await
            .unwrap();
        rx.recv().await.unwrap(); // handshake response

        // Focus changes to a different PID
        broker
            .send(BrokerCommand::FocusChanged {
                frontmost_pid: Some(2000),
            })
            .await
            .unwrap();
        tokio::task::yield_now().await;

        // Client should receive SessionRevoked with FocusLost
        let msg = rx.try_recv().expect("should have a message");
        let ServerMessage::SessionRevoked(rev) = &*msg else {
            panic!("expected SessionRevoked, got {:?}", *msg);
        };
        assert_eq!(rev.reason, SessionRevocationReason::FocusLost);
    }

    #[tokio::test]
    async fn focus_changed_same_pid_keeps_session() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake_with_pid("com.test.app", 1000),
            })
            .await
            .unwrap();
        rx.recv().await.unwrap(); // handshake response

        // Focus changes but PID matches — should NOT revoke
        broker
            .send(BrokerCommand::FocusChanged {
                frontmost_pid: Some(1000),
            })
            .await
            .unwrap();
        tokio::task::yield_now().await;

        assert!(
            rx.try_recv().is_err(),
            "session should NOT be revoked when PID matches"
        );
    }

    #[tokio::test]
    async fn focus_changed_no_session_pid_does_not_revoke() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        // Handshake without PID (legacy/anonymous client)
        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake("com.test.app"), // pid: None
            })
            .await
            .unwrap();
        rx.recv().await.unwrap(); // handshake response

        // Focus changes — session should NOT be revoked (no PID to match)
        broker
            .send(BrokerCommand::FocusChanged {
                frontmost_pid: Some(9999),
            })
            .await
            .unwrap();
        tokio::task::yield_now().await;

        assert!(
            rx.try_recv().is_err(),
            "session should NOT be revoked when session has no PID"
        );
    }

    #[tokio::test]
    async fn escape_hatch_revokes_session_on_sustained_hold() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        // Establish a session.
        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake("com.test.app"),
            })
            .await
            .unwrap();
        let response = rx.recv().await.unwrap();
        let session_id = match &*response {
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted {
                session_id, ..
            }) => session_id.clone(),
            _ => panic!("expected ACCEPTED, got {:?}", *response),
        };

        // Press switch_1 — the escape tracker starts.
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        rx.try_recv().ok(); // consume switch_event

        // Keep the session alive with heartbeats while we wait for the escape
        // threshold (ESCAPE_HOLD_MS = 4000ms > heartbeat timeout 1500ms).
        let hb_broker = broker.clone();
        let hb_sid = session_id;
        let hb_task = tokio::spawn(async move {
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(400)).await;
                if hb_broker
                    .send(BrokerCommand::Heartbeat {
                        client_id,
                        session_id: hb_sid.clone(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Wait for the escape hatch to fire.
        let mut found_escape = false;
        for _ in 0..30 {
            if let Ok(Some(msg)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
            {
                if let ServerMessage::SessionRevoked(rev) = &*msg {
                    if rev.reason == SessionRevocationReason::EscapeHatch {
                        found_escape = true;
                        break;
                    }
                }
            }
        }
        hb_task.abort();
        assert!(
            found_escape,
            "expected EscapeHatch revocation after sustained hold"
        );
    }

    #[tokio::test]
    async fn escape_hatch_does_not_fire_if_switch_released_in_time() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake("com.test.app"),
            })
            .await
            .unwrap();
        let response = rx.recv().await.unwrap();
        let session_id = match &*response {
            ServerMessage::HandshakeResponse(HandshakeResponse::Accepted {
                session_id, ..
            }) => session_id.clone(),
            _ => panic!("expected ACCEPTED"),
        };

        // Press and release quickly (well under ESCAPE_HOLD_MS).
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        rx.try_recv().ok();

        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Released,
                confidence: Some(0.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        rx.try_recv().ok();

        // Keep alive with heartbeats past ESCAPE_HOLD_MS.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(4500);
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            broker
                .send(BrokerCommand::Heartbeat {
                    client_id,
                    session_id: session_id.clone(),
                })
                .await
                .ok();
            tokio::time::sleep(Duration::from_millis(400)).await;
        }

        // Session should still be alive — no EscapeHatch.
        let revoked = matches!(
            rx.try_recv(),
            Ok(msg) if matches!(&*msg, ServerMessage::SessionRevoked(rev) if rev.reason == SessionRevocationReason::EscapeHatch)
        );
        assert!(
            !revoked,
            "session should NOT be revoked if switch was released in time"
        );
    }

    #[tokio::test]
    async fn escape_hatch_tracker_cleared_on_new_session() {
        let broker = spawn(vec![mapping("a", "switch_1")], CaptureControl::default());
        let (client_id, mut rx) = register(&broker, 8).await;
        rx.recv().await.unwrap(); // hello

        // Press switch while in passive/broadcast mode (no session).
        broker
            .send(BrokerCommand::Input(PhysicalEvent {
                mapping_id: "a".into(),
                action: Action::Pressed,
                confidence: Some(100.0),
            }))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        rx.try_recv().ok(); // consume broadcast switch_event

        // Wait long enough that the stale timestamp would trip the escape hatch
        // if it were still in the tracker.
        tokio::time::sleep(Duration::from_millis(ESCAPE_HOLD_MS + 200)).await;

        // Now establish a session. The broker will broadcast release messages
        // for held switches before sending the HandshakeResponse.
        broker
            .send(BrokerCommand::Handshake {
                client_id,
                handshake: handshake("com.test.app"),
            })
            .await
            .unwrap();

        // Drain pre-session release broadcasts, then find Accepted.
        let mut session_id = None;
        for _ in 0..10 {
            let msg = rx.recv().await.unwrap();
            if let ServerMessage::HandshakeResponse(HandshakeResponse::Accepted {
                session_id: sid,
                ..
            }) = &*msg
            {
                session_id = Some(sid.clone());
                break;
            }
        }
        let session_id = session_id.expect("expected ACCEPTED");

        // Keep the session alive with heartbeats and verify no immediate
        // EscapeHatch revocation arrives.
        for _ in 0..5 {
            broker
                .send(BrokerCommand::Heartbeat {
                    client_id,
                    session_id: session_id.clone(),
                })
                .await
                .ok();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let revoked = matches!(
            rx.try_recv(),
            Ok(msg) if matches!(&*msg, ServerMessage::SessionRevoked(rev) if rev.reason == SessionRevocationReason::EscapeHatch)
        );
        assert!(
            !revoked,
            "stale escape_tracker entry from passive mode should not fire on new session"
        );
    }
}
