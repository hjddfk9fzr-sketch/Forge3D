use forge3d::{
    export,
    physics::{Body, BodyKind, Event, EventKind, STEP},
    runtime::{Object, Playback, Scene, Shape},
};
use macroquad::prelude::Vec2;
use std::fs;

fn contains(events: &[Event], kind: EventKind, a: u64, b: u64) -> bool {
    events.iter().any(|event| {
        event.kind == kind && ((event.a, event.b) == (a, b) || (event.b, event.a) == (a, b))
    })
}

#[test]
fn rover_lands_passes_trigger_and_stops_at_barrier() {
    let scene = Scene::physics_demo();
    let [floor, rover, sensor, barrier] = std::array::from_fn(|index| scene.objects[index].id);
    let mut play = Playback::new(scene.clone()).unwrap();
    for _ in 0..90 {
        play.tick(Vec2::ZERO, STEP);
    }
    let position = play.scene().objects[1].position;
    assert!((0.48..=0.52).contains(&position[1]), "{position:?}");
    assert!(contains(
        &play.take_events().0,
        EventKind::CollisionEnter,
        floor,
        rover
    ));

    for _ in 0..58 {
        play.tick(Vec2::X, STEP);
    }
    let position = play.scene().objects[1].position;
    assert!(contains(
        &play.take_events().0,
        EventKind::TriggerEnter,
        sensor,
        rover
    ));
    assert!(play.scene().objects[2].visible);
    assert!((play.scene().camera.target[0] - position[0] * 0.6).abs() < 0.001);

    for _ in 0..150 {
        play.tick(Vec2::X, STEP);
    }
    let events = play.take_events().0;
    assert!(contains(&events, EventKind::TriggerExit, sensor, rover));
    assert!(contains(&events, EventKind::CollisionEnter, barrier, rover));
    assert!(!play.scene().objects[2].visible);
    let position = play.scene().objects[1].position;
    assert!((1.25..=1.35).contains(&position[0]), "{position:?}");
    assert!(play.take_script_errors().is_empty());
    assert_eq!(scene.objects[1].position, [-3.5, 2., 0.]);
}

#[test]
fn exported_physics_scene_runs_at_the_same_fixed_step() {
    let scene = Scene::physics_demo();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("export");
    export::game(&scene, &target).unwrap();
    assert!(target.join("src/physics.rs").is_file());
    assert!(target.join("src/controls.rs").is_file());
    let exported = Scene::decode(&fs::read(target.join("assets/scene.json")).unwrap()).unwrap();
    let mut one_step = Playback::new(scene).unwrap();
    let mut two_steps = Playback::new(exported).unwrap();
    for _ in 0..100 {
        one_step.tick(Vec2::X, STEP);
        one_step.tick(Vec2::X, STEP);
        two_steps.tick(Vec2::X, STEP * 2.);
    }
    assert_eq!(one_step.scene(), two_steps.scene());
    assert_eq!(one_step.take_events(), two_steps.take_events());
}

#[test]
fn dynamic_bodies_and_camera_reference_are_validated() {
    let mut scene = Scene::empty();
    let group = scene
        .add_object(Object::new("Group", Shape::Empty))
        .unwrap();
    let mut cube = Object::new("Cube", Shape::Cube);
    cube.body = Some(Body {
        kind: BodyKind::Dynamic,
        ..Body::default()
    });
    let cube = scene.add_object(cube).unwrap();
    assert!(scene.set_parent(cube, Some(group)).is_err());
    scene.camera_controller = Some(cube);
    assert!(Playback::new(scene.clone()).is_err());
    scene.delete_object(cube, false).unwrap();
    assert!(scene.camera_controller.is_none());
    assert!(Playback::new(scene).is_ok());
}

#[test]
fn kinematic_platform_moves_a_dynamic_body() {
    let mut scene = Scene::empty();
    let mut platform = Object::new("Lift", Shape::Cube);
    platform.scale = [4., 0.5, 4.];
    platform.body = Some(Body {
        kind: BodyKind::Kinematic,
        ..Body::default()
    });
    platform.script = Some("fn update(object, dt, input) { object.y += dt * 0.6; object }".into());
    let lift = scene.add_object(platform).unwrap();
    let mut cube = Object::new("Passenger", Shape::Cube);
    cube.position = [0., 0.8, 0.];
    cube.body = Some(Body {
        kind: BodyKind::Dynamic,
        lock_rotation: true,
        ..Body::default()
    });
    let passenger = scene.add_object(cube).unwrap();
    let mut play = Playback::new(scene).unwrap();
    for _ in 0..120 {
        play.tick(Vec2::ZERO, STEP);
    }
    let objects = &play.scene().objects;
    assert!((1.18..=1.22).contains(&objects[0].position[1]));
    assert!((1.9..=2.0).contains(&objects[1].position[1]));
    assert!(contains(
        &play.take_events().0,
        EventKind::CollisionEnter,
        lift,
        passenger
    ));
}

