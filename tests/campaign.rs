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
