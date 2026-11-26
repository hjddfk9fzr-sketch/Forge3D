use crate::runtime::Scene;
use macroquad::prelude::{EulerRot, Mat4, Quat, Vec2, Vec3};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STEP: f32 = 1. / 60.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    Static,
    Dynamic,
    Kinematic,
    Character,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColliderKind {
    Box,
    Sphere,
    Capsule,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Body {
    pub kind: BodyKind,
    pub collider: ColliderKind,
    /// Full box dimensions, or the sphere's diameter in the X field.
    pub size: [f32; 3],
    pub sensor: bool,
    pub gravity_scale: f32,
    pub friction: f32,
    pub restitution: f32,
    pub lock_rotation: bool,
    pub max_slope_degrees: f32,
    pub jump_height: f32,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            kind: BodyKind::Static,
            collider: ColliderKind::Box,
            size: [1.; 3],
            sensor: false,
            gravity_scale: 1.,
            friction: 0.5,
            restitution: 0.,
            lock_rotation: false,
            max_slope_degrees: 45.,
            jump_height: 1.0,
        }
    }
}

impl Body {
    pub fn character() -> Self {
        Self {
            kind: BodyKind::Character,
            collider: ColliderKind::Capsule,
            size: [0.6, 1., 0.6],
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !self
            .size
            .into_iter()
            .all(|value| (0.001..=10_000.).contains(&value))
        {
            return Err("Collider dimensions must be between 0.001 and 10000.");
        }
        if self.kind == BodyKind::Character
            && (self.collider != ColliderKind::Capsule || self.sensor || self.gravity_scale < 0.)
        {
            return Err("Character bodies require a solid capsule and nonnegative gravity scale.");
        }
        if !(0. ..=89.).contains(&self.max_slope_degrees)
            || !(0. ..=100.).contains(&self.jump_height)
        {
            return Err("Use a slope angle of 0..89 degrees and jump height of 0..100.");
        }
        if !(-10. ..=10.).contains(&self.gravity_scale) {
            return Err("Gravity scale must be between -10 and 10.");
        }
        if !(0. ..=2.).contains(&self.friction) || !(0. ..=1.).contains(&self.restitution) {
            return Err("Friction must be in 0..2 and restitution in 0..1.");
        }
        Ok(())
    }
}

pub fn default_gravity() -> [f32; 3] {
    [0., -9.81, 0.]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    CollisionEnter,
    CollisionExit,
    TriggerEnter,
    TriggerExit,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CollisionEnter => "collision_enter",
            Self::CollisionExit => "collision_exit",
            Self::TriggerEnter => "trigger_enter",
            Self::TriggerExit => "trigger_exit",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    pub a: u64,
    pub b: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RayHit {
    pub object_id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
}

pub(crate) type World = std::rc::Rc<std::cell::RefCell<Option<Physics>>>;

pub(crate) struct Physics {
    queries: QueryPipeline,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad: BroadPhaseMultiSap,
    narrow: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulses: ImpulseJointSet,
    multibodies: MultibodyJointSet,
    ccd: CCDSolver,
    entries: Vec<Entry>,
    contacts: BTreeSet<(u64, u64, bool)>,
}

struct Entry {
    index: usize,
    body: RigidBodyHandle,
    collider: ColliderHandle,
    pose: Pose,
    vertical_speed: f32,
    grounded: bool,
    moved_on_ground: bool,
}

#[derive(Clone, Copy)]
struct Pose {
    center: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl Pose {
    fn isometry(self) -> Isometry<Real> {
        use rapier3d::na::{Quaternion, Translation3, UnitQuaternion};
        let [x, y, z, w] = self.rotation.to_array();
        Isometry::from_parts(
            Translation3::from(self.center.to_array()),
            UnitQuaternion::new_normalize(Quaternion::new(w, x, y, z)),
        )
    }

    fn valid(self) -> bool {
        self.center.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

fn poses(scene: &Scene) -> Vec<Pose> {
    let mut matrices = vec![(Mat4::IDENTITY, Quat::IDENTITY); scene.objects.len()];
    let mut poses = vec![
        Pose {
            center: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE
        };
        scene.objects.len()
    ];
    for (index, _, parent) in scene.object_order() {
        let object = &scene.objects[index];
        let (matrix, rotation) =
            parent.map_or((Mat4::IDENTITY, Quat::IDENTITY), |parent| matrices[parent]);
        let [x, y, z] = object.rotation.map(f32::to_radians);
        let rotation = (rotation * Quat::from_euler(EulerRot::XYZ, x, y, z)).normalize();
        let matrix = matrix * object.matrix();
        matrices[index] = (matrix, rotation);
        poses[index] = Pose {
            center: matrix.transform_point3(Vec3::ZERO),
            rotation,
            scale: Vec3::new(
                matrix.x_axis.truncate().length(),
                matrix.y_axis.truncate().length(),
                matrix.z_axis.truncate().length(),
            ),
        };
    }
    poses
}

fn capsule(config: &Body, scale: Vec3) -> (f32, f32) {
    let radius = (0.5 * config.size[0] * scale.x.max(scale.z)).clamp(0.0005, 10_000.);
    let half_segment = (0.5 * config.size[1] * scale.y - radius).clamp(0., 10_000.);
    (half_segment, radius)
}

fn shape(config: &Body, scale: Vec3) -> SharedShape {
    let size = Vec3::from_array(config.size) * scale;
    match config.collider {
        ColliderKind::Box => {
            let half = (size * 0.5).clamp(Vec3::splat(0.0005), Vec3::splat(10_000.));
            SharedShape::cuboid(half.x, half.y, half.z)
        }
        ColliderKind::Capsule => {
            let (half, radius) = capsule(config, scale);
            SharedShape::capsule_y(half, radius)
        }
        ColliderKind::Sphere => {
            let radius = (config.size[0] * scale.max_element() * 0.5).clamp(0.0005, 10_000.);
            SharedShape::ball(radius)
        }
    }
}

impl Physics {
    pub(crate) fn new(scene: &Scene) -> Result<Option<Self>, String> {
        if !scene.objects.iter().any(|object| object.body.is_some()) {
            return Ok(None);
        }
        let mut world = Self {
            queries: QueryPipeline::new(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad: BroadPhaseMultiSap::new(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulses: ImpulseJointSet::new(),
            multibodies: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            entries: Vec::new(),
            contacts: BTreeSet::new(),
        };
        world.sync(scene)?;
        Ok(Some(world))
    }

    pub(crate) fn moving_on_ground(&self, index: usize) -> bool {
        self.entries
            .iter()
            .any(|e| e.index == index && e.moved_on_ground)
    }

    pub(crate) fn sync(&mut self, scene: &Scene) -> Result<(), String> {
        let poses = poses(scene);
        let mut indices = std::collections::HashMap::new();
        for (index, object) in scene
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.body.is_some())
        {
            if !poses[index].valid() {
                return Err(format!(
                    "Invalid world transform for collider on {}.",
                    object.name
                ));
            }
            indices.insert(object.id, index);
        }
        self.entries.retain_mut(|entry| {
            let id = self.bodies[entry.body].user_data as u64;
            if let Some(index) = indices.remove(&id) {
                entry.index = index;
                return true;
            }
            self.bodies.remove(
                entry.body,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulses,
                &mut self.multibodies,
                true,
            );
            false
        });
        for (index, pose) in poses.into_iter().enumerate() {
            let object = &scene.objects[index];
            let Some(body) = &object.body else { continue };
            if !indices.contains_key(&object.id) {
                continue;
            }
            let builder = match body.kind {
                BodyKind::Static => RigidBodyBuilder::fixed(),
                BodyKind::Dynamic => RigidBodyBuilder::dynamic().ccd_enabled(true),
                BodyKind::Kinematic | BodyKind::Character => {
                    RigidBodyBuilder::kinematic_position_based()
                }
            }
            .user_data(u128::from(object.id))
            .position(pose.isometry())
            .gravity_scale(body.gravity_scale);
            let builder = if body.lock_rotation {
                builder.lock_rotations()
            } else {
                builder
            };
            let rigid_body = self.bodies.insert(builder.build());
            let collider = self.colliders.insert_with_parent(
                ColliderBuilder::new(shape(body, pose.scale))
                    .sensor(body.sensor)
                    .active_collision_types(ActiveCollisionTypes::all())
                    .friction(body.friction)
                    .restitution(body.restitution)
                    .user_data(u128::from(object.id))
                    .build(),
                rigid_body,
                &mut self.bodies,
            );
            self.entries.push(Entry {
                index,
                body: rigid_body,
                collider,
                pose,
                vertical_speed: 0.,
                grounded: false,
                moved_on_ground: false,
            });
        }
        self.queries.update(&self.colliders);
        Ok(())
    }

    pub(crate) fn raycast(
        world: &World,
        origin: [f32; 3],
        direction: [f32; 3],
        distance: f32,
        ignore: Option<u64>,
    ) -> Result<Option<RayHit>, String> {
        let direction = Vec3::from_array(direction);
        let scale = direction.abs().max_element();
        if !origin.iter().all(|v| v.is_finite())
            || !direction.is_finite()
            || scale == 0.
            || !distance.is_finite()
            || distance <= 0.
        {
            return Err(
                "Ray needs a finite origin, nonzero direction and positive finite range.".into(),
            );
        }
        let world = world.borrow();
        let Some(world) = world.as_ref() else {
            return Ok(None);
        };
        let ray = Ray::new(
            origin.into(),
            ((direction / scale).normalize().to_array()).into(),
        );
        let id = |collider: &Collider| {
            world.bodies[collider.parent().expect("colliders have bodies")].user_data as u64
        };
        let accept = |_, collider: &Collider| Some(id(collider)) != ignore;
        let filter = QueryFilter::default().exclude_sensors().predicate(&accept);
        Ok(world
            .queries
            .cast_ray_and_get_normal(
                &world.bodies,
                &world.colliders,
                &ray,
                distance,
                true,
                filter,
            )
            .map(|(handle, hit)| RayHit {
                object_id: id(&world.colliders[handle]),
                distance: hit.time_of_impact,
                point: ray.point_at(hit.time_of_impact).coords.into(),
                normal: hit.normal.into(),
            }))
    }

    pub(crate) fn step(&mut self, scene: &mut Scene, input: Vec2, jump: bool) -> Vec<Event> {
        let poses = poses(scene);
        let mut refresh_queries = false;
        for entry in &mut self.entries {
            let object = &scene.objects[entry.index];
            let config = object
                .body
                .as_ref()
                .expect("body configuration is fixed during play");
            let mut pose = poses[entry.index];
            if !pose.valid() {
                continue;
            }
            if (pose.scale - entry.pose.scale).abs().max_element() > 0.00001 {
                refresh_queries = true;
                self.colliders[entry.collider].set_shape(shape(config, pose.scale));
            } else {
                pose.scale = entry.pose.scale;
            }
            let body = &mut self.bodies[entry.body];
            let changed = pose.center != entry.pose.center
                || pose.rotation.dot(entry.pose.rotation).abs() < 0.9999999;
            match config.kind {
                BodyKind::Static if changed => {
                    body.set_position(pose.isometry(), true);
                    refresh_queries = true;
                }
                BodyKind::Kinematic => body.set_next_kinematic_position(pose.isometry()),
                BodyKind::Dynamic => {
                    if changed {
                        body.set_position(pose.isometry(), true);
                    }
                    if object.speed != 0. {
                        let target = input * object.speed;
                        let velocity = vector![target.x, body.linvel().y, target.y];
                        if velocity != *body.linvel() {
                            body.set_linvel(velocity, true);
                        }
                    }
                }
                _ => {}
            }
            entry.pose = pose;
        }
        let [x, y, z] = scene.gravity;
        if refresh_queries
            && self
                .entries
                .iter()
                .any(|e| scene.objects[e.index].is_character())
        {
            self.bodies
                .propagate_modified_body_positions_to_colliders(&mut self.colliders);
            self.queries.update(&self.colliders);
        }
        for entry in &mut self.entries {
            let object = &scene.objects[entry.index];
            if !object.is_character() {
                continue;
            }
            let config = object.body.as_ref().unwrap();
            let gravity = y.min(0.) * config.gravity_scale;
            if jump && entry.grounded {
                entry.vertical_speed = (-2. * gravity * config.jump_height).sqrt();
            }
            entry.vertical_speed += gravity * STEP;
            let axes = input.clamp_length_max(1.) * object.speed;
            let angle = config.max_slope_degrees.to_radians();
            let controller = rapier3d::control::KinematicCharacterController {
                max_slope_climb_angle: angle,
                min_slope_slide_angle: angle,
                ..Default::default()
            };
            let origin = entry.pose.isometry();
            let motion = controller.move_shape(
                STEP,
                &self.bodies,
                &self.colliders,
                &self.queries,
                self.colliders[entry.collider].shape(),
                &origin,
                vector![axes.x, entry.vertical_speed, axes.y] * STEP,
                QueryFilter::default()
                    .exclude_rigid_body(entry.body)
                    .exclude_sensors(),
                |_| {},
            );
            entry.grounded = motion.grounded && entry.vertical_speed <= 0.;
            entry.moved_on_ground =
                entry.grounded && motion.translation.x.hypot(motion.translation.z) > 0.0001;
            if entry.grounded
                || (entry.vertical_speed > 0.
                    && motion.translation.y < entry.vertical_speed * STEP - 0.001)
            {
                entry.vertical_speed = 0.;
            }
            let mut destination = origin;
            destination.translation.vector += motion.translation;
            self.bodies[entry.body].set_next_kinematic_position(destination);
        }
        self.pipeline.step(
            &vector![x, y, z],
            &IntegrationParameters {
                dt: STEP,
                ..Default::default()
            },
            &mut self.islands,
            &mut self.broad,
            &mut self.narrow,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulses,
            &mut self.multibodies,
            &mut self.ccd,
            Some(&mut self.queries),
            &(),
            &(),
        );
        for entry in &mut self.entries {
            let object = &mut scene.objects[entry.index];
            if !object.is_dynamic() && !object.is_character() {
                continue;
            }
            let body = &self.bodies[entry.body];
            let p = body.translation();
            let q = body.rotation().quaternion();
            let rotation = Quat::from_xyzw(q.i, q.j, q.k, q.w).normalize();
            let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
            object.position = [p.x, p.y, p.z];
            object.rotation = [x, y, z].map(f32::to_degrees);
            entry.pose.center = Vec3::from_array(object.position);
            entry.pose.rotation = rotation;
        }
        self.events()
    }

    fn events(&mut self) -> Vec<Event> {
        let pair = |a: ColliderHandle, b: ColliderHandle, sensor| {
            let a = self.colliders[a].user_data as u64;
            let b = self.colliders[b].user_data as u64;
            (a.min(b), a.max(b), sensor)
        };
        let contacts: BTreeSet<_> = self
            .narrow
            .contact_pairs()
            .filter(|pair| pair.has_any_active_contact)
            .map(|contact| pair(contact.collider1, contact.collider2, false))
            .chain(
                self.narrow
                    .intersection_pairs()
                    .filter(|(_, _, intersection)| *intersection)
                    .map(|(a, b, _)| pair(a, b, true)),
            )
            .collect();
        let event = |&(a, b, sensor): &(u64, u64, bool), entered| Event {
            kind: match (sensor, entered) {
                (false, true) => EventKind::CollisionEnter,
                (false, false) => EventKind::CollisionExit,
                (true, true) => EventKind::TriggerEnter,
                (true, false) => EventKind::TriggerExit,
            },
            a,
            b,
        };
        let events = contacts
            .difference(&self.contacts)
            .map(|pair| event(pair, true))
            .chain(
                self.contacts
                    .difference(&contacts)
                    .map(|pair| event(pair, false)),
            )
            .collect();
        self.contacts = contacts;
        events
    }
}

pub fn draw_gizmos(scene: &Scene, camera: &macroquad::prelude::Camera3D) {
    use macroquad::prelude::*;
    set_camera(camera);
    for (index, pose) in poses(scene).into_iter().enumerate() {
        let Some(config) = &scene.objects[index].body else {
            continue;
        };
        if !pose.valid() {
            continue;
        }
        let color = if config.sensor {
            Color::from_rgba(255, 198, 93, 200)
        } else {
            Color::from_rgba(110, 228, 179, 200)
        };
        unsafe {
            get_internal_gl()
                .quad_gl
                .push_model_matrix(Mat4::from_rotation_translation(pose.rotation, pose.center));
        }
        let size = Vec3::from_array(config.size) * pose.scale;
        match config.collider {
            ColliderKind::Capsule => {
                let (half, radius) = capsule(config, pose.scale);
                for sign in [-1., 1.] {
                    let center = Vec3::Y * half * sign;
                    for i in 0..24 {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        let next = (i + 1) as f32 * std::f32::consts::TAU / 24.;
                        let radial = vec3(angle.cos(), 0., angle.sin()) * radius;
                        draw_line_3d(
                            center + radial,
                            center + vec3(next.cos(), 0., next.sin()) * radius,
                            color,
                        );
                        if i % 6 == 0 {
                            let mut previous = center + radial;
                            for j in 1..=8 {
                                let a = j as f32 * std::f32::consts::FRAC_PI_2 / 8.;
                                let end =
                                    center + radial * a.cos() + Vec3::Y * radius * sign * a.sin();
                                draw_line_3d(previous, end, color);
                                previous = end;
                            }
                            if sign > 0. {
                                draw_line_3d(center + radial, -center + radial, color);
                            }
                        }
                    }
                }
            }
            ColliderKind::Box => draw_cube_wires(Vec3::ZERO, size * 1.002, color),
            ColliderKind::Sphere => draw_sphere_wires(
                Vec3::ZERO,
                config.size[0] * pose.scale.max_element() * 0.501,
                None,
                color,
            ),
        }
        unsafe {
            get_internal_gl().quad_gl.pop_model_matrix();
        }
    }
    set_default_camera();
}
