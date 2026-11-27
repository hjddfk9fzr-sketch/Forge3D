pub use forge_api::*;
pub use game_code as project_code;

const SCENE: &[u8] = include_bytes!("../assets/scene.json");
const SCENES: &[u8] = include_bytes!("../assets/scenes.json");

pub fn component_schemas() -> Result<Vec<component::Schema>, String> {
    let mut schemas = workshop::component_schemas().to_vec();
    for schema in game_code::schemas() {
        schema.validate_definition()?;
        if schemas.iter().any(|other|other.name==schema.name) {
            return Err(format!("Duplicate component '{}'.",schema.name));
        }
        schemas.push(schema);
    }
    Ok(schemas)
}

pub fn systems() -> Result<gameplay::Systems,String> {
    let mut systems=workshop::game_systems();
    game_code::register(&mut systems)?;
    Ok(systems)
}

pub fn check_scene() -> Result<(), String> {
    let game = game::Game::decode(SCENE, Some(SCENES))?;
    game.validate_schemas(&component_schemas()?)?;
    for id in game.scenes.keys() {
        let mut candidate = game.clone();
        candidate.start = id.clone();
        let mut play = game::GamePlayback::with_systems(candidate,progress::Progress::default(),systems()?)?;
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
    launch_mode(window_smoke, None, None, false)
}

pub fn benchmark(frames: usize, scene: Option<&str>) -> Result<(), String> {
    launch_mode(false, Some(frames), scene, false)
}

pub fn preview() -> Result<(),String> {launch_mode(false,None,None,true)}

fn launch_mode(window_smoke: bool, frames: Option<usize>, scene_id: Option<&str>, preview:bool) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut game = game::Game::decode(SCENE, Some(SCENES))?;
    let decode_time = started.elapsed();
    if let Some(id) = scene_id {
        game.start = id.into();
    }
    game.validate_schemas(&component_schemas()?)?;
    let systems=systems()?;
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
    macroquad::Window::from_config(window, runtime::run_measured_with_systems(game, window_smoke, benchmark, systems, preview));
    Ok(())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn quad_main() {
    if let Err(error) = launch(false) {
        eprintln!("{error}");
    }
}
