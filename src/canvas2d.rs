//! Scene-local 2D assets and orthographic rendering. Coordinates are Y-down.
use crate::runtime::Scene;
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
#[path = "canvas2d/physics.rs"]
pub(crate) mod physics;

pub const MAX_WIDTH: u16 = 256;
pub const MAX_HEIGHT: u16 = 128;
pub const MAX_LAYERS: usize = 16;
pub const MAX_ATLASES: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u16,
    pub height: u16,
    pub background: [u8; 4],
    pub gravity: f32,
    pub layers: Vec<TileLayer>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub atlases: BTreeMap<String, Atlas>,
    #[serde(skip)]
    pub(crate) time: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    pub png: Arc<[u8]>,
    pub frame_width: u16,
    pub frame_height: u16,
}

impl Atlas {
    pub fn size(&self) -> Option<(u16, u16)> {
        let header = self.png.get(..24)?;
        if header[..8] != *b"\x89PNG\r\n\x1a\n"
            || &header[12..16] != b"IHDR"
            || self.png.len() > 8 * 1024 * 1024
        {
            return None;
        }
        let dim = |index| {
            u16::try_from(u32::from_be_bytes(
                header[index..index + 4].try_into().ok()?,
            ))
            .ok()
            .filter(|dim| (1..=2048).contains(dim))
        };
        Some((dim(16)?, dim(20)?))
    }

    pub fn frames(&self) -> usize {
        self.size()
            .filter(|(w, h)| {
                self.frame_width > 0
                    && self.frame_height > 0
                    && w.is_multiple_of(self.frame_width)
                    && h.is_multiple_of(self.frame_height)
            })
            .map_or(0, |(w, h)| {
                usize::from(w / self.frame_width) * usize::from(h / self.frame_height)
            })
    }

