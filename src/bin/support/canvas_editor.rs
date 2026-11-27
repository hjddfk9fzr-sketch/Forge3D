use egui_macroquad::egui::{self, PointerButton, Response, Ui};
use forge3d::{
    canvas2d::{Atlas, Body, BodyKind, Sprite, MAX_ATLASES, MAX_HEIGHT, MAX_LAYERS, MAX_WIDTH},
    runtime::{Object, Scene, View},
};
use std::{fs::File, io::Read, path::Path};

pub(super) struct Tools {
    pub painting: bool,
    layer: usize,
    brush: u16,
    file: String,
    id: String,
    frame_size: [u16; 2],
    feedback: String,
    stroke: Option<([i32; 2], u16)>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            painting: false,
            layer: 0,
            brush: 1,
            file: String::new(),
            id: String::new(),
            frame_size: [16, 16],
            feedback: String::new(),
            stroke: None,
        }
    }
}

impl Tools {
    pub fn inspector(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        view: &View,
        selected: Option<usize>,
        folder: &Path,
    ) {
        let canvas = scene.canvas2d.as_mut().unwrap();
        egui::CollapsingHeader::new("Canvas and camera").show(ui, |ui| {
            ui.text_edit_singleline(&mut scene.title);
            let mut width = canvas.width;
            let mut height = canvas.height;
            ui.horizontal(|ui| {
                ui.label("Tiles");
                ui.add(egui::DragValue::new(&mut width).range(1..=MAX_WIDTH));
                ui.add(egui::DragValue::new(&mut height).range(1..=MAX_HEIGHT));
            });
            canvas.resize(width, height);
            ui.color_edit_button_srgba_unmultiplied(&mut canvas.background);
            number(ui, "Gravity", &mut canvas.gravity, 0. ..=200.);
            size(
                ui,
                "Camera X / Y",
                &mut scene.camera.target[..2],
                -5000. ..=5000.,
            );
            number(ui, "View height", &mut scene.camera.distance, 0.25..=500.);
            if ui.button("Use editor camera").clicked() {
                scene.camera = view.clone();
            }
        });
        egui::CollapsingHeader::new("Tile brush")
            .default_open(true)
            .show(ui, |ui| {
                self.layer = self.layer.min(canvas.layers.len().saturating_sub(1));
                if ui
                    .add_enabled(
                        canvas.layers.len() < MAX_LAYERS,
                        egui::Button::new("Add layer"),
                    )
                    .clicked()
                {
                    canvas.add_layer("New layer", false);
                    self.layer = canvas.layers.len() - 1;
                }
                egui::ComboBox::from_id_salt("tile-layer")
                    .selected_text(canvas.layers.get(self.layer).map_or("Empty", |l| &l.name))
                    .show_ui(ui, |ui| {
                        for (index, layer) in canvas.layers.iter().enumerate() {
                            ui.selectable_value(&mut self.layer, index, &layer.name);
                        }
                    });
                let Some(layer) = canvas.layers.get_mut(self.layer) else {
                    return;
                };
                ui.text_edit_singleline(&mut layer.name);
                ui.checkbox(&mut layer.solid, "Solid");
                ui.add(egui::DragValue::new(&mut layer.depth).prefix("Order "));
                let choice = atlas_picker(
                    ui,
                    "tile-atlas",
                    layer.atlas.as_deref(),
                    &canvas.atlases,
                    true,
                );
                if let Some(choice) = choice {
                    layer.atlas = choice;
                }
                let max = layer.atlas.as_ref().map_or(1, |id| {
                    canvas.atlases[id].frames().min(usize::from(u16::MAX)) as u16
                });
                if self.painting {
                    ui.label("Left drag paints. Right drag erases.");
                }
                self.brush = self.brush.clamp(1, max);
                ui.add(
                    egui::DragValue::new(&mut self.brush)
                        .range(1..=max)
                        .prefix("Tile "),
                );
                if layer.atlas.is_none() {
                    ui.color_edit_button_srgba_unmultiplied(&mut layer.color);
                }
                if ui.button("Remove layer").clicked() {
                    canvas.layers.remove(self.layer);
                } else if layer.atlas.is_some() {
                    for cell in &mut layer.cells {
                        *cell = (*cell).min(max);
                    }
                }
            });
        egui::CollapsingHeader::new("Import sprite atlas").show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.file).hint_text("PNG path"));
            ui.add(egui::TextEdit::singleline(&mut self.id).hint_text("Atlas ID"));
            ui.horizontal(|ui| {
                ui.label("Frame pixels");
                for value in &mut self.frame_size {
                    ui.add(egui::DragValue::new(value).range(1..=2048));
                }
            });
            if ui
                .add_enabled(
                    canvas.atlases.len() < MAX_ATLASES,
                    egui::Button::new("Import"),
                )
                .clicked()
            {
                let id = self.id.trim();
                self.feedback = if canvas.atlases.contains_key(id) {
                    "An atlas with this ID already exists.".into()
                } else {
                    match read_atlas(&folder.join(self.file.trim()), id, self.frame_size) {
                        Ok(atlas) => {
                            let frames = atlas.frames();
                            canvas.atlases.insert(id.into(), atlas);
                            format!("{frames} frames imported.")
                        }
                        Err(error) => error,
                    }
                };
            }
            if !self.feedback.is_empty() {
                ui.label(&self.feedback);
            }
        });
        if let Some(object) = selected.and_then(|index| scene.objects.get_mut(index)) {
            ui.separator();
            ui.text_edit_singleline(&mut object.name);
            ui.checkbox(&mut object.visible, "Visible");
            size(
                ui,
                "Position X / Y",
                &mut object.position[..2],
                -5000. ..=5000.,
            );
            if let Some(sprite) = &mut object.sprite2d {
                if let Some(Some(id)) = atlas_picker(
                    ui,
                    "sprite-atlas",
                    Some(&sprite.atlas),
                    &canvas.atlases,
                    false,
                ) {
                    sprite.atlas = id;
                    sprite.first = 0;
                    sprite.frames = 1;
                }
                let max = canvas.atlases[&sprite.atlas].frames();
                ui.add(
                    egui::DragValue::new(&mut sprite.first)
                        .range(0..=(max - 1).min(65535) as u16)
                        .prefix("First frame "),
                );
                let limit = (max - usize::from(sprite.first)).min(65535) as u16;
                sprite.frames = sprite.frames.clamp(1, limit);
                ui.add(
                    egui::DragValue::new(&mut sprite.frames)
                        .range(1..=limit)
                        .prefix("Frames "),
                );
                number(ui, "Frames per second", &mut sprite.fps, 0. ..=120.);
                ui.add(egui::DragValue::new(&mut sprite.depth).prefix("Order "));
                size(ui, "Sprite size", &mut sprite.size, 0.01..=128.);
                if ui.button("Remove sprite").clicked() {
                    object.sprite2d = None;
                }
            } else if let Some(id) = canvas.atlases.keys().next() {
                if ui.button("Add sprite").clicked() {
                    object.sprite2d = Some(Sprite {
                        atlas: id.clone(),
                        first: 0,
                        frames: 1,
                        fps: 0.,
                        depth: 1,
                        size: [1., 1.],
                    });
                }
            }
            ui.separator();
            if let Some(body) = &mut object.body2d {
                egui::ComboBox::from_label("Body")
                    .selected_text(format!("{:?}", body.kind))
                    .show_ui(ui, |ui| {
                        for kind in [
                            BodyKind::Static,
                            BodyKind::Dynamic,
                            BodyKind::Character,
                            BodyKind::Sensor,
                        ] {
                            ui.selectable_value(&mut body.kind, kind, format!("{kind:?}"));
                        }
                    });
                size(ui, "Body size", &mut body.size, 0.01..=32.);
                if body.kind == BodyKind::Character {
                    number(ui, "Move speed", &mut object.speed, 0. ..=100.);
                    number(ui, "Jump speed", &mut body.jump_speed, 0. ..=100.);
                }
                if ui.button("Remove 2D body").clicked() {
                    object.body2d = None;
                }
            } else if ui
                .add_enabled(object.parent.is_none(), egui::Button::new("Add 2D body"))
                .clicked()
            {
                object.body2d = Some(Body::default());
            }
        }
    }

    pub fn viewport(
        &mut self,
        ui: &Ui,
        response: &Response,
        scene: &mut Scene,
        view: &mut View,
        selected: &mut Option<usize>,
    ) {
        let (delta, scroll) = ui.input(|i| (i.pointer.delta(), i.raw_scroll_delta.y));
        let scale = view.distance / response.rect.height().max(1.);
        if response.dragged_by(PointerButton::Middle) {
            view.target[0] -= delta.x * scale;
            view.target[1] -= delta.y * scale;
        }
        if response.hovered() && scroll != 0. {
            view.distance = (view.distance * (-scroll * 0.006).exp()).clamp(0.25, 500.);
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            self.stroke = None;
            return;
        };
        let offset = (pointer - response.rect.center()) * scale;
        let point = [offset.x + view.target[0], offset.y + view.target[1]];
        if self.painting {
            let tile =
                if response.secondary_clicked() || response.dragged_by(PointerButton::Secondary) {
                    0
                } else if response.clicked() || response.dragged_by(PointerButton::Primary) {
                    self.brush
                } else {
                    self.stroke = None;
                    return;
                };
            if !response.rect.contains(pointer) {
                self.stroke = None;
                return;
            }
            let canvas = scene.canvas2d.as_mut().unwrap();
            let end = [point[0].floor() as i32, point[1].floor() as i32];
            let start = self
                .stroke
                .filter(|(_, last_tile)| *last_tile == tile)
                .map_or(end, |(cell, _)| cell);
            let dx = end[0] - start[0];
            let dy = end[1] - start[1];
            let steps = dx.abs().max(dy.abs()).clamp(1, 1024);
            for i in 0..=steps {
                canvas.paint(
                    self.layer,
                    start[0] + dx * i / steps,
                    start[1] + dy * i / steps,
                    tile,
                );
            }
            self.stroke = Some((end, tile));
        } else if response.clicked() {
            let transforms = scene.world_transforms();
            *selected = scene
                .objects
                .iter()
                .enumerate()
                .filter(|(index, object)| {
                    let (matrix, visible) = transforms[*index];
                    let center = matrix.w_axis;
                    let [w, h] = object
                        .sprite2d
                        .as_ref()
                        .map_or([1., 1.], |sprite| sprite.size);
                    visible
                        && (point[0] - center.x).abs()
                            <= w * matrix.x_axis.truncate().length() * 0.5
                        && (point[1] - center.y).abs()
                            <= h * matrix.y_axis.truncate().length() * 0.5
                })
                .max_by_key(|(_, o)| o.sprite2d.as_ref().map_or(0, |sprite| sprite.depth))
                .map(|(index, _)| index);
        }
    }
}

