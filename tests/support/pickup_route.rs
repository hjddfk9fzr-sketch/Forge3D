use macroquad::prelude::Vec2;

fn check_pickups(mut play: GamePlayback) {
    for _ in 0..3 {
        play.tick(Vec2::ZERO, 1. / 60.);
    }
    let count = |play: &GamePlayback| {
        play.scene()
            .objects
            .iter()
            .find(|o| o.name == "Collector")
            .unwrap()
            .components["workshop.inventory"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(count(&play), 1);
    assert!(play.scene().objects.iter().any(|o| o.name == "Near token"));
    assert!(!play.scene().objects.iter().any(|o| o.name == "Far token"));
    for _ in 0..65 {
        play.tick(Vec2::X, 1. / 60.);
    }
    assert_eq!(count(&play), 2);
    assert!(!play.scene().objects.iter().any(|o| o.name == "Near token"));
    assert!(play.take_script_errors().is_empty());
}
