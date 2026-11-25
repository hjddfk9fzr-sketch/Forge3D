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

    pub fn pose(&self, player: Option<&Player>) -> Pose {
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

pub struct Pose {
    global: Vec<Transform>,
    joints: Vec<Vec<Transform>>,
}

impl Pose {
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

pub fn advance(model: &Model, player: &mut Player, dt: f32) {
    let Some(clip) = model
        .rig
        .as_ref()
        .and_then(|rig| rig.clips.iter().find(|clip| clip.name == player.clip))
    else {
        return;
    };
    if !player.playing {
        return;
    }
    let time = player.time + dt * player.speed;
    if clip.duration <= 0. {
        player.time = 0.;
    } else if player.looping {
        player.time = time.rem_euclid(clip.duration);
    } else {
        player.time = time.clamp(0., clip.duration);
        if (player.speed > 0. && time >= clip.duration) || (player.speed < 0. && time <= 0.) {
            player.playing = false;
        }
    }
}
