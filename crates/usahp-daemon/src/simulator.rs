use std::io::{self, BufRead};

use tokio::sync::mpsc;
use tracing::{info, warn};
use usahp_core::{Action, Mapping};

use crate::broker::{BrokerCommand, PhysicalEvent};

pub fn spawn_stdin(broker: mpsc::Sender<BrokerCommand>, mappings: &[Mapping]) {
    let known: std::collections::HashMap<String, String> = mappings
        .iter()
        .map(|mapping| {
            (
                mapping.switch_id.clone(),
                format!("sim:{}", mapping.switch_id),
            )
        })
        .collect();
    let simulation_mappings: Vec<_> = known
        .iter()
        .map(|(switch_id, id)| Mapping {
            id: id.clone(),
            switch_id: switch_id.clone(),
            input: usahp_core::InputKind::Keyboard,
            code: "simulator".into(),
            device: None,
        })
        .collect();

    // Simulator mappings are already included by main when enabled.
    debug_assert!(!simulation_mappings.is_empty());
    std::thread::spawn(move || {
        info!("simulator ready; enter `press <switch_id>` or `release <switch_id>`");
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let mut parts = line.split_whitespace();
            let action = match parts.next() {
                Some("press") => Action::Pressed,
                Some("release") => Action::Released,
                Some(other) => {
                    warn!(command = other, "unknown simulator command");
                    continue;
                }
                None => continue,
            };
            let Some(switch_id) = parts.next() else {
                warn!("simulator command requires a switch id");
                continue;
            };
            let Some(mapping_id) = known.get(switch_id) else {
                warn!(switch_id, "unknown simulated switch");
                continue;
            };
            if broker
                .blocking_send(BrokerCommand::Input(PhysicalEvent {
                    mapping_id: mapping_id.clone(),
                    action,
                    confidence: Some(if action == Action::Pressed {
                        100.0
                    } else {
                        0.0
                    }),
                }))
                .is_err()
            {
                break;
            }
        }
    });
}

pub fn mappings_for(mappings: &[Mapping]) -> Vec<Mapping> {
    let mut switch_ids = std::collections::BTreeSet::new();
    mappings
        .iter()
        .filter_map(|mapping| {
            switch_ids
                .insert(mapping.switch_id.clone())
                .then_some(Mapping {
                    id: format!("sim:{}", mapping.switch_id),
                    switch_id: mapping.switch_id.clone(),
                    input: usahp_core::InputKind::Keyboard,
                    code: "simulator".into(),
                    device: None,
                })
        })
        .collect()
}
