mod config;
mod protocol;
mod state;

pub use config::{Config, ConfigError, InputKind, Mapping, ServerConfig, SimulatorConfig};
pub use protocol::{
    Action, ClientMessage, Handshake, HandshakeRejectionReason, HandshakeResponse, Hello,
    PROTOCOL_VERSION, RequestedMode, ServerMessage, SessionRevocationReason, SessionRevoked,
    SwitchEvent, SwitchSnapshot, SwitchState, valid_app_id, validate_confidence,
};
pub use state::{LogicalTransition, StateError, SwitchStateMachine};
