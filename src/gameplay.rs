//! Systems read one scene snapshot, in object order then component-name order.
use crate::runtime::{Object, Scene, Shape};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn slot<T: Clone + PartialEq>(assets: &mut Vec<T>, asset: &T) -> usize {
    if let Some(index) = assets.iter().position(|existing| existing == asset) {
        return index;
    }
    assets.push(asset.clone());
    assets.len() - 1
}

pub(crate) fn instantiate(
    prefab: &Scene,
    result: &mut Scene,
    parent: Option<u64>,
    id: &str,
) -> Result<u64, String> {
    result.assign_ids()?;
    let mut ids = BTreeMap::new();
    let mut root = None;
    for (index, _, _) in prefab.object_order() {
        let mut object = prefab.objects[index].clone();
        let original = object.id;
        match object.parent {
            Some(old) => object.parent = Some(*ids.get(&old).ok_or("Missing prefab parent.")?),
            None if root.is_none() => object.parent = parent,
            None => return Err(format!("Prefab '{id}' has multiple roots.")),
        }
        if let Shape::Model(index) = object.shape {
            object.shape = Shape::Model(slot(
                &mut result.models,
                prefab.models.get(index).ok_or("Missing prefab model.")?,
            ));
        }
        if let Some(index) = object.script_asset {
            object.script_asset = Some(slot(
                &mut result.scripts,
                prefab.scripts.get(index).ok_or("Missing prefab script.")?,
            ));
        }
        object.id = result.next_id;
        result.next_id = result
            .next_id
            .checked_add(1)
            .ok_or("Scene ran out of object IDs.")?;
        if root.is_none() {
            root = Some(object.id);
        }
        ids.insert(original, object.id);
        result.objects.push(object);
    }
    let root = root.ok_or_else(|| format!("Prefab '{id}' is empty."))?;
    Ok(root)
}

#[derive(Clone, Debug)]
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub name: String,
    pub value: Value,
}

pub enum Command {
    Spawn {
        prefab: String,
        position: [f32; 3],
    },
    Despawn(u64),
    Set {
        id: u64,
        component: String,
        value: Value,
    },
    Send(Message),
    PlaySound(String),
    SetSpin {
        id: u64,
        degrees_per_second: f32,
    },
    PlayAnimation {
        id: u64,
        clip: String,
    },
}

pub struct Context<'a> {
    pub scene: &'a Scene,
    pub object: &'a Object,
    pub data: &'a Value,
    /// Clip events emitted during the current tick.
    pub animation_events: &'a [crate::animation::Event],
    pub messages: &'a [Message],
    pub dt: f32,
    pub actions: crate::controls::GameActions,
}

pub type System = fn(Context<'_>, &mut Vec<Command>) -> Result<(), String>;

#[derive(Default)]
pub struct Systems(BTreeMap<String, System>);

impl Systems {
    pub fn register(&mut self, component: &str, system: System) -> Result<(), String> {
        if component.is_empty() || self.0.contains_key(component) {
            return Err(format!("Empty or duplicate component name '{component}'."));
        }
        self.0.insert(component.into(), system);
        Ok(())
    }

    pub(crate) fn tick(
        &self,
        play: &mut crate::runtime::Playback,
        prefabs: &BTreeMap<String, Scene>,
        messages: &mut Vec<Message>,
        dt: f32,
    ) -> Result<(), String> {
        let animation_events = play.take_animation_events();
        let incoming = std::mem::take(messages);
        let mut commands = Vec::new();
        for object in &play.scene().objects {
            for (key, data) in &object.components {
                if let Some(system) = self.0.get(key) {
                    system(
                        Context {
                            scene: play.scene(),
                            object,
                            data,
                            messages: &incoming,
                            animation_events: &animation_events,
                            actions: play.actions,
                            dt,
                        },
                        &mut commands,
                    )?;
                }
            }
        }
        if commands.is_empty() {
            return Ok(());
        }
        let mut next = play.scene().clone();
        let mut sounds = Vec::new();
        let mut outgoing = Vec::new();
        let mut changed = false;
        for command in commands {
            match command {
                Command::Spawn { prefab, position } => {
                    let template = prefabs
                        .get(&prefab)
                        .ok_or_else(|| format!("Unknown runtime prefab '{prefab}'."))?;
                    let id = instantiate(template, &mut next, None, &prefab)?;
                    let index = next.object_index(id)?;
                    next.objects[index].position = position;
                    changed = true;
                }
                Command::Despawn(id) => {
                    next.delete_object(id, true)?;
                    changed = true;
                }
                Command::Set {
                    id,
                    component,
                    value,
                } => {
                    let index = next.object_index(id)?;
                    next.objects[index].components.insert(component, value);
                    changed = true;
                }
                Command::PlayAnimation { id, clip } => {
                    let index = next.object_index(id)?;
                    let object = &mut next.objects[index];
                    if object.animation.as_ref().map_or("", |p| p.clip.as_str()) != clip {
                        if object.animation_blend > 0. && object.animation_blend_state.is_none() {
                            object.animation_blend_state =
                                Some(crate::animation::BlendState::new(object.animation.as_ref()));
                        }
                        object.animation = (!clip.is_empty()).then(|| crate::animation::Player {
                            clip,
                            time: 0.,
                            playing: true,
                            ..object.animation.clone().unwrap_or_default()
                        });
                        changed = true;
                    }
                }
                Command::SetSpin {
                    id,
                    degrees_per_second,
                } => {
                    let index = next.object_index(id)?;
                    next.objects[index].spin = degrees_per_second;
                    changed = true;
                }
                Command::Send(message) => outgoing.push(message),
                Command::PlaySound(id) => sounds.push(crate::scripting::Effect::AnimationSound(id)),
            }
        }
        if changed {
            play.commit_objects(next)?;
        }
        play.extend_effects(sounds);
        outgoing.retain(|m| play.scene().object_index(m.to).is_ok());
        *messages = outgoing;
        Ok(())
    }
}
