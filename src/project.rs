use crate::runtime::Scene;
use std::{fs, io::Write, path::Path};

pub fn load(path: &Path) -> Result<Scene, String> {
    Scene::decode(&fs::read(path).map_err(|error| error.to_string())?)
}

pub fn save(scene: &Scene, path: &Path, overwrite: bool) -> Result<(), String> {
    let mut scene = scene.clone();
    scene.assign_ids()?;
    scene.validate()?;
    publish_scene(path, overwrite, |file| {
        serde_json::to_writer(file, &scene).map_err(|error| error.to_string())
    })
}

pub(crate) fn publish_scene(
    path: &Path,
    overwrite: bool,
    write: impl FnOnce(&mut fs::File) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    write(file.as_file_mut())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    let result = if overwrite {
        file.persist(path)
    } else {
        file.persist_noclobber(path)
    };
    result.map(|_| ()).map_err(|error| error.to_string())
}

#[derive(Default)]
pub struct History {
    undo: Vec<Scene>,
    redo: Vec<Scene>,
}

impl History {
    pub fn record(&mut self, before: Scene) {
        if self.undo.len() >= 64 {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        Self::restore(&mut self.undo, &mut self.redo, scene)
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        Self::restore(&mut self.redo, &mut self.undo, scene)
    }

    fn restore(from: &mut Vec<Scene>, to: &mut Vec<Scene>, scene: &mut Scene) -> bool {
        let Some(mut previous) = from.pop() else {
            return false;
        };
        previous.next_id = previous.next_id.max(scene.next_id);
        to.push(std::mem::replace(scene, previous));
        true
    }
}

#[derive(serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
}

pub fn entries(directory: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let kind = if file_type.is_dir() {
            "directory"
        } else if file_type.is_file() {
            match name.to_ascii_lowercase().as_str() {
                name if name.ends_with(crate::workspace::EXTENSION) => "project",
                name if name.ends_with(".forge.json") => "scene",
                name if name.ends_with(".rhai") => "script",
                name if [".obj", ".fbx", ".gltf", ".glb"]
                    .iter()
                    .any(|ext| name.ends_with(ext)) =>
                {
                    "model"
                }
                _ => continue,
            }
        } else {
            continue;
        };
        entries.push(Entry { name, kind });
    }
    entries.sort_by(|a, b| {
        (a.kind != "directory", a.name.to_lowercase(), &a.name).cmp(&(
            b.kind != "directory",
            b.name.to_lowercase(),
            &b.name,
        ))
    });
    Ok(entries)
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temp.write_all(bytes).map_err(|error| error.to_string())?;
    temp.persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn migrate(source: &Path, destination: &Path) -> Result<u32, String> {
    let scene = load(source)?;
    save(&scene, destination, false)?;
    Ok(scene.version)
}

/// Duplicates a subtree and keeps its references to scene-local resources.
pub fn duplicate_object(scene: &mut Scene, root: u64) -> Result<u64, String> {
    use std::collections::HashMap;
    scene.object_index(root)?;
    let mut next_id = scene.next_id;
    let mut ids = HashMap::new();
    let mut copies = Vec::new();
    for (index, _, _) in scene.object_order() {
        let original = &scene.objects[index];
        let mut copy = original.clone();
        if original.id == root {
            copy.name.push_str(" Copy");
        } else if let Some(parent) = original.parent.and_then(|parent| ids.get(&parent)) {
            copy.parent = Some(*parent);
        } else {
            continue;
        }
        copy.id = next_id;
        next_id = next_id
            .checked_add(1)
            .ok_or("Scene ran out of object IDs.")?;
        ids.insert(original.id, copy.id);
        copies.push(copy);
    }
    let copy_root = *ids.get(&root).ok_or("Invalid object hierarchy.")?;
    let mut candidate = scene.clone();
    candidate.next_id = next_id;
    candidate.objects.extend(copies);
    candidate.validate_objects()?;
    *scene = candidate;
    Ok(copy_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_overwrite_keeps_the_last_saved_scene() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scene.forge.json");
        let scene = Scene::demo();
        save(&scene, &path, false).unwrap();
        let saved = fs::read(&path).unwrap();

        assert!(publish_scene(&path, true, |staged| {
            staged.write_all(b"{\"version\":").unwrap();
            Err("interrupted write".into())
        })
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(load(&path).unwrap().title, scene.title);

        save(&Scene::empty(), &path, true).unwrap();
        assert!(load(&path).unwrap().objects.is_empty());
    }
}
