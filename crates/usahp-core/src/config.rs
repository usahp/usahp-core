use std::{collections::HashSet, fs, net::Ipv4Addr, path::Path};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub simulator: SimulatorConfig,
    pub mappings: Vec<Mapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub port: u16,
    pub client_queue_capacity: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 7312,
            client_queue_capacity: 256,
        }
    }
}

impl ServerConfig {
    pub fn address(&self) -> (Ipv4Addr, u16) {
        (Ipv4Addr::LOCALHOST, self.port)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimulatorConfig {
    pub stdin: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub id: String,
    pub switch_id: String,
    pub input: InputKind,
    pub code: String,
    pub device: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Keyboard,
    Gamepad,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read configuration: {0}")]
    Read(#[from] std::io::Error),
    #[error("invalid TOML configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("configuration must include at least one mapping")]
    NoMappings,
    #[error("mapping id '{0}' is duplicated")]
    DuplicateMappingId(String),
    #[error("mapping '{0}' has an empty switch_id")]
    EmptySwitchId(String),
    #[error("mapping '{0}' has an empty code")]
    EmptyCode(String),
    #[error("client_queue_capacity must be greater than zero")]
    ZeroQueueCapacity,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.mappings.is_empty() {
            return Err(ConfigError::NoMappings);
        }
        if self.server.client_queue_capacity == 0 {
            return Err(ConfigError::ZeroQueueCapacity);
        }

        let mut ids = HashSet::new();
        for mapping in &self.mappings {
            if !ids.insert(mapping.id.clone()) {
                return Err(ConfigError::DuplicateMappingId(mapping.id.clone()));
            }
            if mapping.switch_id.trim().is_empty() {
                return Err(ConfigError::EmptySwitchId(mapping.id.clone()));
            }
            if mapping.code.trim().is_empty() {
                return Err(ConfigError::EmptyCode(mapping.id.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
        [server]
        port = 7312
        client_queue_capacity = 8

        [simulator]
        stdin = true

        [[mappings]]
        id = "space"
        switch_id = "switch_1"
        input = "keyboard"
        code = "Space"

        [[mappings]]
        id = "enter"
        switch_id = "switch_1"
        input = "keyboard"
        code = "Return"
    "#;

    #[test]
    fn parses_many_to_one_mapping() {
        let config = Config::parse(VALID).unwrap();
        assert_eq!(config.mappings.len(), 2);
        assert_eq!(config.mappings[0].switch_id, config.mappings[1].switch_id);
        assert!(config.simulator.stdin);
    }

    #[test]
    fn rejects_duplicate_mapping_ids() {
        let duplicated = VALID.replace("id = \"enter\"", "id = \"space\"");
        assert!(matches!(
            Config::parse(&duplicated),
            Err(ConfigError::DuplicateMappingId(id)) if id == "space"
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let invalid = VALID.replace("port = 7312", "port = 7312\nremote = true");
        assert!(matches!(
            Config::parse(&invalid),
            Err(ConfigError::Parse(_))
        ));
    }
}
