use super::*;
use crate::runtime::PrefabLink;
use serde_json::Value;

#[derive(Debug)]
pub struct Override {
    pub path: Vec<String>,
    pub inherited: Option<Value>,
    pub local: Option<Value>,
}

fn json(object: &Object) -> Result<Value, String> {
    serde_json::to_value(object).map_err(|e| e.to_string())
}

fn difference(
    path: &mut Vec<String>,
    inherited: Option<&Value>,
    local: Option<&Value>,
    out: &mut Vec<Override>,
) {
    if inherited == local {
        return;
    }
    if let (Some(Value::Object(a)), Some(Value::Object(b))) = (inherited, local) {
        for key in a
            .keys()
            .chain(b.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            path.push(key.clone());
            difference(path, a.get(key), b.get(key), out);
            path.pop();
        }
    } else {
        out.push(Override {
            path: path.clone(),
            inherited: inherited.cloned(),
            local: local.cloned(),
        });
    }
}

fn value_at<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, k| v.get(k))
}

fn patch(object: &mut Object, path: &[String], new: Option<&Value>) -> Result<(), String> {
    let mut value = json(object)?;
    let (key, parents) = path.split_last().ok_or("Missing property.")?;
    let parent = parents
        .iter()
        .try_fold(&mut value, |v, k| v.get_mut(k))
        .and_then(Value::as_object_mut)
        .ok_or("Missing parent property.")?;
    if let Some(new) = new {
        parent.insert(key.clone(), new.clone());
    } else {
        parent.remove(key);
    }
    *object = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(())
}

// Asset slots belong to individual scenes. Compare and transfer them in the destination's namespace.
fn project_object(source: &Object, from: &Scene, to: &Scene) -> Object {
    let mut obj = source.clone();
    if let Shape::Model(i) = obj.shape {
        obj.shape = Shape::Model(
            to.models
                .iter()
                .position(|m| m == &from.models[i])
                .unwrap_or(usize::MAX),
        );
    }
    if let Some(i) = obj.script_asset {
        obj.script_asset = Some(
            to.scripts
                .iter()
                .position(|s| s == &from.scripts[i])
                .unwrap_or(usize::MAX),
        );
    }
    obj
}

fn with_assets(source: &Object, from: &Scene, to: &mut Scene) -> Object {
    if let Shape::Model(i) = source.shape {
        slot(&mut to.models, &from.models[i]);
    }
    if let Some(i) = source.script_asset {
        slot(&mut to.scripts, &from.scripts[i]);
    }
    project_object(source, from, to)
}

impl Project {
    pub fn link_source(&self, name: &str, scene: &mut Scene) -> Result<(), String> {
        let template = self.prefabs.get(name).ok_or("Unknown prefab.")?;
        let nodes = template
            .scene
            .objects
            .iter()
            .map(|o| scene.object_index(o.id))
            .collect::<Result<Vec<_>, _>>()?;
        for index in nodes {
            let node = &mut scene.objects[index];
            node.prefab = Some(PrefabLink {
                overrides: Default::default(),
                name: name.into(),
                source: node.id,
            });
        }
        Ok(())
    }

    fn template(&self, link: &PrefabLink) -> Result<(Scene, usize), String> {
        let prefab = self.expand(
            self.prefabs
                .get(&link.name)
                .ok_or_else(|| format!("Unknown prefab '{}'.", link.name))?,
        )?;
        let i = prefab.object_index(link.source)?;
        Ok((prefab, i))
    }

