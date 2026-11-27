use crate::{
    animation::{self, Influence, Player, Rig},
    controls::{Action, Bindings, GameActions, Gamepads, InputControls, PadButton},
    lighting::{self, Lighting, PointLight, Surface},
    physics::{self, Body, BodyKind, ColliderKind, Event, Physics},
    presentation::{Overlay, Presentation},
    progress::{self, Progress},
    scripting::{input_map, Effect, Scripts, DEFAULT_SCRIPT},
    settings::{Application, SCENE_VERSION},
};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub version: u32,
    #[serde(default)]
    pub application: Application,
    pub title: String,
    pub camera: View,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas2d: Option<crate::canvas2d::Canvas>,
    pub objects: Vec<Object>,
    pub models: Vec<Model>,
    #[serde(default)]
    pub scripts: Vec<ScriptAsset>,
    #[serde(default)]
    pub next_id: u64,
    #[serde(default = "physics::default_gravity")]
    pub gravity: [f32; 3],
    #[serde(default)]
    pub bindings: Bindings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_controller: Option<u64>,
    #[serde(default)]
    pub lighting: Lighting,
    #[serde(default, skip_serializing_if = "Presentation::is_empty")]
    pub presentation: Presentation,
}

impl PartialEq for Scene {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.application == other.application
            && self.title == other.title
            && self.camera == other.camera
            && self.canvas2d == other.canvas2d
            && self.objects == other.objects
            && self.models == other.models
            && self.scripts == other.scripts
            && self.gravity == other.gravity
            && self.bindings == other.bindings
            && self.camera_controller == other.camera_controller
            && self.lighting == other.lighting
            && self.presentation == other.presentation
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            target: [0., 0.7, 0.],
            yaw: 38.,
            pitch: 26.,
            distance: 8.,
        }
    }
}

