use forge3d::{
    compatibility, export,
    game::Game,
    import, jobs,
    presentation::Presentation,
    project,
    runtime::Scene,
    workspace::{Project, EXTENSION},
};
use serde::de::DeserializeOwned;
use std::{
    env,
    ffi::OsStr,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let command = args.first().and_then(|value| value.to_str());
    match (command, args.as_slice()) {
        (Some("--version"), [_]) => {
            println!("forge3d-batch {}", compatibility::ENGINE_VERSION);
            Ok(())
        }
        (Some("info"), [_]) => {
            println!(
                "{}",
                serde_json::to_string(&compatibility::info()).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        (Some("validate"), [_, path]) => {
            let game = load_game(Path::new(path))?;
            export::validate(&game)?;
            println!(
                "Validated {} scene(s), starting with '{}'.",
                game.scenes.len(),
                game.start
            );
            Ok(())
        }
        (Some("package-editor"), [_, target, binary, destination]) => {
            jobs::package_editor(Path::new(binary), Path::new(destination), parse(target)?)
        }
        (Some("demo"), [_, path]) => {
            project::save(&Scene::rendering_demo(), Path::new(path), false)
        }
        (Some("migrate"), [_, source, destination]) => {
            let version = project::migrate(Path::new(source), Path::new(destination))?;
            println!("Scene schema {version}");
            Ok(())
        }
        (Some("export"), [_, source, destination]) => {
            export::collection(&load_game(Path::new(source))?, Path::new(destination))
        }
        (Some("campaign-demo"), [_, path]) => {
            Project::campaign_demo()?.save(Path::new(path), false)
        }
        (Some("project-ui"), [_, path, id, source]) => change_project(path, |project| {
            let id = text(id)?;
            let mut scene = project.scene(id)?;
            let bytes = std::fs::read(source).map_err(|error| error.to_string())?;
            scene.presentation = serde_json::from_slice::<Presentation>(&bytes)
                .map_err(|error| error.to_string())?;
            project.update_scene(id, &scene)
        }),
        (Some("project-demo"), [_, path]) => Project::demo()?.save(Path::new(path), false),
        (Some("project-new"), [_, source, path]) => {
            let scene = project::load(Path::new(source))?;
            Project::new("main", &scene)?.save(Path::new(path), false)
        }
        (Some("project-add"), [_, path, id, source]) => change_project(path, |project| {
            project.add_scene(text(id)?, &project::load(Path::new(source))?)
        }),
        (Some("project-models"), [_, path]) => {
            for (id, model) in Project::load(Path::new(path))?.models() {
                println!("{id}\t{}", model.name);
            }
            Ok(())
        }
        (Some("project-reimport"), [_, path, id, source]) => change_project(path, |project| {
            let id = text(id)?
                .parse()
                .map_err(|_| "Expected a numeric model ID.")?;
            project.reimport_model(id, import::model(Path::new(source))?)
        }),
        (Some("project-list"), [_, path]) => {
            let project = Project::load(Path::new(path))?;
            println!("Start: {}", project.start());
            for (id, title) in project.scenes() {
                println!("{id}\t{title}");
            }
            Ok(())
        }
        (Some("project-start"), [_, path, id]) => {
            change_project(path, |project| project.set_start(text(id)?))
        }
        (Some("prefab-create"), [_, path, id, scene, root]) => change_project(path, |project| {
            let scene = project.scene(text(scene)?)?;
            project.create_prefab(text(id)?, &scene, object_id(root)?)
        }),
        (Some("prefab-insert"), [_, path, id, scene]) => change_project(path, |project| {
            let mut data = project.scene(text(scene)?)?;
            let root = project.instantiate(text(id)?, &mut data, None)?;
            project.update_scene(text(scene)?, &data)?;
            println!("{root}");
            Ok(())
        }),
        (Some("build"), [_, scene, destination, profile]) => run_build(
            Path::new(scene),
            Path::new(destination),
            parse(profile)?,
            None,
        ),
        (Some("build"), [_, scene, destination, profile, target]) => run_build(
            Path::new(scene),
            Path::new(destination),
            parse(profile)?,
            Some(parse(target)?),
        ),
        (Some("retry"), [_, source, destination]) => {
            wait_for_build(|| jobs::Job::retry_export(Path::new(source), Path::new(destination)))
        }
        _ => Err("Usage: forge3d-batch COMMAND
  --version | info | validate SCENE_OR_PROJECT
  demo SCENE | migrate INPUT OUTPUT
  campaign-demo PROJECT | project-demo PROJECT | project-new SCENE PROJECT
  project-add PROJECT ID SCENE | project-ui PROJECT ID UI_JSON
  project-models PROJECT | project-reimport PROJECT MODEL_ID MODEL_FILE
  project-list PROJECT | project-start PROJECT ID
  prefab-create PROJECT PREFAB SCENE_ID OBJECT_ID | prefab-insert PROJECT PREFAB SCENE_ID
  export SCENE_OR_PROJECT DIRECTORY | build SCENE_OR_PROJECT DIRECTORY PROFILE [TARGET]
  retry BUILD_DIRECTORY NEW_DIRECTORY
  package-editor TARGET EXECUTABLE OUTPUT"
            .into()),
    }
}

fn text(argument: &OsStr) -> Result<&str, String> {
    argument
        .to_str()
        .ok_or_else(|| "Expected a text argument.".into())
}

fn object_id(argument: &OsStr) -> Result<u64, String> {
    text(argument)?
        .parse()
        .map_err(|_| "Expected a numeric object ID.".into())
}

fn change_project(
    path: &OsStr,
    change: impl FnOnce(&mut Project) -> Result<(), String>,
) -> Result<(), String> {
    let path = Path::new(path);
    let mut project = Project::load(path)?;
    change(&mut project)?;
    project.save(path, true)
}

fn load_game(path: &Path) -> Result<Game, String> {
    if path.to_string_lossy().ends_with(EXTENSION) {
        Project::load(path)?.game(None)
    } else {
        Ok(Game::single(project::load(path)?))
    }
}

fn parse<T: DeserializeOwned>(argument: &OsStr) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(
        argument.to_str().ok_or("Expected a text argument.")?.into(),
    ))
    .map_err(|error| error.to_string())
}

fn run_build(
    scene: &Path,
    directory: &Path,
    profile: jobs::Profile,
    target: Option<jobs::Target>,
) -> Result<(), String> {
    let game = load_game(scene)?;
    wait_for_build(|| jobs::Job::start_game(game, directory, profile, target))
}

fn wait_for_build(start: impl FnOnce() -> Result<jobs::Job, String>) -> Result<(), String> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal = interrupted.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .map_err(|error| format!("Could not handle Ctrl+C: {error}"))?;
    let mut job = start()?;
    let mut last_stage = "";
    loop {
        if interrupted.swap(false, Ordering::Relaxed) {
            job.cancel();
        }
        if let Some(report) = job.status() {
            println!(
                "{}",
                serde_json::to_string(report).map_err(|error| error.to_string())?
            );
            return if report.state == "succeeded" {
                Ok(())
            } else {
                Err(report
                    .error
                    .as_deref()
                    .unwrap_or("Build failed. See the report.")
                    .into())
            };
        }
        let stage = job.stage().label();
        if stage != last_stage {
            eprintln!("Build · {stage}");
            last_stage = stage;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
