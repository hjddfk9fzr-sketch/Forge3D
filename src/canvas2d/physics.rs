use super::{BodyKind, Canvas};
use crate::{
    controls::GameActions,
    physics::{Event, EventKind},
    runtime::Scene,
};
use macroquad::prelude::Vec2;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy)]
struct Bounds {
    lo: Vec2,
    hi: Vec2,
}

impl Bounds {
    fn around(center: Vec2, size: Vec2) -> Self {
        Self {
            lo: center - size * 0.5,
            hi: center + size * 0.5,
        }
    }
    fn moved(self, by: Vec2) -> Self {
        Self {
            lo: self.lo + by,
            hi: self.hi + by,
        }
    }
    fn union(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn overlaps(self, other: Self) -> bool {
        self.lo.cmplt(other.hi).all() && self.hi.cmpgt(other.lo).all()
    }
}

#[derive(Default)]
struct Motion {
    vertical: f32,
    grounded: bool,
}

#[derive(Default)]
pub(crate) struct World {
    motion: HashMap<u64, Motion>,
    contacts: BTreeSet<(u64, u64)>,
}

const MAX_EVENTS: usize = 4096;

impl World {
    pub fn step(&mut self, scene: &mut Scene, actions: GameActions, dt: f32) -> Vec<Event> {
        let canvas = scene.canvas2d.as_mut().unwrap();
        canvas.time = (canvas.time + dt).rem_euclid(3600.);
        let solids: Vec<_> = scene
            .objects
            .iter()
            .filter_map(|o| {
                o.body2d
                    .as_ref()
                    .filter(|body| body.kind == BodyKind::Static)
                    .map(|body| bounds(o.position, body.size))
            })
            .collect();
        let sensors: Vec<_> = scene
            .objects
            .iter()
            .filter_map(|o| {
                o.body2d
                    .as_ref()
                    .filter(|body| body.kind == BodyKind::Sensor)
                    .map(|body| (o.id, bounds(o.position, body.size)))
            })
            .collect();
        let mut contacts = BTreeSet::new();
        let mut crossed = BTreeSet::new();
        self.motion
            .retain(|id, _| scene.objects.iter().any(|o| o.id == *id));
        for object in &mut scene.objects {
            let Some(body) = &object.body2d else {
                continue;
            };
            if !matches!(body.kind, BodyKind::Character | BodyKind::Dynamic) {
                continue;
            }
            let character = body.kind == BodyKind::Character;
            let state = self.motion.entry(object.id).or_default();
            if character && state.grounded && actions.jump {
                state.vertical = -body.jump_speed;
            }
            state.vertical = (state.vertical + canvas.gravity * dt).clamp(-100., 100.);
            let start = bounds(object.position, body.size);
            let dx = if character {
                actions.movement.x * object.speed * dt
            } else {
                0.
            };
            let dx = sweep(canvas, &solids, start, dx, 0);
            let lateral = start.moved(Vec2::new(dx, 0.));
            let wanted = state.vertical * dt;
            let dy = sweep(canvas, &solids, lateral, wanted, 1);
            state.grounded = wanted >= 0. && dy < wanted;
            if dy != wanted {
                state.vertical = 0.;
            }
            object.position[0] += dx;
            object.position[1] += dy;
            let end = lateral.moved(Vec2::new(0., dy));
            for &(sensor, target) in &sensors {
                let key = (object.id, sensor);
                if end.overlaps(target) {
                    if contacts.len() < MAX_EVENTS {
                        contacts.insert(key);
                    }
                } else if crossed.len() < MAX_EVENTS
                    && (start.union(lateral).overlaps(target)
                        || lateral.union(end).overlaps(target))
                {
                    crossed.insert(key);
                }
            }
        }
        let event = |&(a, b): &(u64, u64), kind| Event { a, b, kind };
        let mut events = Vec::new();
        events.extend(
            contacts
                .difference(&self.contacts)
                .take(MAX_EVENTS)
                .map(|pair| event(pair, EventKind::TriggerEnter)),
        );
        events.extend(
            self.contacts
                .difference(&contacts)
                .take(MAX_EVENTS - events.len())
                .map(|pair| event(pair, EventKind::TriggerExit)),
        );
        for pair in crossed
            .difference(&self.contacts)
            .take((MAX_EVENTS - events.len()) / 2)
        {
            events.push(event(pair, EventKind::TriggerEnter));
            events.push(event(pair, EventKind::TriggerExit));
        }
        self.contacts = contacts;
        events
    }
}

fn bounds(position: [f32; 3], size: [f32; 2]) -> Bounds {
    Bounds::around(Vec2::new(position[0], position[1]), Vec2::from_array(size))
}

fn sweep(canvas: &Canvas, solids: &[Bounds], start: Bounds, wanted: f32, axis: usize) -> f32 {
    if wanted == 0. {
        return 0.;
    }
    let other = 1 - axis;
    let mut delta = wanted;
    let mut test = |wall: Bounds| {
        if start.lo[other] >= wall.hi[other] - 1e-5 || start.hi[other] <= wall.lo[other] + 1e-5 {
            return;
        }
        if wanted > 0. && start.hi[axis] <= wall.lo[axis] + 1e-5 {
            delta = delta.min((wall.lo[axis] - start.hi[axis]).max(0.));
        } else if wanted < 0. && start.lo[axis] >= wall.hi[axis] - 1e-5 {
            delta = delta.max((wall.hi[axis] - start.lo[axis]).min(0.));
        }
    };
    let moved = if axis == 0 {
        Vec2::new(wanted, 0.)
    } else {
        Vec2::new(0., wanted)
    };
    let span = start.union(start.moved(moved));
    let edge = |v: f32, max: u16| v.clamp(0., f32::from(max)) as i32;
    for y in edge(span.lo.y.floor(), canvas.height)..edge(span.hi.y.ceil(), canvas.height) {
        for x in edge(span.lo.x.floor(), canvas.width)..edge(span.hi.x.ceil(), canvas.width) {
            if canvas.solid(x, y) {
                let lo = Vec2::new(x as f32, y as f32);
                test(Bounds {
                    lo,
                    hi: lo + Vec2::ONE,
                });
            }
        }
    }
    for &wall in solids {
        test(wall);
    }
    delta
}
