use forge3d::{
    export, project,
    runtime::Scene,
    settings::{Application, SCENE_VERSION},
};
use serde_json::{json, Value};
use std::{fs, process::Command};

#[test]
fn color_shader_survives_scene_load_and_game_export() {
    use forge3d::lighting::{Surface, PULSE_SHADER};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shader.forge.json");
    let mut scene = Scene::demo();
    scene.lighting.color_shader = PULSE_SHADER.into();
    scene.objects[0].surface = Some(Surface {
        use_color_shader: true,
        ..Surface::default()
    });
    project::save(&scene, &path, false).unwrap();
    let loaded = project::load(&path).unwrap();
    let game = dir.path().join("game");
    export::game(&loaded, &game).unwrap();
    assert_eq!(
        project::load(&game.join("assets/scene.json")).unwrap(),
        scene
    );
    for invalid in ["a".repeat(16385), String::from("a\0b")] {
        scene.lighting.color_shader = invalid;
        assert!(scene.validate_objects().is_err());
    }
}

#[test]
fn alpha_scene_migration_keeps_ids_and_animation_without_rewriting_source() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("alpha.forge.json");
    let latest = temp.path().join("beta.forge.json");
    let mut alpha = serde_json::to_value(Scene::rendering_demo()).unwrap();
    alpha["version"] = json!(1);
    alpha.as_object_mut().unwrap().remove("application");
    let bytes = serde_json::to_vec(&alpha).unwrap();
    fs::write(&first, &bytes).unwrap();

    assert_eq!(project::migrate(&first, &latest).unwrap(), SCENE_VERSION);
    assert_eq!(fs::read(&first).unwrap(), bytes);
    let migrated: Value = serde_json::from_slice(&fs::read(&latest).unwrap()).unwrap();
    let expected = Scene::decode(&bytes).unwrap();
    let actual = project::load(&latest).unwrap();
    assert_eq!(actual.objects, expected.objects);
    assert_eq!(actual.models, expected.models);
    assert_eq!(migrated["application"]["identifier"], "com.forge3d.game");
    assert_eq!(migrated["application"]["android_fullscreen"], true);
    assert!(project::migrate(&first, &latest).is_err());

    alpha["version"] = json!(SCENE_VERSION + 1);
    assert!(Scene::decode(&serde_json::to_vec(&alpha).unwrap())
        .unwrap_err()
        .contains("Unsupported scene version"));
}

#[test]
fn export_uses_the_application_metadata_and_validates_it() {
    let temp = tempfile::tempdir().unwrap();
    let mut scene = Scene::rendering_demo();
    scene.application = Application {
        title: "Beta & Motion".into(),
        identifier: "com.forge3d.motion".into(),
        version: "2.1.0-beta.3".into(),
        version_code: 17,
        width: 960,
        height: 540,
        android_fullscreen: false,
        orientation: forge3d::settings::Orientation::Portrait,
        ..Application::default()
    };
    let game = temp.path().join("game");
    export::game(&scene, &game).unwrap();
    let android: Value =
        serde_json::from_slice(&fs::read(game.join("android/forge-app.json")).unwrap()).unwrap();
    assert_eq!(
        android,
        json!({
            "label":"Beta & Motion","identifier":"com.forge3d.motion","version":"2.1.0-beta.3",
            "version_code":17,"orientation":"portrait","fullscreen":false
        })
    );
    let cargo = fs::read_to_string(game.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("version = \"2.1.0-beta.3\""));
    let embedded = Scene::decode(&fs::read(game.join("assets/scene.json")).unwrap()).unwrap();
    assert_eq!(embedded.application, scene.application);
    scene.application.identifier = "bad/id".into();
    let bad = temp.path().join("bad-game");
    assert!(export::game(&scene, &bad).is_err());
    assert!(!bad.exists());
}

#[test]
fn release_without_signer_does_not_create_a_project() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("scene.forge.json");
    project::save(&Scene::empty(), &source, false).unwrap();
    let output = temp.path().join("unsigned");
    let result = Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
        .arg("build")
        .arg(&source)
        .arg(&output)
        .arg("android-release")
        .env_remove("FORGE3D_ANDROID_KEYSTORE")
        .env_remove("FORGE3D_ANDROID_KEY_ALIAS")
        .env_remove("FORGE3D_ANDROID_STORE_PASSWORD")
        .env_remove("FORGE3D_ANDROID_KEY_PASSWORD")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("FORGE3D_ANDROID_KEYSTORE"));
    assert!(!output.exists());
}

