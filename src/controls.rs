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

#[derive(Clone, Copy, Debug, Default)]
pub struct GameActions {
    pub movement: Vec2,
    pub jump: bool,
    pub interact: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Interact,
    Jump,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
}
impl PadButton {
    pub const ALL: [Self; 6] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftShoulder,
        Self::RightShoulder,
    ];
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStick {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub forward: String,
    pub backward: String,
    pub left: String,
    pub right: String,
    pub arrows: bool,
    pub jump: String,
    pub interact: String,
    pub pad_jump: PadButton,
    pub pad_interact: PadButton,
    pub move_stick: MoveStick,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            forward: "W".into(),
            backward: "S".into(),
            left: "A".into(),
            right: "D".into(),
            arrows: true,
            jump: "Space".into(),
            interact: "E".into(),
            pad_jump: PadButton::South,
            pad_interact: PadButton::West,
            move_stick: MoveStick::Left,
        }
    }
}

impl Bindings {
    pub fn validate(&self) -> Result<(), String> {
        for key in [
            &self.forward,
            &self.backward,
            &self.left,
            &self.right,
            &self.jump,
            &self.interact,
        ] {
            if !KEY_NAMES.contains(&key.as_str()) {
                return Err(format!("Unsupported input key {key}."));
            }
        }
        Ok(())
    }

    pub fn move_axis(&self, input: &InputControls) -> Vec2 {
        let down = |key: &str, arrow| {
            (Key::parse(key).is_some_and(|k| input.keys.contains(&k))
                || self.arrows && input.keys.contains(&arrow)) as u8 as f32
        };
        let keys = vec2(
            down(&self.right, Key::Right) - down(&self.left, Key::Left),
            down(&self.backward, Key::Down) - down(&self.forward, Key::Up),
        )
        .normalize_or_zero();
        let stick = Vec2::from_array(match self.move_stick {
            MoveStick::Left => input.left_stick,
            MoveStick::Right => input.right_stick,
        });
        (keys
            + if stick.is_finite() && stick.length_squared() >= 0.04 {
                stick
            } else {
                Vec2::ZERO
            })
        .clamp_length_max(1.)
    }

    pub fn pressed(&self, action: Action, input: &InputControls) -> bool {
        let (key, button) = match action {
            Action::Interact => (&self.interact, self.pad_interact),
            Action::Jump => (&self.jump, self.pad_jump),
        };
        input.actions.contains(&action)
            || Key::parse(key).is_some_and(|k| input.pressed.contains(&k))
            || input.pad_pressed.contains(&button)
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
    pub pad_pressed: BTreeSet<PadButton>,
    pub left_stick: [f32; 2],
    pub right_stick: [f32; 2],
    pub actions: BTreeSet<Action>,
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
            ..Default::default()
        }
    }

    pub fn clear_edges(&mut self) {
        self.actions.clear();
        self.pad_pressed.clear();
        self.pressed.clear();
        self.released.clear();
        self.mouse.pressed.clear();
        self.mouse.released.clear();
        self.mouse.wheel = [0.; 2];
    }

    pub(crate) fn accumulate(&mut self, latest: &Self) {
        self.actions.extend(&latest.actions);
        self.pad_pressed.extend(&latest.pad_pressed);
        self.left_stick = latest.left_stick;
        self.right_stick = latest.right_stick;
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

pub struct Gamepads {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    device: Option<gilrs::Gilrs>,
}
impl Default for Gamepads {
    fn default() -> Self {
        Self {
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            device: gilrs::Gilrs::new().ok(),
        }
    }
}
impl Gamepads {
    pub fn capture(&mut self, origin: Vec2) -> InputControls {
        let mut input = InputControls::capture(origin);
        self.poll(&mut input);
        input
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn poll(&mut self, input: &mut InputControls) {
        use gilrs::{Axis, Button as B, EventType};
        let Some(device) = self.device.as_mut() else {
            return;
        };
        while let Some(event) = device.next_event() {
            if let EventType::ButtonPressed(button, _) = event.event {
                let mapped = match button {
                    B::South => PadButton::South,
                    B::East => PadButton::East,
                    B::West => PadButton::West,
                    B::North => PadButton::North,
                    B::LeftTrigger => PadButton::LeftShoulder,
                    B::RightTrigger => PadButton::RightShoulder,
                    _ => continue,
                };
                input.pad_pressed.insert(mapped);
            }
        }
        for (_, pad) in device.gamepads() {
            for (axes, dest) in [
                ((Axis::LeftStickX, Axis::LeftStickY), &mut input.left_stick),
                (
                    (Axis::RightStickX, Axis::RightStickY),
                    &mut input.right_stick,
                ),
            ] {
                let stick = vec2(pad.value(axes.0), -pad.value(axes.1));
                if stick.length_squared() > Vec2::from_array(*dest).length_squared() {
                    *dest = stick.to_array();
                }
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    fn poll(&mut self, _input: &mut InputControls) {}
}
