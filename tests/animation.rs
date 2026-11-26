use forge3d::{
    capture, import,
    runtime::{Object, Playback, Scene, Shape},
};
use macroquad::prelude::{Mat4, Vec2};
use serde_json::{json, Value};
use std::fs;

#[test]
fn imports_and_samples_a_skinned_gltf_material() {
    let temp = tempfile::tempdir().unwrap();
    let mut buffer = Vec::new();
    let mut views = Vec::new();
    let mut append = |data: Vec<u8>| {
        let offset = buffer.len();
        buffer.extend(data);
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":buffer.len()-offset}));
    };
    let bytes = |values: &[f32]| {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    };
    append(bytes(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    append(vec![0; 24]);
    append(bytes(&[1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.]));
    append(bytes(&Mat4::IDENTITY.to_cols_array()));
    append(bytes(&[0., 1.]));
    append(bytes(&[0., 0., 0., 0., 2., 0.]));
    append(bytes(&[0., 0., 1., 0., 0., 1.]));
    let accessor = |view, count, kind, component| json!({"bufferView":view,"count":count,"type":kind,"componentType":component});
    let mut position = accessor(0, 3, "VEC3", 5126);
    position["min"] = json!([0, 0, 0]);
    position["max"] = json!([1, 1, 0]);
    let mut times = accessor(4, 2, "SCALAR", 5126);
    times["min"] = json!([0]);
    times["max"] = json!([1]);
    let gltf = json!({
        "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
        "buffers":[{"uri":"arm.bin","byteLength":buffer.len()}],"bufferViews":views,
        "accessors":[position,accessor(1,3,"VEC4",5123),accessor(2,3,"VEC4",5126),accessor(3,1,"MAT4",5126),times,accessor(5,2,"VEC3",5126),accessor(6,3,"VEC2",5126)],
        "nodes":[{"mesh":0,"skin":0},{"name":"Joint"}],
        "skins":[{"joints":[1],"inverseBindMatrices":3}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2,"TEXCOORD_0":6},"material":0}]}],
        "images":[{"uri":"packed.png"}],"textures":[{"source":0}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.6,0.75,0.8,1.0],"metallicFactor":0.8,"roughnessFactor":0.3,
            "metallicRoughnessTexture":{"index":0}},"emissiveFactor":[0.05,0.0,0.25],"emissiveTexture":{"index":0},"normalTexture":{"index":0}}],
        "animations":[{"name":"Lift","samplers":[{"input":4,"output":5,"interpolation":"LINEAR"}],
            "channels":[{"sampler":0,"target":{"node":1,"path":"translation"}}]}]
    });
    fs::write(temp.path().join("arm.bin"), buffer).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 64, 128, 255]))
        .save(temp.path().join("packed.png"))
        .unwrap();
    let path = temp.path().join("arm.gltf");
    fs::write(&path, gltf.to_string()).unwrap();
    let model = import::model(&path).unwrap();
    assert_eq!(model.parts[0].surface.roughness, 0.3);
    assert_eq!(model.parts[0].surface.metallic, 0.8);
    assert_eq!(model.parts[0].surface.emissive, [0.05, 0., 0.25]);
    let texture = model.parts[0].metallic_roughness.as_deref().unwrap();
    assert_eq!(
        image::load_from_memory(texture)
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0,
        [255, 64, 128, 255]
    );
    assert!(model.parts[0].emissive_texture.is_some());
    assert_eq!(
        model.parts[0].normal_map.as_ref().unwrap().tangents.len(),
        model.parts[0].influences.len()
    );
    let mut scene = Scene::empty();
    scene.models.push(model);
    scene
        .add_object(Object::new("Arm", Shape::Model(0)))
        .unwrap();
    let mut play = Playback::new(scene).unwrap();
    for _ in 0..5 {
        play.tick(Vec2::ZERO, 0.1);
    }
    let scene = play.scene();
    let model = &scene.models[0];
    let rig = model.rig.as_ref().unwrap();
    let vertex = rig
        .pose(scene.objects[0].animation.as_ref())
        .vertex(&model.parts[0], 0);
    assert!((vertex.position[1] - 1.).abs() < 0.001, "{vertex:?}");
    assert!(play.take_script_errors().is_empty());
}

#[test]
fn script_can_switch_and_pause_clips_and_preview_uses_the_pose() {
    let mut scene = Scene::rendering_demo();
    let original = capture::png(&scene, 180, 120).unwrap();
    scene.objects[1].script = Some(
        r#"
        fn update(object, dt, input) {
            object.anim_clip = "Reach";
            object.anim_time = 0.6;
            object.anim_playing = false;
            object
        }
    "#
        .into(),
    );
    let mut play = Playback::new(scene).unwrap();
    play.tick(Vec2::ZERO, 0.1);
    let animation = play.scene().objects[1].animation.as_ref().unwrap();
    assert_eq!(animation.clip, "Reach");
    assert_eq!(animation.time, 0.6);
    assert!(!animation.playing);
    assert!(play.take_script_errors().is_empty());
    assert_ne!(original, capture::png(play.scene(), 180, 120).unwrap());
    let saved = serde_json::to_value(play.scene()).unwrap();
    let loaded = Scene::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(saved, serde_json::to_value(loaded).unwrap());
    assert_ne!(saved["models"][0]["rig"], Value::Null);
}