#[test]
fn ray_uses_world_units_stable_ids_and_solid_colliders() {
    let mut scene = Scene::empty();
    let mut ids = Vec::new();
    for (z, sensor) in [(0., false), (1., true), (3., false), (5., false)] {
        let mut object = Object::new("Collider", Shape::Cube);
        object.position[2] = z;
        object.body = Some(Body {
            sensor,
            ..Body::default()
        });
        ids.push(scene.add_object(object).unwrap());
    }
    let play = Playback::new(scene).unwrap();
    let ray = |range, ignore| play.raycast([0.; 3], [0., 0., 8.], range, ignore).unwrap();
    assert_eq!(ray(8., None).unwrap().object_id, ids[0]);
    assert!(ray(2., Some(ids[0])).is_none());
    let hit = ray(8., Some(ids[0])).unwrap();
    assert_eq!(hit.object_id, ids[2]);
    assert_eq!(hit.point, [0., 0., 2.5]);
    assert_eq!(hit.normal, [0., 0., -1.]);
    assert_eq!(hit.distance, 2.5);
    assert!(play.raycast([0.; 3], [0.; 3], 1., None).is_err());
}

#[test]
fn reusable_character_lands_jumps_once_and_slides_along_walls() {
    use forge3d::controls::{InputControls, Key};
    let mut scene = Scene::empty();
    let mut hero = Object::new("Character", Shape::Cube);
    hero.position = [0., 2., 0.];
    hero.speed = 3.;
    hero.body = Some(Body::character());
    scene.add_object(hero).unwrap();
    for (name, position, size) in [
        ("floor", [0., -0.5, 0.], [20., 1., 20.]),
        ("wall", [1.5, 1., 0.], [0.2, 2., 16.]),
    ] {
        let mut obj = Object::new(name, Shape::Cube);
        obj.position = position;
        obj.body = Some(Body {
            size,
            ..Default::default()
        });
        scene.add_object(obj).unwrap();
    }
    let mut play = Playback::new(scene).unwrap();
    for _ in 0..90 {
        play.tick(Vec2::ZERO, STEP);
    }
    let floor = play.scene().objects[0].position[1];
    assert!((0.495..0.53).contains(&floor), "{floor}");
    let mut controls = InputControls::default();
    controls.pressed.insert(Key::Space);
    play.tick_input(Vec2::ZERO, &controls, STEP * 0.5);
    play.tick(Vec2::ZERO, STEP * 0.5);
    let mut apex = floor;
    for i in 0..90 {
        if i == 7 {
            play.tick_input(Vec2::ZERO, &controls, STEP);
        } else {
            play.tick(Vec2::ZERO, STEP);
        }
        apex = apex.max(play.scene().objects[0].position[1]);
    }
    assert!(
        (0.9..1.05).contains(&(apex - floor)),
        "apex {apex} floor {floor}"
    );
    assert!((play.scene().objects[0].position[1] - floor).abs() < 0.02);
    for _ in 0..90 {
        play.tick(Vec2::ONE, STEP);
    }
    let [x, _, z] = play.scene().objects[0].position;
    assert!((1.05..1.12).contains(&x) && z > 2.5, "{x}, {z}");
}

#[test]
fn character_climbs_only_walkable_slopes() {
    let height = |limit| {
        let mut scene = Scene::empty();
        let mut floor = Object::new("Ramp", Shape::Cube);
        floor.rotation[2] = 25.;
        floor.scale = [12., 0.25, 4.];
        floor.body = Some(Body::default());
        scene.add_object(floor).unwrap();
        let mut character = Object::new("Character", Shape::Sphere);
        character.position = [0., 0.85, 0.];
        character.speed = 2.;
        character.body = Some(Body {
            max_slope_degrees: limit,
            ..Body::character()
        });
        scene.add_object(character).unwrap();
        let mut play = Playback::new(scene).unwrap();
        for _ in 0..60 {
            play.tick(Vec2::X, STEP);
        }
        play.scene().objects[1].position[1]
    };
    let climb = height(45.);
    let slide = height(12.);
    assert!(climb > 1.1 && slide < 0.85, "climb {climb}, slide {slide}");
}
