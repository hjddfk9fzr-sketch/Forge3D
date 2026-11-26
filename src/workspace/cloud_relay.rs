//! A standalone two-stage platform game built with ordinary Forge3D scenes and prefabs.
use super::*;
use crate::{
    animation::{Clip, Interpolation, Marker, Node, Player, Property, Rig, Track},
    controls::Action,
    lighting::{ShadowProfile, Skybox, Surface},
    physics::Body,
    relay::{RelayGate, Role},
    runtime::{cube_mesh, LodLevel, ModelVertex, NormalMap, Part, View},
    settings::Application,
};
use std::sync::Arc;

fn courier_model() -> Model {
    let cube = cube_mesh();
    let parts = [
        ([1., 0.76, 1.], [1., 0.72, 0.23, 1.]),
        ([0.36, 0.28, 0.53], [0.2, 0.27, 0.33, 1.]),
        ([0.36, 0.28, 0.53], [0.2, 0.27, 0.33, 1.]),
    ]
    .into_iter()
    .enumerate()
    .map(|(node, (size, color))| Part {
        surface: Surface {
            base_color: color,
            ..Default::default()
        },
        metallic_roughness: None,
        normal_map: None,
        emissive_texture: None,
        node: Some(node),
        skin: None,
        influences: vec![],
        vertices: cube
            .vertices
            .iter()
            .map(|v| ModelVertex {
                position: (v.position * macroquad::prelude::Vec3::from_array(size)
                    + macroquad::prelude::Vec3::Y * if node == 0 { 0.12 } else { 0. })
                .to_array(),
                normal: v.normal.truncate().to_array(),
                uv: [0.; 2],
                color: [1.; 4],
            })
            .collect(),
        indices: cube.indices.iter().copied().map(u32::from).collect(),
        texture: None,
    })
    .collect::<Vec<_>>();
    let root = Node {
        parent: None,
        position: [0.; 3],
        rotation: [0., 0., 0., 1.],
        scale: [1.; 3],
    };
    let boot = |x| Node {
        parent: Some(0),
        position: [x, -0.37, 0.],
        ..root.clone()
    };
    let track = |node, x, poses: [(f32, f32); 5], duration| Track {
        node,
        property: Property::Translation,
        interpolation: Interpolation::Linear,
        times: [0., 0.25, 0.5, 0.75, 1.]
            .map(|phase| phase * duration)
            .to_vec(),
        values: poses.map(|(y, z)| [x, y, z, 0.]).to_vec(),
    };
    let left = [
        (-0.22, 0.04),
        (-0.37, -0.28),
        (-0.37, 0.22),
        (-0.32, 0.24),
        (-0.22, 0.04),
    ];
    let right = [left[2], left[3], left[0], left[1], left[2]];
    let gait = |name: &str, duration, reach| {
        let scaled = |poses: [(f32, f32); 5]| poses.map(|(y, z)| (y, z * reach));
        Clip {
            name: name.into(),
            duration,
            tracks: vec![
                track(1, -0.30, scaled(left), duration),
                track(2, 0.30, scaled(right), duration),
            ],
            markers: [0.25, 0.75]
                .map(|phase| Marker {
                    name: "footstep".into(),
                    time: phase * duration,
                })
                .to_vec(),
        }
    };
    Model {
        lods: Default::default(),
        name: "Courier locomotion".into(),
        parts: parts.into(),
        rig: Some(
            Rig {
                nodes: vec![root.clone(), boot(-0.30), boot(0.30)],
                skins: vec![],
                clips: vec![
                    Clip {
                        name: "Idle".into(),
                        duration: 1.,
                        tracks: vec![],
                        markers: vec![],
                    },
                    gait("Walk", 0.60, 0.68),
                    gait("Run", 0.44, 1.25),
                ],
            }
            .into(),
        ),
    }
}

