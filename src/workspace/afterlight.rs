//! Two small levels for the 2D template.
use super::*;
use crate::{
    canvas2d::{Atlas, Body, BodyKind, Sprite},
    controls::Action,
    settings::Application,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn atlas() -> Result<Atlas, String> {
    let image = RgbaImage::from_fn(16 * 8, 16, |x, y| {
        let frame = x / 16;
        let x = x % 16;
        let color = match frame {
            0 => {
                if y < 3 {
                    [106, 233, 182, 255]
                } else if (x / 4 + y / 4) % 3 == 0 {
                    [46, 103, 107, 255]
                } else {
                    [31, 63, 80, 255]
                }
            }
            1 => {
                if (x + 2 * y) % 7 == 0 {
                    [61, 81, 132, 255]
                } else {
                    [37, 53, 91, 255]
                }
            }
            2..=5 => {
                if (4..=11).contains(&x) && (2..=7).contains(&y) {
                    if y == 5 && x > 6 {
                        [41, 255, 225, 255]
                    } else {
                        [253, 219, 137, 255]
                    }
                } else if (3..=12).contains(&x) && (7..=11).contains(&y) {
                    [220, 123, 94, 255]
                } else if (12..=14).contains(&y) && (x == 4 + frame % 3 || x == 10 - frame % 3) {
                    [255, 221, 160, 255]
                } else {
                    [0; 4]
                }
            }
            6 => {
                if !(2..=13).contains(&x) || y == 0 {
                    [0; 4]
                } else if !(5..=10).contains(&x) || y > 12 {
                    [70, 218, 191, 255]
                } else {
                    [18, 71, 89, 255]
                }
            }
            _ => {
                if (x + y * 3) % 19 < 2 {
                    [45, 79, 93, 255]
                } else {
                    [25, 51, 71, 255]
                }
            }
        };
        Rgba(color)
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Atlas {
        png: bytes.into_inner().into(),
        frame_width: 16,
        frame_height: 16,
    })
}

fn course(app: &Application, id: &str, next: &str, night: bool) -> Result<Scene, String> {
    let mut scene = crate::canvas2d::empty_2d();
    scene.title = if night {
        "Lantern bridge"
    } else {
        "Signal yard"
    }
    .into();
    scene.application = app.clone();
    let canvas = scene.canvas2d.as_mut().unwrap();
    canvas.atlases.insert("afterlight".into(), atlas()?);
    canvas.background = if night {
        [18, 17, 41, 255]
    } else {
        [27, 43, 72, 255]
    };
    canvas.layers[0].atlas = Some("afterlight".into());
    canvas.add_layer("Distant towers", false);
    canvas.layers[1].atlas = Some("afterlight".into());
    canvas.layers[1].depth = -10;
    let gaps = if night {
        [17..20, 26..28]
    } else {
        [12..14, 24..26]
    };
    for x in 0..40 {
        if !gaps.iter().any(|gap| gap.contains(&x)) {
            for y in 10..14 {
                canvas.paint(0, x, y, if y == 10 { 1 } else { 8 });
            }
        }
        let tower = 6 + (x / 3 * 7) % 4;
        for y in tower..10 {
            canvas.paint(1, x, y, 2);
        }
    }
    let mut actor = Object::new("Courier", Shape::Empty);
    actor.position = [3., 9.4, 0.];
    actor.speed = 7.;
    actor.body2d = Some(Body {
        kind: BodyKind::Character,
        size: [0.65, 0.92],
        jump_speed: 11.7,
    });
    actor.sprite2d = Some(Sprite {
        atlas: "afterlight".into(),
        first: 2,
        frames: 4,
        fps: 9.,
        depth: 10,
        size: [1.3, 1.35],
    });
    actor.script = Some(format!(
        r#"
      fn update(object,dt,input) {{ if object.y > 16.0 {{ object.next_scene="{id}"; }} object }}
      fn camera(view,object,dt,input) {{
        view.x=max(9.0,min(object.x,32.0)); view.y=7.8; view.distance=10.5; view
      }}
    "#
    ));
    scene.camera_controller = Some(scene.add_object(actor)?);
    let mut exit = Object::new("Signal door", Shape::Empty);
    exit.position = [35.5, 9., 0.];
    exit.body2d = Some(Body {
        kind: BodyKind::Sensor,
        size: [0.85, 2.],
        ..Body::default()
    });
    exit.sprite2d = Some(Sprite {
        atlas: "afterlight".into(),
        first: 6,
        frames: 1,
        fps: 0.,
        depth: 2,
        size: [1.1, 2.],
    });
    exit.script = Some(format!(
        r#"fn update(object,dt,input) {{ object }}
      fn on_event(object,event) {{ if event.kind=="trigger_enter" && event.other_name=="Courier" {{ object.next_scene="{next}"; }} object }}"#
    ));
    scene.add_object(exit)?;
    scene.presentation.widgets = vec![
        text(
            [0.04, 0.06, 0.35, 0.08],
            if night {
                "02 / LANTERN BRIDGE"
            } else {
                "01 / SIGNAL YARD"
            },
        ),
        action_button([0.80, 0.80, 0.16, 0.12], "jump", "Jump", Action::Jump),
    ];
    Ok(scene)
}

impl Project {
    pub fn platformer() -> Result<Self, String> {
        let app = Application {
            title: "Afterlight".into(),
            identifier: "org.forge3d.afterlight".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            version_code: 170,
            ..Application::default()
        };
        let mut menu = crate::canvas2d::empty_2d();
        menu.title = app.title.clone();
        menu.application = app.clone();
        menu.presentation.hide_movement = true;
        let mut link = Object::new("Start game", Shape::Empty);
        link.script=Some(r#"fn update(object,dt,input) { object } fn on_ui(object,event) { if event.id=="start" { object.next_scene="yard"; } object }"#.into());
        menu.add_object(link)?;
        menu.presentation.widgets = vec![
            text([0.17, 0.22, 0.66, 0.2], "AFTERLIGHT"),
            text(
                [0.08, 0.51, 0.84, 0.07],
                "A / D or arrows to move     Space to jump",
            ),
            button([0.33, 0.68, 0.34, 0.11], "start", "Begin", UiKey::Enter),
        ];
        let mut project = Project::new("menu", &menu)?;
        project.add_scene("yard", &course(&app, "yard", "bridge", false)?)?;
        project.add_scene("bridge", &course(&app, "bridge", "finish", true)?)?;
        menu.title = "Signal restored".into();
        menu.presentation.widgets = vec![
            text([0.12, 0.25, 0.76, 0.15], "SIGNAL RESTORED"),
            button([0.33, 0.65, 0.34, 0.11], "start", "Run again", UiKey::Enter),
        ];
        project.add_scene("finish", &menu)?;
        Ok(project)
    }
}
