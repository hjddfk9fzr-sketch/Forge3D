use forge3d::{
    export,
    runtime::{Object, Playback, Scene, Shape},
};
use macroquad::prelude::Vec2;
use std::fs;

#[test]
fn script_state_survives_ticks_and_restarts_with_playback() {
    let mut scene = Scene::empty();
    let mut object = Object::new("Scripted cube", Shape::Cube);
    object.script = Some(
        r#"
        fn start(object) {
            object.state.count = 0;
            object.y = 2;
            object
        }

        fn update(object, dt, input) {
            object.state.count += 1;
            object.x += input.x * dt * 2.0;
            object.rz = object.state.count * 10;
            object
        }
    "#
        .into(),
    );
    scene.objects.push(object);

    let mut play = Playback::new(scene.clone()).unwrap();
    assert_eq!(play.scene().objects[0].position[1], 2.);
    play.tick(Vec2::X, 0.1);
    play.tick(Vec2::X, 0.1);
    assert_eq!(play.scene().objects[0].position, [0.4, 2., 0.]);
    assert_eq!(play.scene().objects[0].rotation[2], 20.);
    assert!(play.take_script_errors().is_empty());

    assert_eq!(scene.objects[0].position, [0.; 3]);
    let mut restarted = Playback::new(scene.clone()).unwrap();
    restarted.tick(Vec2::ZERO, 0.1);
    assert_eq!(restarted.scene().objects[0].rotation[2], 10.);

    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("game");
    export::game(&scene, &destination).unwrap();
    assert!(destination.join("src/scripting.rs").is_file());

    let saved = fs::read(destination.join("assets/scene.json")).unwrap();
    let exported = Scene::decode(&saved).unwrap();
    assert_eq!(exported.objects[0].script, scene.objects[0].script);
    let mut play = Playback::new(exported).unwrap();
    play.tick(Vec2::X, 0.1);
    assert_eq!(play.scene().objects[0].position, [0.2, 2., 0.]);
}

#[test]
fn script_errors_disable_only_the_failing_instances() {
    let mut scene = Scene::empty();
    for (name, body) in [
        ("Bad transform", "object.x = 4.0; object.y = false; object"),
        ("Infinite loop", "loop { object.x += 1.0; } object"),
        ("Healthy", "object.z += 1.0; object"),
    ] {
        let mut object = Object::new(name, Shape::Cube);
        object.script = Some(format!("fn update(object, dt, input) {{ {body} }}"));
        scene.objects.push(object);
    }
    let mut play = Playback::new(scene).unwrap();
    play.tick(Vec2::ZERO, 0.1);

    let errors = play.take_script_errors();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].contains("Bad transform"));
    assert!(errors[1].contains("Infinite loop"));
    assert_eq!(play.scene().objects[0].position, [0.; 3]);
    assert_eq!(play.scene().objects[1].position, [0.; 3]);

    play.tick(Vec2::ZERO, 0.1);
    assert!(play.take_script_errors().is_empty());
    assert_eq!(play.scene().objects[2].position[2], 2.);
}

#[test]
fn old_scenes_open_and_invalid_scripts_block_export() {
    let mut scene = Scene::empty();
    scene.objects.push(Object::new("Legacy", Shape::Cube));
    let mut alpha = serde_json::to_value(&scene).unwrap();
    alpha["version"] = 1.into();
    alpha.as_object_mut().unwrap().remove("application");
    alpha["objects"][0].as_object_mut().unwrap().remove("id");
    let old_scene = serde_json::to_vec(&alpha).unwrap();
    assert!(!String::from_utf8_lossy(&old_scene).contains("\"script\""));
    let mut scene = Scene::decode(&old_scene).unwrap();
    assert!(scene.objects[0].script.is_none());

    scene.objects[0].script = Some("fn update(object) { object }".into());
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("game");
    let error = export::game(&scene, &destination).unwrap_err();
    assert!(error.contains("Legacy"));
    assert!(!destination.exists());
}
