fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => forge_game::launch(false),
        [flag] if flag == "--component-schemas" => {
            let schemas=forge_game::component_schemas()?;
            println!("{}",serde_json::to_string(&schemas).map_err(|e|e.to_string())?);
            Ok(())
        }
        [flag] if flag == "--preview" => forge_game::preview(),
        [flag] if flag == "--check-scene" => forge_game::check_scene(),
        [flag] if flag == "--window-smoke" => forge_game::launch(true),
        [flag, frames] | [flag, frames, _] if flag == "--benchmark" => {
            let frames = frames.parse().map_err(|_| "Expected a number of benchmark frames.")?;
            forge_game::benchmark(frames, args.get(2).map(String::as_str))
        }
        _ => Err("Usage: forge-game [--check-scene | --window-smoke | --benchmark FRAMES [SCENE_ID]]".into()),
    }
}
