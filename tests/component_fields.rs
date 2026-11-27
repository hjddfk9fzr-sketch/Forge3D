use forge3d::{
    component::{Field, Kind},
    export,
    game::GamePlayback,
    runtime::{Object, Scene, Shape},
    workshop::component_schemas,
    workspace::Project,
};
use std::path::Path;
include!("support/pickup_route.rs");

fn token_project(dir: &Path) -> Project {
    let mut scene = Scene::empty();
    let mut collector = Object::new("Collector", Shape::Cube);
    collector.position = [0.; 3];
    collector.speed = 2.;
    collector
        .components
        .insert("workshop.inventory".into(), 0.into());
    scene.add_object(collector).unwrap();
    let mut project = Project::new("course", &scene).unwrap();
    let schema = component_schemas()
        .iter()
        .find(|s| s.name == "workshop.pickup")
        .unwrap();
    let mut template = Scene::empty();
    let mut token = Object::new("Token", Shape::Cube);
    token
        .components
        .insert(schema.name.clone(), schema.default_value());
    let root = template.add_object(token).unwrap();
    project.create_prefab("token", &template, root).unwrap();
    let path = vec!["components".into(), schema.name.clone()];
    for (name, radius) in [("Far token", 3.), ("Near token", 0.25)] {
        let id = project.instantiate("token", &mut scene, None).unwrap();
        let i = scene.object_index(id).unwrap();
        scene.objects[i].name = name.into();
        scene.objects[i].position = [2., 0., 0.];
        schema.fields[0].set(
            scene.objects[i].components.get_mut(&schema.name).unwrap(),
            radius.into(),
        );
        assert!(project
            .overrides(&scene, id)
            .unwrap()
            .iter()
            .any(|o| o.path == path));
    }
    project.update_scene("course", &scene).unwrap();
    let file = dir.join("tokens.forge-project.json");
    project.save(&file, false).unwrap();
    let project = Project::load(&file).unwrap();
    let mut scene = project.scene("course").unwrap();
    let i = scene
        .objects
        .iter()
        .position(|o| o.name == "Near token")
        .unwrap();
    let id = scene.objects[i].id;
    project.revert_property(&mut scene, id, &path).unwrap();
    assert_eq!(
        scene.objects[i].components[&schema.name],
        schema.default_value()
    );
    project
}

#[test]
fn fields_drive_play_and_restore_the_prefab_radius() {
    let dir = tempfile::tempdir().unwrap();
    let project = token_project(dir.path());
    check_pickups(GamePlayback::new(project.game(None).unwrap()).unwrap());
}

#[test]
fn authored_fields_share_defaults_and_validation() {
    use forge3d::component::Schema;
    let schema = Schema {
        name: "lever".into(),
        trigger: false,
        fields: vec![
            Field::new("enabled", "Enabled", Kind::Toggle(true)),
            Field::new(
                "strength",
                "Strength",
                Kind::Number {
                    default: 2.,
                    min: 1.,
                    max: 5.,
                },
            ),
        ],
    };
    let mut data = schema.default_value();
    assert_eq!(data, serde_json::json!({"enabled":true,"strength":2.}));
    schema.validate(&data, |_| false, |_| false).unwrap();
    schema.fields[1].set(&mut data, 6.into());
    assert!(schema.validate(&data, |_| false, |_| false).is_err());
}

#[test]
#[ignore = "compiles the standalone exported Cargo project"]
fn standalone_pickup_route() {
    let dir = tempfile::tempdir().unwrap();
    let project = token_project(dir.path());
    let out = dir.path().join("export");
    export::collection(&project.game(None).unwrap(), &out).unwrap();
    std::fs::create_dir(out.join("tests")).unwrap();
    std::fs::write(out.join("tests/route.rs"),concat!(
        "use forge_game::game::{Game,GamePlayback};\n",
        include_str!("support/pickup_route.rs"),
        "\n#[test] fn route(){let game=Game::decode(include_bytes!(\"../assets/scene.json\"), Some(include_bytes!(\"../assets/scenes.json\"))).unwrap();check_pickups(GamePlayback::new(game).unwrap());}")).unwrap();
    assert!(std::process::Command::new(env!("CARGO"))
        .current_dir(out)
        .args(["test", "--offline", "--test", "route"])
        .status()
        .unwrap()
        .success());
}

#[test]
#[ignore = "compiles the author workspace and executes its isolated game"]
fn author_component_is_discoverable_and_runs_with_the_exported_player() {
    use forge3d::{
        author, export,
        runtime::{Object, Scene, Shape},
        workspace::Project,
    };
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("spin.forge-project.json");
    let mut scene = Scene::empty();
    let mut object = Object::new("Spinner", Shape::Cube);
    object
        .components
        .insert("project.spin".into(), serde_json::json!({"degrees":90.}));
    scene.add_object(object).unwrap();
    Project::new("main", &scene)
        .unwrap()
        .save(&file, false)
        .unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let directory = export::initialize_rust(&game, &file).unwrap();
    let code = directory.join("game-code/src/lib.rs");
    let original = std::fs::read_to_string(&code).unwrap();
    let game = Project::load(&file).unwrap().game(None).unwrap();
    let player = author::compile(&game, true).unwrap();
    player.check().unwrap();
    let schema = player
        .schemas
        .iter()
        .find(|s| s.name == "project.spin")
        .unwrap();
    assert_eq!(schema.default_value()["degrees"], 60.);
    let out = temp.path().join("export");
    export::collection(&game, &out).unwrap();
    assert_eq!(
        std::fs::read_to_string(out.join("game-code/src/lib.rs")).unwrap(),
        original
    );
    std::fs::create_dir(out.join("tests")).unwrap();
    std::fs::write(out.join("tests/author.rs"),r#"
use forge_game::{game::{Game,GamePlayback},progress::Progress};
#[test]
fn authored_spin() {
    let game=Game::decode(include_bytes!("../assets/scene.json"),Some(include_bytes!("../assets/scenes.json"))).unwrap();
    let mut play=GamePlayback::with_systems(game,Progress::default(),forge_game::systems().unwrap()).unwrap();
    play.tick(macroquad::prelude::Vec2::ZERO,0.1);
    assert_eq!(play.scene().objects[0].spin,90.);
    play.tick(macroquad::prelude::Vec2::ZERO,0.1);
    assert_eq!(play.scene().objects[0].rotation[1],9.);
}
"#).unwrap();
    assert!(std::process::Command::new(env!("CARGO"))
        .current_dir(out)
        .args(["test", "--offline", "--test", "author"])
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .status()
        .unwrap()
        .success());
    std::fs::write(&code, format!("{original}\ninvalid rust statement;")).unwrap();
    let error = author::compile(&game, true).err().unwrap();
    assert!(
        error.contains("game-code/src/lib.rs") && error.contains("error"),
        "{error}"
    );
    std::fs::write(&code, original).unwrap();
}