fn footstep_wav() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav =
        hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec).map_err(|e| e.to_string())?;
    let mut seed = 7u32;
    for frame in 0..1550 {
        let t = frame as f32 / 1550.;
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((seed >> 16) as f32 / 32768.) - 1.;
        let thud = (t * 12. * std::f32::consts::TAU).sin();
        let sample = (0.28 * noise + 0.72 * thud) * (t * 40.).min(1.) * (1. - t).powi(3);
        wav.write_sample((sample * 7500.) as i16)
            .map_err(|e| e.to_string())?;
    }
    wav.finalize().map_err(|e| e.to_string())?;
    Ok(bytes)
}

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

fn deck_model() -> Result<Model, String> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use macroquad::prelude::Vec3;
    let cube = cube_mesh();
    let (vertices, tangents): (Vec<_>, Vec<_>) = cube
        .vertices
        .iter()
        .map(|v| {
            let n = v.normal.truncate();
            let reference = if n.y == 0. { Vec3::Y } else { Vec3::Z };
            let tangent = reference.cross(n);
            let bitangent = n.cross(tangent);
            (
                ModelVertex {
                    position: v.position.to_array(),
                    normal: n.to_array(),
                    uv: [
                        v.position.dot(tangent) + 0.5,
                        v.position.dot(bitangent) + 0.5,
                    ],
                    color: [1.; 4],
                },
                tangent.extend(1.).to_array(),
            )
        })
        .unzip();
    let image = RgbaImage::from_fn(128, 128, |x, y| {
        let wave = |p| (p as f32 / 128. * std::f32::consts::TAU * 8.).sin();
        let n = Vec3::new(wave(x) * 0.65, wave(y) * 0.55, 1.).normalize();
        let [r, g, b] = n
            .to_array()
            .map(|value| ((value * 0.5 + 0.5) * 255.).round() as u8);
        Rgba([r, g, b, 255])
    });
    let mut texture = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut texture, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Model {
        lods: Default::default(),
        name: "Ribbed deck".into(),
        rig: None,
        parts: vec![Part {
            normal_map: Some(NormalMap {
                texture: texture.into_inner(),
                strength: 1.,
                tangents,
            }),
            vertices,
            indices: cube.indices.into_iter().map(u32::from).collect(),
            surface: Surface::default(),
            metallic_roughness: None,
            emissive_texture: None,
            node: None,
            skin: None,
            influences: vec![],
            texture: None,
        }]
        .into(),
    })
}

const DECK_MODEL: usize = 0;
const BASALT_MODEL: usize = 1;

fn basalt_parts(segments: usize, rings: usize) -> Arc<[Part]> {
    use macroquad::prelude::Vec3;
    let point = |ring: usize, side: usize| {
        let height = ring as f32 / (rings - 1) as f32;
        let angle = (side % segments) as f32 / segments as f32 * std::f32::consts::TAU;
        let radius =
            (0.36 + height * 0.13) * (1. + 0.08 * (angle * 5.).cos() + 0.04 * (angle * 3.).sin());
        Vec3::new(angle.sin() * radius, height - 0.5, angle.cos() * radius)
    };
    let mut part = Part {
        vertices: vec![],
        indices: vec![],
        surface: Surface::default(),
        texture: None,
        metallic_roughness: None,
        normal_map: None,
        emissive_texture: None,
        node: None,
        skin: None,
        influences: vec![],
    };
    let mut face = |positions: &[Vec3]| {
        let start = part.vertices.len() as u32;
        let normal = (positions[1] - positions[0])
            .cross(positions[2] - positions[0])
            .normalize();
        part.vertices.extend(positions.iter().map(|p| ModelVertex {
            position: p.to_array(),
            normal: normal.to_array(),
            uv: [0.; 2],
            color: [1.; 4],
        }));
        part.indices.extend([start, start + 1, start + 2]);
        if positions.len() == 4 {
            part.indices.extend([start, start + 2, start + 3]);
        }
    };
    for side in 0..segments {
        for ring in 0..rings - 1 {
            face(&[
                point(ring, side),
                point(ring, side + 1),
                point(ring + 1, side + 1),
                point(ring + 1, side),
            ]);
        }
        let top = rings - 1;
        face(&[Vec3::Y * 0.5, point(top, side), point(top, side + 1)]);
        face(&[Vec3::NEG_Y * 0.5, point(0, side + 1), point(0, side)]);
    }
    vec![part].into()
}

