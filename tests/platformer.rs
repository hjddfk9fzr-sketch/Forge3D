use forge3d::{
    canvas2d::{empty_2d, Body},
    controls::{Action, InputControls},
    export,
    game::{Game, GamePlayback},
    runtime::{Object, Shape},
    workspace::Project,
};
use macroquad::prelude::Vec2;

const STEP: f32 = 1. / 60.;

fn position(run: &GamePlayback) -> [f32; 3] {
    run.scene().objects[0].position
}

fn complete(game: Game) {
    let mut run = GamePlayback::new(game).unwrap();
    assert_eq!(run.current(), "menu");
    run.ui_action("start").unwrap();
    assert!(run.tick(Vec2::ZERO, STEP));
    assert_eq!(run.current(), "yard");
    for _ in 0..30 {
        run.tick(Vec2::ZERO, STEP);
    }
    assert!((position(&run)[1] - 9.54).abs() < 0.001);

    let mut fell = false;
    for _ in 0..220 {
        run.tick(Vec2::X, STEP);
        let [x, y, _] = position(&run);
        fell |= y > 10.;
        if fell && x == 3. {
            break;
        }
    }
    assert!(fell);
    assert_eq!(run.current(), "yard");
    assert_eq!(position(&run)[0], 3.);

    for (level, next, takeoff) in [
        ("yard", "bridge", [11.3, 23.3]),
        ("bridge", "finish", [16.3, 25.3]),
    ] {
        let mut jumps = 0;
        for _ in 0..450 {
            let mut input = InputControls::default();
            if jumps < takeoff.len() && position(&run)[0] >= takeoff[jumps] {
                input.actions.insert(Action::Jump);
                jumps += 1;
            }
            if run.tick_input(Vec2::X, &input, STEP) {
                break;
            }
        }
        assert_eq!(jumps, 2, "{level} stopped at {:?}", position(&run));
        assert_eq!(
            run.current(),
            next,
            "{level} stopped at {:?}",
            position(&run)
        );
    }
    run.ui_action("start").unwrap();
    assert!(run.tick(Vec2::ZERO, STEP));
    assert_eq!(run.current(), "yard");
    assert!(run.take_script_errors().is_empty());
}

#[test]
fn afterlight_round_trips_project_and_export_and_completes_both_levels() {
    let temp = tempfile::tempdir().unwrap();
    let project_file = temp.path().join("Afterlight.forge-project.json");
    Project::platformer()
        .unwrap()
        .save(&project_file, false)
        .unwrap();
    let game = Project::load(&project_file).unwrap().game(None).unwrap();
    complete(game.clone());
    let output = temp.path().join("game");
    export::collection(&game, &output).unwrap();
    let start = std::fs::read(output.join("assets/scene.json")).unwrap();
    let additional = std::fs::read(output.join("assets/scenes.json")).unwrap();
    complete(Game::decode(&start, Some(&additional)).unwrap());
}

#[test]
fn tile_resize_preserves_painted_intersection_and_body_has_to_be_root() {
    let mut scene = empty_2d();
    let canvas = scene.canvas2d.as_mut().unwrap();
    canvas.paint(0, 1, 2, 1);
    canvas.paint(0, 20, 2, 1);
    canvas.add_layer("Decoration", false);
    canvas.paint(1, 0, 0, 1);
    canvas.resize(3, 5);
    assert!(canvas.solid(1, 2));
    assert!(!canvas.solid(0, 0));
    canvas.paint(0, 1, 2, 0);
    assert!(!canvas.solid(1, 2));
    canvas.resize(30, 6);
    assert!(!canvas.solid(20, 2));

    let parent = scene
        .add_object(Object::new("Parent", Shape::Empty))
        .unwrap();
    let mut object = Object::new("Body", Shape::Empty);
    object.body2d = Some(Body::default());
    let child = scene.add_object(object).unwrap();
    assert!(scene.set_parent(child, Some(parent)).is_err());
    scene.validate().unwrap();
}

#[test]
fn sprite_animation_wraps_within_the_atlas_range() {
    let scene = Project::platformer().unwrap().scene("yard").unwrap();
    let sprite = scene.objects[0].sprite2d.as_ref().unwrap();
    assert_eq!(sprite.frame_at(0.), 2);
    assert_eq!(sprite.frame_at(0.34), 5);
    assert_eq!(sprite.frame_at(0.45), 2);
}

#[test]
fn scene_capture_keeps_tiles_below_the_courier() {
    let scene = Project::platformer().unwrap().scene("yard").unwrap();
    let data = forge3d::capture::png(&scene, 320, 180).unwrap();
    let frame = image::load_from_memory(&data).unwrap().into_rgba8();
    // Camera center (10, 8), 15 pixels per tile.
    assert_eq!(frame.get_pixel(100, 121).0, [106, 233, 182, 255]);
    assert_eq!(frame.get_pixel(100, 33).0, [27, 43, 72, 255]);
    assert_eq!(frame.get_pixel(57, 107).0, [41, 255, 225, 255]);
}
