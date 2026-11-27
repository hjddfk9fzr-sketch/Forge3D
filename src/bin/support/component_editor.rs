use egui_macroquad::egui;
use forge3d::{
    component::{Field, Kind, Schema},
    physics::Body,
    runtime::Object,
    workspace::OpenProject,
};
use serde_json::Value;

fn field_ui(
    ui: &mut egui::Ui,
    field: &Field,
    data: &mut Value,
    project: Option<&OpenProject>,
) -> bool {
    let default = field.kind.default_value();
    let value = field.value(data).unwrap_or(&default);
    let mut next = None;
    ui.horizontal(|ui| {
        ui.label(&field.label);
        match field.kind.clone() {
            Kind::Number { default, min, max } => {
                let mut n = value.as_f64().unwrap_or(default);
                if ui
                    .add(egui::DragValue::new(&mut n).range(min..=max).speed(0.05))
                    .changed()
                {
                    next = Some(n.into());
                }
            }
            Kind::Count(default) => {
                let mut n = value.as_u64().unwrap_or(default);
                if ui.add(egui::DragValue::new(&mut n)).changed() {
                    next = Some(n.into());
                }
            }
            Kind::Toggle(default) => {
                let mut b = value.as_bool().unwrap_or(default);
                if ui.checkbox(&mut b, "").changed() {
                    next = Some(b.into());
                }
            }
            kind => {
                let current = value.as_str().unwrap_or("");
                egui::ComboBox::from_id_salt(&field.key)
                    .selected_text(if current.is_empty() {
                        "Choose…"
                    } else {
                        current
                    })
                    .show_ui(ui, |ui| {
                        let mut option = |id: &str, label: &str| {
                            if ui.selectable_label(id == current, label).clicked() {
                                next = Some(id.into());
                            }
                        };
                        match kind {
                            Kind::Choice { options, .. } => {
                                for s in options {
                                    option(&s, &s);
                                }
                            }
                            Kind::Scene => {
                                if let Some(p) = project {
                                    for (id, title) in p.project.scenes() {
                                        option(id, &format!("{title} ({id})"));
                                    }
                                } else {
                                    option("main", "main");
                                }
                            }
                            Kind::Prefab => {
                                if let Some(p) = project {
                                    for id in p.project.prefabs() {
                                        option(id, id);
                                    }
                                }
                            }
                            _ => unreachable!(),
                        }
                    });
            }
        }
    });
    if let Some(v) = next {
        field.set(data, v);
        true
    } else {
        false
    }
}

fn edit(
    ui: &mut egui::Ui,
    schema: &Schema,
    data: &mut Value,
    project: Option<&OpenProject>,
) -> bool {
    if schema.fields.iter().any(|f| !f.key.is_empty()) && !data.is_object() {
        ui.colored_label(egui::Color32::LIGHT_RED, "Expected an object.");
        if ui.button("Reset component").clicked() {
            *data = schema.default_value();
            return true;
        }
        return false;
    }
    let mut changed = false;
    for field in &schema.fields {
        if field.visible(data) {
            changed |= field_ui(ui, field, data, project);
        }
    }
    if let Err(error) = schema.validate(
        data,
        |s| project.map_or(s == "main", |p| p.project.scenes().any(|(id, _)| id == s)),
        |s| project.is_some_and(|p| p.project.prefabs().any(|id| id == s)),
    ) {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
        if ui.small_button("Reset component").clicked() {
            *data = schema.default_value();
            changed = true;
        }
    }
    changed
}

pub fn show(
    ui: &mut egui::Ui,
    object: &mut Object,
    project: Option<&OpenProject>,
    schemas: &[Schema],
) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Gameplay components")
        .default_open(true)
        .show(ui, |ui| {
            ui.menu_button("Add Component", |ui| {
                for schema in schemas {
                    if ui
                        .add_enabled(
                            !object.components.contains_key(&schema.name),
                            egui::Button::new(&schema.name),
                        )
                        .clicked()
                    {
                        object
                            .components
                            .insert(schema.name.clone(), schema.default_value());
                        if schema.trigger {
                            object.body.get_or_insert_with(|| Body {
                                sensor: true,
                                ..Body::default()
                            });
                        }
                        changed = true;
                        ui.close_menu();
                    }
                }
            });
            for schema in schemas {
                if !object.components.contains_key(&schema.name) {
                    continue;
                }
                ui.push_id((object.id, &schema.name), |ui| {
                    ui.separator();
                    let remove = ui
                        .horizontal(|ui| {
                            ui.strong(&schema.name);
                            ui.small_button("Remove").clicked()
                        })
                        .inner;
                    if remove {
                        object.components.remove(&schema.name);
                        changed = true;
                        return;
                    }
                    if let Some(data) = object.components.get_mut(&schema.name) {
                        changed |= edit(ui, schema, data, project);
                    }
                    if schema.trigger && !object.body.as_ref().is_some_and(|b| b.sensor) {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            "This component needs a trigger collider.",
                        );
                        if !object.is_character() && ui.button("Enable trigger collider").clicked()
                        {
                            object.body.get_or_insert_with(Body::default).sensor = true;
                            changed = true;
                        }
                    }
                });
            }
        });
    changed
}
