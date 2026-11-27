use super::{Action, Browser, ACCENT, HEADER, MUTED, PANEL};
use egui_macroquad::egui::{self, RichText};
use forge3d::workspace::EXTENSION;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Projects,
    Templates,
    Browse,
}

pub(super) enum Choice {
    Resume,
    Launch(Action, Option<PathBuf>),
}

pub(super) struct Hub {
    path: Option<PathBuf>,
    projects: Vec<PathBuf>,
    tab: Tab,
    filter: String,
    location: String,
    message: String,
    worker: Option<mpsc::Receiver<io::Result<Discovery>>>,
}

impl Hub {
    pub fn new(path: Option<PathBuf>, directory: &Path) -> Self {
        let mut hub = Self {
            path,
            projects: Vec::new(),
            tab: Tab::Projects,
            filter: String::new(),
            location: directory.display().to_string(),
            message: String::new(),
            worker: None,
        };
        if let Some(path) = &hub.path {
            match fs::read(path) {
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(projects) => hub.projects = projects,
                    Err(error) => hub.message = format!("Could not read the project list: {error}"),
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => hub.message = format!("Could not read the project list: {error}"),
            }
        }
        hub
    }

    pub fn remember(&mut self, path: &Path) {
        if !is_project(path) {
            return;
        }
        let Ok(path) = path.canonicalize() else {
            return;
        };
        if self.projects.first() == Some(&path) {
            return;
        }
        self.projects.retain(|entry| entry != &path);
        self.projects.insert(0, path);
        self.save();
    }

    fn save(&mut self) {
        if let Some(path) = &self.path {
            if let Err(error) = save_list(path, &self.projects) {
                self.message = format!("Could not save the project list: {error}");
            }
        }
    }

    fn discover(&mut self, directory: PathBuf) {
        let (sender, receiver) = mpsc::channel();
        match std::thread::Builder::new()
            .name("project-discovery".into())
            .spawn(move || {
                let _ = sender.send(find_projects(&directory));
            }) {
            Ok(_) => {
                self.worker = Some(receiver);
                self.message = String::from("Finding projects…");
            }
            Err(error) => self.message = format!("Could not start project search: {error}"),
        }
    }

