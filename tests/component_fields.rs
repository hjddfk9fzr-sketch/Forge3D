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
        .insert(schema.name.into(), schema.default_value());
    let root = template.add_object(token).unwrap();
    project.create_prefab("token", &template, root).unwrap();
    let path = vec!["components".into(), schema.name.into()];
    for (name, radius) in [("Far token", 3.), ("Near token", 0.25)] {
        let id = project.instantiate("token", &mut scene, None).unwrap();
        let i = scene.object_index(id).unwrap();
        scene.objects[i].name = name.into();
        scene.objects[i].position = [2., 0., 0.];
        schema.fields[0].set(
            scene.objects[i].components.get_mut(schema.name).unwrap(),
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
        scene.objects[i].components[schema.name],
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
    const SCHEMA: Schema = Schema {
        name: "lever",
        trigger: false,
        fields: &[
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
    let schema = &SCHEMA;
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
