use forge3d::{
    game::{Game, GamePlayback},
    project,
    runtime::{Object, Scene, ScriptAsset, Shape},
    workspace::Project,
};
use macroquad::prelude::Vec2;
use std::{fs, path::Path, process::Command};

#[test]
fn reimport_updates_shared_geometry_in_scenes_and_prefabs_and_rolls_back_incompatible_replacements()
{
    use forge3d::{import, workspace::OpenProject};
    let fixture = |name: &str| Path::new("tests/fixtures").join(name);
    let mut scene = Scene::empty();
    scene
        .models
        .push(import::model(&fixture("model-ascii.fbx")).unwrap());
    let root = scene
        .add_object(Object::new("Door", Shape::Model(0)))
        .unwrap();
    let mut project = Project::new("first", &scene).unwrap();
    project.add_scene("second", &scene).unwrap();
    project.create_prefab("door", &scene, root).unwrap();
    let id = *project.models().keys().next().unwrap();
    let mut open = OpenProject::open(project);
    scene.objects[0].position = [2., 3., 4.];
    open.project = open.snapshot(&scene).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.forge-project.json");
    open.project.save(&path, false).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
        .arg("project-reimport")
        .arg(&path)
        .arg(id.to_string())
        .arg(fixture("model-binary.fbx"))
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let loaded = Project::load(&path).unwrap();
    let mut expected = import::model(&fixture("model-binary.fbx")).unwrap();
    expected.name.clone_from(&scene.models[0].name);
    assert_eq!(loaded.models()[&id], expected);
    assert_eq!(loaded.models().len(), 1);
    assert_eq!(loaded.model_usages(id), open.project.model_usages(id));
    for name in ["first", "second"] {
        assert_eq!(loaded.scene(name).unwrap().models[0], expected);
    }
    assert_eq!(loaded.scene("first").unwrap().objects[0], scene.objects[0]);
    let mut fresh = Scene::empty();
    loaded.instantiate("door", &mut fresh, None).unwrap();
    assert_eq!(fresh.models[0], expected);
    // The existing animation player needs a clip absent from the replacement.
    let animated = Scene::rendering_demo();
    let mut project = Project::new("animated", &animated).unwrap();
    let id = *project.models().keys().next().unwrap();
    assert!(project.reimport_model(id, expected).is_err());
    assert_eq!(project.scene("animated").unwrap(), animated);
}

#[test]
fn model_usage_tracks_objects_by_asset_id_across_scenes_prefabs_and_reload() {
    use forge3d::workspace::{ModelOwner, ModelUsage, OpenProject};

    let mut scene = Scene::empty();
    let model = Scene::rendering_demo().models.remove(0);
    let mut other = model.clone();
    std::sync::Arc::make_mut(&mut other.parts)[0].vertices[0].position[0] += 1.;
    scene.models = vec![model.clone(), other.clone()];
    let door = scene
        .add_object(Object::new("Door", Shape::Model(0)))
        .unwrap();
    let mut project = Project::new("first", &scene).unwrap();
    project.create_prefab("door", &scene, door).unwrap();
    let mut second = Scene::empty();
    second.models = vec![other, model.clone()];
    let exit = second
        .add_object(Object::new("Exit", Shape::Model(1)))
        .unwrap();
    project.add_scene("second", &second).unwrap();
    let id = *project
        .models()
        .iter()
        .find(|(_, m)| **m == model)
        .unwrap()
        .0;
    let unused = *project.models().keys().find(|k| **k != id).unwrap();
    let check = |project: &Project| {
        assert_eq!(
            project.model_usages(id),
            vec![
                ModelUsage {
                    owner: ModelOwner::Scene("first"),
                    object_id: door,
                    object_name: "Door"
                },
                ModelUsage {
                    owner: ModelOwner::Scene("second"),
                    object_id: exit,
                    object_name: "Exit"
                },
                ModelUsage {
                    owner: ModelOwner::Prefab("door"),
                    object_id: door,
                    object_name: "Door"
                },
            ]
        );
        assert!(project.model_usages(unused).is_empty());
    };
    check(&project);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.forge-project.json");
    project.save(&path, false).unwrap();
    let reopened = Project::load(&path).unwrap();
    check(&reopened);

    let open = OpenProject::open(reopened);
    scene.objects[0].shape = Shape::Model(1);
    let snapshot = open.snapshot(&scene).unwrap();
    assert_eq!(snapshot.model_usages(id).len(), 2);
    assert_eq!(snapshot.model_usages(unused)[0].object_name, "Door");
    check(&open.project);
}