fn island_models(scene: &mut Scene) -> Result<(), String> {
    scene.models.push(deck_model()?);
    scene.models.push(Model {
        name: "Basalt column".into(),
        rig: None,
        parts: basalt_parts(24, 5),
        lods: vec![
            LodLevel {
                distance: 14.,
                parts: basalt_parts(12, 3),
            },
            LodLevel {
                distance: 24.,
                parts: basalt_parts(6, 2),
            },
        ]
        .into(),
    });
    Ok(())
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
    deck.shape = Shape::Model(DECK_MODEL);
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
        rock.shape = Shape::Model(BASALT_MODEL);
        rock.rotation[1] = i as f32 * 6.;
        scene.add_object(rock)?;
    }
    Ok(())
}

fn environment_samples(scene: &mut Scene, z: f32) -> Result<(), String> {
    for (name, x, metallic, roughness) in [
        ("Matte sky sample", -1.55, 0., 0.88),
        ("Metal sky sample", 1.55, 1., 0.18),
    ] {
        let mut sample = Object::new(name, Shape::Sphere);
        sample.position = [x, 0.38, z];
        sample.scale = [0.7; 3];
        sample.surface = Some(Surface {
            base_color: [0.75, 0.78, 0.84, 1.],
            metallic,
            roughness,
            ..Surface::default()
        });
        scene.add_object(sample)?;
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
    scene.lighting.ambient = 0.52;
    scene.lighting.environment_strength = 1.15;
    if second {
        scene.lighting.shadow_profile = ShadowProfile::Mobile;
    }
    let x = if second { 1.35 } else { 0. };
    let color = if second {
        [0.69, 0.56, 1.]
    } else {
        [0.28, 0.99, 0.87]
    };
    island_models(&mut scene)?;
    for (cx, z, w, l) in [(0., 0., 4.2, 5.), (x, -6.5, 3.2, 4.6), (0., -13., 4.2, 5.)] {
        island(&mut scene, cx, z, w, l, color)?;
    }
    environment_samples(&mut scene, 1.0)?;
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
        rock.shape = Shape::Model(BASALT_MODEL);
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
    courier.shape = Shape::Model(scene.models.len());
    courier.surface = None;
    scene.models.push(courier_model());
    courier.animate_grounded_motion = true;
    courier.animation_blend = 0.16;
    courier.animation = Some(Player {
        clip: "Idle".into(),
        ..Player::default()
    });
    courier
        .components
        .insert("cloud-relay.footsteps".into(), true.into());
    courier.speed = 4.5;
    courier.body = Some(Body {
        jump_height: 1.45,
        ..Body::character()
    });
    let here = if second { "vesper" } else { "amber" };
    courier.script = Some(format!(
        r#"fn update(object, dt, input) {{
        if object.y < -4.0 {{ object.next_scene="{here}"; }}
        let intent = input.x * input.x + input.z * input.z;
        let held = if intent < 0.01 {{ 0.0 }} else {{ (object.state["move_time"] ?? 0.0) + dt }};
        object.state["move_time"] = min(held, 1.0);
        object.anim_clip = if intent < 0.01 {{ "Idle" }}
            else if intent < 0.55 || held < 0.27 {{ "Walk" }}
            else {{ "Run" }};
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
    scene
        .presentation
        .sounds
        .insert("footstep".into(), footstep_wav()?.into());
    Ok(scene)
}

impl Project {
    pub fn cloud_relay() -> Result<Self, String> {
        let app = Application {
            title: "Cloud Relay".into(),
            identifier: "org.forge3d.cloudrelay".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            version_code: 150,
            ..Default::default()
        };
        let mut menu = Scene::empty();
        menu.application = app.clone();
        menu.title = "Cloud Relay".into();
        menu.lighting.skybox = Skybox::Sunset;
        menu.lighting.environment_strength = 1.15;
        menu.camera = View {
            target: [0., 0.6, -3.],
            yaw: 26.,
            pitch: 9.,
            distance: 7.,
        };
        island_models(&mut menu)?;
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
        finish.lighting.environment_strength = 1.15;
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
