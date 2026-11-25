use crate::{
    animation::Player,
    controls::InputControls,
    physics::Event,
    runtime::{Object, Scene, Shape, View},
};
use macroquad::prelude::Vec2;
use rhai::{CallFnOptions, Engine, Map, Scope, AST};

pub const DEFAULT_SCRIPT: &str = r#"fn update(object, dt, input) {
    object.ry = (object.ry + 70.0 * dt) % 360.0;
    object
}
"#;

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .on_print(|text| eprintln!("{text}"))
        .on_debug(|text, _, _| eprintln!("{text}"))
        .disable_symbol("import")
        .disable_symbol("eval")
        .set_max_operations(25_000)
        .set_max_call_levels(24)
        .set_max_variables(128)
        .set_max_array_size(1024)
        .set_max_map_size(128)
        .set_max_string_size(16_384);
    engine
}

#[derive(Default)]
struct Hooks {
    start: bool,
    event: bool,
    camera: bool,
    ui: bool,
}

fn compile(engine: &Engine, source: &str) -> Result<(AST, Hooks), String> {
    if source.len() > 65_536 {
        return Err("Script exceeds 64 KiB.".into());
    }
    let ast = engine.compile(source).map_err(|error| error.to_string())?;
    let mut hooks = Hooks::default();
    let mut update = false;
    for function in ast.iter_functions() {
        let (present, arity, signature) = match function.name {
            "start" => (&mut hooks.start, 1, "start(object)"),
            "update" => (&mut update, 3, "update(object, dt, input)"),
            "on_event" => (&mut hooks.event, 2, "on_event(object, event)"),
            "on_ui" => (&mut hooks.ui, 2, "on_ui(object, event)"),
            "camera" => (&mut hooks.camera, 4, "camera(view, object, dt, input)"),
            _ => continue,
        };
        if *present || function.params.len() != arity {
            return Err(format!("Use one fn {signature}."));
        }
        *present = true;
    }
    if !update {
        return Err("Define fn update(object, dt, input).".into());
    }
    Ok((ast, hooks))
}

pub fn validate_source(source: &str) -> Result<(), String> {
    compile(&engine(), source).map(|_| ())
}

pub(crate) struct Scripts {
    engine: Engine,
    instances: Vec<Instance>,
    effects: Vec<Effect>,
}

pub(crate) enum Effect {
    Scene(String),
    Save(String),
    Resume,
    Sound(String),
    Volume(f32),
}

struct Instance {
    index: usize,
    ast: AST,
    hooks: Hooks,
    state: Map,
    clips: Vec<String>,
}

const POSITION: [&str; 3] = ["x", "y", "z"];
const ROTATION: [&str; 3] = ["rx", "ry", "rz"];
const SCALE: [&str; 3] = ["sx", "sy", "sz"];

impl Scripts {
    pub(crate) fn compile(scene: &Scene) -> Result<Self, String> {
        let engine = engine();
        let mut instances = Vec::new();
        for (index, object) in scene.objects.iter().enumerate() {
            let source = object.script.as_deref().or_else(|| {
                object
                    .script_asset
                    .and_then(|index| scene.scripts.get(index))
                    .map(|asset| asset.source.as_str())
            });
            if let Some(source) = source {
                let (ast, hooks) = compile(&engine, source)
                    .map_err(|error| format!("{}: {error}", object.name))?;
                instances.push(Instance {
                    index,
                    ast,
                    hooks,
                    state: Map::new(),
                    clips: match object.shape {
                        Shape::Model(index) => scene.models[index]
                            .rig
                            .as_ref()
                            .map_or_else(Vec::new, |rig| {
                                rig.clips.iter().map(|clip| clip.name.clone()).collect()
                            }),
                        _ => Vec::new(),
                    },
                });
            }
        }
        if let Some(id) = scene.camera_controller {
            let index = scene.object_index(id)?;
            if !instances
                .iter()
                .any(|instance| instance.index == index && instance.hooks.camera)
            {
                return Err(format!(
                    "Camera controller {} needs fn camera(view, object, dt, input).",
                    scene.objects[index].name
                ));
            }
        }
        Ok(Self {
            engine,
            instances,
            effects: Vec::new(),
        })
    }