#[test]
fn portable_project_reuses_prefab_assets_and_remaps_object_ids() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let renamed = temp.path().join("renamed");
    fs::create_dir(&old).unwrap();
    let mut first = Scene::rendering_demo();
    let parent = first.objects[1].id;
    first.scripts.push(ScriptAsset {
        name: "Spin".into(),
        source: "fn update(object, dt, input) { object.ry += dt; object }".into(),
    });
    let mut child = Object::new("Child", Shape::Cube);
    child.parent = Some(parent);
    child.script_asset = Some(0);
    first.add_object(child).unwrap();
    let mut second = Scene::demo();
    let mut project = Project::new("first", &first).unwrap();
    project.create_prefab("arm", &first, parent).unwrap();
    let a = project.instantiate("arm", &mut second, None).unwrap();
    let b = project.instantiate("arm", &mut second, None).unwrap();
    assert_ne!(a, b);
    assert_eq!(second.models.len(), 1);
    assert_eq!(second.scripts.len(), 1);
    assert_eq!(
        second
            .objects
            .iter()
            .filter(|object| [Some(a), Some(b)].contains(&object.parent))
            .count(),
        2
    );
    let ids: std::collections::BTreeSet<_> =
        second.objects.iter().map(|object| object.id).collect();
    assert_eq!(ids.len(), second.objects.len());
    project.add_scene("second", &second).unwrap();
    project.set_start("second").unwrap();
    let model_id = *project.models().keys().next().unwrap();
    let script_id = *project.scripts().keys().next().unwrap();
    assert_eq!(project.models().len(), 1);
    assert_eq!(project.scripts().len(), 1);

    project
        .save(&old.join("demo.forge-project.json"), false)
        .unwrap();
    fs::rename(old, &renamed).unwrap();
    let saved = Project::load(&renamed.join("demo.forge-project.json")).unwrap();
    let usages = saved.asset_usages(forge3d::workspace::SharedAsset::Script(script_id));
    assert_eq!(usages.len(), 4);
    assert!(usages.iter().all(|entry| entry.object_name == "Child"));
    assert_eq!(saved.start(), "second");
    assert_eq!(saved.scene("first").unwrap(), first);
    assert_eq!(saved.scene("second").unwrap(), second);
    assert_eq!(
        saved.models().keys().copied().collect::<Vec<_>>(),
        [model_id]
    );
    assert_eq!(
        saved.scripts().keys().copied().collect::<Vec<_>>(),
        [script_id]
    );
    let mut scene = Scene::empty();
    assert_eq!(saved.use_model(model_id, &mut scene).unwrap(), 0);
    assert_eq!(saved.use_script(script_id, &mut scene).unwrap(), 0);
}

#[test]
fn game_transitions_reset_scene_and_report_unknown_destination() {
    let mut demo = Project::demo().unwrap().game(None).unwrap();
    let mut game = GamePlayback::new(demo.clone()).unwrap();
    assert_eq!(game.current(), "entrance");
    assert!(game.tick(Vec2::X, 1. / 60.));
    assert_eq!(game.current(), "workshop");
    assert!(game.tick(Vec2::NEG_X, 1. / 60.));
    assert_eq!(game.current(), "entrance");
    assert!(game.take_script_errors().is_empty());

    let start = &mut demo.scenes.get_mut("entrance").unwrap().objects[0];
    start.script =
        Some(r#"fn update(object, dt, input) { object.next_scene = "missing"; object }"#.into());
    let mut game = GamePlayback::new(demo).unwrap();
    assert!(!game.tick(Vec2::ZERO, 1. / 60.));
    assert_eq!(game.current(), "entrance");
    assert_eq!(game.take_script_errors(), ["Unknown scene 'missing'."]);
}

#[test]
fn batch_converts_a_scene_and_retries_the_entire_export() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
            .current_dir(root)
            .env("CARGO", root.join("missing-cargo"))
            .args(args)
            .output()
            .unwrap()
    };
    let run = |args: &[&str]| {
        let output = command(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let path = "game.forge-project.json";
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpha.1.forge.json");
    fs::copy(fixture, root.join("old.forge.json")).unwrap();
    run(&["project-new", "old.forge.json", path]);
    let saved = Project::load(&root.join(path)).unwrap();
    assert_eq!(
        saved.scene("main").unwrap(),
        project::load(&root.join("old.forge.json")).unwrap()
    );

    run(&["demo", "other.forge.json"]);
    run(&["project-add", path, "other", "other.forge.json"]);
    run(&["project-start", path, "other"]);
    run(&["export", path, "export"]);
    assert!(!command(&["build", path, "first", "check"]).status.success());
    run(&["project-start", path, "main"]);
    assert!(!command(&["retry", "first", "retry"]).status.success());
    let files = ["assets/scene.json", "assets/scenes.json"];
    for file in files {
        let expected = fs::read(root.join("export").join(file)).unwrap();
        assert_eq!(fs::read(root.join("retry").join(file)).unwrap(), expected);
    }
    let start = fs::read(root.join("retry").join(files[0])).unwrap();
    let extra = fs::read(root.join("retry").join(files[1])).unwrap();
    let game = Game::decode(&start, Some(&extra)).unwrap();
    assert_eq!(game.start, "other");
    assert!(game.scenes.contains_key("main"));
}

#[test]
fn duplicate_subtree_keeps_parent_links_and_scene_resource_indices() {
    let mut scene = Scene::rendering_demo();
    let parent = scene.objects[1].id;
    let mut child = Object::new("Child", Shape::Cube);
    child.parent = Some(parent);
    let child_id = scene.add_object(child).unwrap();
    let root = project::duplicate_object(&mut scene, parent).unwrap();
    assert_ne!(root, parent);
    let cloned = &scene.objects[scene.object_index(root).unwrap()];
    assert_eq!(cloned.shape, scene.objects[1].shape);
    assert_eq!(cloned.name, "Skinned arm Copy");
    assert!(scene
        .objects
        .iter()
        .any(|node| node.id != child_id && node.parent == Some(root)));
    assert_eq!(
        scene.objects[scene.object_index(child_id).unwrap()].parent,
        Some(parent)
    );
    assert_eq!(scene.models.len(), 1);
}
