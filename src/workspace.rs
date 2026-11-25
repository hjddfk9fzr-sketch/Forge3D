use crate::gameplay::slot;
use crate::{
    game::{valid_id, Game},
    presentation::{Presentation, UiKey, Widget, WidgetKind},
    project,
    runtime::{Model, Object, Scene, ScriptAsset, Shape},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

pub const EXTENSION: &str = ".forge-project.json";
pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub type AssetId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SharedAsset {
    Model(AssetId),
    Script(AssetId),
}

impl std::fmt::Display for SharedAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(id) => write!(f, "model #{id}"),
            Self::Script(id) => write!(f, "script #{id}"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModelOwner<'a> {
    Scene(&'a str),
    Prefab(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ModelUsage<'a> {
    pub owner: ModelOwner<'a>,
    pub object_id: u64,
    pub object_name: &'a str,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredScene {
    scene: Scene,
    models: Vec<AssetId>,
    scripts: Vec<AssetId>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    version: u32,
    start: String,
    scenes: BTreeMap<String, StoredScene>,
    models: BTreeMap<AssetId, Model>,
    scripts: BTreeMap<AssetId, ScriptAsset>,
    prefabs: BTreeMap<String, StoredScene>,
}

impl Project {
    pub fn new(id: &str, scene: &Scene) -> Result<Self, String> {
        let mut project = Self {
            version: PROJECT_SCHEMA_VERSION,
            start: id.into(),
            scenes: BTreeMap::new(),
            models: BTreeMap::new(),
            scripts: BTreeMap::new(),
            prefabs: BTreeMap::new(),
        };
        project.add_scene(id, scene)?;
        Ok(project)
    }

    pub fn demo() -> Result<Self, String> {
        let mut first = Scene::demo();
        first.title = "Workshop Entrance".into();
        first.application.title = "Forge3D · Two Workshops".into();
        first.application.identifier = "com.forge3d.betademo".into();
        first.application.version = env!("CARGO_PKG_VERSION").into();
        first.application.version_code = 130;
        first.objects[0].script = Some(
            r#"fn update(object, dt, input) {
                if input.x > 0.5 { object.next_scene = "workshop"; }
                object
            }"#
            .into(),
        );
        let mut second = Scene::rendering_demo();
        second.application = first.application.clone();
        second.objects[3].script = Some(
            r#"fn update(object, dt, input) {
                if input.x < -0.5 { object.next_scene = "entrance"; }
                object
            }"#
            .into(),
        );
        let source = &second;
        let mut project = Self::new("entrance", &first)?;
        project.create_prefab("workshop-arm", source, source.objects[1].id)?;
        project.instantiate("workshop-arm", &mut first, None)?;
        let arm = first.objects.last_mut().ok_or("Missing workshop arm.")?;
        arm.position = [-2.8, 0., 0.];
        project.update_scene("entrance", &first)?;
        project.add_scene("workshop", &second)?;
        Ok(project)
    }

    pub fn campaign_demo() -> Result<Self, String> {
        let mut project = Self::demo()?;
        let mut entrance = project.scene("entrance")?;
        let mut workshop = project.scene("workshop")?;
        let mut app = entrance.application.clone();
        app.title = "Forge3D · Workshop Run".into();
        entrance.application = app.clone();
        workshop.application = app.clone();

        let cue: std::sync::Arc<[u8]> = demo_cue()?.into();
        let pilot = &mut entrance.objects[0];
        pilot.body = Some(crate::physics::Body::character());
        pilot
            .components
            .insert("workshop.inventory".into(), 0.into());
        let pilot_id = pilot.id;
        let mut script = level_script(
            "object.z <= -3.65 && object.y > 0.0 && object.x.abs() < 1.0",
            "workshop",
        );
        script.push_str(
            r#"
fn camera(view, object, dt, input) {
    let side = if input.keys.contains("q") { -1.0 } else if input.keys.contains("r") { 1.0 } else { 0.0 };
    view.x = object.x + side;
    view.y = object.y + 0.65;
    view.z = if side == 0.0 { object.z - 1.0 } else { object.z };
    view.distance = 1.0;
    view.yaw = -90.0 * side;
    view.pitch = 0.0;
    view
}"#,
        );
        pilot.script = Some(script);
        entrance.camera_controller = Some(pilot_id);
        let mut door = Object::new("Workshop door", Shape::Cube);
        door.position = [0., 1., -1.8];
        door.scale = [1.2, 2., 0.18];
        door.body = Some(crate::physics::Body::default());
        door.script = Some(format!(
            r#"fn update(object, dt, input) {{
    if object.state.remove("use") == true || input.pressed.contains("e") {{
        let hit = raycast(input.aim_origin, input.aim_direction, 2.0, "{pilot_id}");
        if hit != () && hit.object_id == object.id {{
            object.y = 3.5;
            object.play_sound = "cue";
        }}
    }}
    object
}}
fn on_ui(object, event) {{
    if event.id == "use" {{ object.state["use"] = true; }}
    object
}}"#
        ));
        entrance.add_object(door)?;
        let floor = &mut entrance.objects[3];
        floor.position = [0., -0.05, -1.5];
        floor.scale = [6., 1., 7.];
        floor.body = Some(crate::physics::Body {
            size: [1., 0.1, 1.],
            ..Default::default()
        });
        for x in [-1.8, 1.8] {
            let mut wall = Object::new("Workshop wall", Shape::Cube);
            wall.position = [x, 1., -1.8];
            wall.scale = [2.4, 2., 0.18];
            wall.body = Some(Default::default());
            entrance.add_object(wall)?;
        }
        let mut item = Object::new("Workshop token", Shape::Sphere);
        item.scale = [0.35; 3];
        item.body = Some(crate::physics::Body::default());
        item.components
            .insert("workshop.pickup".into(), 0.65.into());
        item.script =
            Some("fn update(object, dt, input) { object.ry += 80.0 * dt; object }".into());
        let mut template = Scene::empty();
        let root = template.add_object(item)?;
        project.create_prefab("workshop-token", &template, root)?;
        let mut spawner = Object::new("Token spawn", Shape::Empty);
        spawner.position = [0., 0.55, -2.8];
        spawner
            .components
            .insert("workshop.spawn".into(), "workshop-token".into());
        entrance.add_object(spawner)?;
        entrance.presentation.widgets = vec![
            text([0.10, 0.04, 0.80, 0.08], "01  /  THE ENTRANCE"),
            text(
                [0.10, 0.13, 0.80, 0.07],
                "Open the door [E], then walk forward through the token.",
            ),
            button([0.78, 0.75, 0.19, 0.09], "use", "Open [E]", UiKey::E),
            button([0.78, 0.87, 0.19, 0.09], "menu", "Menu", UiKey::Escape),
        ];
        entrance
            .presentation
            .sounds
            .insert("cue".into(), cue.clone());
        workshop.objects[3].script = Some(level_script("object.x <= 0.0", "complete"));
        workshop.presentation.widgets = vec![
            text([0.10, 0.04, 0.80, 0.08], "02  /  THE WORKSHOP"),
            text(
                [0.10, 0.13, 0.80, 0.07],
                "Move the pink sphere left to finish",
            ),
            button([0.78, 0.87, 0.19, 0.09], "menu", "Menu", UiKey::Escape),
        ];
        workshop
            .presentation
            .sounds
            .insert("cue".into(), cue.clone());

        let mut menu = Scene::empty();
        menu.application = app.clone();
        menu.title = "Workshop Run".into();
        let mut controller = Object::new("Main menu", Shape::Empty);
        controller.script = Some(
            r#"fn update(object, dt, input) { object }
fn on_ui(object, event) {
    object.play_sound = "cue";
    if event.id == "start" {
        object.save_scene = "entrance";
        object.next_scene = "entrance";
    } else if event.id == "continue" {
        object.resume_game = true;
    } else if event.id == "quiet" {
        object.volume_step = -0.1;
    } else if event.id == "loud" {
        object.volume_step = 0.1;
    }
    object
}"#
            .into(),
        );
        menu.add_object(controller)?;
        menu.presentation = Presentation {
            hide_movement: true,
            sounds: BTreeMap::from([("cue".into(), cue.clone())]),
            widgets: vec![
                Widget {
                    rect: [0.42, 0.07, 0.16, 0.23],
                    kind: WidgetKind::Image {
                        png: demo_logo()?.into(),
                    },
                },
                text([0.13, 0.30, 0.74, 0.11], "WORKSHOP RUN"),
                button(
                    [0.29, 0.46, 0.42, 0.10],
                    "start",
                    "New run  [Enter]",
                    UiKey::Enter,
                ),
                button(
                    [0.29, 0.59, 0.42, 0.10],
                    "continue",
                    "Continue  [C]",
                    UiKey::C,
                ),
                button([0.25, 0.75, 0.18, 0.09], "quiet", "Volume -", UiKey::Minus),
                text([0.44, 0.75, 0.12, 0.09], "{volume}%"),
                button([0.57, 0.75, 0.18, 0.09], "loud", "Volume +", UiKey::Equal),
                text([0.18, 0.88, 0.64, 0.07], "Checkpoint: {checkpoint}"),
            ],
        };

        let mut complete = Scene::empty();
        complete.application = app;
        complete.title = "Run complete".into();
        let mut controller = Object::new("Results menu", Shape::Empty);
        controller.script = Some(
            r#"fn update(object, dt, input) { object }
fn on_ui(object, event) { object.next_scene = "menu"; object }"#
                .into(),
        );
        complete.add_object(controller)?;
        complete.presentation.hide_movement = true;
        complete.presentation.widgets = vec![
            text([0.10, 0.25, 0.80, 0.17], "RUN COMPLETE"),
            text([0.14, 0.44, 0.72, 0.09], "Both checkpoints reached"),
            button(
                [0.29, 0.62, 0.42, 0.11],
                "menu",
                "Main menu  [Enter]",
                UiKey::Enter,
            ),
        ];
        project.update_scene("entrance", &entrance)?;
        project.update_scene("workshop", &workshop)?;
        project.add_scene("menu", &menu)?;
        project.add_scene("complete", &complete)?;
        project.set_start("menu")?;
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let project: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        project.validate()?;
        Ok(project)
    }

    pub fn save(&self, path: &Path, overwrite: bool) -> Result<(), String> {
        if !path.to_string_lossy().ends_with(EXTENSION) {
            return Err(format!("Project filenames must end in {EXTENSION}."));
        }
        self.validate()?;
        project::publish_scene(path, overwrite, |file| {
            serde_json::to_writer(file, self).map_err(|error| error.to_string())
        })
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != PROJECT_SCHEMA_VERSION {
            return Err(format!("Unsupported project version {}.", self.version));
        }
        if !self.scenes.contains_key(&self.start) {
            return Err(format!("Missing starting scene {}.", self.start));
        }
        let mut missing = Vec::new();
        for (kind, entries) in [("scene", &self.scenes), ("prefab", &self.prefabs)] {
            for (owner, stored) in entries {
                for (asset, ids) in [("model", &stored.models), ("script", &stored.scripts)] {
                    for (slot, id) in ids.iter().enumerate() {
                        let exists = if asset == "model" {
                            self.models.contains_key(id)
                        } else {
                            self.scripts.contains_key(id)
                        };
                        if !exists {
                            missing.push(format!(
                                "{kind} '{owner}': missing {asset} #{id} at slot {slot}"
                            ));
                        }
                    }
                }
                for obj in &stored.scene.objects {
                    let model = match obj.shape {
                        Shape::Model(i) => Some(i),
                        _ => None,
                    };
                    for (asset, slot, count) in [
                        ("model", model, stored.models.len()),
                        ("script", obj.script_asset, stored.scripts.len()),
                    ] {
                        if let Some(slot) = slot.filter(|i| *i >= count) {
                            missing.push(format!(
                                "{kind} '{owner}', object '{}' #{}: missing {asset} slot {slot}",
                                obj.name, obj.id
                            ));
                        }
                    }
                }
            }
        }
        if !missing.is_empty() {
            return Err(missing.join("\n"));
        }
        for (id, scene) in self.scenes.iter().chain(&self.prefabs) {
            check_id(id)?;
            self.expand(scene)?;
        }
        Ok(())
    }

    pub fn start(&self) -> &str {
        &self.start
    }

    pub fn set_start(&mut self, id: &str) -> Result<(), String> {
        if !self.scenes.contains_key(id) {
            return Err(format!("Unknown scene '{id}'."));
        }
        self.start = id.into();
        Ok(())
    }

    pub fn scenes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.scenes
            .iter()
            .map(|(id, scene)| (id.as_str(), scene.scene.title.as_str()))
    }

    pub fn prefabs(&self) -> impl Iterator<Item = &str> {
        self.prefabs.keys().map(String::as_str)
    }

    pub fn models(&self) -> &BTreeMap<AssetId, Model> {
        &self.models
    }

    pub fn model_usages(&self, id: AssetId) -> Vec<ModelUsage<'_>> {
        self.asset_usages(SharedAsset::Model(id))
    }

    pub fn dependencies(&self, owner: ModelOwner<'_>) -> Result<Vec<(SharedAsset, &str)>, String> {
        let stored = match owner {
            ModelOwner::Scene(id) => self
                .scenes
                .get(id)
                .ok_or_else(|| format!("Unknown scene '{id}'."))?,
            ModelOwner::Prefab(id) => self
                .prefabs
                .get(id)
                .ok_or_else(|| format!("Unknown prefab '{id}'."))?,
        };
        let mut assets = Vec::new();
        for id in &stored.models {
            let name = self
                .models
                .get(id)
                .ok_or_else(|| format!("Missing model #{id}."))?;
            assets.push((SharedAsset::Model(*id), name.name.as_str()));
        }
        for id in &stored.scripts {
            let name = self
                .scripts
                .get(id)
                .ok_or_else(|| format!("Missing script #{id}."))?;
            assets.push((SharedAsset::Script(*id), name.name.as_str()));
        }
        assets.sort_by_key(|(asset, _)| *asset);
        assets.dedup();
        Ok(assets)
    }

    pub fn asset_usages(&self, asset: SharedAsset) -> Vec<ModelUsage<'_>> {
        let mut usages = Vec::new();
        for (entries, prefab) in [(&self.scenes, false), (&self.prefabs, true)] {
            for (name, stored) in entries {
                for object in &stored.scene.objects {
                    let matches = match asset {
                        SharedAsset::Model(id) => {
                            matches!(object.shape, Shape::Model(slot) if stored.models.get(slot) == Some(&id))
                        }
                        SharedAsset::Script(id) => {
                            object
                                .script_asset
                                .and_then(|slot| stored.scripts.get(slot))
                                == Some(&id)
                        }
                    };
                    if matches {
                        usages.push(ModelUsage {
                            owner: if prefab {
                                ModelOwner::Prefab(name)
                            } else {
                                ModelOwner::Scene(name)
                            },
                            object_id: object.id,
                            object_name: &object.name,
                        });
                    }
                }
            }
        }
        usages
    }

    pub fn scripts(&self) -> &BTreeMap<AssetId, ScriptAsset> {
        &self.scripts
    }

    /// Adds a batch to the project library, committing only after every file imports.
    pub fn import_assets(
        &mut self,
        paths: &[std::path::PathBuf],
    ) -> Result<Vec<SharedAsset>, String> {
        if paths.is_empty() {
            return Err("Choose at least one asset file.".into());
        }
        let mut models = self.models.clone();
        let mut scripts = self.scripts.clone();
        let mut imported = Vec::with_capacity(paths.len());
        for path in paths {
            let asset = if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rhai"))
            {
                crate::import::script(path)
                    .and_then(|value| intern(&mut scripts, value))
                    .map(SharedAsset::Script)
            } else {
                crate::import::model(path)
                    .and_then(|value| {
                        let mut probe = Scene::empty();
                        probe.models.push(value.clone());
                        probe.validate()?;
                        intern(&mut models, value)
                    })
                    .map(SharedAsset::Model)
            }
            .map_err(|error| format!("{}: {error}", path.display()))?;
            imported.push(asset);
        }
        self.models = models;
        self.scripts = scripts;
        Ok(imported)
    }

    pub fn reimport_script(&mut self, id: AssetId, source: String) -> Result<(), String> {
        let old = self
            .scripts
            .get(&id)
            .ok_or_else(|| format!("Unknown shared script {id}."))?;
        crate::scripting::validate_source(&source)?;
        let replacement = ScriptAsset {
            name: old.name.clone(),
            source,
        };
        if let Some((other, _)) = self
            .scripts
            .iter()
            .find(|(key, value)| **key != id && **value == replacement)
        {
            return Err(format!(
                "This script would be identical to shared script #{other}."
            ));
        }
        self.scripts.insert(id, replacement);
        Ok(())
    }

    pub fn reimport_asset(&mut self, asset: SharedAsset, path: &Path) -> Result<(), String> {
        match asset {
            SharedAsset::Model(id) => self.reimport_model(id, crate::import::model(path)?),
            SharedAsset::Script(id) => {
                self.reimport_script(id, crate::import::script(path)?.source)
            }
        }
    }

    pub fn reimport_model(&mut self, id: AssetId, mut replacement: Model) -> Result<(), String> {
        let old = self
            .models
            .get(&id)
            .ok_or_else(|| format!("Unknown shared model {id}."))?;
        replacement.name.clone_from(&old.name);
        // Project storage interns identical models. Keep two existing IDs distinguishable.
        if let Some((other, _)) = self
            .models
            .iter()
            .find(|(key, model)| **key != id && **model == replacement)
        {
            return Err(format!(
                "This model would be identical to shared model #{other}."
            ));
        }
        let mut probe = Scene::empty();
        probe.models.push(replacement.clone());
        probe.validate()?;
        let old = self.models.insert(id, replacement).expect("existing model");
        if let Err(error) = self.validate() {
            self.models.insert(id, old);
            return Err(format!("Cannot reimport model #{id}: {error}"));
        }
        Ok(())
    }

    pub fn use_model(&self, id: AssetId, scene: &mut Scene) -> Result<usize, String> {
        let model = self
            .models
            .get(&id)
            .ok_or_else(|| format!("Unknown shared model {id}."))?;
        Ok(slot(&mut scene.models, model))
    }

    pub fn use_script(&self, id: AssetId, scene: &mut Scene) -> Result<usize, String> {
        let script = self
            .scripts
            .get(&id)
            .ok_or_else(|| format!("Unknown shared script {id}."))?;
        Ok(slot(&mut scene.scripts, script))
    }

    pub fn add_scene(&mut self, id: &str, scene: &Scene) -> Result<(), String> {
        check_id(id)?;
        if self.scenes.contains_key(id) {
            return Err(format!("Scene '{id}' already exists."));
        }
        let stored = self.store(scene.clone())?;
        self.scenes.insert(id.into(), stored);
        Ok(())
    }

    pub fn update_scene(&mut self, id: &str, scene: &Scene) -> Result<(), String> {
        if !self.scenes.contains_key(id) {
            return Err(format!("Unknown scene '{id}'."));
        }
        let stored = self.store(scene.clone())?;
        self.scenes.insert(id.into(), stored);
        Ok(())
    }

    pub fn scene(&self, id: &str) -> Result<Scene, String> {
        self.expand(
            self.scenes
                .get(id)
                .ok_or_else(|| format!("Unknown scene '{id}'."))?,
        )
    }

    pub fn game(&self, start: Option<&str>) -> Result<Game, String> {
        let start = start.unwrap_or(&self.start).to_owned();
        if !self.scenes.contains_key(&start) {
            return Err(format!("Unknown starting scene '{start}'."));
        }
        Ok(Game {
            start,
            prefabs: self
                .prefabs
                .iter()
                .map(|(id, prefab)| Ok((id.clone(), self.expand(prefab)?)))
                .collect::<Result<_, String>>()?,
            scenes: self
                .scenes
                .iter()
                .map(|(id, scene)| Ok((id.clone(), self.expand(scene)?)))
                .collect::<Result<_, String>>()?,
        })
    }

    pub fn create_prefab(&mut self, id: &str, scene: &Scene, root: u64) -> Result<(), String> {
        check_id(id)?;
        if self.prefabs.contains_key(id) {
            return Err(format!("Prefab '{id}' already exists."));
        }
        scene.object_index(root)?;
        let mut draft = Scene::empty();
        draft.title = id.into();
        let mut indices = BTreeMap::new();
        for (index, _, _) in scene.object_order() {
            let mut object = scene.objects[index].clone();
            if object.id == root {
                object.parent = None;
            } else if let Some(parent) = object.parent.and_then(|parent| indices.get(&parent)) {
                object.parent = Some(*parent);
            } else {
                continue;
            }
            if let Shape::Model(index) = object.shape {
                let model = scene.models.get(index).ok_or("Missing prefab model.")?;
                object.shape = Shape::Model(slot(&mut draft.models, model));
            }
            if let Some(index) = object.script_asset {
                let script = scene.scripts.get(index).ok_or("Missing prefab script.")?;
                object.script_asset = Some(slot(&mut draft.scripts, script));
            }
            indices.insert(object.id, object.id);
            draft.objects.push(object);
        }
        let stored = self.store(draft)?;
        self.prefabs.insert(id.into(), stored);
        Ok(())
    }

    pub fn instantiate(
        &self,
        id: &str,
        scene: &mut Scene,
        parent: Option<u64>,
    ) -> Result<u64, String> {
        if let Some(parent) = parent {
            scene.object_index(parent)?;
        }
        let prefab = self.expand(
            self.prefabs
                .get(id)
                .ok_or_else(|| format!("Unknown prefab '{id}'."))?,
        )?;
        let mut result = scene.clone();
        let root = crate::gameplay::instantiate(&prefab, &mut result, parent, id)?;
        result.validate()?;
        *scene = result;
        Ok(root)
    }

    fn store(&mut self, mut scene: Scene) -> Result<StoredScene, String> {
        scene.assign_ids()?;
        scene.validate()?;
        let models = std::mem::take(&mut scene.models)
            .into_iter()
            .map(|model| intern(&mut self.models, model))
            .collect::<Result<_, _>>()?;
        let scripts = std::mem::take(&mut scene.scripts)
            .into_iter()
            .map(|script| intern(&mut self.scripts, script))
            .collect::<Result<_, _>>()?;
        Ok(StoredScene {
            scene,
            models,
            scripts,
        })
    }

    fn expand(&self, stored: &StoredScene) -> Result<Scene, String> {
        let mut scene = stored.scene.clone();
        scene.models = resolve(&self.models, &stored.models)?;
        scene.scripts = resolve(&self.scripts, &stored.scripts)?;
        scene.assign_ids()?;
        scene.validate()?;
        Ok(scene)
    }
}

