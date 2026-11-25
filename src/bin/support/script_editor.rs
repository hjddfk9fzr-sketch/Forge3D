use egui_macroquad::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
use forge3d::scripting;

#[derive(Clone, Copy)]
pub enum ScriptTarget {
    Inline(u64),
    Shared(usize),
    NewShared,
}

pub struct ScriptEditor {
    pub target: ScriptTarget,
    pub source: String,
    pub name: String,
    check: Option<Result<(), String>>,
}

impl ScriptEditor {
    pub fn new(target: ScriptTarget, name: String, source: String) -> Self {
        Self {
            target,
            name,
            source,
            check: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let mut check = false;
        ui.horizontal_wrapped(|ui| {
            if matches!(self.target, ScriptTarget::NewShared) {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.name);
            } else {
                ui.strong(&self.name);
            }
            check = ui.button("Check syntax").clicked();
            ui.menu_button("Insert example", |ui| {
                for (label, source) in [
                    ("Start", "fn start(object) {\n    object\n}"),
                    ("Update", scripting::DEFAULT_SCRIPT.trim()),
                    ("Keyboard and mouse", "fn update(object, dt, input) {\n    if input.keys.contains(\"left\") { object.x -= 4.0 * dt; }\n    if input.mouse.pressed.contains(\"right\") { object.y += 1.0; }\n    object\n}"),
                    ("UI button", "fn on_ui(object, event) {\n    if event.id == \"button-1\" { object.y += 1.0; }\n    object\n}"),
                ] {
                    if ui.button(label).clicked() {
                        let current = self.source.trim();
                        if current == source {
                            ui.close_menu();
                            continue;
                        }
                        if label == "Keyboard and mouse" && current == scripting::DEFAULT_SCRIPT.trim() {
                            self.source.clear();
                        } else if !current.is_empty() {
                            self.source.push_str("\n\n");
                        }
                        self.source.push_str(source);
                        self.check = None;
                        ui.close_menu();
                    }
                }
            });
        });
        ui.separator();
        let mut layouter =
            |ui: &egui::Ui, text: &str, _: f32| ui.fonts(|fonts| fonts.layout_job(highlight(text)));
        let font = FontId::monospace(14.);
        egui::ScrollArea::both()
            .id_salt("script-source")
            .max_height((ui.ctx().screen_rect().height() * 0.58).clamp(150., 420.))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let lines = (1..=self.source.split('\n').count())
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(lines)
                                .font(font.clone())
                                .color(Color32::GRAY),
                        )
                        .selectable(false),
                    );
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut self.source)
                                .font(font)
                                .code_editor()
                                .margin(egui::Margin::ZERO)
                                .desired_width(ui.available_width().max(300.))
                                .desired_rows(15)
                                .layouter(&mut layouter),
                        )
                        .changed()
                    {
                        self.check = None;
                    }
                });
            });
        if check {
            self.check = Some(scripting::validate_source(&self.source));
        }
        match &self.check {
            Some(Ok(())) => {
                ui.colored_label(Color32::LIGHT_GREEN, "Script compiled");
            }
            Some(Err(error)) => {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            None => {}
        }
    }
}

fn highlight(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let end;
        let color;
        if rest.starts_with("//") {
            end = rest.find('\n').unwrap_or(rest.len());
            color = Color32::from_rgb(132, 151, 171);
        } else if matches!(first, '"' | '\'' | '`') {
            let mut escaped = false;
            end = rest
                .char_indices()
                .skip(1)
                .find_map(|(i, ch)| {
                    let closes = ch == first && !escaped;
                    escaped = ch == '\\' && !escaped;
                    closes.then_some(i + ch.len_utf8())
                })
                .unwrap_or(rest.len());
            color = Color32::from_rgb(224, 185, 137);
        } else if first.is_alphanumeric() || first == '_' {
            end = rest
                .find(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .unwrap_or(rest.len());
            color = if first.is_ascii_digit() {
                Color32::from_rgb(156, 218, 170)
            } else if matches!(
                &rest[..end],
                "fn" | "let"
                    | "const"
                    | "if"
                    | "else"
                    | "while"
                    | "for"
                    | "in"
                    | "loop"
                    | "switch"
                    | "return"
                    | "continue"
                    | "break"
                    | "true"
                    | "false"
                    | "try"
                    | "catch"
                    | "throw"
                    | "import"
                    | "export"
                    | "as"
                    | "private"
            ) {
                Color32::from_rgb(192, 166, 255)
            } else {
                Color32::LIGHT_GRAY
            };
        } else {
            end = first.len_utf8();
            color = Color32::LIGHT_GRAY;
        }
        job.append(
            &rest[..end],
            0.,
            TextFormat::simple(FontId::monospace(14.), color),
        );
        rest = &rest[end..];
    }
    job
}
