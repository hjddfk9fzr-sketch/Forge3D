//! Embedded scenes and the runtime scene switcher.
use crate::{
    progress::Progress,
    runtime::{Playback, Scene},
    scripting::Effect,
};
use macroquad::prelude::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Game {
    pub start: String,
    pub scenes: BTreeMap<String, Scene>,
}

impl Game {
    pub fn single(scene: Scene) -> Self {
        Self {
            start: "main".into(),
            scenes: BTreeMap::from([("main".into(), scene)]),
        }
    }

    pub fn start_scene(&self) -> Result<&Scene, String> {
        self.scenes
            .get(&self.start)
            .ok_or_else(|| format!("Unknown starting scene {}.", self.start))
    }

    // The launch scene keeps its existing path so standalone exports and old
    // build snapshots continue to work.
    pub fn decode(start: &[u8], additional: Option<&[u8]>) -> Result<Self, String> {
        let start = Scene::decode(start)?;
        let Some(additional) = additional else {
            return Ok(Self::single(start));
        };
        let mut game: Self =
            serde_json::from_slice(additional).map_err(|error| error.to_string())?;
        if game.scenes.insert(game.start.clone(), start).is_some() {
            return Err("Additional scenes duplicate the starting scene.".into());
        }
        for (id, scene) in &mut game.scenes {
            if !valid_id(id) {
                return Err(format!("Invalid scene ID {id}."));
            }
            scene.assign_ids()?;
            scene.validate()?;
        }
        Ok(game)
    }
}

pub struct GamePlayback {
    game: Game,
    current: String,
    play: Playback,
    errors: Vec<String>,
    progress: Progress,
    progress_changed: bool,
    sounds: Vec<String>,
}

impl GamePlayback {
    pub fn new(game: Game) -> Result<Self, String> {
        Self::with_progress(game, Progress::default())
    }

    pub fn with_progress(game: Game, progress: Progress) -> Result<Self, String> {
        let play = Playback::new(game.start_scene()?.clone())?;
        Ok(Self {
            current: game.start.clone(),
            game,
            play,
            errors: Vec::new(),
            progress,
            progress_changed: false,
            sounds: Vec::new(),
        })
    }

    pub fn scene(&self) -> &Scene {
        self.play.scene()
    }

    pub fn presentation(&self, id: &str) -> &crate::presentation::Presentation {
        &self.game.scenes[id].presentation
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn ui_action(&mut self, button: &str) -> Result<(), String> {
        self.play.ui_action(button)
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn take_progress_changed(&mut self) -> bool {
        std::mem::take(&mut self.progress_changed)
    }

    pub fn take_sounds(&mut self) -> Vec<String> {
        std::mem::take(&mut self.sounds)
    }

    /// Applies the first script request once per rendered frame. Each scene
    /// starts with fresh physics and script state, including when re-entered.
    pub fn tick(&mut self, movement: Vec2, dt: f32) -> bool {
        if !self.play.has_scene_request() {
            self.play.tick(movement, dt);
        }
        self.sounds.clear();
        let mut next = None;
        for effect in self.play.take_effects() {
            match effect {
                Effect::Scene(id) => {
                    next.get_or_insert(id);
                }
                Effect::Save(id) => {
                    if self.game.scenes.contains_key(&id) {
                        if self.progress.checkpoint.as_deref() != Some(&id) {
                            self.progress.checkpoint = Some(id);
                            self.progress_changed = true;
                        }
                    } else {
                        self.errors
                            .push(format!("Unknown checkpoint scene '{id}'."));
                    }
                }
                Effect::Resume => {
                    if next.is_none() {
                        if let Some(id) = self
                            .progress
                            .checkpoint
                            .as_ref()
                            .filter(|id| self.game.scenes.contains_key(*id))
                        {
                            next = Some(id.clone());
                        }
                    }
                }
                Effect::Sound(id) => {
                    if self.scene().presentation.sounds.contains_key(&id) {
                        if self.sounds.len() < 16 && !self.sounds.contains(&id) {
                            self.sounds.push(id);
                        }
                    } else {
                        self.errors.push(format!("Unknown sound '{id}'."));
                    }
                }
                Effect::Volume(step) => {
                    let volume = (self.progress.volume + step).clamp(0., 1.);
                    self.progress_changed |= self.progress.volume != volume;
                    self.progress.volume = volume;
                }
            }
        }
        let Some(next) = next else {
            return false;
        };
        let result = self
            .game
            .scenes
            .get(&next)
            .ok_or_else(|| format!("Unknown scene '{next}'."))
            .and_then(|scene| Playback::new(scene.clone()));
        match result {
            Ok(play) => {
                self.errors.extend(self.play.take_script_errors());
                self.play = play;
                self.current = next;
                true
            }
            Err(error) => {
                self.errors.push(error);
                false
            }
        }
    }

    pub fn take_events(&mut self) -> (Vec<crate::physics::Event>, usize) {
        self.play.take_events()
    }

    pub fn take_script_errors(&mut self) -> Vec<String> {
        self.errors.append(&mut self.play.take_script_errors());
        std::mem::take(&mut self.errors)
    }
}
