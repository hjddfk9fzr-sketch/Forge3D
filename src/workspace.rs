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
        first.application.version_code = 108;
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
        entrance.objects[0].script = Some(level_script("object.x >= 2.5", "workshop"));
        entrance.presentation.widgets = vec![
            text([0.10, 0.04, 0.80, 0.08], "01  /  THE ENTRANCE"),
            text(
                [0.10, 0.13, 0.80, 0.07],
                "Move right to reach the checkpoint",
            ),
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

    pub fn scripts(&self) -> &BTreeMap<AssetId, ScriptAsset> {
        &self.scripts
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
        result.assign_ids()?;
        let mut ids = BTreeMap::new();
        let mut root = None;
        for (index, _, _) in prefab.object_order() {
            let mut object = prefab.objects[index].clone();
            let original = object.id;
            match object.parent {
                Some(old) => object.parent = Some(ids[&old]),
                None if root.is_none() => object.parent = parent,
                None => return Err(format!("Prefab '{id}' has multiple roots.")),
            }
            if let Shape::Model(index) = object.shape {
                object.shape = Shape::Model(slot(&mut result.models, &prefab.models[index]));
            }
            if let Some(index) = object.script_asset {
                object.script_asset = Some(slot(&mut result.scripts, &prefab.scripts[index]));
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

fn slot<T: Clone + PartialEq>(assets: &mut Vec<T>, asset: &T) -> usize {
    if let Some(index) = assets.iter().position(|existing| existing == asset) {
        return index;
    }
    assets.push(asset.clone());
    assets.len() - 1
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
