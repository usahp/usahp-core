use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: &str = "0.1";

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchSnapshot {
    pub switch_id: String,
    pub state: SwitchState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub protocol_version: String,
    pub switches: Vec<SwitchSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchEvent {
    pub protocol_version: String,
    pub sequence: u64,
    pub monotonic_us: u64,
    pub switch_id: String,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello(Hello),
    SwitchEvent(SwitchEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_to_versioned_tagged_json() {
        let message = ServerMessage::SwitchEvent(SwitchEvent {
            protocol_version: PROTOCOL_VERSION.into(),
            sequence: 42,
            monotonic_us: 1_843_201,
            switch_id: "switch_1".into(),
            action: Action::Pressed,
        });
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["type"], "switch_event");
        assert_eq!(value["protocol_version"], "0.1");
        assert_eq!(value["action"], "pressed");
    }
}
