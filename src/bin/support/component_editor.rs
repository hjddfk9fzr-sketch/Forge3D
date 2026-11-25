use egui_macroquad::egui;
use forge3d::{
    physics::Body,
    relay::{RelayGate, Role, COMPONENT},
    runtime::Object,
    workspace::OpenProject,
};

pub fn show(ui: &mut egui::Ui, object: &mut Object, project: Option<&OpenProject>) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Gameplay components")
        .default_open(true)
        .show(ui, |ui| {
            let result = RelayGate::read(object);
            let mut gate = match result {
                Ok(Some(gate)) => gate,
                Ok(None) => {
                    if ui.button("Add RelayGate").clicked() {
                        RelayGate::default().attach(object);
                        object.body.get_or_insert_with(|| Body {
                            sensor: true,
                            ..Default::default()
                        });
                        changed = true;
                    }
                    return;
                }
                Err(error) => {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        format!("Invalid RelayGate: {error}"),
                    );
                    if ui.button("Reset RelayGate").clicked() {
                        RelayGate::default().attach(object);
                        changed = true;
                    }
                    return;
                }
            };
            ui.horizontal(|ui| {
                ui.strong(COMPONENT);
                if ui.small_button("Remove").clicked() {
                    object.components.remove(COMPONENT);
                    changed = true;
                }
            });
            if changed {
                return;
            }
            let old = gate.clone();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut gate.role, Role::Transmitter, "Transmitter");
                ui.selectable_value(&mut gate.role, Role::Exit, "Exit");
            });
            if gate.role == Role::Exit {
                egui::ComboBox::from_label("Destination")
                    .selected_text(if gate.destination.is_empty() {
                        "Choose a scene"
                    } else {
                        &gate.destination
                    })
                    .show_ui(ui, |ui| {
                        if let Some(project) = project {
                            for (id, title) in project.project.scenes() {
                                ui.selectable_value(
                                    &mut gate.destination,
                                    id.into(),
                                    format!("{title} ({id})"),
                                );
                            }
                        } else {
                            ui.selectable_value(&mut gate.destination, "main".into(), "main");
                        }
                    });
            }
            if gate != old {
                gate.attach(object);
                changed = true;
            }
            if let Err(error) = gate.validate(object, |target| {
                project.map_or(target == "main", |p| {
                    p.project.scenes().any(|(id, _)| id == target)
                })
            }) {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
                if !object.body.as_ref().is_some_and(|body| body.sensor)
                    && !object.is_character()
                    && ui.button("Enable trigger collider").clicked()
                {
                    object.body.get_or_insert_with(Body::default).sensor = true;
                    changed = true;
                }
            }
        });
    changed
}