fn atlas_picker(
    ui: &mut Ui,
    label: &str,
    current: Option<&str>,
    atlases: &std::collections::BTreeMap<String, Atlas>,
    allow_color: bool,
) -> Option<Option<String>> {
    let mut choice = None;
    egui::ComboBox::from_id_salt(label)
        .selected_text(current.unwrap_or("Solid color"))
        .show_ui(ui, |ui| {
            if allow_color
                && ui
                    .selectable_label(current.is_none(), "Solid color")
                    .clicked()
            {
                choice = Some(None);
            }
            for id in atlases.keys() {
                if ui.selectable_label(current == Some(id), id).clicked() {
                    choice = Some(Some(id.clone()));
                }
            }
        });
    choice
}

fn read_atlas(
    path: &Path,
    id: &str,
    [frame_width, frame_height]: [u16; 2],
) -> Result<Atlas, String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("Use lowercase letters, digits, or hyphens for the atlas ID.".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|err| err.to_string())?;
    let atlas = Atlas {
        png: bytes.into(),
        frame_width,
        frame_height,
    };
    if atlas.frames() == 0 {
        return Err("Use a PNG up to 2048 × 2048 and 8 MiB with evenly sized frames.".into());
    }
    image::load_from_memory_with_format(&atlas.png, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(atlas)
}

fn size(ui: &mut Ui, label: &str, values: &mut [f32], range: std::ops::RangeInclusive<f32>) {
    ui.horizontal(|ui| {
        ui.label(label);
        for value in values {
            ui.add(egui::DragValue::new(value).speed(0.05).range(range.clone()));
        }
    });
}

fn number(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    ui.add(
        egui::DragValue::new(value)
            .speed(0.05)
            .prefix(format!("{label} "))
            .range(range),
    );
}

pub(super) fn new_object(view: &View) -> Object {
    let mut object = Object::new("2D object", forge3d::runtime::Shape::Empty);
    object.position = [view.target[0], view.target[1], 0.];
    object
}
