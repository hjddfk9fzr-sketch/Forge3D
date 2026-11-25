use forge3d::{
    export, import, project,
    runtime::{Object, Scene, Shape},
};
use macroquad::prelude::Vec3;
use std::fs;

const ASCII: &str = include_str!("fixtures/model-ascii.fbx");

#[test]
fn ascii_fbx_triangulates_and_preserves_transforms_normals_uvs_and_material() {
    let file = std::path::Path::new("tests/fixtures/model-ascii.fbx");
    let model = import::model(file).unwrap();
    assert_eq!(model.parts.len(), 1);
    let part = &model.parts[0];
    assert_eq!(part.indices.len(), 6);
    assert_eq!(part.surface.base_color, [0.75, 0.25, 0.125, 1.]);
    assert!(part
        .vertices
        .iter()
        .all(|v| (2. ..=3.).contains(&v.position[0])
            && (2. ..=3.).contains(&v.position[1])
            && v.normal[2] > 0.99));
    assert!(part.vertices.iter().any(|vertex| vertex.uv == [1., 0.]));
    for triangle in part.vertices.as_chunks::<3>().0 {
        let p = |i: usize| Vec3::from_array(triangle[i].position);
        assert!((p(1) - p(0)).cross(p(2) - p(0)).z > 0.);
    }
}

#[test]
fn binary_fbx_embedded_texture_round_trips_to_rust_export() {
    let model = import::model(std::path::Path::new("tests/fixtures/model-binary.fbx")).unwrap();
    let part = &model.parts[0];
    assert_eq!(part.indices.len(), 3);
    let image = image::load_from_memory(part.texture.as_ref().unwrap()).unwrap();
    assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [0x8c, 0x42, 0x23, 0x7f]);

    let mut scene = Scene::empty();
    scene.models.push(model);
    scene
        .add_object(Object::new("Triangle", Shape::Model(0)))
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("game");
    export::game(&scene, &output).unwrap();
    let current = project::load(&output.join("assets/scene.json")).unwrap();
    assert_eq!(current.models, scene.models);
    assert!(!fs::read_to_string(output.join("Cargo.toml"))
        .unwrap()
        .contains("ufbx"));
}

#[test]
fn fbx_external_texture_must_be_inside_the_mcp_project() {
    let directory = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap().join("root");
    fs::create_dir(&root).unwrap();
    let fixture = |name: &str| {
        ASCII
            .replace("\r\n", "\n")
            .replace(
                "\n}\nConnections:",
                &format!(
                    r#"
    Texture: 400, "Texture::Paint", "" {{
        Type: "TextureVideoClip"
        Version: 202
        FileName: "{name}"
        RelativeFilename: "{name}"
    }}
}}
Connections:"#
                ),
            )
            .replace(
                "C: \"OO\",300,201",
                "C: \"OO\",300,201\n    C: \"OP\",400,300,\"DiffuseColor\"",
            )
    };
    let picture = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 200, 100, 255]));
    picture.save(directory.path().join("outside.png")).unwrap();
    let file = root.join("scene.fbx");
    fs::write(&file, fixture("../outside.png")).unwrap();
    assert!(import::model_under(&file, &root)
        .unwrap_err()
        .contains("inside the model's project"));

    picture.save(root.join("sidecar.png")).unwrap();
    fs::write(&file, fixture("sidecar.png")).unwrap();
    let model = import::model_under(&file, &root).unwrap();
    assert!(model.parts[0].texture.is_some());
    assert_eq!(project::entries(&root).unwrap()[0].kind, "model");
}
