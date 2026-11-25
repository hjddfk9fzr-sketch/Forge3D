use macroquad::prelude::{
    get_keys_down, get_keys_pressed, get_keys_released, is_key_down, is_mouse_button_down,
    is_mouse_button_pressed, is_mouse_button_released, mouse_position, mouse_wheel, vec2, KeyCode,
    MouseButton, Vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

macro_rules! game_keys {
    ($($key:ident),+ $(,)?) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum Key { $($key),+ }

        pub const KEY_NAMES: &[&str] = &[$(stringify!($key)),+];
        impl Key {
            pub const ALL: &'static [Self] = &[$(Self::$key),+];

            pub fn code(self) -> KeyCode {
                match self { $(Self::$key => KeyCode::$key),+ }
            }

            pub fn from_code(code: KeyCode) -> Option<Self> {
                match code { $(KeyCode::$key => Some(Self::$key),)+ KeyCode::Unknown => None }
            }

            pub fn name(self) -> &'static str {
                match self { $(Self::$key => stringify!($key)),+ }
            }

            pub fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|key| key.name() == name)
            }
        }
    };
}
game_keys!(
    Space,
    Apostrophe,
    Comma,
    Minus,
    Period,
    Slash,
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    Semicolon,
    Equal,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    LeftBracket,
    Backslash,
    RightBracket,
    GraveAccent,
    World1,
    World2,
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    Right,
    Left,
    Down,
    Up,
    PageUp,
    PageDown,
    Home,
    End,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    F25,
    Kp0,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    KpDecimal,
    KpDivide,
    KpMultiply,
    KpSubtract,
    KpAdd,
    KpEnter,
    KpEqual,
    LeftShift,
    LeftControl,
    LeftAlt,
    LeftSuper,
    RightShift,
    RightControl,
    RightAlt,
    RightSuper,
    Menu,
    Back,
);

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
            let selected = Key::parse(name).is_some_and(|key| is_key_down(key.code()));
            (selected || self.arrows && is_key_down(arrow)) as u8 as f32
        };
        vec2(
            pressed(&self.right, KeyCode::Right) - pressed(&self.left, KeyCode::Left),
            pressed(&self.backward, KeyCode::Down) - pressed(&self.forward, KeyCode::Up),
        )
        .normalize_or_zero()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pointer {
    pub position: [f32; 2],
    pub wheel: [f32; 2],
    pub down: BTreeSet<Button>,
    pub pressed: BTreeSet<Button>,
    pub released: BTreeSet<Button>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputControls {
    pub keys: BTreeSet<Key>,
    pub pressed: BTreeSet<Key>,
    pub released: BTreeSet<Key>,
    pub mouse: Pointer,
}

impl InputControls {
    pub fn capture(origin: Vec2) -> Self {
        let buttons = |test: fn(MouseButton) -> bool| {
            [
                (Button::Left, MouseButton::Left),
                (Button::Middle, MouseButton::Middle),
                (Button::Right, MouseButton::Right),
            ]
            .into_iter()
            .filter_map(|(button, backend)| test(backend).then_some(button))
            .collect()
        };
        Self {
            keys: get_keys_down()
                .into_iter()
                .filter_map(Key::from_code)
                .collect(),
            pressed: get_keys_pressed()
                .into_iter()
                .filter_map(Key::from_code)
                .collect(),
            released: get_keys_released()
                .into_iter()
                .filter_map(Key::from_code)
                .collect(),
            mouse: Pointer {
                position: (Vec2::from(mouse_position()) - origin).to_array(),
                wheel: Vec2::from(mouse_wheel()).to_array(),
                down: buttons(is_mouse_button_down),
                pressed: buttons(is_mouse_button_pressed),
                released: buttons(is_mouse_button_released),
            },
        }
    }

    pub fn clear_edges(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse.pressed.clear();
        self.mouse.released.clear();
        self.mouse.wheel = [0.; 2];
    }

    pub(crate) fn accumulate(&mut self, latest: &Self) {
        self.keys.clone_from(&latest.keys);
        self.pressed.extend(&latest.pressed);
        self.released.extend(&latest.released);
        self.mouse.position = latest.mouse.position;
        self.mouse.down.clone_from(&latest.mouse.down);
        self.mouse.pressed.extend(&latest.mouse.pressed);
        self.mouse.released.extend(&latest.mouse.released);
        for (sum, delta) in self.mouse.wheel.iter_mut().zip(latest.mouse.wheel) {
            *sum += delta;
        }
    }
}
