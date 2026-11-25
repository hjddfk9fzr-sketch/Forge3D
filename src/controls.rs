use macroquad::prelude::{is_key_down, vec2, KeyCode, Vec2};
use serde::{Deserialize, Serialize};

pub const KEY_NAMES: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z", "Up", "Down", "Left", "Right", "Space",
];
const KEY_CODES: &[KeyCode] = &[
    KeyCode::A,
    KeyCode::B,
    KeyCode::C,
    KeyCode::D,
    KeyCode::E,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::I,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::M,
    KeyCode::N,
    KeyCode::O,
    KeyCode::P,
    KeyCode::Q,
    KeyCode::R,
    KeyCode::S,
    KeyCode::T,
    KeyCode::U,
    KeyCode::V,
    KeyCode::W,
    KeyCode::X,
    KeyCode::Y,
    KeyCode::Z,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Space,
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub forward: String,
    pub backward: String,
    pub left: String,
    pub right: String,
    pub arrows: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            forward: "W".into(),
            backward: "S".into(),
            left: "A".into(),
            right: "D".into(),
            arrows: true,
        }
    }
}

impl Bindings {
    pub fn validate(&self) -> Result<(), String> {
        for key in [&self.forward, &self.backward, &self.left, &self.right] {
            if !KEY_NAMES.contains(&key.as_str()) {
                return Err(format!("Unsupported input key {key}."));
            }
        }
        Ok(())
    }

    pub fn movement(&self) -> Vec2 {
        let pressed = |name: &str, arrow| {
            let selected = KEY_NAMES
                .iter()
                .position(|&key| key == name)
                .is_some_and(|index| is_key_down(KEY_CODES[index]));
            (selected || self.arrows && is_key_down(arrow)) as u8 as f32
        };
        vec2(
            pressed(&self.right, KeyCode::Right) - pressed(&self.left, KeyCode::Left),
            pressed(&self.backward, KeyCode::Down) - pressed(&self.forward, KeyCode::Up),
        )
        .normalize_or_zero()
    }
}