fn text(rect: [f32; 4], text: &str) -> Widget {
    Widget {
        rect,
        kind: WidgetKind::Text { text: text.into() },
    }
}

fn button(rect: [f32; 4], id: &str, text: &str, key: UiKey) -> Widget {
    Widget {
        rect,
        kind: WidgetKind::Button {
            id: id.into(),
            text: text.into(),
            key: Some(key),
        },
    }
}

fn level_script(condition: &str, next: &str) -> String {
    format!(
        r#"fn update(object, dt, input) {{
    if {condition} {{
        object.play_sound = "cue";
        object.save_scene = "{next}";
        object.next_scene = "{next}";
    }}
    object
}}
fn on_ui(object, event) {{
    if event.id == "menu" {{ object.next_scene = "menu"; }}
    object
}}"#
    )
}

fn demo_cue() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec)
        .map_err(|error| error.to_string())?;
    for frame in 0..2205 {
        let t = frame as f32 / 2205.;
        let time = frame as f32 / 22_050.;
        let attack = (t * 28.).min(1.);
        let wave = (time * 660. * std::f32::consts::TAU).sin()
            + 0.4 * (time * 990. * std::f32::consts::TAU).sin();
        wav.write_sample((wave * attack * (1. - t).powi(2) * 8000.) as i16)
            .map_err(|error| error.to_string())?;
    }
    wav.finalize().map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn demo_logo() -> Result<Vec<u8>, String> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    let image = RgbaImage::from_fn(96, 96, |x, y| {
        let distance = x.abs_diff(48) + y.abs_diff(48);
        if distance > 46 {
            Rgba([0, 0, 0, 0])
        } else if distance >= 40 {
            Rgba([98, 198, 236, 255])
        } else if (43..=52).contains(&x) || ((30..=65).contains(&x) && (30..=39).contains(&y)) {
            Rgba([243, 184, 91, 255])
        } else {
            Rgba([28, 48 + (y / 3) as u8, 96 + (x / 2) as u8, 255])
        }
    });
    let mut output = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(output.into_inner())
}

