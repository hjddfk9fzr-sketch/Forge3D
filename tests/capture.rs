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
