use forge3d::{
    game::{Game, GamePlayback},
    project,
    runtime::{Object, Scene, ScriptAsset, Shape},
    workspace::Project,
};
use macroquad::prelude::Vec2;
use std::{fs, path::Path, process::Command};

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