fn check_id(id: &str) -> Result<(), String> {
    if valid_id(id) {
        Ok(())
    } else {
        Err("Use 1–80 lowercase Latin letters, digits or hyphens for the ID.".into())
    }
}

fn intern<T: PartialEq>(assets: &mut BTreeMap<AssetId, T>, asset: T) -> Result<AssetId, String> {
    if let Some((&id, _)) = assets.iter().find(|(_, existing)| **existing == asset) {
        return Ok(id);
    }
    let id = assets
        .last_key_value()
        .map_or(0, |(&id, _)| id)
        .checked_add(1)
        .ok_or("Project ran out of asset IDs.")?;
    assets.insert(id, asset);
    Ok(id)
}

fn resolve<T: Clone>(assets: &BTreeMap<AssetId, T>, ids: &[AssetId]) -> Result<Vec<T>, String> {
    ids.iter()
        .map(|id| {
            assets
                .get(id)
                .cloned()
                .ok_or_else(|| format!("Missing shared asset {id}."))
        })
        .collect()
}

/// Keeps the working scene separate from the stored scenes so editor undo
/// remains local. A switch or snapshot commits the working scene first.
pub struct OpenProject {
    pub project: Project,
    pub active: String,
}

impl OpenProject {
    pub fn open(project: Project) -> Self {
        let active = project.start().to_owned();
        Self { project, active }
    }

    pub fn snapshot(&self, scene: &Scene) -> Result<Project, String> {
        let mut project = self.project.clone();
        project.update_scene(&self.active, scene)?;
        Ok(project)
    }

    pub fn select(&mut self, id: &str, scene: &Scene) -> Result<Scene, String> {
        if self.active == id {
            return Ok(scene.clone());
        }
        let next = self.project.scene(id)?;
        self.project.update_scene(&self.active, scene)?;
        self.active = id.into();
        Ok(next)
    }
}
