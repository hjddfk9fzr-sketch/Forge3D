use forge3d::{
    export,
    game::{Game, GamePlayback},
    presentation::Presentation,
    progress::Progress,
    workspace::Project,
};
use macroquad::prelude::Vec2;
use serde_json::json;
use std::fs;

fn move_until(play: &mut GamePlayback, movement: Vec2, scene: &str) {
    for _ in 0..180 {
        play.tick(movement, 1. / 60.);
        assert!(play.take_script_errors().is_empty());
        if play.current() == scene {
            return;
        }
    }
    panic!("Did not reach {scene}, current scene is {}", play.current());
}

#[test]
fn campaign_reloads_checkpoint_and_volume_after_relaunch() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data/progress.json");
    let project = Project::campaign_demo().unwrap();
    let destination = temp.path().join("game");
    export::collection(&project.game(None).unwrap(), &destination).unwrap();
    let game = Game::decode(
        &fs::read(destination.join("assets/scene.json")).unwrap(),
        Some(&fs::read(destination.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();

    let mut play = GamePlayback::new(game.clone()).unwrap();
    assert_eq!(play.current(), "menu");
    assert!(play.ui_action("unknown").is_err());
    play.ui_action("continue").unwrap();
    assert!(!play.tick(Vec2::ZERO, 1. / 60.));
    assert_eq!(play.take_sounds(), ["cue"]);
    assert!(!play.take_progress_changed());

    play.ui_action("quiet").unwrap();
    play.tick(Vec2::ZERO, 1. / 60.);
    assert!((play.progress().volume - 0.6).abs() < 1e-6);
    play.ui_action("start").unwrap();
    assert!(play.tick(Vec2::ZERO, 1. / 60.));
    assert_eq!(play.current(), "entrance");
    assert_eq!(play.progress().checkpoint.as_deref(), Some("entrance"));

    move_until(&mut play, Vec2::X, "workshop");
    assert_eq!(play.take_sounds(), ["cue"]);
    assert!(play.take_progress_changed());
    play.progress().save(&path).unwrap();
    drop(play);

    let saved = Progress::load(&path).unwrap();
    assert!((saved.volume - 0.6).abs() < 1e-6);
    assert_eq!(saved.checkpoint.as_deref(), Some("workshop"));
    let mut reopened = GamePlayback::with_progress(game, saved).unwrap();
    assert_eq!(reopened.current(), "menu");
    reopened.ui_action("continue").unwrap();
    assert!(reopened.tick(Vec2::ZERO, 1. / 60.));
    assert_eq!(reopened.current(), "workshop");
    move_until(&mut reopened, Vec2::NEG_X, "complete");
    assert!(reopened.take_progress_changed());
    reopened.progress().save(&path).unwrap();
    assert_eq!(
        Progress::load(&path).unwrap().checkpoint.as_deref(),
        Some("complete")
    );
}

#[test]
fn overlay_rejects_malformed_media_and_overridden_keyboard_actions() {
    let bad_png = json!({"widgets":[{"kind":"image","rect":[0,0,1,1],"png":[137,80,78,71]}]});
    let bad_wav = json!({"sounds":{"cue":[82,73,70,70]}});
    let duplicate = json!({"widgets":[
        {"kind":"button","rect":[0,0,0.2,0.2],"text":"First","id":"first","key":"enter"},
        {"kind":"button","rect":[0,0.3,0.2,0.2],"text":"Second","id":"second","key":"enter"}
    ]});
    for value in [bad_png, bad_wav, duplicate] {
        let presentation: Presentation = serde_json::from_value(value).unwrap();
        assert!(presentation.validate().is_err());
    }
}

#[test]
fn workshop_door_needs_clear_aim_and_reach() {
    use forge3d::{
        controls::{InputControls, Key},
        physics::{Body, STEP},
        runtime::{Object, Playback, Shape},
    };
    let entrance = Project::campaign_demo().unwrap().scene("entrance").unwrap();
    for case in ["near", "button", "far", "blocked", "looking away"] {
        let mut scene = entrance.clone();
        let door = scene
            .objects
            .iter()
            .position(|o| o.name == "Workshop door")
            .unwrap();
        if case == "far" {
            scene.objects[door].position[2] = -5.;
        }
        if case == "blocked" {
            let mut wall = Object::new("Wall", Shape::Cube);
            wall.position = [0., 1., -1.];
            wall.body = Some(Body::default());
            scene.add_object(wall).unwrap();
        }
        let mut play = Playback::new(scene).unwrap();
        let mut controls = InputControls::default();
        if case == "looking away" {
            controls.keys.insert(Key::Q);
            play.tick_input(Vec2::ZERO, &controls, STEP);
        }
        if case == "button" {
            play.ui_action("use").unwrap();
        } else {
            controls.pressed.insert(Key::E);
        }
        play.tick_input(Vec2::ZERO, &controls, STEP);
        let errors = play.take_script_errors();
        assert!(errors.is_empty(), "{case}: {errors:?}");
        let open = matches!(case, "near" | "button");
        assert_eq!(play.scene().objects[door].position[1] > 3., open, "{case}");
        if open {
            assert!(play
                .raycast([0., 1.2, 0.], [0., 0., -1.], 2., None)
                .unwrap()
                .is_none());
        }
    }
}

#[test]
fn runtime_prefab_pickup_survives_export_and_leaves_no_body_or_script() {
    use forge3d::physics::STEP;
    let project = Project::campaign_demo().unwrap();
    let game = project.game(Some("entrance")).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let dir = destination.path().join("game");
    export::collection(&game, &dir).unwrap();
    let decoded = Game::decode(
        &fs::read(dir.join("assets/scene.json")).unwrap(),
        Some(&fs::read(dir.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    for game in [game, decoded] {
        let mut play = GamePlayback::new(game).unwrap();
        play.tick(Vec2::ZERO, STEP);
        let item = play
            .scene()
            .objects
            .iter()
            .find(|o| o.name == "Workshop token")
            .unwrap()
            .id;
        let ray = |p: &GamePlayback| p.raycast([1.25, 2., 0.], [0., -1., 0.], 2., None).unwrap();
        assert_eq!(ray(&play).unwrap().object_id, item);
        play.tick(Vec2::ZERO, STEP);
        assert!(play.scene().objects[play.scene().object_index(item).unwrap()].rotation[1] > 0.);
        for _ in 0..24 {
            play.tick(Vec2::X, STEP);
        }
        assert!(play.scene().object_index(item).is_err());
        assert!(ray(&play).is_none_or(|hit| hit.object_id != item));
        assert_eq!(play.scene().objects[0].components["workshop.inventory"], 1);
        assert!(play.take_script_errors().is_empty());
    }
}

#[test]
fn failed_native_command_batch_keeps_the_original_world() {
    use forge3d::{
        gameplay::{Command, Context, Systems},
        runtime::{Object, Scene, Shape},
    };
    fn fail_after_delete(ctx: Context<'_>, commands: &mut Vec<Command>) -> Result<(), String> {
        commands.push(Command::Despawn(ctx.object.id));
        commands.push(Command::Spawn {
            prefab: "missing".into(),
            position: [0.; 3],
        });
        Ok(())
    }
    let mut scene = Scene::empty();
    let mut owner = Object::new("Command owner", Shape::Cube);
    owner.components.insert("example".into(), true.into());
    owner.body = Some(Default::default());
    let id = scene.add_object(owner).unwrap();
    let mut systems = Systems::default();
    systems.register("example", fail_after_delete).unwrap();
    assert!(systems.register("example", fail_after_delete).is_err());
    let mut play =
        GamePlayback::with_systems(Game::single(scene), Progress::default(), systems).unwrap();
    play.tick(Vec2::ZERO, 1. / 60.);
    assert_eq!(play.scene().objects[0].id, id);
    assert_eq!(
        play.raycast([0., 2., 0.], [0., -1., 0.], 3., None)
            .unwrap()
            .unwrap()
            .object_id,
        id
    );
    assert!(play.take_script_errors()[0].contains("missing"));
}