    pub(crate) fn has_scene_request(&self) -> bool {
        self.effects
            .iter()
            .any(|effect| matches!(effect, Effect::Scene(_) | Effect::Resume))
    }

    pub(crate) fn take_scene_request(&mut self) -> Option<String> {
        let index = self
            .effects
            .iter()
            .position(|effect| matches!(effect, Effect::Scene(_)))?;
        let Effect::Scene(id) = self.effects.remove(index) else {
            unreachable!()
        };
        self.effects
            .retain(|effect| !matches!(effect, Effect::Scene(_)));
        Some(id)
    }

    pub(crate) fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub(crate) fn ui(&mut self, scene: &mut Scene, button: &str, errors: &mut Vec<String>) {
        let mut event = Map::new();
        event.insert("id".into(), button.to_owned().into());
        self.run(scene, errors, Hook::Ui(&event));
    }

    pub(crate) fn start(&mut self, scene: &mut Scene, errors: &mut Vec<String>) {
        self.run(scene, errors, Hook::Start);
    }

    pub(crate) fn step(
        &mut self,
        scene: &mut Scene,
        input: &Map,
        dt: f32,
        errors: &mut Vec<String>,
    ) {
        self.run(scene, errors, Hook::Update(input, dt));
    }

    pub(crate) fn events(&mut self, scene: &mut Scene, events: &[Event], errors: &mut Vec<String>) {
        if events.is_empty() || !self.instances.iter().any(|instance| instance.hooks.event) {
            return;
        }
        let indices: std::collections::HashMap<_, _> = scene
            .objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.id, index))
            .collect();
        for event in events {
            for (recipient, other) in [(event.a, event.b), (event.b, event.a)] {
                let (Some(&index), Some(&other_index)) =
                    (indices.get(&recipient), indices.get(&other))
                else {
                    continue;
                };
                let Ok(i) = self
                    .instances
                    .binary_search_by_key(&index, |instance| instance.index)
                else {
                    continue;
                };
                let instance = &mut self.instances[i];
                if !instance.hooks.event {
                    continue;
                }
                let mut data = Map::new();
                data.insert("kind".into(), event.kind.as_str().into());
                data.insert("other_id".into(), other.to_string().into());
                data.insert(
                    "other_name".into(),
                    scene.objects[other_index].name.clone().into(),
                );
                let object = &mut scene.objects[index];
                match instance.execute(&self.engine, object, Hook::Event(&data)) {
                    Ok(effects) => append_effects(&mut self.effects, effects),
                    Err(error) => {
                        errors.push(format!("{} · on_event: {error}", object.name));
                        self.instances.remove(i);
                    }
                }
            }
        }
    }

    pub(crate) fn camera(
        &mut self,
        scene: &mut Scene,
        input: &Map,
        dt: f32,
        errors: &mut Vec<String>,
    ) {
        let Some(id) = scene.camera_controller else {
            return;
        };
        let Ok(index) = scene.object_index(id) else {
            return;
        };
        let Ok(i) = self
            .instances
            .binary_search_by_key(&index, |instance| instance.index)
        else {
            return;
        };
        let object = &scene.objects[index];
        match self.instances[i].view(&self.engine, object, &scene.camera, input, dt) {
            Ok(view) => scene.camera = view,
            Err(error) => {
                errors.push(format!("{} · camera: {error}", object.name));
                self.instances.remove(i);
            }
        }
    }

    fn run(&mut self, scene: &mut Scene, errors: &mut Vec<String>, hook: Hook<'_>) {
        self.instances.retain_mut(|instance| {
            let object = &mut scene.objects[instance.index];
            if matches!(hook, Hook::Start) && !instance.hooks.start
                || matches!(hook, Hook::Ui(_)) && !instance.hooks.ui
            {
                return true;
            }
            match instance.execute(&self.engine, object, hook) {
                Ok(effects) => {
                    append_effects(&mut self.effects, effects);
                    true
                }
                Err(error) => {
                    let name = match hook {
                        Hook::Start => "start",
                        Hook::Ui(_) => "on_ui",
                        _ => "update",
                    };
                    errors.push(format!("{} · {name}: {error}", object.name));
                    false
                }
            }
        });
    }
}

