//! A standalone two-stage platform game built with ordinary Forge3D scenes and prefabs.
use super::*;
use crate::{
    controls::Action,
    lighting::{Skybox, Surface},
    physics::Body,
    relay::{RelayGate, Role},
    runtime::View,
    settings::Application,
};

fn block(name: &str, position: [f32; 3], size: [f32; 3], color: [f32; 3], glow: bool) -> Object {
    let mut o = Object::new(name, Shape::Cube);
    o.position = position;
    o.scale = size;
    o.surface = Some(Surface {
        base_color: [color[0], color[1], color[2], 1.],
        unlit: glow,
        ..Default::default()
    });
    o
}

fn island(
    scene: &mut Scene,
    x: f32,
    z: f32,
    width: f32,
    length: f32,
    accent: [f32; 3],
) -> Result<(), String> {
    let mut deck = block(
        "Landing deck",
        [x, -0.22, z],
        [width, 0.44, length],
        [0.24, 0.34, 0.40],
        false,
    );
    deck.body = Some(Body::default());
    scene.add_object(deck)?;
    for side in [-1., 1.] {
        scene.add_object(block(
            "Edge light",
            [x + side * (width * 0.5 - 0.08), 0.015, z],
            [0.07, 0.03, length],
            accent,
            true,
        ))?;
    }
    for n in [-1., 0., 1.] {
        let z_mark = z + n * length * 0.29;
        for side in [-1., 1.] {
            let mut chevron = block(
                "Runway chevron",
                [x + side * 0.13, 0.018, z_mark],
                [0.07, 0.028, 0.39],
                accent,
                true,
            );
            chevron.rotation[1] = side * 42.;
            scene.add_object(chevron)?;
        }
    }
    for i in 0..3 {
        let taper = 1. - 0.21 * i as f32;
        let mut rock = block(
            "Floating basalt",
            [x, -0.85 - i as f32 * 0.79, z],
            [width * taper, 0.92, length * taper],
            [0.14, 0.20, 0.28],
            false,
        );
        rock.rotation[1] = i as f32 * 6.;
        scene.add_object(rock)?;
    }
    Ok(())
}

fn arch() -> Result<(Scene, u64), String> {
    let mut scene = Scene::empty();
    let mut gate = Object::new("Relay arch", Shape::Empty);
    gate.body = Some(Body {
        size: [1.8, 5.2, 0.8],
        sensor: true,
        ..Default::default()
    });
    RelayGate::default().attach(&mut gate);
    let root = scene.add_object(gate)?;
    for x in [-1., 1.] {
        let mut post = block(
            "Ceramic post",
            [x, 1.15, 0.],
            [0.18, 2.3, 0.22],
            [0.16, 0.26, 0.34],
            false,
        );
        post.parent = Some(root);
        post.body = Some(Body::default());
        scene.add_object(post)?;
    }
    let mut lamp = block(
        "Signal strip",
        [0., 2.32, 0.],
        [2.25, 0.15, 0.26],
        [0.28, 0.99, 0.87],
        true,
    );
    lamp.parent = Some(root);
    scene.add_object(lamp)?;
    Ok((scene, root))
}

