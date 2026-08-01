mod config;
mod protocol;
mod state;

pub use config::{Config, ConfigError, InputKind, Mapping, ServerConfig, SimulatorConfig};
pub use protocol::{
    Action, Hello, PROTOCOL_VERSION, ServerMessage, SwitchEvent, SwitchSnapshot, SwitchState,
};
pub use state::{LogicalTransition, StateError, SwitchStateMachine};