    fn poll_discovery(&mut self, ctx: &egui::Context) {
        let Some(worker) = &self.worker else {
            return;
        };
        match worker.try_recv() {
            Ok(Ok(found)) => {
                let mut existing: HashSet<_> = self.projects.iter().cloned().collect();
                let new = found
                    .paths
                    .into_iter()
                    .filter(|path| existing.insert(path.clone()));
                let before = self.projects.len();
                self.projects.extend(new);
                self.message = format!("Added {} projects.", self.projects.len() - before);
                if found.skipped > 0 {
                    self.message
                        .push_str(&format!(" Skipped {} unreadable entries.", found.skipped));
                }
                self.save();
                self.tab = Tab::Projects;
                self.worker = None;
            }
            Ok(Err(error)) => {
                self.message = format!("Could not search the folder: {error}");
                self.worker = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.message = "Project search ended early.".into();
                self.worker = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        browser: &mut Browser,
        can_resume: bool,
    ) -> Option<Choice> {
        self.poll_discovery(ctx);
        let mut choice = None;
        egui::TopBottomPanel::top("hub-header")
            .frame(egui::Frame::new().fill(HEADER).inner_margin(18))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("FORGE3D").size(24.).strong().color(ACCENT));
                    ui.label(RichText::new(env!("CARGO_PKG_VERSION")).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if can_resume && ui.button("Back to editor").clicked() {
                            choice = Some(Choice::Resume);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PANEL).inner_margin(24))
            .show(ctx, |ui| {
                ui.set_max_width(1024.);
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Projects, "My projects");
                    ui.selectable_value(&mut self.tab, Tab::Templates, "Templates");
                    ui.selectable_value(&mut self.tab, Tab::Browse, "Browse");
                });
                ui.add_space(12.);
                if !self.message.is_empty() {
                    ui.label(RichText::new(&self.message).color(MUTED));
                }
                let selection = match self.tab {
                    Tab::Projects => self.projects_ui(ui),
                    Tab::Templates => templates_ui(ui),
                    Tab::Browse => self.browse_ui(ui, browser),
                };
                if selection.is_some() {
                    choice = selection;
                }
            });
        choice
    }

    fn projects_ui(&mut self, ui: &mut egui::Ui) -> Option<Choice> {
        let mut choice = None;
        let mut remove = None;
        let empty = self.projects.is_empty();
        ui.horizontal_wrapped(|ui| {
            if !empty && ui.button("New project").clicked() {
                self.tab = Tab::Templates;
            }
            if ui.button("Open file…").clicked() {
                choice = Some(Choice::Launch(Action::Open, None));
            }
            if ui.button("Find projects…").clicked() {
                self.tab = Tab::Browse;
            }
            if !empty {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter projects"));
            }
        });
        ui.add_space(12.);
        if empty {
            ui.heading("Start from a template");
            ui.add_space(6.);
            return templates_ui(ui).or(choice);
        }
        let filter = self.filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, path) in self.projects.iter().enumerate() {
                    if !path.to_string_lossy().to_lowercase().contains(&filter) {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Project");
                    ui.push_id(index, |ui| {
                        egui::Frame::new()
                            .fill(HEADER)
                            .inner_margin(12)
                            .corner_radius(5)
                            .show(ui, |ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button("Remove")
                                            .on_hover_text("Remove from this list")
                                            .clicked()
                                        {
                                            remove = Some(index);
                                        }
                                        if ui.button("Open").clicked() {
                                            choice = Some(Choice::Launch(
                                                Action::Open,
                                                Some(path.clone()),
                                            ));
                                        }
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(
                                                            name.trim_end_matches(EXTENSION),
                                                        )
                                                        .strong(),
                                                    )
                                                    .truncate(),
                                                );
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(path.display().to_string())
                                                            .small()
                                                            .color(MUTED),
                                                    )
                                                    .truncate(),
                                                )
                                                .on_hover_text(path.display().to_string());
                                            },
                                        );
                                    },
                                );
                            });
                    });
                    ui.add_space(5.);
                }
            });
        if let Some(index) = remove {
            self.projects.remove(index);
            self.save();
        }
        choice
    }

    fn browse_ui(&mut self, ui: &mut egui::Ui, browser: &mut Browser) -> Option<Choice> {
        let mut directory = None;
        let mut choice = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Up").clicked() {
                directory = browser.directory.parent().map(Path::to_owned);
            }
            let path =
                ui.add(egui::TextEdit::singleline(&mut self.location).hint_text("Folder path"));
            if ui.button("Go").clicked()
                || path.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                let absolute = Path::new(self.location.trim());
                directory = Some(browser.directory.join(absolute));
            }
            if ui.button("Refresh").clicked() {
                browser.refresh();
            }
        });
        ui.label(
            RichText::new(browser.directory.display().to_string())
                .small()
                .color(MUTED),
        );
        if ui
            .add_enabled(
                self.worker.is_none() && browser.error.is_none(),
                egui::Button::new("Add projects from this folder and subfolders"),
            )
            .clicked()
        {
            self.discover(browser.directory.clone());
        }
        if let Some(error) = &browser.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &browser.entries {
                    match entry.kind {
                        "directory" if ui.button(format!("{}/", entry.name)).clicked() => {
                            directory = Some(browser.directory.join(&entry.name));
                        }
                        "project" | "scene" if ui.button(&entry.name).clicked() => {
                            choice = Some(Choice::Launch(
                                Action::Open,
                                Some(browser.directory.join(&entry.name)),
                            ));
                        }
                        _ => {}
                    }
                }
            });
        if let Some(path) = directory {
            match path.canonicalize().and_then(|path| {
                if path.is_dir() {
                    Ok(path)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "Choose a folder.",
                    ))
                }
            }) {
                Ok(path) => {
                    self.location = path.display().to_string();
                    browser.directory = path;
                    browser.refresh();
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        choice
    }
}

