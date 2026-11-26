use crate::runtime::{Model, ModelVertex, Part};
use macroquad::prelude::{Mat4, Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Player {
    pub clip: String,
    pub time: f32,
    pub speed: f32,
    pub playing: bool,
    pub looping: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            clip: String::new(),
            time: 0.,
            speed: 1.,
            playing: true,
            looping: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rig {
    pub nodes: Vec<Node>,
    pub skins: Vec<Skin>,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub parent: Option<usize>,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<[[f32; 4]; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub tracks: Vec<Track>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

pub const MAX_MARKERS: usize = 64;
pub const MAX_BLEND_SECONDS: f32 = 5.;
pub(crate) const MAX_EVENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub time: f32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub object_id: u64,
    pub clip: String,
    pub name: String,
    pub time: f32,
}

pub(crate) fn valid_markers(markers: &[Marker], duration: f32) -> bool {
    markers.len() <= MAX_MARKERS
        && markers.iter().all(|marker| {
            duration > 0.
                && (0. ..=duration).contains(&marker.time)
                && !marker.name.trim().is_empty()
                && marker.name.len() <= 80
        })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    pub values: Vec<[f32; 4]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Interpolation {
    Step,
    Linear,
    Cubic,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Influence {
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

impl Rig {
    fn order(&self) -> Vec<usize> {
        let mut children = vec![Vec::new(); self.nodes.len()];
        let mut pending = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            match node.parent {
                Some(parent) if parent < children.len() => children[parent].push(index),
                Some(_) => {}
                None => pending.push(index),
            }
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(node) = pending.pop() {
            order.push(node);
            pending.extend(&children[node]);
        }
        order
    }

    pub(crate) fn validate(&self) -> bool {
        let finite = |values: &[f32]| values.iter().all(|value| value.is_finite());
        self.order().len() == self.nodes.len()
            && self.nodes.iter().all(|node| {
                finite(&node.position)
                    && finite(&node.scale)
                    && finite(&node.rotation)
                    && Quat::from_array(node.rotation).length_squared() > 1e-8
            })
            && self.skins.iter().all(|skin| {
                skin.joints.len() == skin.inverse_bind.len()
                    && skin.joints.iter().all(|&node| node < self.nodes.len())
                    && skin
                        .inverse_bind
                        .iter()
                        .flatten()
                        .all(|column| finite(column))
            })
            && self.clips.iter().all(|clip| {
                clip.duration.is_finite()
                    && clip.duration >= 0.
                    && valid_markers(&clip.markers, clip.duration)
                    && clip.tracks.iter().all(|track| {
                        let stride = if track.interpolation == Interpolation::Cubic {
                            3
                        } else {
                            1
                        };
                        track.node < self.nodes.len()
                            && !track.times.is_empty()
                            && finite(&track.times)
                            && track.times[0] >= 0.
                            && track.times.windows(2).all(|pair| pair[0] < pair[1])
                            && track
                                .times
                                .last()
                                .is_some_and(|&value| value <= clip.duration)
                            && track.values.len() == track.times.len().saturating_mul(stride)
                            && track.values.iter().all(|value| finite(value))
                    })
            })
    }

    fn sample(&self, player: Option<&Player>) -> Vec<Node> {
        let mut nodes = self.nodes.clone();
        if let Some((player, clip)) = player.and_then(|player| {
            self.clips
                .iter()
                .find(|clip| clip.name == player.clip)
                .map(|clip| (player, clip))
        }) {
            for track in &clip.tracks {
                let value = track.sample(player.time);
                let node = &mut nodes[track.node];
                match track.property {
                    Property::Translation => node.position = value.truncate().to_array(),
                    Property::Rotation => node.rotation = value.to_array(),
                    Property::Scale => node.scale = value.truncate().to_array(),
                }
            }
        }
        nodes
    }

    pub fn pose(&self, player: Option<&Player>) -> Pose {
        self.compose(&self.sample(player))
    }

    fn compose(&self, nodes: &[Node]) -> Pose {
        let mut global = vec![Mat4::IDENTITY; nodes.len()];
        for index in self.order() {
            let node = &nodes[index];
            let rotation = quaternion(Vec4::from_array(node.rotation));
            let local = Mat4::from_scale_rotation_translation(
                Vec3::from_array(node.scale),
                rotation,
                Vec3::from_array(node.position),
            );
            global[index] = node.parent.map_or(local, |parent| global[parent] * local);
        }
        let joints = self
            .skins
            .iter()
            .map(|skin| {
                skin.joints
                    .iter()
                    .zip(&skin.inverse_bind)
                    .map(|(&joint, inverse)| {
                        Transform::new(global[joint] * Mat4::from_cols_array_2d(inverse))
                    })
                    .collect()
            })
            .collect();
        Pose {
            global: global.into_iter().map(Transform::new).collect(),
            joints,
        }
    }
}

/// Playback-only state. The scene stores the target clip and blend duration.
#[derive(Clone, Debug, PartialEq)]
#[doc(hidden)]
pub struct BlendState {
    target: Option<Player>,
    transition: Option<Transition>,
}

#[derive(Clone, Debug, PartialEq)]
struct Transition {
    from: Source,
    duration: f32,
    elapsed: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Clip(Option<Player>),
    Pose(Vec<Node>),
}

impl BlendState {
    pub(crate) fn new(player: Option<&Player>) -> Self {
        Self {
            target: player.cloned(),
            transition: None,
        }
    }

    pub(crate) fn select(&mut self, rig: &Rig, next: Option<&Player>, seconds: f32) {
        if next.map(|p| p.clip.as_str()) != self.target.as_ref().map(|p| p.clip.as_str()) {
            let from = if self.transition.is_some() {
                Source::Pose(self.sample(rig))
            } else {
                Source::Clip(self.target.take())
            };
            self.transition = Some(Transition {
                from,
                duration: seconds,
                elapsed: 0.,
            });
        }
        self.target = next.cloned();
    }

    pub(crate) fn advance(
        &mut self,
        model: &Model,
        player: Option<&Player>,
        dt: f32,
        motion: bool,
    ) {
        self.target = player.cloned();
        let Some(transition) = &mut self.transition else {
            return;
        };
        if let Source::Clip(Some(source)) = &mut transition.from {
            advance(model, source, if motion { dt } else { 0. });
        }
        transition.elapsed += dt;
        if transition.elapsed >= transition.duration {
            self.transition = None;
        }
    }

    fn sample(&self, rig: &Rig) -> Vec<Node> {
        let mut target = rig.sample(self.target.as_ref());
        if let Some(fade) = &self.transition {
            let sampled;
            let from = match &fade.from {
                Source::Clip(player) => {
                    sampled = rig.sample(player.as_ref());
                    &sampled
                }
                Source::Pose(pose) => pose,
            };
            let weight = (fade.elapsed / fade.duration).clamp(0., 1.);
            for (node, old) in target.iter_mut().zip(from) {
                let lerp = |a: [f32; 3], b| {
                    Vec3::from_array(a)
                        .lerp(Vec3::from_array(b), weight)
                        .to_array()
                };
                node.position = lerp(old.position, node.position);
                node.scale = lerp(old.scale, node.scale);
                node.rotation = quaternion(Vec4::from_array(old.rotation))
                    .slerp(quaternion(Vec4::from_array(node.rotation)), weight)
                    .to_array();
            }
        }
        target
    }

    pub(crate) fn pose(&self, rig: &Rig) -> Pose {
        rig.compose(&self.sample(rig))
    }
}

pub struct Pose {
    global: Vec<Transform>,
    joints: Vec<Vec<Transform>>,
}

impl Pose {
    pub(crate) fn tangent(
        &self,
        part: &Part,
        index: usize,
        normal: [f32; 3],
        frame: [f32; 4],
    ) -> [f32; 4] {
        let [x, y, z, hand] = frame;
        let t = Vec3::new(x, y, z);
        let b = Vec3::from_array(part.vertices[index].normal).cross(t) * hand;
        let (t, b) = if let Some((skin, influence)) = part
            .skin
            .zip(part.influences.get(index))
            .filter(|(_, influence)| influence.weights.iter().any(|&w| w > 0.))
        {
            influence
                .joints
                .into_iter()
                .zip(influence.weights)
                .filter(|&(_, w)| w > 0.)
                .fold(
                    (Vec3::ZERO, Vec3::ZERO),
                    |(sum_t, sum_b), (joint, weight)| {
                        let transform = self.joints[skin][usize::from(joint)].matrix;
                        (
                            sum_t + transform.transform_vector3(t) * weight,
                            sum_b + transform.transform_vector3(b) * weight,
                        )
                    },
                )
        } else if let Some(node) = part.node {
            let transform = self.global[node].matrix;
            (
                transform.transform_vector3(t),
                transform.transform_vector3(b),
            )
        } else {
            (t, b)
        };
        let n = Vec3::from_array(normal);
        let t = (t - n * n.dot(t)).normalize_or_zero();
        t.extend(if n.cross(t).dot(b) < 0. { -1. } else { 1. })
            .to_array()
    }

    pub fn vertex(&self, part: &Part, index: usize) -> ModelVertex {
        let vertex = part.vertices[index];
        let position = Vec3::from_array(vertex.position);
        let normal = Vec3::from_array(vertex.normal);
        if let Some((skin, influence)) = part.skin.zip(part.influences.get(index)) {
            let mut p = Vec3::ZERO;
            let mut n = Vec3::ZERO;
            let mut weight_sum = 0.;
            for (joint, weight) in influence.joints.into_iter().zip(influence.weights) {
                if weight <= 0. {
                    continue;
                }
                let joint = self.joints[skin][usize::from(joint)];
                p += joint.matrix.transform_point3(position) * weight;
                n += joint.normals.transform_vector3(normal) * weight;
                weight_sum += weight;
            }
            if weight_sum > 0. {
                return ModelVertex {
                    position: (p / weight_sum).to_array(),
                    normal: n.normalize_or_zero().to_array(),
                    ..vertex
                };
            }
        }
        let Some(transform) = part.node.map(|node| self.global[node]) else {
            return vertex;
        };
        ModelVertex {
            position: transform.matrix.transform_point3(position).to_array(),
            normal: transform
                .normals
                .transform_vector3(normal)
                .normalize_or_zero()
                .to_array(),
            ..vertex
        }
    }
}

#[derive(Clone, Copy)]
struct Transform {
    matrix: Mat4,
    normals: Mat4,
}

impl Transform {
    fn new(matrix: Mat4) -> Self {
        let normals = if matrix.determinant().abs() < 1e-10 {
            Mat4::IDENTITY
        } else {
            matrix.inverse().transpose()
        };
        Self { matrix, normals }
    }
}

impl Track {
    fn sample(&self, time: f32) -> Vec4 {
        let high = self.times.partition_point(|&start| start <= time);
        let index = high.saturating_sub(1);
        let value = |index| Vec4::from_array(self.values[index]);
        let key = |index| {
            if self.interpolation == Interpolation::Cubic {
                value(index * 3 + 1)
            } else {
                value(index)
            }
        };
        if time <= self.times[0]
            || high >= self.times.len()
            || self.interpolation == Interpolation::Step
        {
            return key(index);
        }
        let span = self.times[high] - self.times[index];
        let t = (time - self.times[index]) / span;
        let a = key(index);
        let b = key(high);
        match self.interpolation {
            Interpolation::Linear if self.property == Property::Rotation => {
                Vec4::from_array(quaternion(a).slerp(quaternion(b), t).to_array())
            }
            Interpolation::Linear | Interpolation::Step => a.lerp(b, t),
            Interpolation::Cubic => {
                let out = value(index * 3 + 2) * span;
                let incoming = value(high * 3) * span;
                let t2 = t * t;
                let t3 = t2 * t;
                a * (2. * t3 - 3. * t2 + 1.)
                    + out * (t3 - 2. * t2 + t)
                    + b * (3. * t2 - 2. * t3)
                    + incoming * (t3 - t2)
            }
        }
    }
}

fn quaternion(value: Vec4) -> Quat {
    let q = Quat::from_array(value.to_array());
    if q.is_finite() && q.length_squared() > 1e-8 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

impl Model {
    pub fn animation_clip(&self, name: &str) -> Option<&Clip> {
        self.rig
            .as_ref()?
            .clips
            .iter()
            .find(|clip| clip.name == name)
    }
}

pub fn advance(model: &Model, player: &mut Player, dt: f32) {
    if let Some(clip) = model.animation_clip(&player.clip) {
        advance_markers(clip, player, dt, &[], 0, |_| {});
    }
}

// Emit crossings over (start, end] forward or [end, start) in reverse.
// At a loop boundary, markers at zero and duration are distinct entries.
pub(crate) fn advance_markers(
    clip: &Clip,
    player: &mut Player,
    dt: f32,
    markers: &[Marker],
    limit: usize,
    mut emit: impl FnMut(&Marker),
) {
    if !player.playing {
        return;
    }
    if clip.duration <= 0. {
        player.time = 0.;
        return;
    }
    let duration = f64::from(clip.duration);
    let start = f64::from(player.time);
    let start = if player.looping {
        start.rem_euclid(duration)
    } else {
        start
    };
    let delta = f64::from(dt) * f64::from(player.speed);
    let end = start + delta;
    // Match the crossing interval to the phase actually stored for the next tick.
    let end = if player.looping {
        let phase = end.rem_euclid(duration);
        let time = phase as f32;
        player.time = if time == clip.duration { 0. } else { time };
        end - phase + f64::from(time)
    } else {
        player.time = end.clamp(0., duration) as f32;
        if (delta > 0. && player.time >= clip.duration) || (delta < 0. && player.time <= 0.) {
            player.playing = false;
        }
        f64::from(player.time)
    };
    let traveled = (end - start) * delta.signum();
    if traveled <= 0. || markers.is_empty() || limit == 0 {
        return;
    }

    let mut crossings: Vec<_> = markers
        .iter()
        .filter_map(|marker| {
            let offset = (f64::from(marker.time) - start) * delta.signum();
            let distance = if player.looping {
                let distance = offset.rem_euclid(duration);
                if distance == 0. {
                    duration
                } else {
                    distance
                }
            } else {
                offset
            };
            (distance > 0. && distance <= traveled).then_some((distance, marker))
        })
        .collect();
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut count = 0;
    let mut cycle = 0.;
    while !crossings.is_empty() {
        for &(distance, marker) in &crossings {
            if count == limit || cycle + distance > traveled {
                return;
            }
            emit(marker);
            count += 1;
        }
        if !player.looping {
            return;
        }
        cycle += duration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_crossings_respect_pause_speed_direction_and_loop_boundaries() {
        let clip = Clip {
            name: "Stride".into(),
            duration: 1.,
            tracks: vec![],
            markers: [(0.75, "late"), (0.25, "early"), (0., "start"), (1., "end")]
                .map(|(time, name)| Marker {
                    time,
                    name: name.into(),
                })
                .to_vec(),
        };
        let mut player = Player {
            time: 0.125,
            speed: 2.,
            playing: false,
            ..Player::default()
        };
        let tick = |p: &mut Player, dt| {
            let mut fired = vec![];
            advance_markers(&clip, p, dt, &clip.markers, MAX_EVENTS, |m| {
                fired.push(m.name.clone())
            });
            fired
        };
        assert!(tick(&mut player, 0.5).is_empty());
        assert_eq!(player.time, 0.125);
        player.playing = true;
        assert_eq!(tick(&mut player, 0.0625), ["early"]);
        assert!(tick(&mut player, 0.0625).is_empty());
        assert_eq!(tick(&mut player, 0.375), ["late", "start", "end"]);
        assert_eq!(player.time, 0.125);
        player.speed = -2.;
        assert_eq!(tick(&mut player, 0.25), ["start", "end", "late"]);
        player.looping = false;
        assert_eq!(tick(&mut player, 0.5), ["early", "start"]);
        assert_eq!(player.time, 0.);
        assert!(!player.playing);
        assert!(tick(&mut player, 0.25).is_empty());
    }

    #[test]
    fn rapid_retarget_starts_at_the_visible_pose_and_finishes_at_the_new_clip() {
        let clip = |name: &str, x: f32| Clip {
            name: name.into(),
            duration: 1.,
            markers: vec![],
            tracks: vec![Track {
                node: 0,
                property: Property::Translation,
                interpolation: Interpolation::Linear,
                times: vec![0., 1.],
                values: vec![[x, 0., 0., 0.], [x + 2., 0., 0., 0.]],
            }],
        };
        let rig = Rig {
            nodes: vec![Node {
                parent: None,
                position: [0.; 3],
                rotation: Quat::IDENTITY.to_array(),
                scale: [1.; 3],
            }],
            skins: vec![],
            clips: vec![clip("Idle", 0.), clip("Walk", 10.), clip("Run", 20.)],
        };
        let player = |name: &str| Player {
            clip: name.into(),
            ..Player::default()
        };
        let model = Model {
            lods: Default::default(),
            name: "test".into(),
            parts: vec![].into(),
            rig: Some(rig.clone().into()),
        };
        let idle = player("Idle");
        let walk = player("Walk");
        let run = player("Run");
        let mut blend = BlendState::new(Some(&idle));
        let x = |b: &BlendState| b.sample(&rig)[0].position[0];
        blend.select(&rig, Some(&walk), 0.4);
        assert_eq!(x(&blend), 0.);
        blend.advance(&model, Some(&walk), 0.2, true);
        assert!((x(&blend) - 5.2).abs() < 1e-5);
        let visible = x(&blend);
        blend.select(&rig, Some(&run), 0.4);
        assert_eq!(x(&blend), visible);
        blend.advance(&model, Some(&run), 0.4, true);
        assert_eq!(x(&blend), 20.);
        assert!(blend.transition.is_none());
    }

    #[test]
    fn normal_map_frame_follows_skinned_geometry_and_reflections() {
        let mut model = crate::runtime::Scene::rendering_demo().models.remove(0);
        let rig = std::sync::Arc::make_mut(model.rig.as_mut().unwrap());
        rig.nodes[0].rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array();
        rig.nodes[0].scale = [-1., 1., 1.];
        let pose = rig.pose(None);
        let part = &model.parts[0];
        let original = Vec3::from_array(part.vertices[0].normal);
        let t = crate::runtime::tangent_reference(original);
        let normal = pose.vertex(part, 0).normal;
        let tangent = Vec4::from_array(pose.tangent(part, 0, normal, t.extend(1.).to_array()));
        let expected =
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * (t * Vec3::new(-1., 1., 1.));
        assert!(tangent.truncate().distance(expected) < 1e-5);
        assert!(tangent.truncate().dot(Vec3::from_array(normal)).abs() < 1e-5);
        assert_eq!(tangent.w, -1.);
    }
}
