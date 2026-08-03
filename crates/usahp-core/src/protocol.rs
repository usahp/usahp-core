use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: &str = "0.3.0";

/// Validate a confidence value: must be finite and in `[0.0, 100.0]`.
/// Returns `Some(value)` if valid, `None` if the value is invalid (NaN, Inf,
/// or out of range). Used at deserialization boundaries.
pub fn validate_confidence(value: f32) -> Option<f32> {
    if value.is_finite() && (0.0..=100.0).contains(&value) {
        Some(value)
    } else {
        None
    }
}

/// Validate an optional confidence value. `None` passes through (binary /
/// unknown). `Some(x)` is validated via [`validate_confidence`].
#[allow(dead_code)]
pub fn validate_confidence_opt(value: Option<f32>) -> Option<Option<f32>> {
    match value {
        None => Some(None),
        Some(v) => validate_confidence(v).map(Some),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SwitchState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwitchSnapshot {
    pub switch_id: String,
    pub state: SwitchState,
    /// Current confidence for this switch. `None` for binary sources or
    /// when the broker has not received a confidence-bearing event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hello {
    pub protocol_version: String,
    pub switches: Vec<SwitchSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwitchEvent {
    pub protocol_version: String,
    pub sequence: u64,
    pub monotonic_us: u64,
    pub switch_id: String,
    pub action: Action,
    /// Analog activation confidence in `[0.0, 100.0]`.
    ///
    /// - `None` (field absent on the wire): the source does not report
    ///   confidence. No current hardware path produces this — keyboard,
    ///   gamepad, and simulator inputs always set `Some`.
    /// - `Some(100.0)`: binary switch pressed — maximum confidence.
    /// - `Some(0.0)`: binary switch released, or analog source reporting
    ///   genuine zero. This is **distinct from `None`**: the source actively
    ///   measured a zero probability, not "I don't know."
    /// - `Some(45.2)`: analog source (BCI, facial gesture, pressure) reporting
    ///   45.2% activation probability.
    ///
    /// Clients decide activation thresholds (e.g., only act when confidence >
    /// 85.0). The daemon never interprets confidence — it passes it through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "String", into = "String")]
pub enum RequestedMode {
    ExclusiveForeground,
    Unsupported(String),
}

impl From<String> for RequestedMode {
    fn from(value: String) -> Self {
        if value == "exclusive_foreground" {
            Self::ExclusiveForeground
        } else {
            Self::Unsupported(value)
        }
    }
}

impl From<RequestedMode> for String {
    fn from(value: RequestedMode) -> Self {
        match value {
            RequestedMode::ExclusiveForeground => "exclusive_foreground".into(),
            RequestedMode::Unsupported(value) => value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handshake {
    pub protocol_version: String,
    pub app_id: String,
    pub requested_mode: RequestedMode,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HandshakeRejectionReason {
    ProtocolMismatch,
    InvalidAppId,
    UnsupportedMode,
    SessionBusy,
    CaptureUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HandshakeResponse {
    Accepted {
        protocol_version: String,
        session_id: String,
        heartbeat_interval_ms: u32,
        missed_heartbeat_limit: u32,
    },
    Rejected {
        protocol_version: String,
        reason: HandshakeRejectionReason,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionRevocationReason {
    HeartbeatTimeout,
    QueueOverflow,
    ExplicitRevocation,
    FocusLost,
    EscapeHatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRevoked {
    pub protocol_version: String,
    pub session_id: String,
    pub reason: SessionRevocationReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello(Hello),
    SwitchEvent(SwitchEvent),
    HandshakeResponse(HandshakeResponse),
    SessionRevoked(SessionRevoked),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Handshake(Handshake),
    Heartbeat { session_id: String },
}

pub fn valid_app_id(app_id: &str) -> bool {
    (1..=128).contains(&app_id.len())
        && app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_to_versioned_tagged_json() {
        let value = serde_json::to_value(ServerMessage::SwitchEvent(SwitchEvent {
            protocol_version: PROTOCOL_VERSION.into(),
            sequence: 42,
            monotonic_us: 1_843_201,
            switch_id: "switch_1".into(),
            action: Action::Pressed,
            confidence: Some(100.0),
        }))
        .unwrap();
        assert_eq!(value["type"], "switch_event");
        assert_eq!(value["protocol_version"], "0.3.0");
        assert_eq!(value["action"], "pressed");
        assert_eq!(value["confidence"], 100.0);
    }

    #[test]
    fn confidence_none_is_absent_from_wire() {
        let value = serde_json::to_value(SwitchEvent {
            protocol_version: PROTOCOL_VERSION.into(),
            sequence: 1,
            monotonic_us: 0,
            switch_id: "s".into(),
            action: Action::Pressed,
            confidence: None,
        })
        .unwrap();
        assert!(
            value.get("confidence").is_none(),
            "None confidence must be absent from JSON"
        );
    }

    #[test]
    fn confidence_some_zero_is_present_on_wire() {
        let value = serde_json::to_value(SwitchEvent {
            protocol_version: PROTOCOL_VERSION.into(),
            sequence: 1,
            monotonic_us: 0,
            switch_id: "s".into(),
            action: Action::Released,
            confidence: Some(0.0),
        })
        .unwrap();
        assert_eq!(
            value["confidence"], 0.0,
            "Some(0.0) must be present — distinct from None"
        );
    }

    #[test]
    fn missing_confidence_deserializes_to_none() {
        let json = r#"{"type":"switch_event","protocol_version":"0.3.0","sequence":1,"monotonic_us":0,"switch_id":"s","action":"pressed"}"#;
        let parsed: ServerMessage = serde_json::from_str(json).unwrap();
        let ServerMessage::SwitchEvent(ev) = parsed else {
            panic!()
        };
        assert_eq!(ev.confidence, None);
    }

    #[test]
    fn confidence_carried_through_snapshot() {
        let snap = SwitchSnapshot {
            switch_id: "s".into(),
            state: SwitchState::Pressed,
            confidence: Some(72.5),
        };
        let value = serde_json::to_value(&snap).unwrap();
        assert_eq!(value["confidence"], 72.5);
        let back: SwitchSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(back.confidence, Some(72.5));
    }

    #[test]
    fn validate_confidence_rejects_invalid() {
        assert_eq!(validate_confidence(0.0), Some(0.0));
        assert_eq!(validate_confidence(100.0), Some(100.0));
        assert_eq!(validate_confidence(50.5), Some(50.5));
        assert_eq!(validate_confidence(-0.1), None);
        assert_eq!(validate_confidence(100.1), None);
        assert_eq!(validate_confidence(f32::NAN), None);
        assert_eq!(validate_confidence(f32::INFINITY), None);
        assert_eq!(validate_confidence(f32::NEG_INFINITY), None);
    }

    #[test]
    fn accepted_and_rejected_responses_have_distinct_shapes() {
        let accepted = serde_json::to_value(HandshakeResponse::Accepted {
            protocol_version: PROTOCOL_VERSION.into(),
            session_id: "random-session".into(),
            heartbeat_interval_ms: 500,
            missed_heartbeat_limit: 3,
        })
        .unwrap();
        assert_eq!(accepted["status"], "ACCEPTED");
        assert!(accepted.get("reason").is_none());

        let rejected = serde_json::to_value(HandshakeResponse::Rejected {
            protocol_version: PROTOCOL_VERSION.into(),
            reason: HandshakeRejectionReason::SessionBusy,
        })
        .unwrap();
        assert_eq!(rejected["reason"], "SESSION_BUSY");
        assert!(rejected.get("session_id").is_none());
    }

    #[test]
    fn heartbeat_includes_session_id() {
        let value = serde_json::to_value(ClientMessage::Heartbeat {
            session_id: "opaque".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"type":"heartbeat","session_id":"opaque"})
        );
    }

    #[test]
    fn app_ids_are_strict_ascii_identifiers() {
        for valid in ["app", "org.example.Switch_1", "a-b"] {
            assert!(valid_app_id(valid));
        }
        for invalid in ["", "has space", "café", "slash/app"] {
            assert!(!valid_app_id(invalid));
        }
        assert!(!valid_app_id(&"a".repeat(129)));
    }

    #[test]
    fn unsupported_mode_deserializes_for_a_typed_rejection() {
        let message: ClientMessage = serde_json::from_str(
            r#"{"type":"handshake","protocol_version":"0.2.0","app_id":"app","requested_mode":"shared"}"#,
        )
        .unwrap();
        assert!(matches!(
            message,
            ClientMessage::Handshake(Handshake {
                requested_mode: RequestedMode::Unsupported(mode),
                ..
            }) if mode == "shared"
        ));
    }
}