impl View {
    pub fn camera(&self) -> Camera3D {
        let (yaw, pitch) = (
            self.yaw.to_radians(),
            self.pitch.clamp(-89., 89.).to_radians(),
        );
        let target = Vec3::from_array(self.target);
        let offset = vec3(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Camera3D {
            position: target + offset * self.distance.max(0.1),
            target,
            up: Vec3::Y,
            z_far: 2000.,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<PrefabLink>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub components: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Player>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub animation_blend: f32,
    #[doc(hidden)]
    #[serde(skip)]
    pub animation_blend_state: Option<animation::BlendState>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub animation_markers: std::collections::BTreeMap<String, Vec<animation::Marker>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub animate_grounded_motion: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<PointLight>,
    #[serde(default = "casts_shadows")]
    pub cast_shadows: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body2d: Option<crate::canvas2d::Body>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite2d: Option<crate::canvas2d::Sprite>,
    #[serde(default)]
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_asset: Option<usize>,
    pub name: String,
    pub shape: Shape,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub visible: bool,
    pub spin: f32,
    pub speed: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefabLink {
    // Preserve local ownership when a later template edit happens to match the local value.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub overrides: std::collections::BTreeSet<Vec<String>>,
    pub name: String,
    pub source: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Shape {
    Empty,
    Cube,
    Sphere,
    Plane,
    Model(usize),
}

fn casts_shadows() -> bool {
    true
}

fn is_zero(value: &f32) -> bool {
    *value == 0.
}

impl Object {
    pub fn new(name: impl Into<String>, shape: Shape) -> Self {
        Self {
            prefab: None,
            animation: None,
            animation_blend: 0.,
            animation_blend_state: None,
            animation_markers: Default::default(),
            animate_grounded_motion: false,
            surface: None,
            normal_strength: None,
            light: None,
            cast_shadows: true,
            components: Default::default(),
            body: None,
            body2d: None,
            sprite2d: None,
            id: 0,
            parent: None,
            script_asset: None,
            name: name.into(),
            shape,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
            visible: true,
            spin: 0.,
            speed: 0.,
            script: None,
        }
    }

    pub(crate) fn animation_pose(&self, rig: &Rig) -> animation::Pose {
        self.animation_blend_state.as_ref().map_or_else(
            || rig.pose(self.animation.as_ref()),
            |blend| blend.pose(rig),
        )
    }

    pub fn is_character(&self) -> bool {
        self.body
            .as_ref()
            .is_some_and(|b| b.kind == BodyKind::Character)
    }

    pub fn is_dynamic(&self) -> bool {
        self.body
            .as_ref()
            .is_some_and(|body| body.kind == BodyKind::Dynamic)
    }

    pub fn matrix(&self) -> Mat4 {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        Mat4::from_scale_rotation_translation(
            Vec3::from_array(self.scale),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            Vec3::from_array(self.position),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub parts: Arc<[Part]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<Arc<Rig>>,
    #[serde(default, skip_serializing_if = "is_empty_lods")]
    pub lods: Arc<[LodLevel]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LodLevel {
    pub distance: f32,
    pub parts: Arc<[Part]>,
}

fn is_empty_lods(levels: &[LodLevel]) -> bool {
    levels.is_empty()
}

impl Model {
    pub const MAX_LODS: usize = 4;

    pub fn lod_level(&self, distance: f32, previous: Option<usize>) -> usize {
        let Some(mut level) = previous.filter(|&i| i <= self.lods.len()) else {
            return self
                .lods
                .partition_point(|entry| entry.distance <= distance);
        };
        while level < self.lods.len() && distance >= self.lods[level].distance * 1.1 {
            level += 1;
        }
        while level > 0 && distance < self.lods[level - 1].distance * 0.9 {
            level -= 1;
        }
        level
    }

    pub fn parts_at(&self, level: usize) -> &[Part] {
        level
            .checked_sub(1)
            .and_then(|i| self.lods.get(i))
            .map_or(&self.parts, |lod| &lod.parts)
    }
}

fn triangle_count(parts: &[Part]) -> usize {
    parts.iter().map(|part| part.indices.len() / 3).sum()
}

impl PartialEq for Model {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && (Arc::ptr_eq(&self.parts, &other.parts) || self.parts == other.parts)
            && (Arc::ptr_eq(&self.lods, &other.lods) || self.lods == other.lods)
            && match (&self.rig, &other.rig) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                (None, None) => true,
                _ => false,
            }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptAsset {
    pub name: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    #[serde(default)]
    pub surface: Surface,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic_roughness: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_map: Option<NormalMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emissive_texture: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub influences: Vec<Influence>,
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalMap {
    pub texture: Vec<u8>,
    pub strength: f32,
    pub tangents: Vec<[f32; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Scene {
    pub fn empty() -> Self {
        Self {
            version: SCENE_VERSION,
            application: Application::default(),
            title: "Untitled Scene".into(),
            camera: View::default(),
            canvas2d: None,
            objects: Vec::new(),
            models: Vec::new(),
            scripts: Vec::new(),
            next_id: 1,
            gravity: physics::default_gravity(),
            bindings: Bindings::default(),
            camera_controller: None,
            lighting: Lighting::default(),
            presentation: Presentation::default(),
        }
    }

    pub fn demo() -> Self {
        let mut scene = Self {
            title: "Orbit Workshop".into(),
            ..Self::empty()
        };
        let mut cube = Object::new("Pilot", Shape::Cube);
        cube.position = [0., 0.55, 0.];
        cube.speed = 3.;
        let mut sphere = Object::new("Beacon", Shape::Sphere);
        sphere.position = [1.8, 0.65, -0.7];
        let mut marker = Object::new("Spinner", Shape::Cube);
        marker.position = [-1.65, 0.6, -0.75];
        marker.rotation = [20., 25., 15.];
        marker.scale = [0.65; 3];
        marker.script = Some(DEFAULT_SCRIPT.into());
        let mut floor = Object::new("Platform", Shape::Plane);
        floor.scale = [6., 1., 4.5];
        for object in [cube, sphere, marker, floor] {
            scene.add_object(object).expect("demo IDs fit the scene");
        }
        scene
    }

    pub fn physics_demo() -> Self {
        let mut scene = Self {
            title: "Physics Workshop".into(),
            ..Self::empty()
        };
        let mut floor = Object::new("Floor", Shape::Cube);
        floor.position = [0., -0.25, 0.];
        floor.scale = [12., 0.5, 7.];
        floor.body = Some(Body::default());

        let mut player = Object::new("Rover", Shape::Sphere);
        player.position = [-3.5, 2., 0.];
        player.speed = 3.;
        player.body = Some(Body {
            kind: BodyKind::Dynamic,
            collider: ColliderKind::Sphere,
            lock_rotation: true,
            ..Body::default()
        });
        player.script = Some(
            r#"fn update(object, dt, input) { object }
fn camera(view, object, dt, input) {
    view.x = object.x * 0.6;
    view.z = object.z * 0.6;
    view
}
"#
            .into(),
        );

        let mut trigger = Object::new("Trigger Beacon", Shape::Cube);
        trigger.position = [-0.4, 0.75, 0.];
        trigger.scale = [0.8, 1.5, 1.4];
        trigger.visible = false;
        trigger.body = Some(Body {
            sensor: true,
            ..Body::default()
        });
        trigger.script = Some(
            r#"fn update(object, dt, input) { object }
fn on_event(object, event) {
    if event.other_name == "Rover" {
        if event.kind == "trigger_enter" { object.visible = true; }
        if event.kind == "trigger_exit" { object.visible = false; }
    }
    object
}
"#
            .into(),
        );

        let mut barrier = Object::new("Barrier", Shape::Cube);
        barrier.position = [2.1, 0.6, 0.];
        barrier.scale = [0.6, 1.2, 3.4];
        barrier.body = Some(Body::default());
        scene.add_object(floor).expect("demo has IDs");
        scene.camera_controller = Some(scene.add_object(player).expect("demo has IDs"));
        scene.add_object(trigger).expect("demo has IDs");
        scene.add_object(barrier).expect("demo has IDs");
        scene
    }

    pub fn rendering_demo() -> Self {
        use animation::{Clip, Interpolation, Node, Property, Skin, Track};
        let mut scene = Self {
            title: "Light & Motion Workshop".into(),
            ..Self::empty()
        };
        scene.camera = View {
            target: [0., 1.25, 0.],
            yaw: 33.,
            pitch: 24.,
            distance: 8.,
        };
        scene.lighting = Lighting {
            direction: [-0.65, 0.8, 0.5],
            shadow_extent: 6.,
            ..Lighting::default()
        };
        let mut floor = Object::new("Shadow platform", Shape::Cube);
        floor.position = [0., -0.2, 0.];
        floor.scale = [8., 0.4, 5.5];
        floor.surface = Some(Surface {
            base_color: [0.28, 0.38, 0.55, 1.],
            roughness: 0.85,
            ..Surface::default()
        });

        let mesh = cube_mesh();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut influences = Vec::new();
        for (center, size, joint, tint) in [
            (
                vec3(0., 0.64, 0.),
                vec3(0.5, 1.28, 0.5),
                0,
                [0.95, 0.46, 0.1, 1.],
            ),
            (
                vec3(0., 1.77, 0.),
                vec3(0.42, 1.26, 0.42),
                1,
                [0.88, 0.25, 0.08, 1.],
            ),
            (
                vec3(0., 2.39, 0.),
                vec3(0.7, 0.22, 0.64),
                1,
                [0.65, 0.15, 0.03, 1.],
            ),
        ] {
            let start = vertices.len() as u32;
            for vertex in &mesh.vertices {
                vertices.push(ModelVertex {
                    position: (center + vertex.position * size).to_array(),
                    normal: vertex.normal.truncate().to_array(),
                    uv: [0.; 2],
                    color: tint,
                });
                influences.push(Influence {
                    joints: [joint, 0, 0, 0],
                    weights: [1., 0., 0., 0.],
                });
            }
            indices.extend(mesh.indices.iter().map(|&index| start + u32::from(index)));
        }
        let part = Part {
            vertices,
            indices,
            texture: None,
            metallic_roughness: None,
            normal_map: None,
            emissive_texture: None,
            node: Some(0),
            skin: Some(0),
            influences,
            surface: Surface {
                metallic: 0.45,
                roughness: 0.32,
                ..Surface::default()
            },
        };
        let nodes = vec![
            Node {
                parent: None,
                position: [0.; 3],
                rotation: Quat::IDENTITY.to_array(),
                scale: [1.; 3],
            },
            Node {
                parent: Some(0),
                position: [0., 1.2, 0.],
                rotation: Quat::IDENTITY.to_array(),
                scale: [1.; 3],
            },
        ];
        let skins = vec![Skin {
            joints: vec![0, 1],
            inverse_bind: vec![
                Mat4::IDENTITY.to_cols_array_2d(),
                Mat4::from_translation(vec3(0., -1.2, 0.)).to_cols_array_2d(),
            ],
        }];
        let clip = |name: &str, axis: Vec3, angle: f32| Clip {
            name: name.into(),
            duration: 2.4,
            markers: Vec::new(),
            tracks: vec![Track {
                node: 1,
                property: Property::Rotation,
                interpolation: Interpolation::Linear,
                times: vec![0., 0.6, 1.2, 1.8, 2.4],
                values: [0., angle, 0., -angle, 0.]
                    .map(|angle| Quat::from_axis_angle(axis, angle).to_array())
                    .to_vec(),
            }],
        };
        scene.models.push(Model {
            lods: Default::default(),
            name: "Workshop Arm".into(),
            parts: vec![part].into(),
            rig: Some(Arc::new(Rig {
                nodes,
                skins,
                clips: vec![clip("Sweep", Vec3::Z, 0.85), clip("Reach", Vec3::X, 0.65)],
            })),
        });
        let mut arm = Object::new("Skinned arm", Shape::Model(0));
        arm.position = [-0.8, 0., 0.];

        let mut smooth = Object::new("Polished metal", Shape::Sphere);
        smooth.position = [1.35, 0.65, 0.45];
        smooth.scale = [1.3; 3];
        smooth.surface = Some(Surface {
            metallic: 0.85,
            roughness: 0.16,
            base_color: [0.76, 0.95, 1., 1.],
            ..Surface::default()
        });
        let mut matte = Object::new("Rough ceramic", Shape::Sphere);
        matte.position = [2.5, 0.5, -0.65];
        matte.speed = 2.;
        matte.surface = Some(Surface {
            roughness: 0.91,
            base_color: [1., 0.62, 0.78, 1.],
            ..Surface::default()
        });
        let mut point = Object::new("Blue fill light", Shape::Empty);
        point.position = [1.9, 2.5, 1.7];
        point.light = Some(PointLight {
            intensity: 13.,
            ..PointLight::default()
        });
        for object in [floor, arm, smooth, matte, point] {
            scene.add_object(object).expect("demo has IDs");
        }
        scene
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut scene: Self = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        match scene.version {
            // Alpha scenes stored the launch settings implicitly. Their defaults
            // reconstruct the original identifier, host window and Android layout.
            1 => scene.version = SCENE_VERSION,
            SCENE_VERSION => {}
            version => return Err(format!("Unsupported scene version {version}. This engine reads versions 1–{SCENE_VERSION}.")),
        }
        scene.assign_ids()?;
        scene.validate()?;
        Ok(scene)
    }

    pub fn assign_ids(&mut self) -> Result<(), String> {
        let max_id = self
            .objects
            .iter()
            .map(|object| object.id)
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id.saturating_add(1)).max(1);
        for object in &mut self.objects {
            if object.id == 0 {
                object.id = self.next_id;
                self.next_id = self
                    .next_id
                    .checked_add(1)
                    .ok_or("Scene ran out of object IDs.")?;
            }
        }
        Ok(())
    }

    pub fn add_object(&mut self, mut object: Object) -> Result<u64, String> {
        self.assign_ids()?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("Scene ran out of object IDs.")?;
        object.id = id;
        if object.animation.is_none() {
            if let Shape::Model(index) = object.shape {
                object.animation = self
                    .models
                    .get(index)
                    .and_then(|model| model.rig.as_ref())
                    .and_then(|rig| rig.clips.first())
                    .map(|clip| Player {
                        clip: clip.name.clone(),
                        ..Player::default()
                    });
            }
        }
        self.objects.push(object);
        Ok(id)
    }

    pub fn object_index(&self, id: u64) -> Result<usize, String> {
        if id != 0 {
            if let Some(index) = self.objects.iter().position(|object| object.id == id) {
                return Ok(index);
            }
        }
        Err(format!("Unknown object ID {id}."))
    }

    pub fn set_parent(&mut self, child: u64, parent: Option<u64>) -> Result<(), String> {
        let index = self.object_index(child)?;
        if parent.is_some() && self.objects[index].body2d.is_some() {
            return Err("2D bodies must be at the scene root.".into());
        }
        if parent.is_some()
            && (self.objects[index].is_dynamic() || self.objects[index].is_character())
        {
            return Err("Dynamic and character bodies must be at the scene root.".into());
        }
        let mut ancestor = parent;
        while let Some(id) = ancestor {
            if id == child {
                return Err("Parenting would create a cycle.".into());
            }
            ancestor = self.objects[self.object_index(id)?].parent;
        }
        self.objects[index].parent = parent;
        Ok(())
    }

    pub fn delete_object(&mut self, id: u64, recursive: bool) -> Result<(), String> {
        self.object_index(id)?;
        if !recursive && self.objects.iter().any(|object| object.parent == Some(id)) {
            return Err("Object has children. Set recursive to delete the subtree.".into());
        }
        let mut removed = HashSet::from([id]);
        for (index, _, parent) in self.object_order() {
            if parent.is_some_and(|parent| removed.contains(&self.objects[parent].id)) {
                removed.insert(self.objects[index].id);
            }
        }
        self.objects.retain(|object| !removed.contains(&object.id));
        if self
            .camera_controller
            .is_some_and(|id| removed.contains(&id))
        {
            self.camera_controller = None;
        }
        Ok(())
    }

    /// Parent-first order with scene indices and indentation for the hierarchy.
    pub fn object_order(&self) -> Vec<(usize, usize, Option<usize>)> {
        let indices: HashMap<_, _> = self
            .objects
            .iter()
            .enumerate()
            .filter(|(_, object)| object.id != 0)
            .map(|(index, object)| (object.id, index))
            .collect();
        let mut children = vec![Vec::new(); self.objects.len()];
        let mut roots = Vec::new();
        for (index, object) in self.objects.iter().enumerate() {
            match object.parent {
                Some(id) => {
                    if let Some(&parent) = indices.get(&id) {
                        children[parent].push(index);
                    }
                }
                None => roots.push(index),
            }
        }
        let mut pending: Vec<_> = roots
            .into_iter()
            .rev()
            .map(|index| (index, 0, None))
            .collect();
        let mut order = Vec::with_capacity(self.objects.len());
        while let Some((index, depth, parent)) = pending.pop() {
            order.push((index, depth, parent));
            pending.extend(
                children[index]
                    .iter()
                    .rev()
                    .map(|&child| (child, depth + 1, Some(index))),
            );
        }
        order
    }

    pub fn world_transforms(&self) -> Vec<(Mat4, bool)> {
        let mut world = vec![(Mat4::IDENTITY, false); self.objects.len()];
        for (index, _, parent) in self.object_order() {
            let object = &self.objects[index];
            let (matrix, visible) = parent.map_or((Mat4::IDENTITY, true), |parent| world[parent]);
            world[index] = (matrix * object.matrix(), visible && object.visible);
        }
        world
    }

    pub fn remove_model(&mut self, index: usize) -> Result<(), String> {
        if index >= self.models.len() {
            return Err("Unknown model asset.".into());
        }
        if self
            .objects
            .iter()
            .any(|object| object.shape == Shape::Model(index))
        {
            return Err("Remove this model's instances before removing its asset.".into());
        }
        self.models.remove(index);
        for object in &mut self.objects {
            if let Shape::Model(current) = &mut object.shape {
                if *current > index {
                    *current -= 1;
                }
            }
        }
        Ok(())
    }

    pub fn remove_script(&mut self, index: usize) -> Result<(), String> {
        if index >= self.scripts.len() {
            return Err("Unknown script asset.".into());
        }
        if self
            .objects
            .iter()
            .any(|object| object.script_asset == Some(index))
        {
            return Err("Detach this script from every object before removing its asset.".into());
        }
        self.scripts.remove(index);
        for object in &mut self.objects {
            if let Some(current) = &mut object.script_asset {
                if *current > index {
                    *current -= 1;
                }
            }
        }
        Ok(())
    }

    pub fn validate_objects(&self) -> Result<(), String> {
        self.application.validate()?;
        if let Some(canvas) = &self.canvas2d {
            canvas.validate(self)?;
        }
        if !self
            .camera
            .target
            .into_iter()
            .chain([self.camera.yaw, self.camera.pitch, self.camera.distance])
            .all(f32::is_finite)
        {
            return Err("Camera settings must use finite numbers.".into());
        }
        self.bindings.validate()?;
        lighting::validate_color_shader(&self.lighting.color_shader)?;
        if !self.lighting.validate() {
            return Err("Invalid lighting settings.".into());
        }
        if !self
            .gravity
            .into_iter()
            .all(|value| (-1000. ..=1000.).contains(&value))
        {
            return Err("Gravity components must be within -1000..1000.".into());
        }
        if let Some(id) = self.camera_controller {
            self.object_index(id)?;
        }
        let mut ids = HashSet::new();
        for object in &self.objects {
            if object.light.as_ref().is_some_and(|light| !light.validate())
                || object
                    .surface
                    .as_ref()
                    .is_some_and(|surface| !surface.validate())
            {
                return Err(format!("Invalid light or material on {}.", object.name));
            }
            if object
                .normal_strength
                .is_some_and(|value| !value.is_finite())
            {
                return Err(format!("Invalid normal strength on {}.", object.name));
            }
            if !(0. ..=animation::MAX_BLEND_SECONDS).contains(&object.animation_blend) {
                return Err(format!(
                    "Invalid animation transition duration on {}.",
                    object.name
                ));
            }
            let model = match object.shape {
                Shape::Model(index) => self.models.get(index),
                _ => None,
            };
            for (name, markers) in &object.animation_markers {
                // Overrides for a removed clip can be recovered after reimporting it.
                let duration = model
                    .and_then(|m| m.animation_clip(name))
                    .map_or(f32::MAX, |c| c.duration);
                if !animation::valid_markers(markers, duration) {
                    return Err(format!(
                        "Invalid animation markers for clip '{name}' on {}.",
                        object.name
                    ));
                }
            }
            if let Some(state) = &object.animation {
                if !state.time.is_finite()
                    || state.time < 0.
                    || !(-10. ..=10.).contains(&state.speed)
                    || model.and_then(|m| m.animation_clip(&state.clip)).is_none()
                {
                    return Err(format!("Invalid animation on {}.", object.name));
                }
            }
            if let Some(body) = &object.body {
                body.validate()
                    .map_err(|error| format!("{}: {error}", object.name))?;
                if (object.is_dynamic() || object.is_character()) && object.parent.is_some() {
                    return Err(format!(
                        "{}: dynamic and character bodies must be at the scene root.",
                        object.name
                    ));
                }
            }
            if !object
                .position
                .into_iter()
                .chain(object.rotation)
                .chain(object.scale)
                .chain([object.spin, object.speed])
                .all(f32::is_finite)
            {
                return Err(format!("Invalid transform or behavior on {}.", object.name));
            }
            if object.id != 0 && !ids.insert(object.id) {
                return Err(format!("Duplicate object ID {}.", object.id));
            }
            if let Some(index) = object.script_asset {
                if index >= self.scripts.len() || object.script.is_some() {
                    return Err(format!("Invalid script attachment on {}.", object.name));
                }
            }
            if let Shape::Model(index) = object.shape {
                if index >= self.models.len() {
                    return Err(format!("Missing model for {}", object.name));
                }
            }
        }
        if self.object_order().len() != self.objects.len() {
            return Err("Scene contains a missing parent or a parenting cycle.".into());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != SCENE_VERSION {
            return Err(format!("Unsupported scene version {}", self.version));
        }
        self.validate_objects()?;
        self.presentation.validate()?;
        for model in &self.models {
            if model.rig.as_ref().is_some_and(|rig| !rig.validate()) {
                return Err(format!("Invalid animation rig for {}.", model.name));
            }
            let mut distance = 0.;
            let mut triangles = triangle_count(&model.parts);
            if model.lods.len() > Model::MAX_LODS || (!model.lods.is_empty() && model.rig.is_some())
            {
                return Err(format!(
                    "LOD needs a static model with at most {} levels: {}.",
                    Model::MAX_LODS,
                    model.name
                ));
            }
            for lod in model.lods.iter() {
                let count = triangle_count(&lod.parts);
                if !(0.1..=10_000.).contains(&lod.distance)
                    || lod.distance <= distance
                    || count == 0
                    || count >= triangles
                {
                    return Err(format!(
                        "LOD distances must increase and triangle counts decrease: {}.",
                        model.name
                    ));
                }
                distance = lod.distance;
                triangles = count;
            }
            if model.parts.is_empty()
                || model
                    .parts
                    .iter()
                    .chain(model.lods.iter().flat_map(|lod| lod.parts.iter()))
                    .any(|part| {
                        !part.surface.validate()
                            || part.normal_map.as_ref().is_some_and(|map| {
                                !map.strength.is_finite()
                                    || map.texture.is_empty()
                                    || map.tangents.len() != part.vertices.len()
                                    || map.tangents.iter().any(|t| {
                                        !t.iter().all(|x| x.is_finite()) || t[3].abs() != 1.
                                    })
                            })
                            || part.node.is_some_and(|node| {
                                model.rig.as_ref().is_none_or(|rig| node >= rig.nodes.len())
                            })
                            || match part.skin {
                                Some(index) => model
                                    .rig
                                    .as_ref()
                                    .and_then(|rig| rig.skins.get(index))
                                    .is_none_or(|skin| {
                                        part.influences.len() != part.vertices.len()
                                            || part.influences.iter().any(|influence| {
                                                influence
                                                    .joints
                                                    .into_iter()
                                                    .zip(influence.weights)
                                                    .any(|(joint, weight)| {
                                                        !weight.is_finite()
                                                            || weight < 0.
                                                            || (weight > 0.
                                                                && usize::from(joint)
                                                                    >= skin.joints.len())
                                                    })
                                            })
                                    }),
                                None => !part.influences.is_empty(),
                            }
                            || part.indices.is_empty()
                            || part.indices.len() % 3 != 0
                            || part
                                .indices
                                .iter()
                                .any(|&index| index as usize >= part.vertices.len())
                    })
            {
                return Err(format!("Invalid triangles in {}", model.name));
            }
        }
        Ok(())
    }
}

pub struct PreparedScene {
    cube: Mesh,
    sphere: Mesh,
    plane: Mesh,
    models: Vec<PreparedModel>,
    lod_history: HashMap<u64, (usize, usize)>,
    stats: RenderStats,
    renderer: Option<lighting::Renderer>,
    sky: Option<Material>,
    render2d: crate::canvas2d::Render,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub model_triangles: usize,
    pub full_detail_triangles: usize,
    pub lod_instances: usize,
}

struct PreparedModel {
    base: Arc<[Part]>,
    lods: Arc<[LodLevel]>,
    levels: Vec<Vec<PreparedPart>>,
}

impl PreparedModel {
    fn new(model: &Model) -> Self {
        let levels = std::iter::once(model.parts.as_ref())
            .chain(model.lods.iter().map(|lod| lod.parts.as_ref()))
            .map(|parts| parts.iter().map(prepare_part).collect())
            .collect();
        Self {
            base: model.parts.clone(),
            lods: model.lods.clone(),
            levels,
        }
    }
}

struct PreparedPart {
    batches: Vec<Batch>,
    metal: Option<Texture2D>,
    emissive: Option<Texture2D>,
    normal: Option<Texture2D>,
}

struct Batch {
    mesh: Mesh,
    source: Vec<usize>,
}

impl PreparedScene {
    pub fn new(scene: &Scene) -> Self {
        let renderer = match lighting::Renderer::new(&scene.lighting) {
            Ok(renderer) => Some(renderer),
            Err(error) => {
                macroquad::logging::error!("PBR renderer unavailable: {}", error);
                None
            }
        };
        Self {
            render2d: Default::default(),
            sky: lighting::sky_material()
                .inspect_err(|e| {
                    macroquad::logging::error!("Skybox: {}", e);
                })
                .ok(),
            cube: cube_mesh(),
            sphere: sphere_mesh(),
            plane: plane_mesh(),
            models: scene.models.iter().map(PreparedModel::new).collect(),
            renderer,
            lod_history: HashMap::new(),
            stats: RenderStats::default(),
        }
    }

    pub fn render_stats(&self) -> RenderStats {
        self.stats
    }

    pub fn apply_color_shader(&mut self, source: &str) -> Result<(), String> {
        self.renderer
            .as_mut()
            .ok_or("GPU renderer unavailable.")?
            .apply_shader(source)
    }

    pub fn draw(&mut self, scene: &Scene, camera: &Camera3D, grid: bool, selected: Option<usize>) {
        if scene.canvas2d.is_some() {
            self.render2d.draw(scene, camera, grid, selected);
            return;
        }
        self.models.truncate(scene.models.len());
        for (index, model) in scene.models.iter().enumerate() {
            if let Some(prepared) = self.models.get_mut(index) {
                if !Arc::ptr_eq(&prepared.base, &model.parts)
                    || !Arc::ptr_eq(&prepared.lods, &model.lods)
                {
                    *prepared = PreparedModel::new(model);
                    self.lod_history.retain(|_, (asset, _)| *asset != index);
                }
            } else {
                self.models.push(PreparedModel::new(model));
            }
        }
        let world = scene.world_transforms();
        let mut active = HashMap::new();
        self.stats = RenderStats::default();
        let levels: Vec<_> = scene
            .objects
            .iter()
            .zip(&world)
            .map(|(object, (matrix, visible))| {
                let Shape::Model(index) = object.shape else {
                    return 0;
                };
                if !visible {
                    return 0;
                }
                let model = &scene.models[index];
                let level = if model.lods.is_empty() {
                    0
                } else {
                    let distance = camera
                        .position
                        .distance(matrix.transform_point3(Vec3::ZERO));
                    let previous = self
                        .lod_history
                        .get(&object.id)
                        .filter(|&&(asset, _)| asset == index)
                        .map(|&(_, level)| level);
                    let level = model.lod_level(distance, previous);
                    active.insert(object.id, (index, level));
                    level
                };
                self.stats.model_triangles += triangle_count(model.parts_at(level));
                self.stats.full_detail_triangles += triangle_count(&model.parts);
                self.stats.lod_instances += usize::from(level > 0);
                level
            })
            .collect();
        self.lod_history = active;
        let poses: Vec<_> = scene
            .objects
            .iter()
            .zip(&world)
            .map(|(object, (_, visible))| {
                if !visible {
                    return None;
                }
                let Shape::Model(index) = object.shape else {
                    return None;
                };
                scene.models[index]
                    .rig
                    .as_ref()
                    .map(|rig| object.animation_pose(rig))
            })
            .collect();
        let mut renderer = self.renderer.take();
        if let Some(gpu) = &mut renderer {
            gpu.sync_shadow_profile(&scene.lighting);
            gpu.sync_shader(&scene.lighting.color_shader);
        }
        if let Some(gpu) = &renderer {
            let shadow = gpu.shadow_camera(camera.target, &scene.lighting);
            if scene.lighting.shadows {
                set_camera(&shadow);
                clear_background(WHITE);
                gl_use_material(&gpu.depth);
                self.geometry(scene, &world, &poses, &levels, Some(gpu), true);
                gl_use_default_material();
            }
            set_camera(camera);
            let mut points: Vec<_> = scene
                .objects
                .iter()
                .zip(&world)
                .filter_map(|(object, (matrix, visible))| {
                    object
                        .light
                        .as_ref()
                        .filter(|_| *visible)
                        .map(|light| (matrix.transform_point3(Vec3::ZERO), light))
                })
                .collect();
            points.sort_by(|(a, _), (b, _)| {
                a.distance_squared(camera.position)
                    .total_cmp(&b.distance_squared(camera.position))
            });
            gpu.lights(camera, &shadow, &scene.lighting, &points);
        } else {
            set_camera(camera);
        }
        if let Some(sky) = &self.sky {
            scene.lighting.skybox.draw(sky, &self.cube, camera.position);
        }
        if grid {
            draw_grid_ex(
                40,
                1.,
                Color::from_rgba(50, 63, 81, 255),
                Color::from_rgba(80, 98, 122, 255),
                vec3(0., -0.015, 0.),
                Quat::IDENTITY,
            );
        }
        if let Some(gpu) = &renderer {
            gl_use_material(&gpu.main);
        }
        self.geometry(scene, &world, &poses, &levels, renderer.as_ref(), false);
        gl_use_default_material();
        if let Some(index) = selected {
            if let Some((matrix, true)) = world.get(index) {
                let origin = matrix.transform_point3(Vec3::ZERO);
                for (axis, color) in [(Vec3::X, RED), (Vec3::Y, GREEN), (Vec3::Z, SKYBLUE)] {
                    draw_line_3d(origin, origin + axis * 1.5, color);
                }
            }
        }
        set_default_camera();
        self.renderer = renderer;
    }

    fn geometry(
        &mut self,
        scene: &Scene,
        world: &[(Mat4, bool)],
        poses: &[Option<animation::Pose>],
        levels: &[usize],
        gpu: Option<&lighting::Renderer>,
        shadow: bool,
    ) {
        let default = Surface::default();
        let mut last_maps = [None; 3];
        if let Some(gpu) = gpu.filter(|_| !shadow) {
            gpu.maps([None; 3]);
        }
        for (index, &(matrix, visible)) in world.iter().enumerate() {
            let object = &scene.objects[index];
            if !visible || (shadow && !object.cast_shadows) || object.shape == Shape::Empty {
                continue;
            }
            unsafe { get_internal_gl().quad_gl.push_model_matrix(matrix) };
            match object.shape {
                Shape::Empty => {}
                Shape::Cube | Shape::Sphere | Shape::Plane => {
                    let surface = object.surface.as_ref().unwrap_or(&default);
                    configure_surface(gpu, shadow, matrix, surface, [None; 3], 0., &mut last_maps);
                    if !shadow || surface.alpha != lighting::Alpha::Blend {
                        draw_mesh(match object.shape {
                            Shape::Cube => &self.cube,
                            Shape::Sphere => &self.sphere,
                            _ => &self.plane,
                        });
                    }
                }
                Shape::Model(model) => {
                    let level = levels[index];
                    for (part, prepared) in scene.models[model]
                        .parts_at(level)
                        .iter()
                        .zip(&mut self.models[model].levels[level])
                    {
                        let surface = object.surface.as_ref().unwrap_or(&part.surface);
                        if shadow && surface.alpha == lighting::Alpha::Blend {
                            continue;
                        }
                        configure_surface(
                            gpu,
                            shadow,
                            matrix,
                            surface,
                            [
                                prepared.metal.as_ref(),
                                prepared.emissive.as_ref(),
                                prepared.normal.as_ref(),
                            ],
                            part.normal_map
                                .as_ref()
                                .map_or(0., |map| object.normal_strength.unwrap_or(map.strength)),
                            &mut last_maps,
                        );
                        for batch in &mut prepared.batches {
                            if let Some(pose) = &poses[index] {
                                for (vertex, &source) in
                                    batch.mesh.vertices.iter_mut().zip(&batch.source)
                                {
                                    let animated = pose.vertex(part, source);
                                    vertex.position = Vec3::from_array(animated.position);
                                    let tangent = part.normal_map.as_ref().map(|map| {
                                        pose.tangent(
                                            part,
                                            source,
                                            animated.normal,
                                            map.tangents[source],
                                        )
                                    });
                                    vertex.normal = gpu_normal(animated.normal, tangent);
                                }
                            }
                            draw_mesh(&batch.mesh);
                        }
                    }
                }
            }
            unsafe { get_internal_gl().quad_gl.pop_model_matrix() };
        }
    }
}

fn configure_surface(
    gpu: Option<&lighting::Renderer>,
    shadow: bool,
    matrix: Mat4,
    surface: &Surface,
    textures: [Option<&Texture2D>; 3],
    normal_strength: f32,
    last_maps: &mut [Option<macroquad::miniquad::TextureId>; 3],
) {
    let Some(gpu) = gpu else { return };
    if shadow {
        gpu.caster(surface);
        return;
    }
    let maps = textures.map(|map| map.map(Texture2D::raw_miniquad_id));
    if maps != *last_maps {
        // Custom sampler bindings are per pipeline, so submit earlier batches before changing them.
        unsafe {
            get_internal_gl().flush();
        }
        gpu.maps(textures);
        *last_maps = maps;
    }
    let normal = if matrix.determinant().abs() > 1e-10 {
        matrix.inverse().transpose()
    } else {
        Mat4::IDENTITY
    };
    gpu.main.set_uniform("NormalMatrix", normal);
    gpu.main.set_uniform(
        "NormalStrength",
        if textures[2].is_some() {
            normal_strength
        } else {
            0.
        },
    );
    gpu.main.set_uniform(
        "ModelHandedness",
        if matrix.determinant() < 0. {
            -1_f32
        } else {
            1.
        },
    );
    gpu.surface(surface);
}

pub(crate) fn tangent_reference(normal: Vec3) -> Vec3 {
    let axis = if normal.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    axis.cross(normal).normalize_or_zero()
}

fn gpu_normal(normal: [f32; 3], tangent: Option<[f32; 4]>) -> Vec4 {
    let n = Vec3::from_array(normal);
    let Some([x, y, z, hand]) = tangent else {
        return n.extend(0.);
    };
    let t = Vec3::new(x, y, z);
    if n == Vec3::ZERO || t == Vec3::ZERO {
        return n.extend(0.);
    }
    let basis = tangent_reference(n);
    let angle = t.dot(n.cross(basis)).atan2(t.dot(basis));
    n.extend(hand * (1.5 + angle / std::f32::consts::TAU))
}

fn raw_vertex(position: Vec3, normal: Vec3, uv: Vec2, color: [f32; 4]) -> Vertex {
    let mut vertex = Vertex::new2(
        position,
        uv,
        Color::new(color[0], color[1], color[2], color[3]),
    );
    vertex.normal = normal.extend(0.);
    vertex
}

fn plane_mesh() -> Mesh {
    Mesh {
        vertices: [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
            .into_iter()
            .map(|(x, z)| {
                raw_vertex(
                    vec3(x, 0., z),
                    Vec3::Y,
                    vec2(x + 0.5, z + 0.5),
                    [0.18, 0.22, 0.30, 1.],
                )
            })
            .collect(),
        indices: vec![0, 2, 1, 0, 3, 2],
        texture: None,
    }
}

pub(crate) fn cube_mesh() -> Mesh {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for normal in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        let reference = if normal.y == 0. { Vec3::Y } else { Vec3::Z };
        let horizontal = reference.cross(normal);
        let vertical = normal.cross(horizontal);
        let start = vertices.len() as u16;
        for (x, y) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
            vertices.push(raw_vertex(
                (normal + x * horizontal + y * vertical) * 0.5,
                normal,
                Vec2::ZERO,
                [0.47, 0.59, 1., 1.],
            ));
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|index| start + index));
    }
    Mesh {
        vertices,
        indices,
        texture: None,
    }
}

pub(crate) fn sphere_mesh() -> Mesh {
    const RINGS: u16 = 24;
    const SLICES: u16 = 32;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for ring in 0..=RINGS {
        let latitude = std::f32::consts::PI * f32::from(ring) / f32::from(RINGS);
        for slice in 0..=SLICES {
            let longitude = std::f32::consts::TAU * f32::from(slice) / f32::from(SLICES);
            let normal = vec3(
                latitude.sin() * longitude.cos(),
                latitude.cos(),
                latitude.sin() * longitude.sin(),
            );
            vertices.push(raw_vertex(
                normal * 0.5,
                normal,
                Vec2::ZERO,
                [0.24, 0.9, 0.74, 1.],
            ));
            if ring < RINGS && slice < SLICES {
                let top = ring * (SLICES + 1) + slice;
                let bottom = top + SLICES + 1;
                indices.extend([top, top + 1, bottom, bottom, top + 1, bottom + 1]);
            }
        }
    }
    Mesh {
        vertices,
        indices,
        texture: None,
    }
}

fn texture(bytes: Option<&Vec<u8>>) -> Option<Texture2D> {
    bytes
        .and_then(|bytes| Image::from_file_with_format(bytes, Some(ImageFormat::Png)).ok())
        .map(|image| Texture2D::from_image(&image))
}

fn prepare_part(part: &Part) -> PreparedPart {
    let base = texture(part.texture.as_ref());
    let batches = part
        .indices
        .chunks(6_000)
        .map(|chunk| {
            let mut vertices = Vec::new();
            let mut indices = Vec::with_capacity(chunk.len());
            let mut remap = HashMap::new();
            let mut source = Vec::new();
            for &id in chunk {
                indices.push(*remap.entry(id).or_insert_with(|| {
                    let vertex = part.vertices[id as usize];
                    let index = vertices.len() as u16;
                    source.push(id as usize);
                    let mut gpu_vertex = raw_vertex(
                        Vec3::from_array(vertex.position),
                        Vec3::from_array(vertex.normal),
                        Vec2::from_array(vertex.uv),
                        vertex.color,
                    );
                    gpu_vertex.normal = gpu_normal(
                        vertex.normal,
                        part.normal_map
                            .as_ref()
                            .map(|map| map.tangents[id as usize]),
                    );
                    vertices.push(gpu_vertex);
                    index
                }));
            }
            Batch {
                mesh: Mesh {
                    vertices,
                    indices,
                    texture: base.clone(),
                },
                source,
            }
        })
        .collect();
    PreparedPart {
        batches,
        metal: texture(part.metallic_roughness.as_ref()),
        emissive: texture(part.emissive_texture.as_ref()),
        normal: part
            .normal_map
            .as_ref()
            .and_then(|map| texture(Some(&map.texture))),
    }
}

pub fn update(scene: &mut Scene, movement: Vec2, dt: f32) {
    let dt = dt.clamp(0., 0.1);
    for object in &mut scene.objects {
        if !object.is_dynamic() || object.spin != 0. {
            object.rotation[1] = (object.rotation[1] + object.spin * dt).rem_euclid(360.);
        }
        if !object.is_dynamic() && !object.is_character() {
            object.position[0] += movement.x * object.speed * dt;
            object.position[2] += movement.y * object.speed * dt;
        }
    }
}

pub fn movement_input() -> Vec2 {
    Bindings::default().movement()
}

pub struct Playback {
    scene: Scene,
    scripts: Scripts,
    relays: crate::relay::Relays,
    script_errors: Vec<String>,
    physics: physics::World,
    physics2d: crate::canvas2d::physics::World,
    accumulated: f64,
    pending_controls: InputControls,
    pub(crate) actions: GameActions,
    events: Vec<Event>,
    dropped_events: usize,
    animation_events: Vec<animation::Event>,
}

impl Playback {
    pub fn new(mut scene: Scene) -> Result<Self, String> {
        scene.assign_ids()?;
        scene.validate_objects()?;
        for object in &mut scene.objects {
            object.animation_blend_state = None;
        }
        let relays = crate::relay::Relays::from_scene(&scene)?;
        let mut scripts = Scripts::compile(&scene)?;
        let mut script_errors = Vec::new();
        scripts.start(&mut scene, &mut script_errors);
        let physics = physics::World::new(std::cell::RefCell::new(if scene.canvas2d.is_some() {
            None
        } else {
            Physics::new(&scene)?
        }));
        scripts.bind_physics(physics.clone());
        let initial_input = input_map(
            GameActions::default(),
            &InputControls::default(),
            &scene.camera,
        );
        scripts.camera(&mut scene, &initial_input, 0., &mut script_errors);
        Ok(Self {
            scene,
            scripts,
            relays,
            script_errors,
            physics,
            physics2d: Default::default(),
            accumulated: 0.,
            pending_controls: InputControls::default(),
            actions: GameActions::default(),
            events: Vec::new(),
            dropped_events: 0,
            animation_events: Vec::new(),
        })
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub(crate) fn commit_objects(&mut self, mut next: Scene) -> Result<(), String> {
        next.validate_objects()?;
        let mut relays = crate::relay::Relays::from_scene(&next)?;
        relays.preserve_charge(&self.relays);
        // Commands preserve the order of surviving objects and append new IDs.
        let first = next
            .objects
            .iter()
            .position(|o| o.id >= self.scene.next_id)
            .unwrap_or(next.objects.len());
        let added = if first < next.objects.len() {
            let mut scripts = Scripts::compile(&next)?;
            scripts.bind_physics(self.physics.clone());
            scripts.start_added(first, &mut next, &mut self.script_errors);
            Some(scripts)
        } else {
            None
        };
        let mut physics = self.physics.borrow_mut();
        if next.canvas2d.is_some() || !next.objects.iter().any(|o| o.body.is_some()) {
            *physics = None;
        } else if let Some(world) = physics.as_mut() {
            world.sync(&next)?;
        } else {
            *physics = Physics::new(&next)?;
        }
        self.scripts.retain_existing(&self.scene, &next);
        if let Some(scripts) = added {
            self.scripts.merge(scripts);
        }
        self.relays = relays;
        self.scene = next;
        Ok(())
    }

    pub fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        distance: f32,
        ignore: Option<u64>,
    ) -> Result<Option<physics::RayHit>, String> {
        Physics::raycast(&self.physics, origin, direction, distance, ignore)
    }

    pub fn tick(&mut self, movement: Vec2, dt: f32) {
        self.tick_input(movement, &InputControls::default(), dt)
    }

    pub fn tick_input(&mut self, movement: Vec2, controls: &InputControls, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0., 0.1)
        } else {
            0.
        };
        self.animation_events.clear();
        let movement = (movement + self.scene.bindings.move_axis(controls)).clamp_length_max(1.);
        self.actions = GameActions {
            movement,
            ..Default::default()
        };
        self.pending_controls.accumulate(controls);
        if self.scene.canvas2d.is_none() && self.physics.borrow().is_none() {
            self.run_step(movement, dt);
            return;
        }
        self.accumulated += f64::from(dt);
        let step = f64::from(physics::STEP);
        while self.accumulated + 1e-8 >= step {
            self.accumulated = (self.accumulated - step).max(0.);
            self.run_step(movement, physics::STEP);
        }
    }

    fn run_step(&mut self, movement: Vec2, dt: f32) {
        for object in &mut self.scene.objects {
            if object.animation_blend > 0.
                && object.animation_blend_state.is_none()
                && matches!(object.shape, Shape::Model(_))
            {
                object.animation_blend_state =
                    Some(animation::BlendState::new(object.animation.as_ref()));
            }
        }
        let actions = GameActions {
            movement,
            jump: self
                .scene
                .bindings
                .pressed(Action::Jump, &self.pending_controls),
            interact: self
                .scene
                .bindings
                .pressed(Action::Interact, &self.pending_controls),
        };
        self.actions.jump |= actions.jump;
        self.actions.interact |= actions.interact;
        let input = input_map(actions, &self.pending_controls, &self.scene.camera);
        self.pending_controls.clear_edges();
        if self.scene.canvas2d.is_none() {
            update(&mut self.scene, movement, dt);
        }
        self.scripts
            .step(&mut self.scene, &input, dt, &mut self.script_errors);
        let events = if self.scene.canvas2d.is_some() {
            Some(self.physics2d.step(&mut self.scene, actions, dt))
        } else {
            self.physics
                .borrow_mut()
                .as_mut()
                .map(|physics| physics.step(&mut self.scene, movement, actions.jump))
        };
        if let Some(events) = events {
            self.scripts
                .extend_effects(self.relays.events(&events, &self.scene));
            self.scripts
                .events(&mut self.scene, &events, &mut self.script_errors);
            let preserved = events.len().min(4096 - self.events.len());
            self.dropped_events += events.len() - preserved;
            self.events.extend(events.into_iter().take(preserved));
        }
        let first_event = self.animation_events.len();
        {
            let world = self.physics.borrow();
            for (index, object) in self.scene.objects.iter_mut().enumerate() {
                let Shape::Model(model) = object.shape else {
                    continue;
                };
                let model = &self.scene.models[model];
                let Some(rig) = model.rig.as_deref() else {
                    continue;
                };
                let motion = !object.animate_grounded_motion
                    || world
                        .as_ref()
                        .is_some_and(|world| world.moving_on_ground(index));
                if let Some(blend) = &mut object.animation_blend_state {
                    blend.select(rig, object.animation.as_ref(), object.animation_blend);
                }
                if let Some(player) = object.animation.as_mut().filter(|_| motion) {
                    if let Some(clip) = model.animation_clip(&player.clip) {
                        let markers = object
                            .animation_markers
                            .get(&clip.name)
                            .map_or(clip.markers.as_slice(), Vec::as_slice);
                        animation::advance_markers(
                            clip,
                            player,
                            dt,
                            markers,
                            animation::MAX_EVENTS - self.animation_events.len(),
                            |marker| {
                                self.animation_events.push(animation::Event {
                                    object_id: object.id,
                                    clip: clip.name.clone(),
                                    name: marker.name.clone(),
                                    time: marker.time,
                                });
                            },
                        );
                    }
                }
                if let Some(blend) = &mut object.animation_blend_state {
                    blend.advance(model, object.animation.as_ref(), dt, motion);
                }
            }
        }
        self.scripts.animation_events(
            &mut self.scene,
            &self.animation_events[first_event..],
            &mut self.script_errors,
        );
        self.scripts
            .camera(&mut self.scene, &input, dt, &mut self.script_errors);
    }

    pub(crate) fn has_scene_request(&self) -> bool {
        self.scripts.has_scene_request()
    }

    pub fn ui_action(&mut self, button: &str) -> Result<(), String> {
        if !self.scene.presentation.has_button(button) {
            return Err(format!("Unknown UI button '{button}'."));
        }
        if let Some(action) = self.scene.presentation.button_action(button) {
            self.pending_controls.actions.insert(action);
        }
        self.scripts
            .ui(&mut self.scene, button, &mut self.script_errors);
        Ok(())
    }

    pub fn take_animation_events(&mut self) -> Vec<animation::Event> {
        std::mem::take(&mut self.animation_events)
    }

    pub(crate) fn extend_effects(&mut self, effects: Vec<Effect>) {
        self.scripts.extend_effects(effects);
    }

    pub(crate) fn take_effects(&mut self) -> Vec<Effect> {
        self.scripts.take_effects()
    }

    pub fn take_scene_request(&mut self) -> Option<String> {
        self.scripts.take_scene_request()
    }

    pub fn take_events(&mut self) -> (Vec<Event>, usize) {
        (
            std::mem::take(&mut self.events),
            std::mem::take(&mut self.dropped_events),
        )
    }

    pub fn take_script_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.script_errors)
    }
}

pub async fn run(scene: Scene, window_smoke: bool) {
    run_game(crate::game::Game::single(scene), window_smoke).await;
}

pub async fn run_game(game: crate::game::Game, window_smoke: bool) {
    run_measured(game, window_smoke, None).await;
}

pub async fn run_measured(
    game: crate::game::Game,
    window_smoke: bool,
    benchmark: Option<crate::performance::Benchmark>,
) {
    run_measured_with_systems(
        game,
        window_smoke,
        benchmark,
        crate::workshop::game_systems(),
        false,
    )
    .await;
}

pub async fn run_measured_with_systems(
    game: crate::game::Game,
    window_smoke: bool,
    mut benchmark: Option<crate::performance::Benchmark>,
    systems: crate::gameplay::Systems,
    preview: bool,
) {
    let automated = window_smoke || benchmark.is_some();
    let start = game.start.clone();
    let save_path = (!automated && !preview)
        .then(|| {
            game.start_scene()
                .ok()
                .and_then(|scene| progress::path(&scene.application.identifier))
        })
        .flatten();
    let progress = save_path.as_deref().map_or_else(Progress::default, |path| {
        Progress::load(path).unwrap_or_else(|error| {
            macroquad::logging::warn!("Could not load progress: {error}");
            Progress::default()
        })
    });
    let mut playback = match crate::game::GamePlayback::with_systems(game, progress, systems) {
        Ok(playback) => playback,
        Err(error) => {
            macroquad::logging::error!("{}", error);
            if automated {
                std::process::exit(1);
            }
            loop {
                clear_background(Color::from_rgba(22, 28, 42, 255));
                draw_text(
                    "Could not start the scene. See the game log.",
                    18.,
                    36.,
                    24.,
                    WHITE,
                );
                next_frame().await;
            }
        }
    };
    let mut prepared = PreparedScene::new(playback.scene());
    let mut pads = Gamepads::default();
    let mut overlay = Overlay::new(&playback.scene().presentation);
    let mut frames = 0;
    loop {
        let frame_started = benchmark.as_ref().map(|_| std::time::Instant::now());
        let viewport = Rect::new(0., 0., screen_width(), screen_height());
        let show_controls = !playback.scene().presentation.hide_movement
            && playback.scene().objects.iter().any(|object| {
                object.speed != 0. || object.script.is_some() || object.script_asset.is_some()
            });
        let input = if automated {
            InputControls::default()
        } else {
            pads.capture(Vec2::ZERO)
        };
        let action = (!automated)
            .then(|| {
                Overlay::action_with_pad(
                    &playback.scene().presentation,
                    viewport,
                    input.pad_pressed.contains(&PadButton::South),
                )
                .map(str::to_owned)
            })
            .flatten();
        if let Some(button) = action.as_deref() {
            if let Err(error) = playback.ui_action(button) {
                macroquad::logging::error!("{error}");
            }
        }
        let movement = Vec2::ZERO;
        #[cfg(target_os = "android")]
        let mut movement = movement;
        let controls = touch_controls();
        #[cfg(target_os = "android")]
        if !automated && show_controls {
            for touch in touches() {
                if matches!(
                    touch.phase,
                    TouchPhase::Started | TouchPhase::Stationary | TouchPhase::Moved
                ) {
                    for (rect, _, direction) in controls {
                        if rect.contains(touch.position) {
                            movement += direction;
                        }
                    }
                }
            }
        }
        let before = playback.current().to_owned();
        let switched = playback.tick_input(movement.clamp_length_max(1.), &input, get_frame_time());
        let sounds = playback.take_sounds();
        overlay
            .play(
                &before,
                playback.presentation(&before),
                sounds,
                playback.progress().volume,
            )
            .await;
        if switched {
            prepared = PreparedScene::new(playback.scene());
            overlay.set_scene(&playback.scene().presentation);
        }
        if playback.take_progress_changed() {
            let result = save_path
                .as_deref()
                .map_or(Ok(()), |path| playback.progress().save(path));
            if let Err(error) = result {
                macroquad::logging::warn!("Could not save progress: {error}");
            }
        }
        for error in playback.take_script_errors() {
            macroquad::logging::error!("{}", error);
            if automated {
                std::process::exit(1);
            }
        }
        playback.take_events();
        clear_background(Color::from_rgba(22, 28, 42, 255));
        prepared.draw(
            playback.scene(),
            &playback.scene().camera.camera(),
            false,
            None,
        );
        if cfg!(target_os = "android") && show_controls && !switched {
            for (rect, label, _) in controls {
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Color::new(0.18, 0.26, 0.42, 0.8),
                );
                draw_text(
                    label,
                    rect.x + rect.w * 0.34,
                    rect.y + rect.h * 0.68,
                    rect.h * 0.55,
                    WHITE,
                );
            }
        }
        overlay.draw(
            &playback.scene().presentation,
            viewport,
            playback.progress(),
        );
        let submit = frame_started.map(|at| at.elapsed());
        next_frame().await;
        if let (Some(measure), Some(at), Some(submit)) = (&mut benchmark, frame_started, submit) {
            if measure.frame(submit, at.elapsed()) {
                println!(
                    "{}",
                    measure.report(&start, [screen_width() as u32, screen_height() as u32])
                );
                macroquad::miniquad::window::quit();
                return;
            }
        }
        if window_smoke {
            frames += 1;
            if frames == 8 {
                println!(
                    "Forge3D rendered {frames} frames at {}×{}.",
                    screen_width(),
                    screen_height()
                );
                macroquad::miniquad::window::quit();
                return;
            }
        }
    }
}

fn touch_controls() -> [(Rect, &'static str, Vec2); 4] {
    let size = (screen_width().min(screen_height()) * 0.12).max(20.);
    let bottom = screen_height() - size - 12.;
    let x = 12.;
    [
        (
            Rect::new(x + size, bottom - size, size, size),
            "^",
            -Vec2::Y,
        ),
        (Rect::new(x, bottom, size, size), "<", -Vec2::X),
        (Rect::new(x + size, bottom, size, size), "v", Vec2::Y),
        (Rect::new(x + 2. * size, bottom, size, size), ">", Vec2::X),
    ]
}
