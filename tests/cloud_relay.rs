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

#[test]
fn courier_emits_footsteps_only_when_traversing_the_deck_in_play_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("relay.forge-project.json");
    let mut project = Project::cloud_relay().unwrap();
    let mut amber = project.scene("amber").unwrap();
    let courier = amber
        .objects
        .iter_mut()
        .find(|o| o.name == "Courier")
        .unwrap();
    // A local marker override must survive saving and delivery to Rhai.
    courier.animation_markers.insert(
        "Walk".into(),
        vec![forge3d::animation::Marker {
            name: "footstep".into(),
            time: 0.125,
        }],
    );
    courier.script.as_mut().unwrap().push_str(r#"
      fn on_event(object, event) {
        if event.kind == "animation" && event.clip == "Walk" && event.name == "footstep" && event.time == 0.125 {
            object.state["steps"] = (object.state["steps"] ?? 0) + 1;
            object.sx += 0.001;
        }
        object
      }"#);
    project.update_scene("amber", &amber).unwrap();
    project.save(&file, false).unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let output = dir.path().join("export");
    export::collection(&game, &output).unwrap();
    let exported = Game::decode(
        &std::fs::read(output.join("assets/scene.json")).unwrap(),
        Some(&std::fs::read(output.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    for game in [game, exported] {
        let mut play = GamePlayback::new(game).unwrap();
        play.ui_action("start").unwrap();
        play.tick(Vec2::ZERO, STEP);
        let actor = |p: &GamePlayback| {
            p.scene()
                .objects
                .iter()
                .find(|o| o.name == "Courier")
                .unwrap()
                .clone()
        };
        for _ in 0..20 {
            play.tick(Vec2::ZERO, STEP);
            assert!(play.take_sounds().is_empty());
        }
        assert_eq!(actor(&play).animation.unwrap().time, 0.);
        let mut sounds = 0;
        for _ in 0..12 {
            play.tick(Vec2::NEG_Y * 0.55, STEP);
            sounds += play
                .take_sounds()
                .iter()
                .filter(|s| s.as_str() == "footstep")
                .count();
        }
        assert_eq!(sounds, 1);
        assert!((actor(&play).scale[0] - 0.721).abs() < 1e-5);
        let before = actor(&play).animation.unwrap().time;
        let mut input = InputControls::default();
        input.pressed.insert(Key::Space);
        play.tick_input(Vec2::NEG_Y * 0.55, &input, STEP);
        assert!(play.take_sounds().is_empty());
        for _ in 0..12 {
            play.tick(Vec2::NEG_Y * 0.55, STEP);
            assert!(play.take_sounds().is_empty());
        }
        assert_eq!(actor(&play).animation.unwrap().time, before);
        assert!(play.take_script_errors().is_empty());
    }
}

#[test]
fn relay_switches_idle_walk_run_and_mutes_the_outgoing_clip() {
    use forge3d::animation::Marker;
    let project = Project::cloud_relay().unwrap();
    let mut game = project.game(None).unwrap();
    let amber = game.scenes.get_mut("amber").unwrap();
    let courier = amber
        .objects
        .iter_mut()
        .find(|o| o.name == "Courier")
        .unwrap();
    courier.animation_markers.insert(
        "Walk".into(),
        vec![Marker {
            time: 0.30,
            name: "footstep".into(),
        }],
    );
    courier.animation_markers.insert(
        "Run".into(),
        vec![Marker {
            time: 0.05,
            name: "footstep".into(),
        }],
    );
    let mut play = GamePlayback::new(game).unwrap();
    play.ui_action("start").unwrap();
    for _ in 0..20 {
        play.tick(Vec2::ZERO, STEP);
    }
    let clip = |p: &GamePlayback| {
        p.scene()
            .objects
            .iter()
            .find(|o| o.name == "Courier")
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .clip
            .clone()
    };
    assert_eq!(clip(&play), "Idle");
    for _ in 0..13 {
        play.tick(Vec2::NEG_Y, STEP);
        assert_eq!(clip(&play), "Walk");
        assert!(play.take_sounds().is_empty());
    }
    let mut run_steps = 0;
    for _ in 0..10 {
        play.tick(Vec2::NEG_Y, STEP);
        run_steps += play
            .take_sounds()
            .iter()
            .filter(|id| id.as_str() == "footstep")
            .count();
    }
    assert_eq!(clip(&play), "Run");
    assert_eq!(run_steps, 1);
    play.tick(Vec2::ZERO, STEP);
    assert_eq!(clip(&play), "Idle");
    play.tick(Vec2::NEG_Y * 0.5, STEP);
    assert_eq!(clip(&play), "Walk");
    assert!(play.take_sounds().is_empty());
    assert!(play.take_script_errors().is_empty());
}

#[test]
fn deck_normal_map_and_its_override_survive_reimport_save_and_export() {
    use forge3d::{runtime::Shape, workspace::ModelOwner};
    let mut project = Project::cloud_relay().unwrap();
    let mut amber = project.scene("amber").unwrap();
    let deck = amber
        .objects
        .iter_mut()
        .find(|o| o.name == "Landing deck")
        .unwrap();
    let deck_id = deck.id;
    deck.normal_strength = Some(0.);
    project.update_scene("amber", &amber).unwrap();
    let id = project
        .dependencies(ModelOwner::Scene("amber"))
        .unwrap()
        .into_iter()
        .find_map(|(asset, name)| {
            if name == "Ribbed deck" {
                match asset {
                    forge3d::workspace::SharedAsset::Model(id) => Some(id),
                    _ => None,
                }
            } else {
                None
            }
        })
        .unwrap();
    let mut model = project.models()[&id].clone();
    std::sync::Arc::make_mut(&mut model.parts)[0]
        .normal_map
        .as_mut()
        .unwrap()
        .strength = 0.75;
    project.reimport_model(id, model).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("relay.forge-project.json");
    project.save(&file, false).unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let output = temporary.path().join("game");
    export::collection(&game, &output).unwrap();
    let exported = Game::decode(
        &std::fs::read(output.join("assets/scene.json")).unwrap(),
        Some(&std::fs::read(output.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    for name in ["amber", "vesper"] {
        let scene = &exported.scenes[name];
        let deck = &scene.objects[scene.object_index(deck_id).unwrap()];
        let Shape::Model(index) = deck.shape else {
            panic!("expected a deck model")
        };
        let map = scene.models[index].parts[0].normal_map.as_ref().unwrap();
        assert_eq!(map.strength, 0.75);
        assert_eq!(
            deck.normal_strength,
            if name == "amber" { Some(0.) } else { None }
        );
        assert!(image::load_from_memory(&map.texture).is_ok());
    }
}

#[test]
fn sky_lighting_and_both_shadow_profiles_survive_cloud_relay_export() {
    use forge3d::lighting::{Lighting, ShadowProfile, Skybox};
    let legacy: Lighting = serde_json::from_value(serde_json::json!({
        "skybox": "sunset", "shadow_extent": 18.0
    }))
    .unwrap();
    assert_eq!(legacy.environment_strength, 0.);
    assert_eq!(legacy.shadow_settings(), (1024, 18.));

    let mut project = Project::cloud_relay().unwrap();
    let mut amber = project.scene("amber").unwrap();
    amber.lighting.skybox = Skybox::Day;
    amber.lighting.environment_strength = 1.4;
    amber.lighting.shadow_extent = 22.;
    amber.lighting.shadow_resolution = 2048;
    amber.lighting.shadow_profile = ShadowProfile::Mobile;
    amber.lighting.mobile_shadow_resolution = 256;
    amber.lighting.mobile_shadow_extent = 7.;
    assert_eq!(amber.lighting.shadow_settings(), (256, 7.));
    project.update_scene("amber", &amber).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("relay.forge-project.json");
    project.save(&file, false).unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let output = temporary.path().join("game");
    export::collection(&game, &output).unwrap();
    let exported = Game::decode(
        &std::fs::read(output.join("assets/scene.json")).unwrap(),
        Some(&std::fs::read(output.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    let mut light = exported.scenes["amber"].lighting.clone();
    assert_eq!(light, amber.lighting);
    light.shadow_profile = ShadowProfile::Desktop;
    assert_eq!(light.shadow_settings(), (2048, 22.));
    assert_eq!(
        exported.scenes["vesper"].lighting.shadow_settings(),
        (512, 9.)
    );
    light.shadow_resolution = 123;
    assert!(!light.validate());
}

#[test]
fn basalt_lod_hysteresis_and_collision_are_independent() {
    use forge3d::{
        game::Game,
        physics::Body,
        runtime::{Object, Scene, Shape},
    };
    let project = Project::cloud_relay().unwrap();
    let model = project
        .models()
        .values()
        .find(|model| model.name == "Basalt column")
        .unwrap();
    assert_eq!(model.lods.len(), 2);
    assert_eq!(model.lod_level(100., Some(0)), 2);
    assert_eq!(model.lod_level(1., Some(2)), 0);
    let mut level = 0;
    for d in [13.9, 14.1, 13.9, 14.2] {
        level = model.lod_level(d, Some(level));
        assert_eq!(level, 0);
    }
    assert_eq!(model.lod_level(15.5, Some(level)), 1);
    assert_eq!(model.lod_level(14., Some(1)), 1);
    assert_eq!(model.lod_level(12.5, Some(1)), 0);
    assert!(model.parts_at(1)[0].indices.len() < model.parts_at(0)[0].indices.len());
    assert!(model.parts_at(2)[0].indices.len() < model.parts_at(1)[0].indices.len());

    let mut scene = Scene::empty();
    scene.models.push(model.clone());
    let mut rock = Object::new("Collidable rock", Shape::Model(0));
    rock.body = Some(Body::default());
    let id = scene.add_object(rock).unwrap();
    let play = GamePlayback::new(Game::single(scene)).unwrap();
    for distance in [1., 100.] {
        let level = model.lod_level(distance, None);
        assert_eq!(level, if distance < 2. { 0 } else { 2 });
        let hit = play
            .raycast([0., 0., distance], [0., 0., -1.], distance + 1., None)
            .unwrap()
            .unwrap();
        assert_eq!(hit.object_id, id);
        assert!((hit.point[2] - 0.5).abs() < 1e-5);
    }
}

#[test]
fn editing_shared_lod_survives_cli_save_and_game_export() {
    let mut project = Project::cloud_relay().unwrap();
    let id = *project
        .models()
        .iter()
        .find(|(_, m)| m.name == "Basalt column")
        .unwrap()
        .0;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("relay.forge-project.json");
    project.save(&file, false).unwrap();
    let mesh = temp.path().join("lower.obj");
    std::fs::write(&mesh, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_forge3d-batch"))
        .arg("project-model-lod")
        .arg(&file)
        .arg(id.to_string())
        .arg("24")
        .arg(&mesh)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    project = Project::load(&file).unwrap();
    let model = &project.models()[&id];
    assert_eq!(model.lods[1].parts[0].indices.len(), 3);
    let mut invalid = model.clone();
    std::sync::Arc::make_mut(&mut invalid.lods)[1].parts = invalid.parts.clone();
    assert!(project.reimport_model(id, invalid).is_err());
    assert_eq!(project.models()[&id].lods[1].parts[0].indices.len(), 3);

    let out = temp.path().join("game");
    export::collection(&project.game(None).unwrap(), &out).unwrap();
    let game = Game::decode(
        &std::fs::read(out.join("assets/scene.json")).unwrap(),
        Some(&std::fs::read(out.join("assets/scenes.json")).unwrap()),
    )
    .unwrap();
    for id in ["launch", "amber", "vesper"] {
        let model = game.scenes[id]
            .models
            .iter()
            .find(|m| m.name == "Basalt column")
            .unwrap();
        assert_eq!(model.lods[1].distance, 24.);
        assert_eq!(model.lods[1].parts[0].indices.len(), 3);
    }
}
