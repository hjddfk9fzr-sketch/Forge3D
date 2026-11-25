use serde::{Deserialize, Serialize};

pub const SCENE_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
    Sensor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Application {
    pub title: String,
    pub identifier: String,
    pub version: String,
    pub version_code: u32,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub fullscreen: bool,
    pub android_fullscreen: bool,
    pub orientation: Orientation,
}

impl Default for Application {
    fn default() -> Self {
        Self {
            title: String::new(),
            identifier: "com.forge3d.game".into(),
            version: "0.1.0".into(),
            version_code: 1,
            width: 1280,
            height: 720,
            resizable: true,
            fullscreen: false,
            android_fullscreen: true,
            orientation: Orientation::Landscape,
        }
    }
}

impl Application {
    pub fn validate(&self) -> Result<(), String> {
        let parts: Vec<_> = self.identifier.split('.').collect();
        if self.identifier.len() > 150
            || parts.len() < 2
            || !parts.into_iter().all(|part| {
                part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        {
            return Err("Use a reverse-DNS application identifier such as com.studio.game.".into());
        }
        if self.title.chars().count() > 100 || self.title.chars().any(char::is_control) {
            return Err("Use at most 100 printable characters in the application title.".into());
        }
        let version = semver::Version::parse(&self.version)
            .map_err(|_| "Use a semantic game version such as 1.2.0.")?;
        if self.version.len() > 64 || !version.build.is_empty() {
            return Err("Game versions support prerelease tags and up to 64 characters.".into());
        }
        if !(1..=2_100_000_000).contains(&self.version_code) {
            return Err("Android version code must be between 1 and 2100000000.".into());
        }
        if !(320..=7680).contains(&self.width) || !(240..=4320).contains(&self.height) {
            return Err("Window dimensions must be within 320–7680 by 240–4320.".into());
        }
        Ok(())
    }

    pub fn display_title<'a>(&'a self, scene_title: &'a str) -> std::borrow::Cow<'a, str> {
        use std::borrow::Cow;
        let title = if self.title.trim().is_empty() {
            scene_title
        } else {
            &self.title
        };
        if title.trim().is_empty() {
            return Cow::Borrowed("Forge3D Game");
        }
        if title.chars().count() <= 100 && !title.chars().any(char::is_control) {
            return Cow::Borrowed(title);
        }
        let clean: String = title
            .chars()
            .filter(|character| !character.is_control())
            .take(100)
            .collect();
        if clean.trim().is_empty() {
            Cow::Borrowed("Forge3D Game")
        } else {
            Cow::Owned(clean)
        }
    }
}