#[test]
fn historical_scenes_keep_their_data_through_migration_and_export() {
    let cases: &[(&str, &[u8])] = &[
        ("alpha.1", include_bytes!("fixtures/alpha.1.forge.json")),
        (
            "alpha.2",
            include_bytes!("fixtures/alpha.2-hierarchy.forge.json"),
        ),
        ("alpha.3", include_bytes!("fixtures/alpha.3.forge.json")),
        ("alpha.5", include_bytes!("fixtures/alpha.5.forge.json")),
        ("beta.1", include_bytes!("fixtures/beta.1.forge.json")),
    ];
    for (name, bytes) in cases {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.forge.json");
        let migrated = temp.path().join("migrated.forge.json");
        let output = temp.path().join("game");
        fs::write(&input, bytes).unwrap();
        let expected = project::load(&input).unwrap();
        assert_eq!(project::migrate(&input, &migrated).unwrap(), SCENE_VERSION);
        let actual = project::load(&migrated).unwrap();
        assert_eq!(actual, expected, "{name}");
        assert_eq!(actual.next_id, expected.next_id, "{name}");
        assert_eq!(fs::read(&input).unwrap(), *bytes, "{name}");
        export::game(&actual, &output).unwrap();
        let exported = project::load(&output.join("assets/scene.json")).unwrap();
        assert_eq!(exported, expected, "{name}");
        assert_eq!(exported.next_id, expected.next_id, "{name}");

        let original: Value = serde_json::from_slice(bytes).unwrap();
        for (before, after) in original["objects"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&exported.objects)
        {
            if let Some(id) = before["id"].as_u64() {
                assert_eq!(after.id, id, "{name}");
            } else {
                assert_ne!(after.id, 0, "{name}");
            }
            assert_eq!(after.parent, before["parent"].as_u64(), "{name}");
        }
        if *name == "alpha.2" {
            assert_eq!(exported.objects[2].script_asset, Some(0));
            assert_eq!(exported.scripts[0].name, "Rotate");
            assert_eq!(exported.next_id, 42);
        }
        if *name == "alpha.5" {
            assert!(exported.models[0].rig.is_some());
            assert!(exported
                .objects
                .iter()
                .any(|object| object.animation.is_some()));
        }
    }
}

#[test]
fn missing_toolchain_reports_its_stage_and_can_retry_the_original_scene() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("scene.forge.json");
    let first = temp.path().join("failed");
    let second = temp.path().join("retry");
    project::save(&Scene::demo(), &source, false).unwrap();

    let batch = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge3d-batch"));
        command.env("CARGO", temp.path().join("missing-cargo"));
        command
    };
    let result = batch()
        .arg("build")
        .arg(&source)
        .arg(&first)
        .arg("check")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["stage"], "toolchain");
    assert_eq!(report["code"], "missing_toolchain");
    assert!(first.join("forge-build.stdout.log").is_file());

    project::save(&Scene::empty(), &source, true).unwrap();
    let result = batch()
        .arg("retry")
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let original = fs::read(first.join("assets/scene.json")).unwrap();
    assert_eq!(
        fs::read(second.join("assets/scene.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(first.join("forge-build-plan.json")).unwrap(),
        fs::read(second.join("forge-build-plan.json")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn gradle_signing_error_appears_in_the_build_report() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let key = root.join("test-key.p12");
    fs::write(&key, b"fixture").unwrap();
    let gradle = root.join("gradle");
    fs::write(
        &gradle,
        b"#!/bin/sh\necho \"Execution failed for task ':validateSigningRelease'.\" >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&gradle, fs::Permissions::from_mode(0o700)).unwrap();
    let scene = root.join("scene.forge.json");
    project::save(&Scene::demo(), &scene, false).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
        .arg("build")
        .arg(scene)
        .arg(root.join("build"))
        .arg("android-release")
        .env("FORGE3D_GRADLE", gradle)
        .env("FORGE3D_ANDROID_KEYSTORE", key)
        .env("FORGE3D_ANDROID_KEY_ALIAS", "test")
        .env("FORGE3D_ANDROID_STORE_PASSWORD", "test")
        .env("FORGE3D_ANDROID_KEY_PASSWORD", "test")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["stage"], "build");
    assert_eq!(report["code"], "android_signing");
    assert_eq!(report["exit_code"], 1);
}

#[test]
fn rendering_demo_has_a_movable_object_for_platform_input_checks() {
    use forge3d::runtime::Playback;
    use macroquad::prelude::Vec2;

    let scene = Scene::rendering_demo();
    let index = scene
        .objects
        .iter()
        .position(|object| object.name == "Rough ceramic")
        .unwrap();
    let before = scene.objects[index].position;
    let mut play = Playback::new(scene).unwrap();
    play.tick(Vec2::X, 1. / 60.);
    assert!(play.scene().objects[index].position[0] > before[0]);
}

#[test]
fn validate_checks_scripts_across_all_project_scenes_without_writing() {
    use forge3d::{
        game::Game,
        runtime::{Object, Scene, Shape},
    };
    let start = Scene::empty();
    let mut secondary = Scene::empty();
    let mut object = Object::new("Broken", Shape::Empty);
    object.script = Some("fn start(object) { object }".into());
    secondary.add_object(object).unwrap();
    let mut game = Game::single(start);
    game.scenes.insert("secondary".into(), secondary);
    let error = forge3d::export::validate(&game).unwrap_err();
    assert!(error.contains("secondary: Broken"), "{error}");
}

#[test]
fn editor_package_preserves_an_existing_archive() {
    use forge3d::jobs::{package_editor, Target};
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("editor");
    let archive = tmp.path().join("Forge3D.app.zip");
    fs::write(&binary, b"editor bytes").unwrap();
    package_editor(&binary, &archive, Target::MacosArm64).unwrap();
    let original = fs::read(&archive).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&original)).unwrap();
    let executable = zip.by_name("Forge3D.app/Contents/MacOS/Forge3D").unwrap();
    assert_eq!(executable.unix_mode().unwrap() & 0o777, 0o755);
    assert!(package_editor(&binary, &archive, Target::MacosArm64).is_err());
    assert_eq!(fs::read(&archive).unwrap(), original);
}
