use super::{editor_camera, Command, TransformTool, HEADER, LINE, TRANSFORM_TOOLS};
use egui_macroquad::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
};
use forge3d::runtime::View;
use macroquad::prelude::Vec3;

pub(super) struct Gizmo {
    tools: Rect,
    cube: Rect,
}

impl Gizmo {
    pub fn new(viewport: Rect) -> Option<Self> {
        if viewport.width() < 200. || viewport.height() < 123. {
            return None;
        }
        Some(Self {
            tools: Rect::from_min_size(viewport.min + egui::vec2(9., 9.), egui::vec2(76., 104.)),
            cube: Rect::from_min_size(
                viewport.right_top() + egui::vec2(-103., 9.),
                egui::vec2(94., 94.),
            ),
        })
    }

    pub fn contains(&self, point: Pos2) -> bool {
        self.tools.contains(point) || self.cube.contains(point)
    }

    pub fn show(&self, ui: &mut Ui, view: &mut View, active_tool: &mut TransformTool) {
        for panel in [self.tools, self.cube] {
            ui.painter().rect(
                panel,
                6.,
                HEADER,
                Stroke::new(1_f32, LINE),
                StrokeKind::Inside,
            );
        }
        for (index, tool) in TRANSFORM_TOOLS.into_iter().enumerate() {
            let command = Command::Tool(tool);
            let rect = Rect::from_min_size(
                self.tools.min + egui::vec2(4., 4. + 24. * index as f32),
                egui::vec2(68., 23.),
            );
            let button = egui::Button::new(command.title()).selected(tool == *active_tool);
            let shortcut = ui.ctx().format_shortcut(&command.key());
            if ui.put(rect, button).on_hover_text(shortcut).clicked() {
                *active_tool = tool;
            }
        }

        let hitbox = ui.interact(self.cube, ui.id().with("orientation-cube"), Sense::click());
        let faces = visible_faces(view, self.cube.center());
        let hovered = hitbox.hover_pos().and_then(|point| {
            faces
                .iter()
                .position(|face| inside_polygon(&face.corners, point))
        });
        for (index, face) in faces.iter().enumerate() {
            let color = if hovered == Some(index) {
                face.axis.color.gamma_multiply(1.32)
            } else {
                face.axis.color
            };
            ui.painter().add(Shape::convex_polygon(
                face.corners.to_vec(),
                color,
                Stroke::new(1_f32, HEADER),
            ));
            if face.depth > 0.31 {
                ui.painter().text(
                    face.center,
                    Align2::CENTER_CENTER,
                    face.axis.label,
                    FontId::proportional(12.),
                    Color32::WHITE,
                );
            }
        }
        if let Some(face) = hovered.map(|index| faces[index].axis) {
            if hitbox.clicked() {
                view.yaw = face.yaw;
                view.pitch = face.pitch;
            }
            hitbox.on_hover_text(format!("Look from {}", face.label));
        }
    }
}

#[derive(Clone, Copy)]
struct Axis {
    normal: Vec3,
    label: &'static str,
    color: Color32,
    yaw: f32,
    pitch: f32,
}

const AXES: [Axis; 6] = [
    Axis {
        normal: Vec3::X,
        label: "+X",
        color: Color32::from_rgb(160, 63, 65),
        yaw: 90.,
        pitch: 0.,
    },
    Axis {
        normal: Vec3::NEG_X,
        label: "−X",
        color: Color32::from_rgb(114, 49, 57),
        yaw: 270.,
        pitch: 0.,
    },
    Axis {
        normal: Vec3::Y,
        label: "+Y",
        color: Color32::from_rgb(43, 125, 77),
        yaw: 0.,
        pitch: 89.,
    },
    Axis {
        normal: Vec3::NEG_Y,
        label: "−Y",
        color: Color32::from_rgb(34, 90, 63),
        yaw: 0.,
        pitch: -89.,
    },
    Axis {
        normal: Vec3::Z,
        label: "+Z",
        color: Color32::from_rgb(55, 102, 172),
        yaw: 0.,
        pitch: 0.,
    },
    Axis {
        normal: Vec3::NEG_Z,
        label: "−Z",
        color: Color32::from_rgb(52, 78, 134),
        yaw: 180.,
        pitch: 0.,
    },
];

struct Face {
    axis: Axis,
    corners: [Pos2; 4],
    center: Pos2,
    depth: f32,
}

fn visible_faces(view: &View, origin: Pos2) -> Vec<Face> {
    let basis = editor_camera::basis(view);
    let project = |p: Vec3| origin + egui::vec2(p.dot(basis.right), -p.dot(basis.up)) * 23.;
    AXES.into_iter()
        .filter_map(|axis| {
            let n = axis.normal;
            let depth = n.dot(basis.back);
            if depth < 0.001 {
                return None;
            }
            let u = if n.y.abs() > 0.5 { Vec3::X } else { Vec3::Y };
            let v = n.cross(u);
            Some(Face {
                axis,
                corners: [n - u - v, n + u - v, n + u + v, n - u + v].map(project),
                center: project(n),
                depth,
            })
        })
        .collect()
}

fn inside_polygon(corners: &[Pos2; 4], point: Pos2) -> bool {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .all(|(&start, &end)| {
            let edge = end - start;
            let offset = point - start;
            edge.x * offset.y - edge.y * offset.x <= 0.
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_cube_only_hits_the_visible_faces() {
        let origin = Pos2::new(100., 100.);
        let faces = visible_faces(&View::default(), origin);
        assert_eq!(
            faces.iter().map(|f| f.axis.label).collect::<Vec<_>>(),
            ["+X", "+Y", "+Z"]
        );
        for face in &faces {
            assert_eq!(
                faces
                    .iter()
                    .filter(|other| inside_polygon(&other.corners, face.center))
                    .count(),
                1
            );
            assert!(inside_polygon(&face.corners, face.center));
        }
        let view = View {
            yaw: 180.,
            pitch: 0.,
            ..View::default()
        };
        let faces = visible_faces(&view, origin);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].axis.label, "−Z");
        assert!(inside_polygon(&faces[0].corners, origin));
    }
}
