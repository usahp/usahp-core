use std::collections::{BTreeMap, BTreeSet, HashMap};

use thiserror::Error;

use crate::{Action, Mapping, SwitchSnapshot, SwitchState};

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalTransition {
    pub switch_id: String,
    pub action: Action,
    pub confidence: Option<f32>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StateError {
    #[error("unknown physical mapping '{0}'")]
    UnknownMapping(String),
    #[error("mapping '{mapping_id}' is already {action:?}")]
    InvalidTransition { mapping_id: String, action: Action },
}

#[derive(Debug)]
pub struct SwitchStateMachine {
    mapping_to_switch: HashMap<String, String>,
    active_by_switch: BTreeMap<String, BTreeSet<String>>,
}

impl SwitchStateMachine {
    pub fn new(mappings: &[Mapping]) -> Self {
        let mapping_to_switch = mappings
            .iter()
            .map(|mapping| (mapping.id.clone(), mapping.switch_id.clone()))
            .collect();
        let mut active_by_switch = BTreeMap::new();
        for mapping in mappings {
            active_by_switch
                .entry(mapping.switch_id.clone())
                .or_default();
        }
        Self {
            mapping_to_switch,
            active_by_switch,
        }
    }

    pub fn apply(
        &mut self,
        mapping_id: &str,
        action: Action,
        confidence: Option<f32>,
    ) -> Result<Option<LogicalTransition>, StateError> {
        let switch_id = self
            .mapping_to_switch
            .get(mapping_id)
            .ok_or_else(|| StateError::UnknownMapping(mapping_id.to_owned()))?
            .clone();
        let active = self.active_by_switch.get_mut(&switch_id).unwrap();

        match action {
            Action::Pressed => {
                if !active.insert(mapping_id.to_owned()) {
                    return Err(StateError::InvalidTransition {
                        mapping_id: mapping_id.to_owned(),
                        action,
                    });
                }
                Ok((active.len() == 1).then_some(LogicalTransition {
                    switch_id,
                    action,
                    confidence,
                }))
            }
            Action::Released => {
                if !active.remove(mapping_id) {
                    return Err(StateError::InvalidTransition {
                        mapping_id: mapping_id.to_owned(),
                        action,
                    });
                }
                Ok(active.is_empty().then_some(LogicalTransition {
                    switch_id,
                    action,
                    confidence,
                }))
            }
        }
    }

    pub fn snapshots(&self) -> Vec<SwitchSnapshot> {
        self.active_by_switch
            .iter()
            .map(|(switch_id, active)| SwitchSnapshot {
                switch_id: switch_id.clone(),
                state: if active.is_empty() {
                    SwitchState::Released
                } else {
                    SwitchState::Pressed
                },
                confidence: None,
            })
            .collect()
    }

    /// Clears every active physical mapping and returns one ordered release per
    /// logical switch that was pressed.
    pub fn release_all(&mut self) -> Vec<LogicalTransition> {
        self.active_by_switch
            .iter_mut()
            .filter_map(|(switch_id, active)| {
                (!active.is_empty()).then(|| {
                    active.clear();
                    LogicalTransition {
                        switch_id: switch_id.clone(),
                        action: Action::Released,
                        confidence: Some(0.0),
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::InputKind;

    use super::*;

    fn mappings() -> Vec<Mapping> {
        ["space", "enter"]
            .into_iter()
            .map(|id| Mapping {
                id: id.into(),
                switch_id: "switch_1".into(),
                input: InputKind::Keyboard,
                code: id.into(),
                device: None,
            })
            .collect()
    }

    #[test]
    fn many_to_one_emits_only_outer_edges() {
        let mut state = SwitchStateMachine::new(&mappings());
        assert!(
            state
                .apply("space", Action::Pressed, None)
                .unwrap()
                .is_some()
        );
        assert!(
            state
                .apply("enter", Action::Pressed, None)
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .apply("space", Action::Released, None)
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .apply("enter", Action::Released, None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn rejects_duplicate_edges() {
        let mut state = SwitchStateMachine::new(&mappings());
        state.apply("space", Action::Pressed, None).unwrap();
        assert!(matches!(
            state.apply("space", Action::Pressed, None),
            Err(StateError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn release_all_clears_many_to_one_state_in_switch_order() {
        let mut all = mappings();
        all.push(Mapping {
            id: "z".into(),
            switch_id: "switch_0".into(),
            input: InputKind::Keyboard,
            code: "z".into(),
            device: None,
        });
        let mut state = SwitchStateMachine::new(&all);
        state.apply("space", Action::Pressed, None).unwrap();
        state.apply("enter", Action::Pressed, None).unwrap();
        state.apply("z", Action::Pressed, None).unwrap();
        let releases = state.release_all();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].switch_id, "switch_0");
        assert_eq!(releases[1].switch_id, "switch_1");
        assert!(
            state
                .snapshots()
                .iter()
                .all(|s| s.state == SwitchState::Released)
        );
        assert!(matches!(
            state.apply("space", Action::Released, None),
            Err(StateError::InvalidTransition { .. })
        ));
    }
}
