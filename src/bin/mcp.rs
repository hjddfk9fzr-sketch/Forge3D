use forge3d::{
    capture, compatibility, controls, export,
    game::{Game, GamePlayback},
    import, jobs, physics, project,
    runtime::{Object, Playback, Scene, ScriptAsset, Shape, View},
    scripting,
    workspace::{OpenProject, Project, EXTENSION},
};
use macroquad::prelude::Vec2;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

#[path = "support/http.rs"]
mod http;

fn supported(version: &str) -> bool {
    matches!(
        version,
        "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25"
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let mut root = None;
    let mut address = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("Expected an option value")?;
        match flag.to_str() {
            Some("--root") if root.is_none() => root = Some(PathBuf::from(value)),
            Some("--http") if address.is_none() => {
                address = Some(value.to_str().ok_or("Invalid HTTP address")?.parse()?)
            }
            _ => return Err("Usage: forge3d-mcp [--root DIRECTORY] [--http ADDRESS:PORT]".into()),
        }
    }
    let root = root.unwrap_or(env::current_dir()?).canonicalize()?;
    if !root.is_dir() {
        return Err("The project root must be a directory.".into());
    }
    if let Some(address) = address {
        return http::serve(root, address);
    }

    let mut session = Session::new(root);
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str(&line) {
            Ok(message) => session.message(message),
            Err(_) => Some(rpc_error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut out, &response)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

struct Session {
    root: PathBuf,
    scene: Scene,
    project: Option<OpenProject>,
    history: project::History,
    builds: BTreeMap<u64, jobs::Job>,
    next_job: u64,
}

impl Session {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            scene: Scene::empty(),
            project: None,
            history: project::History::default(),
            builds: BTreeMap::new(),
            next_job: 1,
        }
    }
    fn message(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = message["method"]
            .as_str()
            .filter(|_| message["jsonrpc"] == "2.0")
        else {
            return Some(rpc_error(id, -32600, "Invalid request"));
        };
        message.get("id")?;
        let params = &message["params"];
        let result = match method {
            "initialize" => {
                let requested = params["protocolVersion"].as_str().unwrap_or_default();
                let protocol = if supported(requested) {
                    requested
                } else {
                    "2025-11-25"
                };
                json!({
                    "protocolVersion": protocol,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "forge3d", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Create or open a scene. Read scene_snapshot for IDs. Save the scene or export a standalone Rust game."
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(rpc_error(id, -32602, "Expected a tool name"));
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.call(name, arguments) {
                    Ok(mut value) => {
                        let image = value
                            .as_object_mut()
                            .and_then(|object| object.remove("image_base64"));
                        let mut content = vec![json!({"type":"text","text":value.to_string()})];
                        if let Some(data) = image {
                            content
                                .push(json!({"type":"image","mimeType":"image/png","data":data}));
                        }
                        json!({"content":content,"structuredContent":value})
                    }
                    Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
                }
            }
            _ => return Some(rpc_error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        if !args.is_object() {
            return Err("Expected an object for arguments.".into());
        }
        match name {
            "scene_snapshot" => Ok(snapshot(&self.scene)),
            "scene_new" => {
                let demo = optional(&args, "demo", false)?;
                let physics_demo = optional(&args, "physics_demo", false)?;
                let rendering_demo = optional(&args, "rendering_demo", false)?;
                let mut scene = match (demo, physics_demo, rendering_demo) {
                    (true, false, false) => Scene::demo(),
                    (false, true, false) => Scene::physics_demo(),
                    (false, false, true) => Scene::rendering_demo(),
                    (false, false, false) => Scene::empty(),
                    _ => return Err("Choose one demo.".into()),
                };
                if let Some(title) = args.get("title") {
                    scene.title = parse(title.clone())?;
                }
                self.commit(scene)?;
                Ok(snapshot(&self.scene))
            }
            "project_new" => {
                let id = optional(&args, "scene", "main".to_owned())?;
                self.project = Some(OpenProject::open(Project::new(&id, &self.scene)?));
                self.project_info()
            }
            "campaign_demo" => {
                self.open_project(Project::campaign_demo()?)?;
                self.project_info()
            }
            "presentation_get" => serde_json::to_value(&self.scene.presentation).map_err(|error| error.to_string()),
            "presentation_set" => self.edit(|scene| {
                let presentation: forge3d::presentation::Presentation = field(&args, "presentation")?;
                presentation.validate()?;
                scene.presentation = presentation;
                Ok(json!({"widgets":scene.presentation.widgets.len(),"sounds":scene.presentation.sounds.keys().collect::<Vec<_>>()}))
            }),
            "project_demo" => {
                self.open_project(Project::demo()?)?;
                self.project_info()
            }
            "engine_info" => {
                serde_json::to_value(compatibility::info()).map_err(|error| error.to_string())
            }
            "project_validate" => {
                let game = self.game()?;
                export::validate(&game)?;
                Ok(json!({"valid":true, "scenes":game.scenes.len(), "starting_scene":game.start}))
            }
            "project_open" => {
                let path = self.read_path(&field::<String>(&args, "path")?)?;
                self.open_project(Project::load(&path)?)?;
                self.project_info()
            }
            "project_info" => self.project_info(),
            "project_close" => {
                self.project = None;
                self.history = project::History::default();
                Ok(snapshot(&self.scene))
            }
            "project_save" => {
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                if !path.to_string_lossy().ends_with(EXTENSION) {
                    return Err(format!("Project filenames must end in {EXTENSION}."));
                }
                let active = self.project.as_mut().ok_or("Open or create a project first.")?;
                let snapshot = active.snapshot(&self.scene)?;
                snapshot.save(&path, optional(&args, "overwrite", false)?)?;
                active.project = snapshot;
                Ok(json!({"path":path}))
            }
            "project_add_scene" => {
                let id: String = field(&args, "scene")?;
                let source: Option<String> = optional(&args, "source", None)?;
                let scene = match source {
                    Some(source) => project::load(&self.read_path(&source)?)?,
                    None => {
                        let mut scene = Scene::empty();
                        scene.title = id.clone();
                        scene.application = self.scene.application.clone();
                        scene
                    }
                };
                self.edit_project()?.project.add_scene(&id, &scene)?;
                self.project_info()
            }
            "project_select_scene" => {
                let id: String = field(&args, "scene")?;
                let project = self.project.as_mut().ok_or("Open or create a project first.")?;
                self.scene = project.select(&id, &self.scene)?;
                self.history = project::History::default();
                Ok(snapshot(&self.scene))
            }
            "project_start" => {
                let id: String = field(&args, "scene")?;
                self.edit_project()?.project.set_start(&id)?;
                self.project_info()
            }
            "prefab_create" => {
                let name: String = field(&args, "prefab")?;
                let root = field(&args, "root")?;
                let project = self.project.as_mut().ok_or("Open or create a project first.")?;
                project.project.create_prefab(&name, &self.scene, root)?;
                self.project_info()
            }
            "prefab_instantiate" => {
                let name: String = field(&args, "prefab")?;
                let parent = optional(&args, "parent", None)?;
                let project = self.project.as_ref().ok_or("Open or create a project first.")?;
                let mut scene = self.scene.clone();
                let root = project.project.instantiate(&name, &mut scene, parent)?;
                self.commit(scene)?;
                Ok(json!({"root":root}))
            }
            "project_use_asset" => {
                let kind: String = field(&args, "kind")?;
                let id = field(&args, "asset_id")?;
                let project = self.project.as_ref().ok_or("Open or create a project first.")?;
                let mut scene = self.scene.clone();
                let index = match kind.as_str() {
                    "model" => project.project.use_model(id, &mut scene)?,
                    "script" => project.project.use_script(id, &mut scene)?,
                    _ => return Err("Unknown shared asset kind.".into()),
                };
                self.commit(scene)?;
                Ok(json!({"index":index,"asset_id":id}))
            }
            "project_simulate" => {
                let (frames, dt, input) = simulation_input(&args, 60)?;
                let mut play = GamePlayback::new(self.game()?)?;
                let actions: Vec<UiAction> = optional(&args, "actions", Vec::new())?;
                if actions.len() > 64 || actions.iter().any(|action| action.frame >= frames) {
                    return Err("Use up to 64 UI actions at frame indices inside the simulation.".into());
                }
                let mut visited = vec![play.current().to_owned()];
                let mut errors = Vec::new();
                let mut sounds = Vec::new();
                let mut controls: controls::InputControls = optional(&args, "controls", Default::default())?;
                for frame in 0..frames {
                    for action in actions.iter().filter(|action| action.frame == frame) {
                        play.ui_action(&action.button)?;
                    }
                    if play.tick_input(input, &controls, dt) {
                        visited.push(play.current().to_owned());
                    }
                    controls.clear_edges();
                    errors.extend(play.take_script_errors());
                    sounds.extend(play.take_sounds().into_iter().take(256_usize.saturating_sub(sounds.len())));
                    play.take_events();
                }
                errors.extend(play.take_script_errors());
                Ok(json!({"frames":frames,"scene":play.current(),"visited":visited,"snapshot":snapshot(play.scene()),"script_errors":errors,"progress":play.progress(),"sounds":sounds}))
            }
            "scene_open" => {
                let path = self.read_path(&field::<String>(&args, "path")?)?;
                self.commit(project::load(&path)?)?;
                Ok(snapshot(&self.scene))
            }
            "scene_migrate" => {
                let source = self.read_path(&field::<String>(&args,"source")?)?;
                let destination = self.output_path(&field::<String>(&args,"destination")?)?;
                if !destination.to_string_lossy().ends_with(".forge.json") {
                    return Err("Scene filenames must end in .forge.json.".into());
                }
                let version = project::migrate(&source,&destination)?;
                Ok(json!({"version":version,"path":destination}))
            }
            "scene_save" => {
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                if !path.to_string_lossy().ends_with(".forge.json") {
                    return Err("Scene filenames must end in .forge.json.".into());
                }
                project::save(&self.scene, &path, optional(&args, "overwrite", false)?)?;
                Ok(json!({"path": path}))
            }
            "scene_settings" => self.edit(|scene| {
                if let Some(title) = args.get("title") {
                    scene.title = parse(title.clone())?;
                }
                if let Some(application) = args.get("application") {
                    let mut settings = serde_json::to_value(&scene.application).map_err(|error| error.to_string())?;
                    for (key,value) in application.as_object().ok_or("Application settings must be an object.")? {
                        settings[key] = value.clone();
                    }
                    scene.application = parse(settings)?;
                }
                if let Some(camera) = args.get("camera") {
                    let view: View = parse(camera.clone())?;
                    if !(-89. ..=89.).contains(&view.pitch)
                        || !(0.1..=500.).contains(&view.distance)
                    {
                        return Err(
                            "Camera pitch must be within -89..89 and distance within 0.1..500."
                                .into(),
                        );
                    }
                    scene.camera = view;
                }
                if let Some(gravity) = args.get("gravity") { scene.gravity = parse(gravity.clone())?; }
                if let Some(lighting) = args.get("lighting") {
                    let mut settings = serde_json::to_value(&scene.lighting).map_err(|error| error.to_string())?;
                    for (key,value) in lighting.as_object().ok_or("Lighting must be an object.")? {
                        settings[key] = value.clone();
                    }
                    scene.lighting = parse(settings)?;
                }
                if let Some(bindings) = args.get("bindings") {
                    let mut settings = serde_json::to_value(&scene.bindings).map_err(|error| error.to_string())?;
                    for (key,value) in bindings.as_object().ok_or("Bindings must be an object.")? {
                        settings[key] = value.clone();
                    }
                    scene.bindings = parse(settings)?;
                }
                if let Some(id) = args.get("camera_controller") { scene.camera_controller = parse(id.clone())?; }
                Ok(json!({"title":scene.title,"version":scene.version,"application":scene.application,"camera":scene.camera,"gravity":scene.gravity,"lighting":scene.lighting,"bindings":scene.bindings,"camera_controller":scene.camera_controller}))
            }),
            "object_create" => self.edit(|scene| {
                let shape = field(&args, "shape")?;
                let id = scene.add_object(Object::new(field::<String>(&args, "name")?, shape))?;
                patch_object(scene, id, &args)
            }),
            "object_update" => self.edit(|scene| patch_object(scene, field(&args, "id")?, &args)),
            "object_duplicate" => self.edit(|scene| {
                let id = field(&args, "id")?;
                let root = project::duplicate_object(scene, id)?;
                Ok(json!({"root":root}))
            }),
            "object_delete" => self.edit(|scene| {
                let id = field(&args, "id")?;
                scene.delete_object(id, optional(&args, "recursive", false)?)?;
                Ok(json!({"deleted": id}))
            }),
            "import_model" => {
                let path = self.read_path(&field::<String>(&args, "path")?)?;
                let model = import::model_under(&path, &self.root)?;
                self.edit(move |scene| {
                    let index = scene.models.len();
                    let name = model.name.clone();
                    scene.models.push(model);
                    let id = if optional(&args, "instantiate", true)? {
                        Some(scene.add_object(Object::new(&name, Shape::Model(index)))?)
                    } else {
                        None
                    };
                    Ok(json!({"model":index,"name":name,"object_id":id}))
                })
            }
            "script_check" => {
                scripting::validate_source(&field::<String>(&args, "source")?)?;
                Ok(json!({"valid":true}))
            }
            "script_asset_set" => {
                let name = field(&args, "name")?;
                let source = match (args.get("source"), args.get("path")) {
                    (Some(source), None) => parse(source.clone())?,
                    (None, Some(path)) => {
                        fs::read_to_string(self.read_path(&parse::<String>(path.clone())?)?)
                            .map_err(|error| error.to_string())?
                    }
                    _ => return Err("Supply exactly one of source or path.".into()),
                };
                scripting::validate_source(&source)?;
                self.edit(|scene| {
                    let script = ScriptAsset { name, source };
                    let index = match args.get("index") {
                        None => {
                            let index = scene.scripts.len();
                            scene.scripts.push(script);
                            index
                        }
                        Some(index) => {
                            let index: usize = parse(index.clone())?;
                            *scene
                                .scripts
                                .get_mut(index)
                                .ok_or("Unknown script asset.")? = script;
                            index
                        }
                    };
                    Ok(json!({"asset":index}))
                })
            }
            "asset_remove" => self.edit(|scene| {
                let index = field(&args, "index")?;
                let kind = field::<String>(&args, "kind")?;
                match kind.as_str() {
                    "model" => scene.remove_model(index)?,
                    "script" => scene.remove_script(index)?,
                    _ => return Err("Asset kind must be model or script.".into()),
                }
                Ok(json!({"removed":index,"kind":kind}))
            }),
            "project_list" => {
                let path = self
                    .root
                    .join(optional(&args, "path", ".".to_owned())?)
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                if !path.starts_with(&self.root) || !path.is_dir() {
                    return Err("Expected a directory inside the project root.".into());
                }
                Ok(
                    json!({"path":path.strip_prefix(&self.root).unwrap_or(&path),"entries":project::entries(&path)?}),
                )
            }
            "simulate" | "test_scene" => {
                let (frames, mut play) = playback(&self.scene, &args, 1)?;
                let errors = play.take_script_errors();
                let (events, dropped_events) = play.take_events();
                if name == "test_scene" {
                    let mut failures = assertions(
                        play.scene(),
                        optional(&args, "assertions", Vec::<Assertion>::new())?,
                    )?;
                    let expected: Vec<physics::Event> = optional(&args, "expect_events", Vec::new())?;
                    if expected.len() > 64 { return Err("Use at most 64 event assertions.".into()); }
                    let requires_events = !expected.is_empty();
                    for expectation in expected {
                        if !events.iter().any(|event| event.kind == expectation.kind &&
                            ((event.a,event.b) == (expectation.a,expectation.b)
                                || (event.b,event.a) == (expectation.a,expectation.b))) {
                            failures.push(json!({"field":"event","expected":expectation}));
                        }
                    }
                    return Ok(json!({"passed":errors.is_empty() && failures.is_empty() && (!requires_events || dropped_events == 0),"frames":frames,"failures":failures,"script_errors":errors,"events":events,"dropped_events":dropped_events}));
                }
                Ok(json!({"frames":frames,"objects":play.scene().objects,"camera":play.scene().camera,"events":events,"dropped_events":dropped_events,"script_errors":errors}))
            }
            "scene_capture" => {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                if path.extension().is_none_or(|extension| extension != "png") {
                    return Err("Capture filename must end in .png.".into());
                }
                let inline = optional(&args, "inline", true)?;
                let width = optional(&args, "width", 640_u32)?;
                let height = optional(&args, "height", 360_u32)?;
                let (frames, mut play) = playback(&self.scene, &args, 0)?;
                let errors = play.take_script_errors();
                let image = capture::png(play.scene(), width, height)?;
                project::write_new(&path, &image)?;
                let mut value = json!({"path":path,"width":width,"height":height,"frames":frames,"script_errors":errors});
                if inline {
                    value["image_base64"] = json!(STANDARD.encode(image));
                }
                Ok(value)
            }
            "build_game" => {
                let profile = field(&args, "profile")?;
                let target = optional(&args,"target",None)?;
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                let evicted = self.build_eviction(None)?;
                let job = jobs::Job::start_game(self.game()?, &path, profile, target)?;
                Ok(self.register_build(job, evicted))
            }
            "build_status" => {
                let id: u64 = field(&args, "job")?;
                let job = self.builds.get_mut(&id).ok_or("Unknown build job.")?;
                let path = job.directory().to_owned();
                Ok(match job.status() {
                    Some(report) => {
                        json!({"job":id,"state":report.state,"stage":report.stage,"path":path,"report":report})
                    }
                    None => json!({"job":id,"state":"running","stage":job.stage(),"path":path}),
                })
            }
            "build_cancel" => {
                let id: u64 = field(&args, "job")?;
                let job = self.builds.get_mut(&id).ok_or("Unknown build job.")?;
                Ok(json!({"job":id,"cancel_requested":job.cancel()}))
            }
            "build_retry" => {
                let id: u64 = field(&args, "job")?;
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                let previous = self.builds.get_mut(&id).ok_or("Unknown build job.")?;
                if previous.status().is_none() {
                    return Err("Wait for the build to finish before retrying.".into());
                }
                let evicted = self.build_eviction(Some(id))?;
                let job = self.builds.get_mut(&id).ok_or("Unknown build job.")?.retry(&path)?;
                Ok(self.register_build(job, evicted))
            }
            "artifact_read" => {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let id = field::<u64>(&args, "job")?;
                let index = field::<usize>(&args, "index")?;
                let offset = optional(&args, "offset", 0_u64)?;
                let limit = optional(&args, "limit", 262_144_u64)?;
                if !(1..=524_288).contains(&limit) {
                    return Err("Read between 1 and 524288 bytes.".into());
                }
                let job = self.builds.get_mut(&id).ok_or("Unknown build job.")?;
                let directory = job.directory().to_owned();
                let report = job.status().ok_or("The build is still running.")?;
                let path = report
                    .artifacts
                    .get(index)
                    .ok_or("Unknown build artifact.")?
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                if !path.starts_with(directory) || !path.is_file() {
                    return Err("The build artifact is unavailable.".into());
                }
                let mut file = fs::File::open(&path).map_err(|error| error.to_string())?;
                let total = file.metadata().map_err(|error| error.to_string())?.len();
                if offset > total {
                    return Err("Offset exceeds artifact size.".into());
                }
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                file.take(limit)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                let next = offset + bytes.len() as u64;
                Ok(
                    json!({"name":path.file_name(),"offset":offset,"next_offset":next,"size":total,"eof":next>=total,"base64":STANDARD.encode(bytes)}),
                )
            }
            "undo" | "redo" => {
                let changed = if name == "undo" {
                    self.history.undo(&mut self.scene)
                } else {
                    self.history.redo(&mut self.scene)
                };
                Ok(
                    json!({"changed": changed,"can_undo": self.history.can_undo(),"can_redo": self.history.can_redo()}),
                )
            }
            "export_game" => {
                let path = self.output_path(&field::<String>(&args, "path")?)?;
                export::collection(&self.game()?, &path)?;
                Ok(json!({"path":path,"manifest":path.join("Cargo.toml")}))
            }
            _ => Err(format!("Unknown tool {name}.")),
        }
    }

    fn open_project(&mut self, project: Project) -> Result<(), String> {
        let open = OpenProject::open(project);
        self.scene = open.project.scene(&open.active)?;
        self.project = Some(open);
        self.history = project::History::default();
        Ok(())
    }

    fn edit_project(&mut self) -> Result<&mut OpenProject, String> {
        self.project
            .as_mut()
            .ok_or_else(|| "Open or create a project first.".into())
    }

    fn project_info(&mut self) -> Result<Value, String> {
        let open = self
            .project
            .as_mut()
            .ok_or("Open or create a project first.")?;
        open.project.update_scene(&open.active, &self.scene)?;
        let project = &open.project;
        Ok(json!({
            "active":open.active, "start":project.start(),
            "scenes":project.scenes().map(|(id,title)| json!({"id":id,"title":title})).collect::<Vec<_>>(),
            "prefabs":project.prefabs().collect::<Vec<_>>(),
            "models":project.models().iter().map(|(id, model)| json!({"id":id,"name":model.name})).collect::<Vec<_>>(),
            "scripts":project.scripts().iter().map(|(id, script)| json!({"id":id,"name":script.name})).collect::<Vec<_>>(),
        }))
    }

    fn game(&self) -> Result<Game, String> {
        match &self.project {
            Some(project) => project.snapshot(&self.scene)?.game(None),
            None => Ok(Game::single(self.scene.clone())),
        }
    }

    fn edit(
        &mut self,
        operation: impl FnOnce(&mut Scene) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let mut candidate = self.scene.clone();
        let value = operation(&mut candidate)?;
        self.commit(candidate)?;
        Ok(value)
    }

    fn commit(&mut self, candidate: Scene) -> Result<(), String> {
        candidate.validate_objects()?;
        if candidate != self.scene {
            self.history
                .record(std::mem::replace(&mut self.scene, candidate));
        }
        Ok(())
    }

    fn read_path(&self, path: &str) -> Result<PathBuf, String> {
        let path = self
            .root
            .join(path)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err("Expected a file inside the project root.".into());
        }
        Ok(path)
    }

    fn build_eviction(&mut self, previous: Option<u64>) -> Result<Option<u64>, String> {
        if self.builds.len() >= 16 {
            let mut finished = self
                .builds
                .iter_mut()
                .filter_map(|(id, running)| running.status().is_some().then_some(*id));
            let remove = finished.find(|id| Some(*id) != previous).or(previous);
            return remove
                .map(Some)
                .ok_or("Too many build jobs in this session.".into());
        }
        Ok(None)
    }

    fn register_build(&mut self, job: jobs::Job, evicted: Option<u64>) -> Value {
        if let Some(evicted) = evicted {
            self.builds.remove(&evicted);
        }
        let id = self.next_job;
        self.next_job += 1;
        let response =
            json!({"job":id,"state":"running","stage":job.stage(),"path":job.directory()});
        self.builds.insert(id, job);
        response
    }

    fn output_path(&self, path: &str) -> Result<PathBuf, String> {
        let path = self.root.join(path);
        let name = path.file_name().ok_or("Enter a destination name.")?;
        let parent = path
            .parent()
            .ok_or("Enter a destination inside the project root.")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !parent.starts_with(&self.root) {
            return Err("Write destinations must be inside the project root.".into());
        }
        let destination = parent.join(name);
        if fs::symlink_metadata(&destination).is_ok_and(|info| info.is_symlink()) {
            return Err("Use a destination without a symbolic link.".into());
        }
        Ok(destination)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UiAction {
    frame: u64,
    button: String,
}

fn simulation_input(args: &Value, default: u64) -> Result<(u64, f32, Vec2), String> {
    let frames = optional(args, "frames", default)?;
    let dt = optional(args, "dt", 1_f32 / 60.)?;
    if frames > 600 || !(0.0..=0.1).contains(&dt) {
        return Err("Use up to 600 frames and a frame duration between 0 and 0.1 seconds.".into());
    }
    let input = Vec2::from_array(optional(args, "input", [0_f32; 2])?);
    if !input.is_finite() {
        return Err("Input must be finite.".into());
    }
    Ok((frames, dt, input.clamp_length_max(1.)))
}

fn playback(scene: &Scene, args: &Value, default: u64) -> Result<(u64, Playback), String> {
    let (frames, dt, input) = simulation_input(args, default)?;
    let mut play = Playback::new(scene.clone())?;
    let mut controls: controls::InputControls = optional(args, "controls", Default::default())?;
    for _ in 0..frames {
        play.tick_input(input, &controls, dt);
        controls.clear_edges();
    }
    Ok((frames, play))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assertion {
    id: u64,
    position: Option<[f32; 3]>,
    rotation: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
    visible: Option<bool>,
    tolerance: Option<f32>,
}

fn assertions(scene: &Scene, assertions: Vec<Assertion>) -> Result<Vec<Value>, String> {
    if assertions.len() > 64 {
        return Err("Use at most 64 assertions.".into());
    }
    let mut failures = Vec::new();
    for assertion in assertions {
        let tolerance = assertion.tolerance.unwrap_or(0.001);
        if !(0.0..=100.).contains(&tolerance) {
            return Err("Invalid assertion tolerance.".into());
        }
        let index = scene.object_index(assertion.id)?;
        let object = &scene.objects[index];
        for (field, expected, actual) in [
            ("position", assertion.position, object.position),
            ("rotation", assertion.rotation, object.rotation),
            ("scale", assertion.scale, object.scale),
        ] {
            if let Some(expected) = expected {
                if expected
                    .into_iter()
                    .zip(actual)
                    .any(|(one, two)| (one - two).abs() > tolerance)
                {
                    failures.push(json!({"id":assertion.id,"field":field,"expected":expected,"actual":actual}));
                }
            }
        }
        if assertion
            .visible
            .is_some_and(|expected| expected != object.visible)
        {
            failures.push(json!({"id":assertion.id,"field":"visible","expected":assertion.visible,"actual":object.visible}));
        }
    }
    Ok(failures)
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Attachment {
    Inline { source: String },
    Shared { asset: usize },
}

fn patch_object(scene: &mut Scene, id: u64, args: &Value) -> Result<Value, String> {
    let index = scene.object_index(id)?;
    if let Some(body) = args.get("body") {
        scene.objects[index].body = parse(body.clone())?;
    }
    if let Some(parent) = args.get("parent") {
        scene.set_parent(id, parse(parent.clone())?)?;
    }
    let object = &mut scene.objects[index];
    if let Some(value) = args.get("name") {
        object.name = parse(value.clone())?;
    }
    if let Some(value) = args.get("shape") {
        object.shape = parse(value.clone())?;
    }
    if let Some(value) = args.get("position") {
        object.position = parse(value.clone())?;
    }
    if let Some(value) = args.get("rotation") {
        object.rotation = parse(value.clone())?;
    }
    if let Some(value) = args.get("scale") {
        object.scale = parse(value.clone())?;
    }
    if let Some(value) = args.get("visible") {
        object.visible = parse(value.clone())?;
    }
    if let Some(value) = args.get("spin") {
        object.spin = parse(value.clone())?;
    }
    if let Some(value) = args.get("speed") {
        object.speed = parse(value.clone())?;
    }
    if let Some(value) = args.get("animation") {
        object.animation = parse(value.clone())?;
    }
    if let Some(value) = args.get("surface") {
        object.surface = parse(value.clone())?;
    }
    if let Some(value) = args.get("light") {
        object.light = parse(value.clone())?;
    }
    if let Some(value) = args.get("cast_shadows") {
        object.cast_shadows = parse(value.clone())?;
    }
    if let Some(value) = args.get("script") {
        object.script = None;
        object.script_asset = None;
        match parse::<Option<Attachment>>(value.clone())? {
            None => {}
            Some(Attachment::Inline { source }) => {
                scripting::validate_source(&source)?;
                object.script = Some(source);
            }
            Some(Attachment::Shared { asset }) => object.script_asset = Some(asset),
        }
    }
    Ok(json!({"object": object}))
}

fn snapshot(scene: &Scene) -> Value {
    let models: Vec<_> = scene
        .models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let triangles: usize = model.parts.iter().map(|part| part.indices.len() / 3).sum();
            let clips: Vec<_> = model.rig.iter().flat_map(|rig| rig.clips.iter())
                .map(|clip| json!({"name":clip.name,"duration":clip.duration})).collect();
            let skins = model.rig.as_ref().map_or(0, |rig| rig.skins.len());
            json!({"index":index,"name":model.name,"triangles":triangles,"clips":clips,"skins":skins})
        })
        .collect();
    json!({"title":scene.title,"version":scene.version,"application":scene.application,"camera":scene.camera,"gravity":scene.gravity,"lighting":scene.lighting,"bindings":scene.bindings,"camera_controller":scene.camera_controller,"objects":scene.objects,"models":models,"scripts":scene.scripts})
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn field<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    parse(
        args.get(name)
            .cloned()
            .ok_or_else(|| format!("Missing argument {name}."))?,
    )
    .map_err(|error| format!("{name}: {error}"))
}

fn optional<T: serde::de::DeserializeOwned>(
    args: &Value,
    name: &str,
    fallback: T,
) -> Result<T, String> {
    if args.get(name).is_some() {
        field(args, name)
    } else {
        Ok(fallback)
    }
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{
        "type":"object","properties":properties,"required":required,"additionalProperties":false
    }})
}

fn tools() -> Vec<Value> {
    let string = json!({"type":"string"});
    let boolean = json!({"type":"boolean"});
    let index = json!({"type":"integer","minimum":0});
    let path = json!({"type":"string","description":"Path inside the project root."});
    let vec3 = json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}});
    let id = json!({"type":"integer","minimum":1});
    let key = json!({"enum":controls::KEY_NAMES});
    let key_names: Vec<_> = controls::KEY_NAMES
        .iter()
        .map(|key| key.to_ascii_lowercase())
        .collect();
    let key_set = json!({"type":"array","uniqueItems":true,"items":{"enum":key_names}});
    let buttons =
        json!({"type":"array","uniqueItems":true,"items":{"enum":["left","middle","right"]}});
    let pair = json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}});
    let controls = json!({"type":"object","additionalProperties":false,"properties":{
        "keys":key_set,"pressed":key_set,"released":key_set,
        "mouse":{"type":"object","additionalProperties":false,"properties":{
            "position":pair,"wheel":pair,"down":buttons,"pressed":buttons,"released":buttons
        }}
    }});
    let bindings = json!({"type":"object","properties":{"forward":key,"backward":key,"left":key,"right":key,"arrows":boolean},"additionalProperties":false});
    let body = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{
        "kind":{"enum":["static","dynamic","kinematic"]},"collider":{"enum":["box","sphere"]},
        "size":vec3,"sensor":boolean,"gravity_scale":{"type":"number","minimum":-10,"maximum":10},
        "friction":{"type":"number","minimum":0,"maximum":2},"restitution":{"type":"number","minimum":0,"maximum":1},"lock_rotation":boolean
    },"additionalProperties":false}]});
    let expect_events = json!({"type":"array","maxItems":64,"items":{"type":"object","properties":{
        "kind":{"enum":["collision_enter","collision_exit","trigger_enter","trigger_exit"]},"a":id,"b":id
    },"required":["kind","a","b"],"additionalProperties":false}});
    let number = json!({"type":"number"});
    let vec4 = json!({"type":"array","minItems":4,"maxItems":4,"items":number});
    let surface = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{
        "base_color":vec4,"metallic":number,"roughness":number,"emissive":vec3,"unlit":boolean,
        "alpha":{"enum":["opaque","mask","blend"]},"alpha_cutoff":number
    },"additionalProperties":false}]});
    let point = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{
        "color":vec3,"intensity":number,"range":number
    },"additionalProperties":false}]});
    let animation = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{
        "clip":string,"time":number,"speed":number,"playing":boolean,"looping":boolean
    },"required":["clip"],"additionalProperties":false}]});
    let application = json!({"type":"object","properties":{
        "title":string,"identifier":string,"version":string,"version_code":{"type":"integer","minimum":1,"maximum":2100000000},
        "width":{"type":"integer","minimum":320,"maximum":7680},"height":{"type":"integer","minimum":240,"maximum":4320},
        "resizable":boolean,"fullscreen":boolean,"android_fullscreen":boolean,
        "orientation":{"enum":["landscape","portrait","sensor"]}
    },"additionalProperties":false});
    let light_settings = json!({"type":"object","properties":{"direction":vec3,"color":vec3,
        "intensity":number,"ambient":number,"shadows":boolean,"shadow_extent":number},"additionalProperties":false});
    let patch = json!({
        "name":string,
        "shape":{"anyOf":[{"enum":["Empty","Cube","Sphere","Plane"]},{"type":"object","properties":{"Model":index},"required":["Model"],"additionalProperties":false}]},
        "parent":{"type":["integer","null"],"minimum":1,"description":"Parent object ID or null for the root. Local transform is preserved."},
        "position":vec3,"rotation":vec3,"scale":vec3,"visible":boolean,
        "spin":{"type":"number"},"speed":{"type":"number"},"body":body,"animation":animation,"surface":surface,"light":point,"cast_shadows":boolean,
        "script":{"anyOf":[{"type":"null"},{"type":"object","properties":{"source":string},"required":["source"],"additionalProperties":false},{"type":"object","properties":{"asset":index},"required":["asset"],"additionalProperties":false}]}
    });
    let mut update = patch.clone();
    update["id"] = json!({"type":"integer","minimum":1});
    let frame = json!({"type":"integer","minimum":0,"maximum":600});
    let dt = json!({"type":"number","minimum":0,"maximum":0.1});
    let axes = json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}});
    let pixels = json!({"type":"integer","minimum":64,"maximum":1024});
    vec![
        tool("project_list","List directories, projects, scenes, models and scripts.",json!({"path":path}),&[]),
        tool("engine_info","Read the Forge3D version and supported project, scene, progress, Rhai and MCP API generations.",json!({}),&[]),
        tool("project_validate","Check export requirements, including scripts in all scenes, without writing files.",json!({}),&[]),
        tool("project_new","Create a portable project from the current scene.",json!({"scene":string}),&[]),
        tool("campaign_demo","Open Workshop Run, the menu and two-level game.",json!({}),&[]),
        tool("presentation_get","Read the active scene's UI and audio definitions.",json!({}),&[]),
        tool("presentation_set","Replace and validate the active scene's UI widgets and sounds.",json!({"presentation":{"type":"object"}}),&["presentation"]),
        tool("project_demo","Open the demonstration project with two scenes and a prefab.",json!({}),&[]),
        tool("project_open","Open a .forge-project.json manifest.",json!({"path":path}),&["path"]),
        tool("project_info","List project scene IDs, the start scene, prefabs and shared assets.",json!({}),&[]),
        tool("project_close","Close the project and keep the active scene as a standalone working scene.",json!({}),&[]),
        tool("project_save","Save the current project and its active scene.",json!({"path":path,"overwrite":boolean}),&["path"]),
        tool("project_add_scene","Add an empty scene or import an existing standalone scene.",json!({"scene":string,"source":path}),&["scene"]),
        tool("project_select_scene","Commit the working scene and edit a different project scene.",json!({"scene":string}),&["scene"]),
        tool("project_start","Set the scene used when the project game starts.",json!({"scene":string}),&["scene"]),
        tool("prefab_create","Store an object subtree as a reusable project prefab.",json!({"prefab":string,"root":id}),&["prefab","root"]),
        tool("prefab_instantiate","Copy a prefab into the active scene and assign fresh object IDs.",json!({"prefab":string,"parent":{"type":["integer","null"],"minimum":1}}),&["prefab"]),
        tool("project_use_asset","Add a shared asset by project ID and return its slot in the active scene.",json!({"kind":{"enum":["model","script"]},"asset_id":id}),&["kind","asset_id"]),
        tool("project_simulate","Simulate scene transitions, button actions and in-memory progress.",json!({"frames":frame,"dt":dt,"input":axes,"controls":controls,"actions":{"type":"array","maxItems":64,"items":{"type":"object","properties":{"frame":frame,"button":string},"required":["frame","button"],"additionalProperties":false}}}),&[]),
        tool("scene_capture","Capture a PNG preview of the game camera, optionally after simulation.",json!({"path":path,"width":pixels,"height":pixels,"frames":frame,"dt":dt,"input":axes,"controls":controls,"inline":boolean}),&["path"]),
        tool("test_scene","Simulate the scene and check local object transforms or script errors.",json!({"frames":frame,"dt":dt,"input":axes,"controls":controls,"expect_events":expect_events,"assertions":{"type":"array","maxItems":64,"items":{"type":"object","required":["id"],"properties":{"id":{"type":"integer","minimum":1},"position":vec3,"rotation":vec3,"scale":vec3,"visible":boolean,"tolerance":{"type":"number","minimum":0,"maximum":100}},"additionalProperties":false}}}),&[]),
        tool("build_game","Export a fresh game and start a build or toolchain check in the background.",json!({"path":path,"profile":{"enum":["check","test","debug","release","android-debug","android-release","android-bundle"]},"target":{"enum":["windows-x64","macos-arm64","linux-x64",null]}}),&["path","profile"]),
        tool("artifact_read","Read a build artifact in base64 chunks, including from remote MCP clients.",json!({"job":{"type":"integer","minimum":1},"index":index,"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":524288}}),&["job","index"]),
        tool("build_cancel","Request cancellation of a running build and its child processes.",json!({"job":{"type":"integer","minimum":1}}),&["job"]),
        tool("build_retry","Repeat a finished attempt using its original scene and profile in a new directory.",json!({"job":{"type":"integer","minimum":1},"path":path}),&["job","path"]),
        tool("build_status","Poll a build job for status, logs and artifact paths.",json!({"job":{"type":"integer","minimum":1}}),&["job"]),
        tool(
            "scene_snapshot",
            "Read scene metadata, object IDs, cameras, scripts and mesh summaries.",
            json!({}),
            &[],
        ),
        tool(
            "scene_new",
            "Start an empty scene or load the built-in demo.",
            json!({"title":string,"demo":boolean,"physics_demo":boolean,"rendering_demo":boolean}),
            &[],
        ),
        tool(
            "scene_open",
            "Open a saved Forge3D scene.",
            json!({"path":path}),
            &["path"],
        ),
        tool("scene_migrate","Write a new copy of a scene in the current format.",json!({"source":path,"destination":path}),&["source","destination"]),
        tool(
            "scene_save",
            "Save the current scene to a .forge.json file.",
            json!({"path":path,"overwrite":boolean}),
            &["path"],
        ),
        tool(
            "scene_settings",
            "Edit application metadata, scene title, lighting, camera, gravity or keyboard bindings.",
            json!({"title":string,"application":application,"gravity":vec3,"lighting":light_settings,"bindings":bindings,"camera_controller":{"type":["integer","null"],"minimum":1},"camera":{"type":"object","properties":{"target":vec3,"yaw":{"type":"number"},"pitch":{"type":"number"},"distance":{"type":"number"}},"required":["target","yaw","pitch","distance"]}}),
            &[],
        ),
        tool(
            "object_create",
            "Create an empty object, primitive or imported mesh instance.",
            patch,
            &["name", "shape"],
        ),
        tool(
            "object_update",
            "Update fields for an object by ID. Transforms are local to its parent.",
            update,
            &["id"],
        ),
        tool("object_duplicate","Duplicate an object subtree while keeping its resource references.",json!({"id":id}),&["id"]),
        tool(
            "object_delete",
            "Delete an object. Set recursive to delete a subtree.",
            json!({"id":{"type":"integer","minimum":1},"recursive":boolean}),
            &["id"],
        ),
        tool(
            "import_model",
            "Import OBJ, FBX, glTF or GLB. Set instantiate to false to import the asset only.",
            json!({"path":path,"instantiate":boolean}),
            &["path"],
        ),
        tool(
            "script_check",
            "Compile Rhai and verify update(object, dt, input) and optional start(object).",
            json!({"source":string}),
            &["source"],
        ),
        tool(
            "script_asset_set",
            "Create or replace a shared Rhai asset using inline source or a project file.",
            json!({"name":string,"index":index,"path":path,"source":string}),
            &["name"],
        ),
        tool("asset_remove","Remove an unused model or script asset. Read the snapshot to refresh asset indices afterward.",json!({"kind":{"enum":["model","script"]},"index":index}),&["kind","index"]),
        tool(
            "simulate",
            "Run a bounded headless Play session and return object transforms and script errors.",
            json!({"frames":frame,"dt":dt,"input":axes,"controls":controls}),
            &[],
        ),
        tool(
            "undo",
            "Undo the latest scene edit in this session.",
            json!({}),
            &[],
        ),
        tool(
            "redo",
            "Redo the latest undone scene edit in this session.",
            json!({}),
            &[],
        ),
        tool(
            "export_game",
            "Export the current scene into a new standalone Rust Cargo project.",
            json!({"path":path}),
            &["path"],
        ),
    ]
}