#[derive(Clone, Copy)]
enum Hook<'a> {
    Start,
    Update(&'a Map, f32),
    Event(&'a Map),
    Ui(&'a Map),
}

fn append_effects(pending: &mut Vec<Effect>, effects: Vec<Effect>) {
    const MAX_EFFECTS: usize = 256;
    pending.extend(
        effects
            .into_iter()
            .take(MAX_EFFECTS.saturating_sub(pending.len())),
    );
}

pub(crate) fn input_map(movement: Vec2, controls: &InputControls) -> Map {
    use crate::controls::{Button, Key};
    let keys = |values: &std::collections::BTreeSet<Key>| {
        values
            .iter()
            .map(|key| key.name().to_lowercase().into())
            .collect::<rhai::Array>()
    };
    let buttons = |values: &std::collections::BTreeSet<Button>| {
        values
            .iter()
            .map(|button| button.name().into())
            .collect::<rhai::Array>()
    };
    let mut mouse = Map::new();
    for (name, value) in [
        ("x", controls.mouse.position[0]),
        ("y", controls.mouse.position[1]),
        ("wheel_x", controls.mouse.wheel[0]),
        ("wheel_y", controls.mouse.wheel[1]),
    ] {
        mouse.insert(name.into(), f64::from(value).into());
    }
    mouse.insert("down".into(), buttons(&controls.mouse.down).into());
    mouse.insert("pressed".into(), buttons(&controls.mouse.pressed).into());
    mouse.insert("released".into(), buttons(&controls.mouse.released).into());
    let mut map = Map::new();
    map.insert("x".into(), f64::from(movement.x).into());
    map.insert("z".into(), f64::from(movement.y).into());
    map.insert("keys".into(), keys(&controls.keys).into());
    map.insert("pressed".into(), keys(&controls.pressed).into());
    map.insert("released".into(), keys(&controls.released).into());
    map.insert("mouse".into(), mouse.into());
    map
}

impl Instance {
    fn object_map(&self, object: &Object) -> Map {
        let mut map = Map::new();
        for (keys, values) in [
            (POSITION, object.position),
            (ROTATION, object.rotation),
            (SCALE, object.scale),
        ] {
            for (key, value) in keys.into_iter().zip(values) {
                map.insert(key.into(), f64::from(value).into());
            }
        }
        map.insert("visible".into(), object.visible.into());
        map.insert("state".into(), self.state.clone().into());
        if !self.clips.is_empty() {
            let fallback = Player::default();
            let player = object.animation.as_ref().unwrap_or(&fallback);
            map.insert("anim_clip".into(), player.clip.clone().into());
            map.insert("anim_time".into(), f64::from(player.time).into());
            map.insert("anim_speed".into(), f64::from(player.speed).into());
            map.insert("anim_playing".into(), player.playing.into());
            map.insert("anim_loop".into(), player.looping.into());
        }
        map
    }

    fn view(
        &self,
        engine: &Engine,
        object: &Object,
        camera: &View,
        input: &Map,
        dt: f32,
    ) -> Result<View, String> {
        let mut view = Map::new();
        for (key, value) in POSITION.into_iter().zip(camera.target).chain([
            ("yaw", camera.yaw),
            ("pitch", camera.pitch),
            ("distance", camera.distance),
        ]) {
            view.insert(key.into(), f64::from(value).into());
        }
        let result: Map = engine
            .call_fn_with_options(
                CallFnOptions::new().eval_ast(false),
                &mut Scope::new(),
                &self.ast,
                "camera",
                (view, self.object_map(object), f64::from(dt), input.clone()),
            )
            .map_err(|error| error.to_string())?;
        let [yaw, pitch, distance] = vector(&result, ["yaw", "pitch", "distance"])?;
        if !(-89. ..=89.).contains(&pitch) || !(0.1..=500.).contains(&distance) {
            return Err("Use a camera pitch within -89..89 and distance within 0.1..500.".into());
        }
        Ok(View {
            target: vector(&result, POSITION)?,
            yaw,
            pitch,
            distance,
        })
    }

    fn execute(
        &mut self,
        engine: &Engine,
        object: &mut Object,
        hook: Hook<'_>,
    ) -> Result<Vec<Effect>, String> {
        let options = CallFnOptions::new().eval_ast(false);
        let mut scope = Scope::new();
        let data = self.object_map(object);
        let mut result: Map = match hook {
            Hook::Start => {
                engine.call_fn_with_options(options, &mut scope, &self.ast, "start", (data,))
            }
            Hook::Update(input, dt) => engine.call_fn_with_options(
                options,
                &mut scope,
                &self.ast,
                "update",
                (data, f64::from(dt), input.clone()),
            ),
            Hook::Event(event) => engine.call_fn_with_options(
                options,
                &mut scope,
                &self.ast,
                "on_event",
                (data, event.clone()),
            ),
            Hook::Ui(event) => engine.call_fn_with_options(
                options,
                &mut scope,
                &self.ast,
                "on_ui",
                (data, event.clone()),
            ),
        }
        .map_err(|error| error.to_string())?;

        let mut effects = Vec::new();
        for (field, effect) in [
            ("next_scene", Effect::Scene as fn(String) -> Effect),
            ("save_scene", Effect::Save),
            ("play_sound", Effect::Sound),
        ] {
            if let Some(value) = result.remove(field) {
                let id = value
                    .into_string()
                    .map_err(|_| format!("Expected object.{field} to be an ID string."))?;
                if !id.is_empty() {
                    effects.push(effect(id));
                }
            }
        }
        if let Some(value) = result.get("resume_game") {
            if value
                .as_bool()
                .map_err(|_| "Expected object.resume_game to be a boolean.")?
            {
                effects.push(Effect::Resume);
            }
        }
        if result.contains_key("volume_step") {
            effects.push(Effect::Volume(scalar(&result, "volume_step")?));
        }
        let position = vector(&result, POSITION)?;
        let rotation = vector(&result, ROTATION)?;
        let scale = vector(&result, SCALE)?;
        let visible = result
            .get("visible")
            .and_then(|value| value.as_bool().ok())
            .ok_or("Expected object.visible to be a boolean.")?;
        let state = result
            .remove("state")
            .and_then(|value| value.try_cast::<Map>())
            .ok_or("Expected object.state to be a map.")?;

        let animation = if self.clips.is_empty() {
            object.animation.clone()
        } else {
            let name = result
                .remove("anim_clip")
                .and_then(|value| value.into_string().ok())
                .ok_or("Expected object.anim_clip to be a clip name.")?;
            if name.is_empty() {
                None
            } else {
                if !self.clips.contains(&name) {
                    return Err(format!("Unknown animation clip {name}."));
                }
                let mut time = scalar(&result, "anim_time")?;
                let speed = scalar(&result, "anim_speed")?;
                if time < 0. || !(-10. ..=10.).contains(&speed) {
                    return Err("Animation time must be positive and speed within -10..10.".into());
                }
                if let Some(old) = &object.animation {
                    if name != old.clip && time == old.time {
                        time = 0.;
                    }
                }
                let boolean = |key| {
                    result
                        .get(key)
                        .and_then(|value| value.as_bool().ok())
                        .ok_or_else(|| format!("Expected object.{key} to be a boolean."))
                };
                Some(Player {
                    clip: name,
                    time,
                    speed,
                    playing: boolean("anim_playing")?,
                    looping: boolean("anim_loop")?,
                })
            }
        };

        object.animation = animation;
        object.position = position;
        object.rotation = rotation;
        object.scale = scale;
        object.visible = visible;
        self.state = state;
        Ok(effects)
    }
}

fn vector(result: &Map, keys: [&str; 3]) -> Result<[f32; 3], String> {
    let mut vector = [0.; 3];
    for (output, key) in vector.iter_mut().zip(keys) {
        *output = scalar(result, key)?;
    }
    Ok(vector)
}

fn scalar(result: &Map, key: &str) -> Result<f32, String> {
    result
        .get(key)
        .and_then(|value| {
            value
                .as_float()
                .or_else(|_| value.as_int().map(|value| value as f64))
                .ok()
        })
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("Expected object.{key} to be a finite number."))
}