    pub fn overrides(&self, scene: &Scene, id: u64) -> Result<Vec<Override>, String> {
        let obj = &scene.objects[scene.object_index(id)?];
        let link = obj
            .prefab
            .as_ref()
            .ok_or("Object is not a linked instance.")?;
        let stored = self.prefabs.get(&link.name).ok_or("Missing prefab.")?;
        let source = &stored.scene.objects[stored.scene.object_index(link.source)?];
        // Resolve only the attached assets when drawing the inspector.
        let mut projected = source.clone();
        if let Shape::Model(i) = source.shape {
            let model = &self.models[&stored.models[i]];
            projected.shape = Shape::Model(
                scene
                    .models
                    .iter()
                    .position(|m| m == model)
                    .unwrap_or(usize::MAX),
            );
        }
        if let Some(i) = source.script_asset {
            let script = &self.scripts[&stored.scripts[i]];
            projected.script_asset = Some(
                scene
                    .scripts
                    .iter()
                    .position(|s| s == script)
                    .unwrap_or(usize::MAX),
            );
        }
        let mut from = json(&projected)?;
        let mut to = json(obj)?;
        for key in ["id", "parent", "prefab"] {
            from.as_object_mut().unwrap().remove(key);
            to.as_object_mut().unwrap().remove(key);
        }
        if source.parent.is_none() {
            for key in ["position", "rotation"] {
                from.as_object_mut().unwrap().remove(key);
                to.as_object_mut().unwrap().remove(key);
            }
        }
        let mut differences = Vec::new();
        difference(&mut Vec::new(), Some(&from), Some(&to), &mut differences);
        for path in &link.overrides {
            if value_at(&from, path) == value_at(&to, path) && value_at(&from, path).is_some() {
                differences.push(Override {
                    path: path.clone(),
                    inherited: value_at(&from, path).cloned(),
                    local: value_at(&to, path).cloned(),
                });
            }
        }
        Ok(differences)
    }

    pub fn revert_property(
        &self,
        scene: &mut Scene,
        id: u64,
        path: &[String],
    ) -> Result<(), String> {
        if !self.overrides(scene, id)?.iter().any(|o| o.path == path) {
            return Err("No override for this property.".into());
        }
        let i = scene.object_index(id)?;
        let (template, src) = self.template(scene.objects[i].prefab.as_ref().unwrap())?;
        let mut draft = scene.clone();
        let source = json(&with_assets(&template.objects[src], &template, &mut draft))?;
        patch(&mut draft.objects[i], path, value_at(&source, path))?;
        draft.objects[i]
            .prefab
            .as_mut()
            .unwrap()
            .overrides
            .retain(|key| !key.starts_with(path));
        draft.validate()?;
        *scene = draft;
        Ok(())
    }

    /// Commit the active scene first, then change the template and all inheriting scene nodes atomically.
    pub fn apply_property(
        &mut self,
        active: &str,
        scene: &Scene,
        id: u64,
        path: &[String],
    ) -> Result<(), String> {
        if !self.overrides(scene, id)?.iter().any(|o| o.path == path) {
            return Err("No override for this property.".into());
        }
        let obj = &scene.objects[scene.object_index(id)?];
        let link = obj.prefab.as_ref().unwrap();
        let (before, index) = self.template(link)?;
        let mut after = before.clone();
        let edit = json(&with_assets(obj, scene, &mut after))?;
        patch(&mut after.objects[index], path, value_at(&edit, path))?;
        after.validate()?;
        let mut draft = self.clone();
        draft.update_scene(active, scene)?;
        let stored = draft.store(after.clone())?;
        draft.prefabs.insert(link.name.clone(), stored);
        for name in self.scenes.keys() {
            let mut current = draft.scene(name)?;
            let indices: Vec<_> = current
                .objects
                .iter()
                .enumerate()
                .filter(|(_, o)| {
                    o.prefab
                        .as_ref()
                        .is_some_and(|l| l.name == link.name && l.source == link.source)
                })
                .map(|(i, _)| i)
                .collect();
            if indices.is_empty() {
                continue;
            }
            let old = json(&project_object(&before.objects[index], &before, &current))?;
            let new = json(&with_assets(&after.objects[index], &after, &mut current))?;
            for i in indices {
                let local = json(&current.objects[i])?;
                let node = &mut current.objects[i];
                let ownership = &mut node.prefab.as_mut().unwrap().overrides;
                if name == active && node.id == id {
                    ownership.retain(|key| !key.starts_with(path));
                } else if !ownership
                    .iter()
                    .any(|key| key.starts_with(path) || path.starts_with(key))
                {
                    if value_at(&local, &path[..path.len() - 1]).is_some_and(Value::is_object)
                        && value_at(&local, path) == value_at(&old, path)
                    {
                        patch(node, path, value_at(&new, path))?;
                    } else if value_at(&local, path) == value_at(&new, path) {
                        ownership.insert(path.to_vec());
                    }
                }
            }
            draft.update_scene(name, &current)?;
        }
        *self = draft;
        Ok(())
    }
}
