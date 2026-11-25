use forge3d::{
    runtime::{Object, Scene, Shape},
    workspace::{ModelOwner, Project, SharedAsset},
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn batch(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn success(directory: &Path, args: &[&str]) -> String {
    let output = batch(directory, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const MANIFEST: &str = "game.forge-project.json";
const SCRIPT: &str = "fn update(object, dt, input) { object }";

#[test]
fn mixed_batch_dependencies_and_script_reimport_survive_project_move() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("original");
    fs::create_dir(&old).unwrap();
    Project::new("main", &Scene::empty())
        .unwrap()
        .save(&old.join(MANIFEST), false)
        .unwrap();
    fs::write(
        old.join("door.fbx"),
        include_bytes!("fixtures/model-ascii.fbx"),
    )
    .unwrap();
    fs::write(old.join("spin.rhai"), SCRIPT).unwrap();

    let before = fs::read(old.join(MANIFEST)).unwrap();
    let invalid = batch(
        &old,
        &["project-import", MANIFEST, "door.fbx", "missing.rhai"],
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("missing.rhai"));
    assert_eq!(fs::read(old.join(MANIFEST)).unwrap(), before);

    for _ in 0..2 {
        success(&old, &["project-import", MANIFEST, "door.fbx", "spin.rhai"]);
    }
    let mut project = Project::load(&old.join(MANIFEST)).unwrap();
    assert_eq!((project.models().len(), project.scripts().len()), (1, 1));
    let mut scene = project.scene("main").unwrap();
    let model_slot = project.use_model(1, &mut scene).unwrap();
    let mut instance = Object::new("Door", Shape::Model(model_slot));
    instance.script_asset = Some(project.use_script(1, &mut scene).unwrap());
    let root = scene.add_object(instance).unwrap();
    project.update_scene("main", &scene).unwrap();
    project.add_scene("level", &scene).unwrap();
    project.create_prefab("door", &scene, root).unwrap();
    project.save(&old.join(MANIFEST), true).unwrap();

    let moved = dir.path().join("moved");
    fs::rename(&old, &moved).unwrap();
    fs::remove_file(moved.join("door.fbx")).unwrap();
    fs::remove_file(moved.join("spin.rhai")).unwrap();
    for (kind, owner) in [("scene", "level"), ("prefab", "door")] {
        let deps = success(&moved, &["project-deps", MANIFEST, kind, owner]);
        assert!(deps.contains("model #1\tdoor") && deps.contains("script #1\tspin.rhai"));
    }
    let uses = success(&moved, &["project-uses", MANIFEST, "script", "1"]);
    assert_eq!(uses.lines().count(), 3);
    assert!(uses.contains("prefab\tdoor") && uses.contains("scene\tlevel"));
    let replacement = "fn update(object, dt, input) { object.x += dt; object }";
    fs::write(moved.join("replacement.rhai"), replacement).unwrap();
    success(
        &moved,
        &[
            "project-reimport",
            MANIFEST,
            "script",
            "1",
            "replacement.rhai",
        ],
    );
    let project = Project::load(&moved.join(MANIFEST)).unwrap();
    assert_eq!(project.scripts()[&1].name, "spin.rhai");
    assert_eq!(
        project.scene("level").unwrap().scripts[0].source,
        replacement
    );
    let mut fresh = Scene::empty();
    project.instantiate("door", &mut fresh, None).unwrap();
    assert_eq!(fresh.scripts[0].source, replacement);
    assert_eq!(
        project.dependencies(ModelOwner::Prefab("door")).unwrap()[1].0,
        SharedAsset::Script(1)
    );
    let saved = fs::read(moved.join(MANIFEST)).unwrap();
    fs::write(moved.join("replacement.rhai"), "fn invalid(").unwrap();
    assert!(!batch(
        &moved,
        &[
            "project-reimport",
            MANIFEST,
            "script",
            "1",
            "replacement.rhai"
        ]
    )
    .status
    .success());
    assert_eq!(fs::read(moved.join(MANIFEST)).unwrap(), saved);
    success(&moved, &["validate", MANIFEST]);
    success(&moved, &["export", MANIFEST, "game"]);
    let exported = forge3d::project::load(&moved.join("game/assets/scene.json")).unwrap();
    assert_eq!(exported.models, fresh.models);
    assert_eq!(exported.scripts, fresh.scripts);
}

#[test]
fn diagnostics_identify_missing_assets_and_invalid_object_slots_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(MANIFEST);
    let mut scene = Scene::empty();
    scene.scripts.push(forge3d::runtime::ScriptAsset {
        name: "shared".into(),
        source: SCRIPT.into(),
    });
    let mut object = Object::new("Door", Shape::Cube);
    object.script_asset = Some(0);
    let root = scene.add_object(object).unwrap();
    let mut project = Project::new("main", &scene).unwrap();
    project.create_prefab("door", &scene, root).unwrap();
    project.save(&path, false).unwrap();
    let mut data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data["scripts"] = serde_json::json!({});
    data["prefabs"]["door"]["scene"]["objects"][0]["script_asset"] = 23.into();
    let bytes = serde_json::to_vec(&data).unwrap();
    fs::write(&path, &bytes).unwrap();
    let output = batch(dir.path(), &["validate", MANIFEST]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    for message in [
        "scene 'main': missing script #1",
        "prefab 'door': missing script #1",
        "object 'Door' #1: missing script slot 23",
    ] {
        assert!(error.contains(message), "{error}");
    }
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn concurrent_cli_imports_keep_both_commits() {
    let dir = tempfile::tempdir().unwrap();
    Project::new("main", &Scene::empty())
        .unwrap()
        .save(&dir.path().join(MANIFEST), false)
        .unwrap();
    std::thread::scope(|scope| {
        for name in ["one.rhai", "two.rhai"] {
            let path = dir.path();
            fs::write(path.join(name), SCRIPT).unwrap();
            scope.spawn(move || success(path, &["project-import", MANIFEST, name]));
        }
    });
    assert_eq!(
        Project::load(&dir.path().join(MANIFEST))
            .unwrap()
            .scripts()
            .len(),
        2
    );
}
