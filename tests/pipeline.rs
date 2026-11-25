use forge3d::{
    export, import,
    runtime::{self, Object, Scene, Shape},
};
use macroquad::prelude::Vec2;
use serde_json::json;
use std::fs;

#[test]
fn obj_resources_round_trip_into_a_standalone_game() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("triangle.obj");
    fs::write(
        &source,
        "mtllib triangle.mtl\no Triangle\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl Paint\nf 1/1 2/2 3/3\n",
    ).unwrap();
    fs::write(
        temp.path().join("triangle.mtl"),
        "newmtl Paint\nKd 0.8 0.5 0.2\nmap_Kd paint.png\n",
    )
    .unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 50, 90, 255]))
        .save(temp.path().join("paint.png"))
        .unwrap();

    let model = import::model(&source).unwrap();
    assert_eq!(model.parts[0].indices.len(), 3);
    assert!(model.parts[0].texture.is_some());
    assert_eq!(model.parts[0].vertices[0].normal, [0., 0., 1.]);

    let mut scene = Scene::empty();
    scene.models.push(model);
    let mut object = Object::new("Triangle", Shape::Model(0));
    object.speed = 2.;
    object.spin = 60.;
    scene.objects.push(object);

    let destination = temp.path().join("game");
    export::game(&scene, &destination).unwrap();
    assert!(export::game(&Scene::empty(), &destination).is_err());
    assert!(destination.join("src/runtime.rs").is_file());
    assert!(destination.join("Cargo.toml").is_file());
    assert!(destination.join("android/build.gradle").is_file());
    assert!(destination
        .join("android/src/main/AndroidManifest.xml")
        .is_file());

    let mut game_scene =
        Scene::decode(&fs::read(destination.join("assets/scene.json")).unwrap()).unwrap();
    assert!(game_scene.models[0].parts[0].texture.is_some());
    runtime::update(&mut game_scene, Vec2::X, 0.1);
    assert_eq!(game_scene.objects[0].position, [0.2, 0., 0.]);
    assert_eq!(game_scene.objects[0].rotation, [0., 6., 0.]);
}

#[test]
fn obj_reports_missing_material_and_resolves_textures_relative_to_it() {
    let temp = tempfile::tempdir().unwrap();
    let materials = temp.path().join("materials");
    fs::create_dir(&materials).unwrap();
    let obj = temp.path().join("triangle.obj");
    fs::write(
        &obj,
        "mtllib materials/paint.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl Paint\nf 1 2 3\n",
    )
    .unwrap();
    let error = import::model(&obj).unwrap_err();
    assert!(error.contains("paint.mtl"), "{error}");
    fs::write(
        materials.join("paint.mtl"),
        "newmtl Paint\nmap_Kd paint.png\n",
    )
    .unwrap();
    let error = import::model(&obj).unwrap_err();
    assert!(error.contains("paint.png"), "{error}");
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 50, 90, 255]));
    image.save(materials.join("paint.png")).unwrap();
    assert!(import::model(&obj).unwrap().parts[0].texture.is_some());
    fs::rename(materials.join("paint.png"), temp.path().join("paint.png")).unwrap();
    assert!(import::model(&obj).unwrap().parts[0].texture.is_some());
}

#[test]
fn gltf_and_glb_preserve_node_transforms() {
    let temp = tempfile::tempdir().unwrap();
    let positions: Vec<u8> = [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let mut document = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"translation": [2, 3, 4], "mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "buffers": [{"byteLength": positions.len()}],
        "bufferViews": [{"buffer": 0, "byteLength": positions.len()}],
        "accessors": [{
            "bufferView": 0, "componentType": 5126, "count": 3,
            "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]
        }]
    });

    let mut json_bytes = serde_json::to_vec(&document).unwrap();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut glb = b"glTF\x02\x00\x00\x00".to_vec();
    glb.extend(((28 + json_bytes.len() + positions.len()) as u32).to_le_bytes());
    glb.extend((json_bytes.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json_bytes);
    glb.extend((positions.len() as u32).to_le_bytes());
    glb.extend(b"BIN\x00");
    glb.extend(&positions);
    fs::write(temp.path().join("triangle.glb"), glb).unwrap();

    document["buffers"][0]["uri"] = json!("triangle.bin");
    fs::write(temp.path().join("triangle.gltf"), document.to_string()).unwrap();
    let missing = import::model(&temp.path().join("triangle.gltf")).unwrap_err();
    assert!(missing.contains("triangle.bin"), "{missing}");
    fs::write(temp.path().join("triangle.bin"), positions).unwrap();

    for file in ["triangle.gltf", "triangle.glb"] {
        let model = import::model(&temp.path().join(file)).unwrap();
        assert_eq!(model.parts[0].indices, [0, 1, 2]);
        assert_eq!(model.parts[0].vertices[0].position, [2., 3., 4.]);
        assert_eq!(model.parts[0].vertices[0].normal, [0., 0., 1.]);
    }
}
