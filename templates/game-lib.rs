#[allow(dead_code)]
mod animation;
#[allow(dead_code)]
pub mod controls;
#[allow(dead_code)]
pub mod game;
pub mod gameplay;
#[allow(dead_code)]
mod lighting;
#[allow(dead_code)]
mod performance;
#[allow(dead_code)]
mod physics;
#[allow(dead_code)]
mod presentation;
#[allow(dead_code)]
mod progress;
#[allow(dead_code)]
pub mod runtime;
#[allow(dead_code)]
mod scripting;
#[allow(dead_code)]
mod settings;

const SCENE: &[u8] = include_bytes!("../assets/scene.json");
const SCENES: &[u8] = include_bytes!("../assets/scenes.json");

pub fn check_scene() -> Result<(), String> {
    let game = game::Game::decode(SCENE, Some(SCENES))?;
    for id in game.scenes.keys() {
        let mut candidate = game.clone();
        candidate.start = id.clone();
        let mut play = game::GamePlayback::new(candidate)?;
        for _ in 0..120 {
            play.tick(macroquad::prelude::Vec2::ZERO, 1. / 60.);
            let errors = play.take_script_errors();
            if !errors.is_empty() {
                return Err(format!("{id}: {}", errors.join("\n")));
            }
        }
    }
    Ok(())
}

pub fn launch(window_smoke: bool) -> Result<(), String> {
    launch_mode(window_smoke, None, None)
}

pub fn benchmark(frames: usize, scene: Option<&str>) -> Result<(), String> {
    launch_mode(false, Some(frames), scene)
}

fn launch_mode(window_smoke: bool, frames: Option<usize>, scene_id: Option<&str>) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut game = game::Game::decode(SCENE, Some(SCENES))?;
    let decode_time = started.elapsed();
    if let Some(id) = scene_id {
        game.start = id.into();
    }
    let scene = game.start_scene()?;
    let benchmark = frames.map(|frames| performance::Benchmark::new(frames, started, decode_time)).transpose()?;
    let window = macroquad::prelude::Conf {
        window_title: scene.application.display_title(&scene.title).into(),
        window_width: scene.application.width as i32,
        window_height: scene.application.height as i32,
        window_resizable: scene.application.resizable,
        fullscreen: if cfg!(target_os = "android") {
            scene.application.android_fullscreen
        } else {
            scene.application.fullscreen
        },
        ..Default::default()
    };
    macroquad::Window::from_config(window, runtime::run_measured(game, window_smoke, benchmark));
    Ok(())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn quad_main() {
    if let Err(error) = launch(false) {
        eprintln!("{error}");
    }
}
pub mod workshop;
pub mod relay;

pub mod component;
