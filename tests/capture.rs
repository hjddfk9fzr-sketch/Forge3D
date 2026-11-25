use forge3d::{capture, runtime::Scene};

#[test]
fn software_preview_draws_the_scene_and_obeys_visibility() {
    let mut scene = Scene::demo();
    let png = |scene: &Scene| {
        image::load_from_memory(&capture::png(scene, 160, 96).unwrap())
            .unwrap()
            .into_rgb8()
    };
    let visible = png(&scene);
    assert!(visible.pixels().any(|pixel| pixel.0 != [22, 28, 42]));
    for object in &mut scene.objects {
        object.visible = false;
    }
    let hidden = png(&scene);
    assert!(hidden.pixels().all(|pixel| pixel.0 == [22, 28, 42]));
    assert!(capture::png(&scene, 10_000, 10_000).is_err());
}

#[test]
fn skybox_follows_the_camera_and_presets_survive_scene_serialization() {
    use forge3d::lighting::Skybox;
    let mut scene = Scene::empty();
    let mut skies = Vec::new();
    for preset in [Skybox::Day, Skybox::Sunset, Skybox::Night] {
        scene.lighting.skybox = preset;
        let saved = serde_json::to_vec(&scene).unwrap();
        let restored = Scene::decode(&saved).unwrap();
        skies.push(capture::png(&restored, 128, 96).unwrap());
    }
    assert_ne!(skies[0], skies[1]);
    assert_ne!(skies[1], skies[2]);
    scene.camera.target[0] += 20.;
    assert_eq!(capture::png(&scene, 128, 96).unwrap(), skies[2]);
    scene.camera.yaw += 90.;
    assert_ne!(capture::png(&scene, 128, 96).unwrap(), skies[2]);
}