    fn rect(&self, frame: usize) -> Rect {
        let columns = usize::from(self.size().unwrap().0 / self.frame_width);
        let w = f32::from(self.frame_width);
        let h = f32::from(self.frame_height);
        Rect::new(
            (frame % columns) as f32 * w,
            (frame / columns) as f32 * h,
            w,
            h,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileLayer {
    pub name: String,
    pub depth: i16,
    pub solid: bool,
    pub color: [u8; 4],
    pub atlas: Option<String>,
    /// Zero is empty. Other values are one-based atlas frame indices.
    pub cells: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    pub atlas: String,
    pub first: u16,
    pub frames: u16,
    pub fps: f32,
    pub depth: i16,
    pub size: [f32; 2],
}

impl Sprite {
    pub fn frame_at(&self, time: f32) -> usize {
        usize::from(self.first)
            + ((time.max(0.) * self.fps) as usize % usize::from(self.frames.max(1)))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    Static,
    Dynamic,
    Character,
    Sensor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub kind: BodyKind,
    pub size: [f32; 2],
    pub jump_speed: f32,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            kind: BodyKind::Static,
            size: [0.8, 0.9],
            jump_speed: 11.,
        }
    }
}

impl Canvas {
    pub fn new(width: u16, height: u16) -> Self {
        let mut canvas = Self {
            width: width.clamp(1, MAX_WIDTH),
            height: height.clamp(1, MAX_HEIGHT),
            background: [24, 33, 54, 255],
            gravity: 27.,
            layers: Vec::new(),
            atlases: BTreeMap::new(),
            time: 0.,
        };
        canvas.add_layer("Ground", true);
        canvas
    }

    pub fn add_layer(&mut self, name: &str, solid: bool) {
        if self.layers.len() < MAX_LAYERS {
            self.layers.push(TileLayer {
                name: name.into(),
                depth: 0,
                solid,
                color: [57, 121, 114, 255],
                atlas: None,
                cells: vec![0; usize::from(self.width) * usize::from(self.height)],
            });
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        let width = width.clamp(1, MAX_WIDTH);
        let height = height.clamp(1, MAX_HEIGHT);
        if (width, height) == (self.width, self.height) {
            return;
        }
        for layer in &mut self.layers {
            let mut cells = vec![0; usize::from(width) * usize::from(height)];
            let count = usize::from(width.min(self.width));
            for y in 0..usize::from(height.min(self.height)) {
                let a = y * usize::from(width);
                let b = y * usize::from(self.width);
                cells[a..a + count].copy_from_slice(&layer.cells[b..b + count]);
            }
            layer.cells = cells;
        }
        self.width = width;
        self.height = height;
    }

    pub fn paint(&mut self, layer: usize, x: i32, y: i32, tile: u16) {
        if let Some(index) = self.index(x, y) {
            if let Some(layer) = self.layers.get_mut(layer) {
                let max = layer
                    .atlas
                    .as_ref()
                    .and_then(|id| self.atlases.get(id))
                    .map_or(u16::MAX, |atlas| {
                        atlas.frames().min(usize::from(u16::MAX)) as u16
                    });
                layer.cells[index] = tile.min(max);
            }
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        ((0..i32::from(self.width)).contains(&x) && (0..i32::from(self.height)).contains(&y))
            .then(|| y as usize * usize::from(self.width) + x as usize)
    }

    pub fn solid(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|index| {
            self.layers
                .iter()
                .any(|layer| layer.solid && layer.cells[index] != 0)
        })
    }

    pub(crate) fn validate(&self, scene: &Scene) -> Result<(), String> {
        if !(1..=MAX_WIDTH).contains(&self.width)
            || !(1..=MAX_HEIGHT).contains(&self.height)
            || !(0. ..=200.).contains(&self.gravity)
            || self.layers.len() > MAX_LAYERS
            || self.atlases.len() > MAX_ATLASES
            || self
                .atlases
                .iter()
                .any(|(id, atlas)| !crate::game::valid_id(id) || atlas.frames() == 0)
        {
            return Err("Invalid 2D canvas or atlas.".into());
        }
        for layer in &self.layers {
            let frames = layer
                .atlas
                .as_ref()
                .map(|id| self.atlases.get(id).map_or(0, Atlas::frames));
            if layer.cells.len() != usize::from(self.width) * usize::from(self.height)
                || frames == Some(0)
                || frames.is_some_and(|max| layer.cells.iter().any(|&tile| usize::from(tile) > max))
            {
                return Err(format!("Invalid 2D layer '{}'.", layer.name));
            }
        }
        for object in &scene.objects {
            if object.body2d.as_ref().is_some_and(|body| {
                object.parent.is_some()
                    || body.size.iter().any(|size| !(0.01..=32.).contains(size))
                    || !(0. ..=100.).contains(&body.jump_speed)
                    || !(0. ..=100.).contains(&object.speed)
            }) {
                return Err(format!("Invalid 2D body on '{}'.", object.name));
            }
            if object.sprite2d.as_ref().is_some_and(|sprite| {
                sprite.frames == 0
                    || !(0. ..=120.).contains(&sprite.fps)
                    || sprite.size.iter().any(|d| !(0.01..=128.).contains(d))
                    || self.atlases.get(&sprite.atlas).is_none_or(|a| {
                        usize::from(sprite.first) + usize::from(sprite.frames) > a.frames()
                    })
            }) {
                return Err(format!("Invalid 2D sprite on '{}'.", object.name));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Render {
    textures: HashMap<String, (Arc<[u8]>, Option<Texture2D>)>,
}

fn rgba(c: [u8; 4]) -> Color {
    Color::from_rgba(c[0], c[1], c[2], c[3])
}

impl Render {
    pub fn draw(&mut self, scene: &Scene, view: &Camera3D, grid: bool, selected: Option<usize>) {
        let Some(canvas) = &scene.canvas2d else {
            return;
        };
        let height = view.position.distance(view.target).max(0.25);
        let aspect = view
            .aspect
            .unwrap_or_else(|| screen_width() / screen_height().max(1.));
        let area = area(view.target, height, aspect);
        let mut camera = Camera2D::from_display_rect(area);
        // On the window framebuffer Macroquad flips the camera's Y zoom.
        camera.zoom.y = -camera.zoom.y;
        camera.viewport = view.viewport;
        set_camera(&camera);
        draw_rectangle(area.x, area.y, area.w, area.h, rgba(canvas.background));
        self.textures
            .retain(|name, _| canvas.atlases.contains_key(name));
        for (name, atlas) in &canvas.atlases {
            if self
                .textures
                .get(name)
                .is_none_or(|(png, _)| !Arc::ptr_eq(png, &atlas.png))
            {
                let texture = Image::from_file_with_format(&atlas.png, Some(ImageFormat::Png))
                    .ok()
                    .map(|image| {
                        let texture = Texture2D::from_image(&image);
                        texture.set_filter(FilterMode::Nearest);
                        texture
                    });
                self.textures
                    .insert(name.clone(), (atlas.png.clone(), texture));
            }
        }
        primitives(scene, canvas, area, |destination, texture, color| {
            if let Some((id, source)) = texture {
                if let Some((_, Some(texture))) = self.textures.get(id) {
                    draw_texture_ex(
                        texture,
                        destination.x,
                        destination.y,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(vec2(destination.w, destination.h)),
                            source: Some(source),
                            ..Default::default()
                        },
                    );
                }
            } else {
                draw_rectangle(
                    destination.x,
                    destination.y,
                    destination.w,
                    destination.h,
                    rgba(color),
                );
            }
        });
        if grid {
            for x in 0..=canvas.width {
                draw_line(
                    f32::from(x),
                    0.,
                    f32::from(x),
                    f32::from(canvas.height),
                    0.022,
                    Color::new(1., 1., 1., 0.12),
                );
            }
            for y in 0..=canvas.height {
                draw_line(
                    0.,
                    f32::from(y),
                    f32::from(canvas.width),
                    f32::from(y),
                    0.022,
                    Color::new(1., 1., 1., 0.12),
                );
            }
        }
        if let Some(index) = selected.filter(|&i| i < scene.objects.len()) {
            let p = scene.world_transforms()[index]
                .0
                .transform_point3(Vec3::ZERO);
            let object = &scene.objects[index];
            let [w, h] = object.body2d.as_ref().map_or([1.1, 1.1], |body| body.size);
            draw_rectangle_lines(p.x - w * 0.5, p.y - h * 0.5, w, h, 0.05, YELLOW);
        }
        set_default_camera();
    }
}

pub(crate) fn area(target: Vec3, height: f32, aspect: f32) -> Rect {
    let width = height * aspect;
    Rect::new(
        target.x - width * 0.5,
        target.y - height * 0.5,
        width,
        height,
    )
}

fn tile_source<'a>(canvas: &Canvas, id: &'a str, frame: usize) -> Option<(&'a str, Rect)> {
    let atlas = canvas
        .atlases
        .get(id)
        .filter(|atlas| frame < atlas.frames())?;
    Some((id, atlas.rect(frame)))
}

pub(crate) fn primitives(
    scene: &Scene,
    canvas: &Canvas,
    area: Rect,
    mut draw: impl FnMut(Rect, Option<(&str, Rect)>, [u8; 4]),
) {
    let world = scene.world_transforms();
    let mut items: Vec<_> = canvas
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.depth, false, i))
        .collect();
    items.extend(scene.objects.iter().enumerate().filter_map(|(i, o)| {
        o.sprite2d
            .as_ref()
            .filter(|_| world[i].1)
            .map(|s| (s.depth, true, i))
    }));
    items.sort_unstable();
    for (_, sprite, index) in items {
        if sprite {
            let s = scene.objects[index].sprite2d.as_ref().unwrap();
            let matrix = world[index].0;
            let p = matrix.transform_point3(Vec3::ZERO);
            let size = vec2(
                s.size[0] * matrix.x_axis.truncate().length(),
                s.size[1] * matrix.y_axis.truncate().length(),
            );
            let destination = Rect::new(p.x - size.x * 0.5, p.y - size.y * 0.5, size.x, size.y);
            draw(
                destination,
                tile_source(canvas, &s.atlas, s.frame_at(canvas.time)),
                [0; 4],
            );
        } else {
            let layer = &canvas.layers[index];
            let bound = |n: f32, max: u16| n.clamp(0., f32::from(max)) as usize;
            for y in
                bound(area.y.floor(), canvas.height)..bound((area.y + area.h).ceil(), canvas.height)
            {
                for x in bound(area.x.floor(), canvas.width)
                    ..bound((area.x + area.w).ceil(), canvas.width)
                {
                    let tile = layer.cells[y * usize::from(canvas.width) + x];
                    if tile == 0 {
                        continue;
                    }
                    let destination = Rect::new(x as f32, y as f32, 1., 1.);
                    if let Some(id) = &layer.atlas {
                        draw(
                            destination,
                            tile_source(canvas, id, usize::from(tile) - 1),
                            [0; 4],
                        );
                    } else {
                        draw(destination, None, layer.color);
                    }
                }
            }
        }
    }
}

pub fn empty_2d() -> Scene {
    let mut scene = Scene::empty();
    scene.title = "Untitled 2D Scene".into();
    scene.canvas2d = Some(Canvas::new(40, 18));
    scene.camera.target = [10., 8., 0.];
    scene.camera.distance = 12.;
    scene
}
