use egui_macroquad::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use forge3d::presentation::{validate_png, Presentation, UiKey, Widget, WidgetKind};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path, sync::Arc};

pub struct UiDesigner {
    pub draft: Presentation,
    selected: Option<usize>,
    image_path: String,
    sound_id: String,
    sound_path: String,
    error: Option<String>,
    previews: BTreeMap<usize, (Arc<[u8]>, egui::TextureHandle)>,
}

impl UiDesigner {
    pub fn new(draft: Presentation) -> Self {
        Self {
            selected: (!draft.widgets.is_empty()).then_some(0),
            draft,
            image_path: String::new(),
            sound_id: String::new(),
            sound_path: String::new(),
            error: None,
            previews: BTreeMap::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, aspect: f32) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Text").clicked() {
                self.add(WidgetKind::Text {
                    text: "New label".into(),
                });
            }
            if ui.button("+ Button").clicked() {
                let id = (1..)
                    .map(|number| format!("button-{number}"))
                    .find(|id| !self.draft.has_button(id))
                    .expect("finite number of buttons");
                self.add(WidgetKind::Button {
                    id,
                    text: "New button".into(),
                    key: None,
                    action: None,
                });
            }
            ui.checkbox(
                &mut self.draft.hide_movement,
                "Hide Android movement controls",
            );
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("PNG file");
            ui.add(
                egui::TextEdit::singleline(&mut self.image_path)
                    .hint_text("Path to a PNG")
                    .desired_width(230.),
            );
            if ui.button("+ Image").clicked() {
                match self.image() {
                    Ok(png) => self.add(WidgetKind::Image { png }),
                    Err(error) => self.error = Some(error),
                }
            }
            let current_is_image = self
                .selected
                .and_then(|i| self.draft.widgets.get(i))
                .is_some_and(|widget| matches!(widget.kind, WidgetKind::Image { .. }));
            if ui
                .add_enabled(current_is_image, egui::Button::new("Replace image"))
                .clicked()
            {
                match self.image() {
                    Ok(png) => {
                        self.draft.widgets[self.selected.unwrap()].kind = WidgetKind::Image { png };
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        });
        ui.separator();

        ui.columns(2, |columns| {
            let width = columns[0].available_width();
            let height = (width / aspect.max(0.3)).clamp(145., 340.);
            let width = width.min(height * aspect.max(0.3));
            self.canvas(&mut columns[0], Vec2::new(width, height));
            columns[0].label("Drag a widget to move it. Drag its corner to resize.");
            self.properties(&mut columns[1]);
        });

        ui.separator();
        ui.collapsing("Sounds", |ui| {
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("ui-sounds")
                .max_height(100.)
                .show(ui, |ui| {
                    for id in self.draft.sounds.keys() {
                        ui.horizontal(|ui| {
                            ui.monospace(id);
                            if ui.small_button("Remove").clicked() {
                                remove = Some(id.clone());
                            }
                        });
                    }
                });
            if let Some(id) = remove {
                self.draft.sounds.remove(&id);
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.sound_id)
                        .hint_text("Sound ID")
                        .desired_width(100.),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.sound_path)
                        .hint_text("Path to WAV")
                        .desired_width(195.),
                );
                if ui.button("Add / replace sound").clicked() {
                    let result = (|| {
                        let audio = read_media(&self.sound_path, 2 * 1024 * 1024)?;
                        let mut draft = self.draft.clone();
                        draft.sounds.insert(self.sound_id.trim().to_owned(), audio);
                        draft.validate()?;
                        self.draft = draft;
                        Ok::<_, String>(())
                    })();
                    self.error = result.err();
                }
            });
        });
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }

    fn add(&mut self, kind: WidgetKind) {
        if self.draft.widgets.len() >= 64 {
            self.error = Some("A scene can have up to 64 widgets.".into());
            return;
        }
        self.draft.widgets.push(Widget {
            rect: [0.08, 0.08, 0.34, 0.13],
            kind,
        });
        self.selected = Some(self.draft.widgets.len() - 1);
        self.error = None;
    }

    fn image(&self) -> Result<Arc<[u8]>, String> {
        let bytes = read_media(&self.image_path, 1024 * 1024)?;
        validate_png(&bytes)?;
        Ok(bytes)
    }

    fn canvas(&mut self, ui: &mut egui::Ui, size: Vec2) {
        let (canvas, background) = ui.allocate_exact_size(size, Sense::click());
        let paint = ui.painter().with_clip_rect(canvas);
        paint.rect_filled(canvas, 4., Color32::from_rgb(15, 22, 34));
        for tenth in 1..10 {
            let pos = canvas.min + size * (tenth as f32 / 10.);
            let grid = Stroke::new(1_f32, Color32::from_gray(36));
            paint.vline(pos.x, canvas.y_range(), grid);
            paint.hline(canvas.x_range(), pos.y, grid);
        }
        if background.clicked() {
            self.selected = None;
        }
        for (i, widget) in self.draft.widgets.iter_mut().enumerate() {
            let rect = widget_rect(canvas, widget.rect);
            let id = background.id.with(i);
            let response = ui.interact(rect, id, Sense::click_and_drag());
            let selected = self.selected == Some(i);
            let corner = Rect::from_center_size(rect.max - Vec2::splat(5.), Vec2::splat(13.));
            let resize = selected.then(|| ui.interact(corner, id.with("resize"), Sense::drag()));
            let delta = ui.input(|input| input.pointer.delta()) / size;
            if response.clicked() || response.drag_started() {
                self.selected = Some(i);
            }
            let [x, y, w, h] = &mut widget.rect;
            if resize.as_ref().is_some_and(egui::Response::dragged) {
                *x = x.min(0.99);
                *y = y.min(0.99);
                *w = (*w + delta.x).clamp(0.01_f32.min(1. - *x), 1. - *x);
                *h = (*h + delta.y).clamp(0.01_f32.min(1. - *y), 1. - *y);
            } else if response.dragged() {
                *x = (*x + delta.x).clamp(0., (1. - *w).max(0.));
                *y = (*y + delta.y).clamp(0., (1. - *h).max(0.));
            }
            let rect = widget_rect(canvas, widget.rect);
            match &widget.kind {
                WidgetKind::Image { png } => {
                    if let Some(handle) = preview(&mut self.previews, i, png, ui.ctx()) {
                        let fit = rect.size() / handle.size_vec2();
                        let bounds = Rect::from_center_size(
                            rect.center(),
                            handle.size_vec2() * fit.x.min(fit.y),
                        );
                        paint.image(
                            handle.id(),
                            bounds,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                            Color32::WHITE,
                        );
                    }
                }
                WidgetKind::Text { text } | WidgetKind::Button { text, .. } => {
                    let button = matches!(widget.kind, WidgetKind::Button { .. });
                    if button {
                        paint.rect_filled(rect, 2., Color32::from_rgb(34, 59, 102));
                    }
                    let font_size =
                        (rect.height() * if button { 0.54 } else { 0.75 }).clamp(1., 92.);
                    let font = egui::FontId::proportional(font_size);
                    let galley = paint.layout_no_wrap(text.clone(), font, Color32::WHITE);
                    let fit = (rect.width() * 0.9 / galley.size().x.max(1.)).min(1.);
                    let galley = if fit < 1. {
                        let font = egui::FontId::proportional((font_size * fit).max(1.));
                        paint.layout_no_wrap(text.clone(), font, Color32::WHITE)
                    } else {
                        galley
                    };
                    paint.with_clip_rect(rect.intersect(canvas)).galley(
                        rect.center() - galley.size() * 0.5,
                        galley,
                        Color32::WHITE,
                    );
                }
            }
            let stroke = if self.selected == Some(i) {
                Color32::from_rgb(110, 222, 196)
            } else {
                Color32::from_gray(80)
            };
            paint.rect_stroke(
                rect,
                2.,
                Stroke::new(1_f32, stroke),
                egui::StrokeKind::Inside,
            );
            if self.selected == Some(i) {
                paint.rect_filled(
                    Rect::from_center_size(rect.max - Vec2::splat(5.), Vec2::splat(9.)),
                    2.,
                    stroke,
                );
            }
        }
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        ui.strong("Widgets");
        egui::ScrollArea::vertical()
            .id_salt("widget-list")
            .max_height(110.)
            .show(ui, |ui| {
                for (i, widget) in self.draft.widgets.iter().enumerate() {
                    let label = match &widget.kind {
                        WidgetKind::Text { text } => format!("Text · {text}"),
                        WidgetKind::Button { id, .. } => format!("Button · {id}"),
                        WidgetKind::Image { .. } => format!("Image {}", i + 1),
                    };
                    ui.selectable_value(&mut self.selected, Some(i), label);
                }
            });
        let Some(i) = self.selected else {
            return;
        };
        ui.separator();
        let n = self.draft.widgets.len();
        let mut remove = false;
        let mut order = i;
        ui.horizontal_wrapped(|ui| {
            remove = ui.small_button("Remove").clicked();
            if ui
                .add_enabled(i > 0, egui::Button::new("Send back"))
                .clicked()
            {
                order = i - 1;
            }
            if ui
                .add_enabled(i + 1 < n, egui::Button::new("Bring forward"))
                .clicked()
            {
                order = i + 1;
            }
        });
        if remove {
            self.draft.widgets.remove(i);
            self.selected = (n > 1).then_some(i.min(n.saturating_sub(2)));
            self.previews.clear();
            return;
        }
        if order != i {
            self.draft.widgets.swap(i, order);
            self.selected = Some(order);
        }
        let widget = &mut self.draft.widgets[order];
        match &mut widget.kind {
            WidgetKind::Text { text } => {
                ui.label("Text");
                ui.text_edit_singleline(text);
            }
            WidgetKind::Button {
                id,
                text,
                key,
                action,
            } => {
                ui.label("Button ID");
                ui.text_edit_singleline(id);
                ui.label("Text");
                ui.text_edit_singleline(text);
                egui::ComboBox::from_id_salt("widget-action")
                    .selected_text(format!("Game action: {action:?}"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(action, None, "None");
                        for a in [
                            forge3d::controls::Action::Interact,
                            forge3d::controls::Action::Jump,
                        ] {
                            ui.selectable_value(action, Some(a), format!("{a:?}"));
                        }
                    });
                egui::ComboBox::from_id_salt("widget-key")
                    .selected_text(key.map_or("No keyboard shortcut", UiKey::name))
                    .height(235.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(key, None, "None");
                        for &choice in UiKey::ALL {
                            ui.selectable_value(key, Some(choice), choice.name());
                        }
                    });
            }
            WidgetKind::Image { png } => {
                ui.small(format!("PNG · {} KB", png.len().div_ceil(1024)));
            }
        }
        let [x, y, w, h] = &mut widget.rect;
        ui.label("Bounds (% of game view)");
        egui::Grid::new("widget-bounds").show(ui, |ui| {
            for (a, b) in [
                (("X", &mut *x, 0., 1. - *w), ("Y", &mut *y, 0., 1. - *h)),
                (("W", w, 0.01, 1.), ("H", h, 0.01, 1.)),
            ] {
                for (name, value, min, max) in [a, b] {
                    ui.label(name);
                    let mut percent = *value * 100.;
                    if ui
                        .add(
                            egui::DragValue::new(&mut percent)
                                .range((min * 100.)..=(max.max(min) * 100.))
                                .speed(0.4),
                        )
                        .changed()
                    {
                        *value = percent / 100.;
                    }
                }
                ui.end_row();
            }
        });
        *x = x.min((1. - *w).max(0.));
        *y = y.min((1. - *h).max(0.));
    }
}

fn widget_rect(canvas: Rect, [x, y, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(
        canvas.min + canvas.size() * Vec2::new(x, y),
        canvas.size() * Vec2::new(w, h),
    )
}

fn preview<'a>(
    cache: &'a mut BTreeMap<usize, (Arc<[u8]>, egui::TextureHandle)>,
    slot: usize,
    bytes: &Arc<[u8]>,
    ctx: &egui::Context,
) -> Option<&'a egui::TextureHandle> {
    if !cache
        .get(&slot)
        .is_some_and(|(old, _)| Arc::ptr_eq(bytes, old))
    {
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .ok()?
            .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let handle = ctx.load_texture("game-ui-image", pixels, egui::TextureOptions::LINEAR);
        cache.insert(slot, (bytes.clone(), handle));
    }
    cache.get(&slot).map(|entry| &entry.1)
}

fn read_media(path: &str, limit: u64) -> Result<Arc<[u8]>, String> {
    let mut bytes = Vec::new();

    File::open(Path::new(path.trim()))
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Media file is too large.".into());
    }
    Ok(bytes.into())
}
