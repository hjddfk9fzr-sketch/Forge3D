use crate::game::valid_id;
use macroquad::{audio, prelude::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    pub widgets: Vec<Widget>,
    pub sounds: BTreeMap<String, Arc<[u8]>>,
    pub hide_movement: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    /// Left, top, width and height normalized to the game viewport.
    pub rect: [f32; 4],
    #[serde(flatten)]
    pub kind: WidgetKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WidgetKind {
    Text {
        text: String,
    },
    Image {
        png: Arc<[u8]>,
    },
    Button {
        id: String,
        text: String,
        #[serde(default)]
        key: Option<UiKey>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<crate::controls::Action>,
    },
}

pub use crate::controls::Key as UiKey;

impl Presentation {
    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty() && self.sounds.is_empty() && !self.hide_movement
    }

    pub fn button_action(&self, button: &str) -> Option<crate::controls::Action> {
        self.widgets.iter().find_map(|widget| match &widget.kind {
            WidgetKind::Button { id, action, .. } if id == button => *action,
            _ => None,
        })
    }

    pub fn has_button(&self, id: &str) -> bool {
        self.widgets.iter().any(
            |widget| matches!(&widget.kind, WidgetKind::Button { id: value, .. } if id == value),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.widgets.len() > 64 || self.sounds.len() > 32 {
            return Err("Use at most 64 UI widgets and 32 sounds per scene.".into());
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for widget in &self.widgets {
            let [x, y, w, h] = widget.rect;
            if ![x, y, w, h].iter().all(|v| (0.0..=1.0).contains(v))
                || w <= 0.
                || h <= 0.
                || x + w > 1.00001
                || y + h > 1.00001
            {
                return Err("UI bounds must fit in the normalized viewport.".into());
            }
            match &widget.kind {
                WidgetKind::Text { text } | WidgetKind::Button { text, .. } => {
                    if text.trim().is_empty()
                        || text.len() > 256
                        || text.chars().any(char::is_control)
                    {
                        return Err("UI text must contain 1–256 bytes of printable text.".into());
                    }
                }
                WidgetKind::Image { png } => validate_png(png)?,
            }
            if let WidgetKind::Button { id, key, .. } = &widget.kind {
                if !valid_id(id) || !ids.insert(id) || key.is_some_and(|key| !keys.insert(key)) {
                    return Err("Use unique valid button IDs and keyboard shortcuts.".into());
                }
            }
        }
        for (id, wav) in &self.sounds {
            if !valid_id(id) || !valid_wav(wav) {
                return Err(format!(
                    "Sound '{id}' must be a PCM mono or stereo WAV under 2 MiB."
                ));
            }
        }
        Ok(())
    }
}

pub fn validate_png(png: &[u8]) -> Result<(), String> {
    if png.len() > 1024 * 1024 || !png.starts_with(b"\x89PNG\r\n\x1a\n") || png.len() < 24 {
        return Err("UI images must be PNG files under 1 MiB.".into());
    }
    let side = |offset| u32::from_be_bytes(png[offset..offset + 4].try_into().expect("PNG header"));
    if &png[12..16] != b"IHDR"
        || ![side(16), side(20)]
            .into_iter()
            .all(|size| (1..=1024).contains(&size))
    {
        return Err("Use PNG images with dimensions from 1 to 1024 pixels.".into());
    }
    Image::from_file_with_format(png, Some(ImageFormat::Png))
        .map_err(|error| format!("Invalid UI PNG: {error}"))?;
    Ok(())
}

fn valid_wav(bytes: &[u8]) -> bool {
    if bytes.len() > 2 * 1024 * 1024 {
        return false;
    }
    let Ok(mut wav) = hound::WavReader::new(std::io::Cursor::new(bytes)) else {
        return false;
    };
    let spec = wav.spec();
    spec.sample_format == hound::SampleFormat::Int
        && spec.bits_per_sample == 16
        && (1..=2).contains(&spec.channels)
        && (8000..=48000).contains(&spec.sample_rate)
        && wav.duration() != 0
        && wav.samples::<i16>().all(|sample| sample.is_ok())
}

pub struct Overlay {
    images: Vec<Option<Texture2D>>,
    sounds: BTreeMap<String, BTreeMap<String, audio::Sound>>,
}

impl Overlay {
    pub fn new(presentation: &Presentation) -> Self {
        let mut overlay = Self {
            images: Vec::new(),
            sounds: BTreeMap::new(),
        };
        overlay.set_scene(presentation);
        overlay
    }

    pub fn set_scene(&mut self, presentation: &Presentation) {
        self.images = presentation
            .widgets
            .iter()
            .map(|widget| match &widget.kind {
                WidgetKind::Image { png } => {
                    Image::from_file_with_format(png, Some(ImageFormat::Png))
                        .ok()
                        .map(|image| Texture2D::from_image(&image))
                }
                _ => None,
            })
            .collect();
    }

    pub fn action(presentation: &Presentation, viewport: Rect) -> Option<&str> {
        Self::action_with_pad(presentation, viewport, false)
    }

    pub fn action_with_pad(
        presentation: &Presentation,
        viewport: Rect,
        confirm: bool,
    ) -> Option<&str> {
        let point = Vec2::from(mouse_position());
        let pressed = is_mouse_button_pressed(MouseButton::Left);
        let taps = if cfg!(target_os = "android") {
            touches()
        } else {
            Vec::new()
        };
        presentation.widgets.iter().rev().find_map(|widget| {
            let WidgetKind::Button { id, key, .. } = &widget.kind else {
                return None;
            };
            let rect = bounds(viewport, widget);
            (key.is_some_and(|key| is_key_pressed(key.code()))
                || (pressed && rect.contains(point))
                || (confirm && presentation.hide_movement && *key == Some(UiKey::Enter))
                || taps
                    .iter()
                    .any(|t| t.phase == TouchPhase::Started && rect.contains(t.position)))
            .then_some(id.as_str())
        })
    }

    pub async fn play(
        &mut self,
        scene: &str,
        presentation: &Presentation,
        ids: Vec<String>,
        volume: f32,
    ) {
        if ids.is_empty() {
            return;
        }
        let sounds = self.sounds.entry(scene.to_owned()).or_default();
        for id in ids {
            if let Some(wav) = presentation.sounds.get(&id) {
                if !sounds.contains_key(&id) {
                    match audio::load_sound_from_bytes(wav).await {
                        Ok(sound) => {
                            sounds.insert(id.clone(), sound);
                        }
                        Err(error) => {
                            macroquad::logging::warn!("Could not play {id}: {error}");
                        }
                    }
                }
                if let Some(sound) = sounds.get(&id) {
                    audio::play_sound(
                        sound,
                        audio::PlaySoundParams {
                            looped: false,
                            volume,
                        },
                    );
                }
            }
        }
    }

    pub fn draw(
        &self,
        presentation: &Presentation,
        viewport: Rect,
        progress: &crate::progress::Progress,
    ) {
        let point = Vec2::from(mouse_position());
        for (widget, image) in presentation.widgets.iter().zip(&self.images) {
            let rect = bounds(viewport, widget);
            match &widget.kind {
                WidgetKind::Text { text } => {
                    if text.contains("{volume}") || text.contains("{checkpoint}") {
                        let value = text
                            .replace("{volume}", &(progress.volume * 100.).round().to_string())
                            .replace(
                                "{checkpoint}",
                                progress.checkpoint.as_deref().unwrap_or("none"),
                            );
                        label(rect, &value, false);
                    } else {
                        label(rect, text, false);
                    }
                }
                WidgetKind::Image { .. } => {
                    if let Some(image) = image {
                        let fit = (rect.w / image.width()).min(rect.h / image.height());
                        let size = vec2(image.width(), image.height()) * fit;
                        let origin = rect.center() - size / 2.;
                        draw_texture_ex(
                            image,
                            origin.x,
                            origin.y,
                            WHITE,
                            DrawTextureParams {
                                dest_size: Some(size),
                                ..Default::default()
                            },
                        );
                    }
                }
                WidgetKind::Button { text, .. } => {
                    let color = if rect.contains(point) {
                        Color::from_rgba(64, 100, 158, 255)
                    } else {
                        Color::from_rgba(34, 59, 102, 250)
                    };
                    draw_rectangle(rect.x, rect.y, rect.w, rect.h, color);
                    draw_rectangle_lines(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        1.,
                        Color::from_rgba(107, 154, 232, 255),
                    );
                    label(rect, text, true);
                }
            }
        }
    }
}

fn bounds(viewport: Rect, widget: &Widget) -> Rect {
    let [x, y, w, h] = widget.rect;
    Rect::new(
        viewport.x + x * viewport.w,
        viewport.y + y * viewport.h,
        w * viewport.w,
        h * viewport.h,
    )
}

fn label(rect: Rect, text: &str, button: bool) {
    let size = (rect.h * if button { 0.54 } else { 0.75 }).clamp(1., 92.) as u16;
    let width = measure_text(text, None, size, 1.).width;
    let size = (f32::from(size) * (rect.w * 0.9 / width.max(1.)).min(1.)).max(1.) as u16;
    let metrics = measure_text(text, None, size, 1.);
    let x = rect.center().x - metrics.width / 2.;
    let y = rect.center().y + metrics.height / 2.;
    draw_text(text, x + 1., y + 1., f32::from(size), BLACK);
    draw_text(text, x, y, f32::from(size), WHITE);
}
