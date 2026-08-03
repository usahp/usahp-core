use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
};
use usahp_core::Config;

use crate::{
    broker::{self, BrokerCommand},
    input::{self, CaptureControl},
    management::{ActiveSessionSnapshot, ConnectionSnapshot},
    server,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServiceSnapshot {
    pub phase: ServicePhase,
    pub config_path: String,
    pub address: String,
    pub capture_enabled: bool,
    pub switches: Vec<usahp_core::SwitchSnapshot>,
    pub connections: Vec<ConnectionSnapshot>,
    pub active_session: Option<ActiveSessionSnapshot>,
    pub error: Option<String>,
}

struct ServerTask {
    running: watch::Sender<bool>,
    task: JoinHandle<Result<()>>,
}

pub struct ServiceSupervisor {
    config_path: PathBuf,
    config: Config,
    broker: tokio::sync::mpsc::Sender<BrokerCommand>,
    broker_snapshot: watch::Receiver<crate::management::BrokerSnapshot>,
    phase: ServicePhase,
    error: Option<String>,
    server: Option<ServerTask>,
}

pub type SharedService = Arc<Mutex<ServiceSupervisor>>;

impl ServiceSupervisor {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let config_path = path.as_ref().to_path_buf();
        let config = validate_config(&config_path)?;

        let mut broker_mappings = config.mappings.clone();
        if config.simulator.stdin {
            broker_mappings.extend(crate::simulator::mappings_for(&config.mappings));
        }
        let capture = CaptureControl::new_enabled();
        let broker_handle = broker::spawn_managed(broker_mappings, capture.clone());
        input::spawn(
            &config.mappings,
            broker_handle.commands.clone(),
            capture.clone(),
        )?;
        let (reply, result) = oneshot::channel();
        broker_handle
            .commands
            .send(BrokerCommand::SetRunning {
                running: false,
                reply,
            })
            .await
            .context("broker stopped while initializing service")?;
        result
            .await
            .context("broker did not acknowledge initial pause")?
            .map_err(anyhow::Error::msg)?;

        #[cfg(target_os = "macos")]
        crate::focus_watcher::spawn(broker_handle.commands.clone());

        Ok(Self {
            config_path,
            config,
            broker: broker_handle.commands,
            broker_snapshot: broker_handle.snapshots,
            phase: ServicePhase::Stopped,
            error: None,
            server: None,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.phase == ServicePhase::Running {
            return Ok(());
        }
        self.phase = ServicePhase::Starting;
        self.error = None;
        let listener = match TcpListener::bind(self.config.server.address()).await {
            Ok(listener) => listener,
            Err(error) => {
                return self.fail(
                    anyhow::Error::new(error).context("could not bind loopback WebSocket server"),
                );
            }
        };
        if let Err(error) = self.set_broker_running(true).await {
            return self.fail(error);
        }
        let (running, receiver) = watch::channel(true);
        let broker = self.broker.clone();
        let capacity = self.config.server.client_queue_capacity;
        let task = tokio::spawn(server::serve_until_stopped(
            listener, broker, capacity, receiver,
        ));
        self.server = Some(ServerTask { running, task });
        self.phase = ServicePhase::Running;
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.phase == ServicePhase::Stopped {
            return Ok(());
        }
        self.phase = ServicePhase::Stopping;
        self.error = None;
        let broker_result = self.set_broker_running(false).await;
        if let Some(server) = self.server.take() {
            let _ = server.running.send(false);
            match server.task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "USAHP server stopped with error"),
                Err(error) => tracing::warn!(%error, "USAHP server task failed"),
            }
        }
        match broker_result {
            Ok(()) => {
                self.phase = ServicePhase::Stopped;
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }

    pub fn snapshot(&self) -> ServiceSnapshot {
        let broker = self.broker_snapshot.borrow().clone();
        ServiceSnapshot {
            phase: self.phase,
            config_path: self.config_path.display().to_string(),
            address: format!(
                "{}:{}",
                self.config.server.address().0,
                self.config.server.address().1
            ),
            capture_enabled: broker.capture_enabled,
            switches: broker.switches,
            connections: broker.connections,
            active_session: broker.active_session,
            error: self.error.clone(),
        }
    }

    async fn set_broker_running(&self, running: bool) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.broker
            .send(BrokerCommand::SetRunning { running, reply })
            .await
            .context("broker stopped")?;
        result
            .await
            .context("broker did not acknowledge service command")?
            .map_err(anyhow::Error::msg)
    }

    fn fail<T>(&mut self, error: anyhow::Error) -> Result<T> {
        self.phase = ServicePhase::Error;
        self.error = Some(format!("{error:#}"));
        Err(error)
    }
}

pub fn validate_config(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref();
    let config =
        Config::load(path).with_context(|| format!("failed to load {}", path.display()))?;
    input::validate(&config.mappings)?;
    ensure_loopback(&config)?;
    Ok(config)
}

pub async fn run_headless(config_path: PathBuf) -> Result<()> {
    let mut service = ServiceSupervisor::load(&config_path).await?;
    if service.config.simulator.stdin {
        crate::simulator::spawn_stdin(service.broker.clone(), &service.config.mappings);
    }
    service.start().await?;
    tokio::signal::ctrl_c()
        .await
        .context("could not listen for Ctrl+C")?;
    service.stop().await
}

pub async fn stop_shared(service: &SharedService) -> Result<()> {
    service.lock().await.stop().await
}

pub fn require_config_path(path: Option<PathBuf>) -> Result<PathBuf> {
    path.ok_or_else(|| anyhow::anyhow!("no USAHP configuration selected"))
}

pub fn ensure_loopback(config: &Config) -> Result<()> {
    if !config.server.address().0.is_loopback() {
        bail!("USAHP Control requires a loopback server address");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_config(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("usahp-{name}-{}.toml", std::process::id()))
    }

    #[test]
    fn validates_selected_configuration_without_starting_capture() {
        let path = temporary_config("valid-control-config");
        std::fs::write(
            &path,
            r#"
[server]
port = 0

[[mappings]]
id = "space"
switch_id = "switch_1"
input = "keyboard"
code = "Space"
"#,
        )
        .unwrap();
        let config = validate_config(&path).unwrap();
        assert_eq!(config.mappings[0].switch_id, "switch_1");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reports_missing_and_invalid_selected_configuration() {
        let missing = temporary_config("missing-control-config");
        assert!(validate_config(&missing).is_err());

        let invalid = temporary_config("invalid-control-config");
        std::fs::write(&invalid, "mappings = []").unwrap();
        assert!(validate_config(&invalid).is_err());
        std::fs::remove_file(invalid).unwrap();
    }
}