fn templates_ui(ui: &mut egui::Ui) -> Option<Choice> {
    let mut choice = None;
    let templates = [
        (
            "Empty 2D",
            "Orthographic camera and a tile layer.",
            Action::New2d,
        ),
        (
            "Empty 3D",
            "Perspective camera and an empty scene.",
            Action::New,
        ),
        (
            "Afterlight",
            "2D platformer with sprites, tile collisions and two levels.",
            Action::PlatformerDemo,
        ),
        (
            "Cloud Relay",
            "3D platforming through floating islands.",
            Action::CloudRelay,
        ),
        (
            "Workshop Run",
            "3D campaign with a menu, two levels and pickups.",
            Action::CampaignDemo,
        ),
        (
            "Two Workshops",
            "Small 3D project for editing multiple scenes.",
            Action::ProjectDemo,
        ),
        (
            "Physics Workshop",
            "3D scene with rigid bodies and a playable character.",
            Action::PhysicsDemo,
        ),
        (
            "Light & Motion",
            "3D scene with lighting, shadows and animation.",
            Action::RenderingDemo,
        ),
    ];
    let columns = (ui.available_width() / 260.).floor().clamp(1., 4.) as usize;
    let card_width = (ui.available_width() / columns as f32 - 40.).clamp(80., 225.);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in templates.chunks(columns) {
            ui.horizontal(|ui| {
                for &(title, description, action) in row {
                    egui::Frame::new()
                        .fill(HEADER)
                        .corner_radius(6)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(card_width);
                                ui.label(RichText::new(title).size(17.).strong());
                                ui.add_sized(
                                    [card_width, 38.],
                                    egui::Label::new(RichText::new(description).color(MUTED))
                                        .wrap(),
                                );
                                if ui.button("Use template").clicked() {
                                    choice = Some(Choice::Launch(action, None));
                                }
                            });
                        });
                }
            });
            ui.add_space(6.);
        }
    });
    choice
}

fn is_project(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(EXTENSION))
}

fn save_list(path: &Path, projects: &[PathBuf]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(file.as_file_mut(), projects)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

struct Discovery {
    paths: Vec<PathBuf>,
    skipped: usize,
}

fn find_projects(root: &Path) -> io::Result<Discovery> {
    let root = root.canonicalize()?;
    let mut result = Discovery {
        paths: Vec::new(),
        skipped: 0,
    };
    let mut directories = vec![fs::read_dir(root)?];
    while let Some(entries) = directories.last_mut() {
        let Some(entry) = entries.next() else {
            directories.pop();
            continue;
        };
        let Ok((entry, kind)) = entry.and_then(|entry| Ok((entry.path(), entry.file_type()?)))
        else {
            result.skipped += 1;
            continue;
        };
        if kind.is_file() && is_project(&entry) {
            result.paths.push(entry);
        } else if kind.is_dir()
            && !entry.file_name().is_some_and(|name| {
                name == "target"
                    || name == "node_modules"
                    || name.to_string_lossy().starts_with('.')
            })
        {
            match fs::read_dir(entry) {
                Ok(entries) => directories.push(entries),
                Err(_) => result.skipped += 1,
            }
        }
    }
    result.paths.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_finds_nested_projects_without_build_outputs() {
        let dir = tempfile::tempdir().unwrap();
        for child in [
            "own/first.forge-project.json",
            "own/nested/second.forge-project.json",
            "own/target/generated.forge-project.json",
            "own/scene.forge.json",
        ] {
            let path = dir.path().join(child);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{}").unwrap();
        }
        let found = find_projects(&dir.path().join("own")).unwrap();
        assert_eq!(found.paths.len(), 2);
        assert!(found.paths[0].ends_with("first.forge-project.json"));
        assert!(found.paths[1].ends_with("second.forge-project.json"));
    }

    #[test]
    fn list_survives_restart_with_duplicate_paths_coalesced() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("settings/projects.json");
        let project = dir.path().join("test.forge-project.json");
        fs::write(&project, "{}").unwrap();
        let mut hub = Hub::new(Some(config.clone()), dir.path());
        hub.remember(&project);
        hub.remember(&dir.path().join("./test.forge-project.json"));
        let restored = Hub::new(Some(config), dir.path());
        assert_eq!(restored.projects, [project.canonicalize().unwrap()]);
    }
}
