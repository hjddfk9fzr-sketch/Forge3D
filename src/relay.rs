//! Relay gates use the existing component map, so prefab properties have ordinary override paths.
use crate::{
    physics::{Event, EventKind},
    runtime::{Object, Scene},
    scripting::Effect,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const COMPONENT: &str = "RelayGate";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Transmitter,
    Exit,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayGate {
    pub role: Role,
    #[serde(default)]
    pub destination: String,
}

impl RelayGate {
    pub fn read(object: &Object) -> Result<Option<Self>, String> {
        object
            .components
            .get(COMPONENT)
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn attach(&self, object: &mut Object) {
        object
            .components
            .insert(COMPONENT.into(), serde_json::json!(self));
    }

    pub fn validate(
        &self,
        object: &Object,
        has_scene: impl Fn(&str) -> bool,
    ) -> Result<(), String> {
        if !object.body.as_ref().is_some_and(|body| body.sensor) {
            return Err("RelayGate needs a trigger (sensor) collider.".into());
        }
        if self.role == Role::Exit && !has_scene(&self.destination) {
            return Err(format!(
                "RelayGate exit references missing scene '{}'. Choose a destination.",
                self.destination
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Relays {
    gates: HashMap<u64, RelayGate>,
    characters: HashSet<u64>,
    charged: HashSet<u64>,
}

impl Relays {
    pub fn from_scene(scene: &Scene) -> Result<Self, String> {
        let mut state = Self::default();
        for object in &scene.objects {
            if let Some(gate) = RelayGate::read(object)
                .map_err(|e| format!("{} #{}: {e}", object.name, object.id))?
            {
                state.gates.insert(object.id, gate);
            }
            if object.is_character() {
                state.characters.insert(object.id);
            }
        }
        Ok(state)
    }

    pub fn preserve_charge(&mut self, old: &Self) {
        self.charged
            .extend(old.charged.intersection(&self.characters).copied());
    }

    pub fn events(&mut self, events: &[Event], scene: &Scene) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.gates.is_empty() {
            return effects;
        }
        for event in events.iter().filter(|e| e.kind == EventKind::TriggerEnter) {
            for (gate, actor) in [(event.a, event.b), (event.b, event.a)] {
                let Some(gate) = self.gates.get(&gate) else {
                    continue;
                };
                if !self.characters.contains(&actor) {
                    continue;
                }
                match gate.role {
                    Role::Transmitter if self.charged.insert(actor) => {}
                    Role::Exit if self.charged.contains(&actor) => {
                        effects.push(Effect::Scene(gate.destination.clone()))
                    }
                    _ => continue,
                }
                if scene.presentation.sounds.contains_key("signal") {
                    effects.push(Effect::Sound("signal".into()));
                }
            }
        }
        effects
    }
}