fn course(project: &Project, app: &Application, second: bool) -> Result<Scene, String> {
    let mut scene = Scene::empty();
    scene.application = app.clone();
    scene.title = if second {
        "Vesper Crossing"
    } else {
        "Amber Reach"
    }
    .into();
    scene.lighting.skybox = if second {
        Skybox::Night
    } else {
        Skybox::Sunset
    };
    scene.lighting.ambient = 0.72;
    let x = if second { 1.35 } else { 0. };
    let color = if second {
        [0.69, 0.56, 1.]
    } else {
        [0.28, 0.99, 0.87]
    };
    for (cx, z, w, l) in [(0., 0., 4.2, 5.), (x, -6.5, 3.2, 4.6), (0., -13., 4.2, 5.)] {
        island(&mut scene, cx, z, w, l, color)?;
    }
    for (cx, z) in [(x, -6.5), (0., -13.)] {
        let id = project.instantiate("relay-arch", &mut scene, None)?;
        let index = scene.object_index(id)?;
        scene.objects[index].position = [cx, 0., z];
        if z == -13. {
            scene.objects[index].name = "Exit arch".into();
            RelayGate {
                role: Role::Exit,
                destination: if second { "finish" } else { "vesper" }.into(),
            }
            .attach(&mut scene.objects[index]);
        }
        if second {
            let strip = scene
                .objects
                .iter_mut()
                .find(|o| o.parent == Some(id) && o.name == "Signal strip")
                .unwrap();
            strip.surface.as_mut().unwrap().base_color = [color[0], color[1], color[2], 1.];
        }
    }
    for (cx, y, z) in [
        (-9., -4., -7.),
        (12., -6., -14.),
        (-15., -8., -25.),
        (16., -4., -31.),
    ] {
        let mut rock = block(
            "Drifting spire",
            [cx, y, z],
            [2.4, 5.6, 2.6],
            [0.20, 0.24, 0.38],
            false,
        );
        rock.rotation = [8., 27., 11.];
        scene.add_object(rock)?;
    }
    let mut courier = block(
        "Courier",
        [0., 0.66, 1.],
        [0.72, 1., 0.72],
        [1., 0.72, 0.23],
        false,
    );
    courier.speed = 4.5;
    courier.body = Some(Body {
        jump_height: 1.45,
        ..Body::character()
    });
    let here = if second { "vesper" } else { "amber" };
    courier.script = Some(format!(
        r#"fn update(object, dt, input) {{
        if object.y < -4.0 {{ object.next_scene="{here}"; }}
        object
    }}
    fn camera(view,object,dt,input) {{
        view.x=object.x*0.6; view.y=0.7; view.z=object.z-1.6;
        view.yaw=12.0; view.pitch=16.0; view.distance=7.4; view
    }}
    fn on_ui(object,event) {{ if event.id=="retry" {{ object.next_scene="{here}"; }} object }}"#
    ));
    let hero = scene.add_object(courier)?;
    for (name, pos, size, color, glow) in [
        (
            "Helmet",
            [0., 0.47, 0.],
            [1.13, 0.42, 1.09],
            [0.96, 0.94, 0.76],
            false,
        ),
        (
            "Visor band",
            [0., 0.49, 0.],
            [1.15, 0.13, 1.11],
            [0.08, 0.57, 0.66],
            true,
        ),
        (
            "Signal pack",
            [0., 0., 0.57],
            [0.62, 0.55, 0.24],
            [0.15, 0.21, 0.28],
            false,
        ),
        (
            "Pack indicator",
            [0., 0.06, 0.71],
            [0.40, 0.085, 0.04],
            [0.30, 1., 0.86],
            true,
        ),
        (
            "Aerial",
            [0.40, 0.81, 0.],
            [0.06, 0.33, 0.06],
            [0.95, 0.72, 0.23],
            true,
        ),
    ] {
        let mut part = block(name, pos, size, color, glow);
        part.parent = Some(hero);
        scene.add_object(part)?;
    }
    scene.camera_controller = Some(hero);
    scene.camera = View {
        target: [0., 0.7, -0.6],
        yaw: 12.,
        pitch: 16.,
        distance: 7.4,
    };
    scene.presentation.widgets = vec![
        text(
            [0.02, 0.025, 0.36, 0.07],
            if second {
                "02  /  VESPER CROSSING"
            } else {
                "01  /  AMBER REACH"
            },
        ),
        text(
            [0.14, 0.11, 0.72, 0.055],
            "Jump the gaps. Pass through both lit arches.",
        ),
        action_button([0.78, 0.80, 0.18, 0.12], "jump", "Jump", Action::Jump),
        button([0.80, 0.03, 0.16, 0.075], "retry", "Retry", UiKey::R),
    ];
    scene
        .presentation
        .sounds
        .insert("signal".into(), demo_cue()?.into());
    Ok(scene)
}

impl Project {
    pub fn cloud_relay() -> Result<Self, String> {
        let app = Application {
            title: "Cloud Relay".into(),
            identifier: "org.forge3d.cloudrelay".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            version_code: 143,
            ..Default::default()
        };
        let mut menu = Scene::empty();
        menu.application = app.clone();
        menu.title = "Cloud Relay".into();
        menu.lighting.skybox = Skybox::Sunset;
        menu.camera = View {
            target: [0., 0.6, -3.],
            yaw: 26.,
            pitch: 9.,
            distance: 7.,
        };
        island(&mut menu, 0., -3., 3.5, 5.2, [0.28, 0.99, 0.87])?;
        let mut title = Object::new("Launch relay", Shape::Empty);
        title.script = Some(
            r#"fn update(object,dt,input) { object }
            fn on_ui(object,event) { if event.id=="start" { object.next_scene="amber"; } object }"#
                .into(),
        );
        menu.add_object(title)?;
        menu.presentation.hide_movement = true;
        menu.presentation.widgets = vec![
            text([0.12, 0.12, 0.76, 0.18], "CLOUD RELAY"),
            text(
                [0.08, 0.33, 0.84, 0.07],
                "Carry a signal across two floating archipelagos.",
            ),
            text(
                [0.08, 0.58, 0.84, 0.065],
                "WASD / arrows to move     Space to jump",
            ),
            button(
                [0.32, 0.72, 0.36, 0.11],
                "start",
                "Begin relay",
                UiKey::Enter,
            ),
        ];
        let mut project = Project::new("launch", &menu)?;
        let (prefab, root) = arch()?;
        project.create_prefab("relay-arch", &prefab, root)?;
        let arch = project.instantiate("relay-arch", &mut menu, None)?;
        let at = menu.object_index(arch)?;
        menu.objects[at].position = [0., 0., -4.4];
        project.update_scene("launch", &menu)?;
        for (name, second) in [("amber", false), ("vesper", true)] {
            project.add_scene(name, &course(&project, &app, second)?)?;
        }
        let mut finish = menu;
        finish.title = "Signal delivered".into();
        finish.lighting.skybox = Skybox::Day;
        finish.presentation.widgets = vec![
            text([0.09, 0.22, 0.82, 0.16], "SIGNAL DELIVERED"),
            text(
                [0.1, 0.43, 0.8, 0.075],
                "Both island relays are back online.",
            ),
            button([0.32, 0.67, 0.36, 0.11], "start", "Run again", UiKey::Enter),
        ];
        project.add_scene("finish", &finish)?;
        Ok(project)
    }
}
