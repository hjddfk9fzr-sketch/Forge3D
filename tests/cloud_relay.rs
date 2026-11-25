use forge3d::{
    controls::{InputControls, Key},
    export,
    game::{Game, GamePlayback},
    physics::STEP,
    workspace::Project,
};
use macroquad::prelude::Vec2;

#[test]
fn relay_can_be_lost_completed_and_restarted_after_export() {
    let game = Project::cloud_relay().unwrap().game(None).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("game");
    export::collection(&game, &out).unwrap();
    let exported = Game::decode(
        &std::fs::read(out.join("assets/scene.json")).unwrap(),
        Some(&std::fs::read(out.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    run_route(exported, "finish");
}

fn run_route(game: Game, finish: &str) {
    let mut play = GamePlayback::new(game).unwrap();
    play.ui_action("start").unwrap();
    play.tick(Vec2::ZERO, STEP);
    assert_eq!(play.current(), "amber");
    let player = |p: &GamePlayback| {
        p.scene()
            .objects
            .iter()
            .find(|o| o.name == "Courier")
            .unwrap()
            .position
    };
    let mut fell = false;
    for _ in 0..240 {
        play.tick(Vec2::NEG_Y, STEP);
        if player(&play)[1] < -1.0 {
            fell = true;
        }
        if fell && player(&play)[2] > 0.5 {
            break;
        }
    }
    assert!(fell && player(&play)[2] > 0.5);
    for (level, next) in [("amber", "vesper"), ("vesper", finish)] {
        let mut jumped = [false; 2];
        for _ in 0..480 {
            let [x, _, z] = player(&play);
            let mut input = InputControls::default();
            for (i, edge) in [-2.18, -8.50].into_iter().enumerate() {
                if !jumped[i] && z <= edge {
                    input.pressed.insert(Key::Space);
                    jumped[i] = true;
                }
            }
            let target = if level == "vesper" && z > -7.4 {
                1.35
            } else {
                0.
            };
            let steer = (target - x).clamp(-1., 1.);
            play.tick_input(Vec2::new(steer, -1.).normalize(), &input, STEP);
            let errors = play.take_script_errors();
            assert!(errors.is_empty(), "{errors:?}");
            if play.current() != level {
                break;
            }
        }
        assert_eq!(play.current(), next);
    }
    play.ui_action("start").unwrap();
    play.tick(Vec2::ZERO, STEP);
    assert_eq!(play.current(), "amber");
}

#[test]
fn prefab_properties_propagate_across_saved_scenes_and_revert_individually() {
    use forge3d::{
        lighting::Surface,
        physics::Body,
        runtime::{Object, Scene, Shape},
    };
    let mut source = Scene::empty();
    let mut door = Object::new("Door", Shape::Cube);
    door.body = Some(Body::default());
    door.surface = Some(Surface::default());
    let root = source.add_object(door).unwrap();
    let mut project = Project::new("a", &source).unwrap();
    project.create_prefab("door", &source, root).unwrap();
    project.link_source("door", &mut source).unwrap();
    let mut second = Scene::empty();
    let other = project.instantiate("door", &mut second, None).unwrap();
    second.objects[0].position = [7., 1., 2.];
    second.objects[0].surface.as_mut().unwrap().base_color = [1., 0., 0., 1.];
    second.objects[0].body.as_mut().unwrap().friction = 0.25;
    project.add_scene("b", &second).unwrap();
    source.objects[0].body.as_mut().unwrap().size = [2., 3., 0.2];
    project
        .apply_property("a", &source, root, &["body".into(), "size".into()])
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("doors.forge-project.json");
    project.save(&path, false).unwrap();
    let project = Project::load(&path).unwrap();
    let game = project.game(None).unwrap();
    for id in ["a", "b"] {
        assert_eq!(
            game.scenes[id].objects[0].body.as_ref().unwrap().size,
            [2., 3., 0.2]
        );
    }
    let mut second = project.scene("b").unwrap();
    assert_eq!(second.objects[0].position, [7., 1., 2.]);
    assert_eq!(
        second.objects[0].surface.as_ref().unwrap().base_color,
        [1., 0., 0., 1.]
    );
    assert_eq!(second.objects[0].body.as_ref().unwrap().friction, 0.25);
    assert_eq!(project.overrides(&second, other).unwrap().len(), 2);
    project
        .revert_property(&mut second, other, &["surface".into(), "base_color".into()])
        .unwrap();
    assert_eq!(
        second.objects[0].surface.as_ref().unwrap().base_color,
        [1.; 4]
    );
    assert_eq!(project.overrides(&second, other).unwrap().len(), 1);
}

#[test]
fn a_local_override_stays_local_if_the_prefab_temporarily_matches_it() {
    use forge3d::runtime::{Object, Scene, Shape};
    let mut a = Scene::empty();
    let id = a.add_object(Object::new("Arch", Shape::Cube)).unwrap();
    let mut p = Project::new("a", &a).unwrap();
    p.create_prefab("arch", &a, id).unwrap();
    p.link_source("arch", &mut a).unwrap();
    let mut b = Scene::empty();
    let copy = p.instantiate("arch", &mut b, None).unwrap();
    b.objects[0].speed = 3.;
    p.add_scene("b", &b).unwrap();
    let path = vec!["speed".to_owned()];
    for speed in [3., 7.] {
        a.objects[0].speed = speed;
        p.apply_property("a", &a, id, &path).unwrap();
        b = p.scene("b").unwrap();
        assert_eq!(b.objects[0].speed, 3.);
        assert!(p
            .overrides(&b, copy)
            .unwrap()
            .iter()
            .any(|o| o.path == path));
        a = p.scene("a").unwrap();
    }
    p.revert_property(&mut b, copy, &path).unwrap();
    assert_eq!(b.objects[0].speed, 7.);
}

#[test]
fn renamed_gates_route_to_an_edited_destination_after_save_and_export() {
    use forge3d::relay::{RelayGate, Role};
    let mut project = Project::cloud_relay().unwrap();
    project
        .add_scene("alternate", &project.scene("finish").unwrap())
        .unwrap();
    for level in ["amber", "vesper"] {
        let mut scene = project.scene(level).unwrap();
        for o in &mut scene.objects {
            if let Some(mut gate) = RelayGate::read(o).unwrap() {
                o.name = "Renamed arch".into();
                if gate.role == Role::Exit && level == "vesper" {
                    gate.destination = "alternate".into();
                }
                gate.attach(o);
            }
        }
        project.update_scene(level, &scene).unwrap();
    }
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("route.forge-project.json");
    project.save(&file, false).unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let out = tmp.path().join("export");
    export::collection(&game, &out).unwrap();
    run_route(
        Game::decode(
            &std::fs::read(out.join("assets/scene.json")).unwrap(),
            Some(&std::fs::read(out.join("assets/scenes.json")).unwrap()),
        )
        .unwrap(),
        "alternate",
    );
}

#[test]
fn missing_gate_destination_names_the_object_before_play_and_export() {
    let mut game = Project::cloud_relay().unwrap().game(None).unwrap();
    game.scenes.remove("vesper");
    for message in [
        export::validate(&game).unwrap_err(),
        GamePlayback::new(game).err().unwrap(),
    ] {
        for part in ["amber", "Exit arch", "vesper"] {
            assert!(message.contains(part), "{message}");
        }
    }
}

#[test]
fn applying_gate_destination_preserves_local_roles_and_other_exits() {
    use forge3d::relay::{RelayGate, Role, COMPONENT};
    let mut project = Project::cloud_relay().unwrap();
    let mut scene = project.scene("amber").unwrap();
    let exit = scene
        .objects
        .iter()
        .find(|o| {
            RelayGate::read(o)
                .unwrap()
                .is_some_and(|g| g.role == Role::Exit)
        })
        .unwrap()
        .id;
    let path = [
        String::from("components"),
        COMPONENT.into(),
        "destination".into(),
    ];
    project
        .apply_property("amber", &scene, exit, &path)
        .unwrap();
    let vesper = project.scene("vesper").unwrap();
    let gates: Vec<_> = vesper
        .objects
        .iter()
        .filter_map(|o| RelayGate::read(o).unwrap())
        .collect();
    assert!(gates
        .iter()
        .any(|g| g.role == Role::Transmitter && g.destination == "vesper"));
    assert!(gates
        .iter()
        .any(|g| g.role == Role::Exit && g.destination == "finish"));
    scene = project.scene("amber").unwrap();
    let i = scene.object_index(exit).unwrap();
    RelayGate {
        role: Role::Exit,
        destination: "finish".into(),
    }
    .attach(&mut scene.objects[i]);
    project.revert_property(&mut scene, exit, &path).unwrap();
    assert_eq!(
        RelayGate::read(&scene.objects[i]).unwrap().unwrap(),
        RelayGate {
            role: Role::Exit,
            destination: "vesper".into()
        }
    );
}
