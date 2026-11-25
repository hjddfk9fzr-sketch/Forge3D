#[path = "support/script_editor.rs"]
mod script_editor;
#[path = "support/ui_designer.rs"]
mod ui_designer;

use script_editor::{ScriptEditor, ScriptTarget};
use ui_designer::UiDesigner;

use egui_macroquad::egui::{self, Color32, RichText};
use forge3d::{
    animation, capture, controls, export,
    game::{Game, GamePlayback},
    import, jobs, lighting, physics,
    presentation::Overlay,
    project,
    runtime::{Object, PreparedScene, Scene, ScriptAsset, Shape, View},
    scripting,
    settings::{Application, Orientation},
    workspace::{ModelOwner, OpenProject, Project, SharedAsset, EXTENSION},
};
use macroquad::prelude::*;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

const PANEL: Color32 = Color32::from_rgb(31, 34, 39);
const HEADER: Color32 = Color32::from_rgb(22, 24, 29);
const LINE: Color32 = Color32::from_rgb(61, 66, 74);
const ACCENT: Color32 = Color32::from_rgb(110, 222, 196);
const MUTED: Color32 = Color32::from_rgb(153, 169, 195);
const UI_SCALES: [u16; 5] = [100, 125, 150, 175, 200];
const DEFAULT_UI_SCALE: u16 = 125;

fn window() -> Conf {
    Conf {
        window_title: concat!("Forge3D ", env!("CARGO_PKG_VERSION"), " · Scene Editor").into(),
        window_width: 1440,
        window_height: 900,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(window)]
async fn main() {
    egui_macroquad::cfg(configure_style);
    let mut editor = Editor::new();
    let smoke = env::args_os().any(|value| value == "--window-smoke");
    let mut frames = 0;
    loop {
        egui_macroquad::ui(|ctx| editor.ui(ctx));
        editor.draw().await;
        egui_macroquad::draw();
        next_frame().await;
        if smoke {
            frames += 1;
            if frames == 8 {
                println!("Forge3D editor rendered {frames} frames.");
                macroquad::miniquad::window::quit();
                return;
            }
        }
    }
}

fn ui_scale_path() -> Option<PathBuf> {
    let home = || env::var_os("HOME").map(PathBuf::from);
    let directory = if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)?
            .join("Forge3D")
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support/Forge3D")
    } else {
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home().map(|path| path.join(".config")))?;
        config.join("forge3d")
    };
    Some(directory.join("editor-scale"))
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(43, 47, 54);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(43, 47, 54);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(59, 67, 78);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(59, 67, 78);
    style.visuals.selection.bg_fill = Color32::from_rgb(43, 78, 79);
    style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    style.visuals.window_stroke = egui::Stroke::new(1.0_f32, LINE);
    style.spacing.interact_size.y = 22.;
    style.spacing.item_spacing = egui::vec2(6., 4.);
    ctx.set_style(style);
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
}

#[derive(Clone, Copy)]
enum Action {
    New,
    PhysicsDemo,
    RenderingDemo,
    Open,
    Save,
    Import,
    ImportScript,
    Export,
    Build,
    RetryBuild,
    Application,
    Capture,
    ProjectNew,
    ProjectDemo,
    CampaignDemo,
    Presentation,
    Script,
    ProjectSave,
    ProjectAdd,
    PrefabCreate,
    AssetUsage,
    ColorShader,
    Reimport(SharedAsset),
    BatchImport,
    Dependencies,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::New => "New scene",
            Self::PhysicsDemo => "Open physics demo",
            Self::RenderingDemo => "Open lighting and animation demo",
            Self::Open => "Open scene",
            Self::Save => "Save scene",
            Self::Import => "Import model",
            Self::ImportScript => "Import Rhai script",
            Self::Export => "Export Rust game",
            Self::Build => "Build game",
            Self::RetryBuild => "Retry build",
            Self::Application => "Application settings",
            Self::Capture => "Capture PNG",
            Self::ProjectNew => "Create project from scene",
            Self::ProjectDemo => "Open two-scene demo",
            Self::CampaignDemo => "Open Workshop Run",
            Self::Presentation => "Game UI Editor",
            Self::Script => "Script Editor",
            Self::ProjectSave => "Save project",
            Self::ProjectAdd => "Add scene to project",
            Self::BatchImport => "Batch import",
            Self::Dependencies => "Dependencies",
            Self::Reimport(SharedAsset::Model(_)) => "Reimport model",
            Self::Reimport(SharedAsset::Script(_)) => "Reimport script",
            Self::ColorShader => "Scene color shader",
            Self::AssetUsage => "Where used",
            Self::PrefabCreate => "Create prefab",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::ColorShader => {
                "GLSL function body · vec3 forge_color(vec3 rgb, vec2 uv, float time)"
            }
            Self::BatchImport => "Choose files below or enter one path per line. Relative paths use the displayed directory.",
            Self::Dependencies => "Shared assets embedded in the selected scene or prefab.",
            Self::Reimport(SharedAsset::Script(_)) => "Replacement .rhai file",
            Self::Reimport(SharedAsset::Model(_)) => "Replacement model file (.obj, .fbx, .gltf or .glb)",
            Self::AssetUsage => "Select a model or shared script to see the objects that use it.",
            Self::New => "Create an empty scene",
            Self::PhysicsDemo => "Load the Physics Workshop scene",
            Self::RenderingDemo => "Load the Light & Motion Workshop scene",
            Self::Open => "Scene or project file (.forge.json or .forge-project.json)",
            Self::Save => "Scene file (.forge.json)",
            Self::ProjectNew | Self::ProjectSave => "Project file (.forge-project.json)",
            Self::ProjectDemo => "Open the two-workshop demonstration project",
            Self::CampaignDemo => "Open the game with a menu and two levels",
            Self::Presentation => "Design the game overlay for this scene",
            Self::Script => "Rhai source",
            Self::ProjectAdd => "Optional path to an existing .forge.json scene",
            Self::PrefabCreate => "Prefab ID (lowercase letters, digits or hyphens)",
            Self::Import => "Model file (.obj, .fbx, .gltf or .glb)",
            Self::ImportScript => "Script file (.rhai)",
            Self::Export | Self::Build | Self::RetryBuild => {
                "Path to a new folder for the Cargo project"
            }
            Self::Capture => "Path to a new .png file",
            Self::Application => "Application metadata for desktop and Android exports",
        }
    }
}

fn asset_usage_rows(project: &Project, asset: SharedAsset) -> Vec<String> {
    project
        .asset_usages(asset)
        .into_iter()
        .map(|usage| {
            let (kind, owner) = match usage.owner {
                ModelOwner::Scene(name) => ("Scene", name),
                ModelOwner::Prefab(name) => ("Prefab", name),
            };
            format!(
                "{kind} {owner} · {} #{}",
                usage.object_name, usage.object_id
            )
        })
        .collect()
}

fn batch_import_fields(ui: &mut egui::Ui, browser: &Browser, paths: &mut String) {
    ui.label(browser.directory.display().to_string());
    egui::ScrollArea::vertical()
        .max_height(125.)
        .show(ui, |ui| {
            for entry in browser
                .entries
                .iter()
                .filter(|e| matches!(e.kind, "model" | "script"))
            {
                let mut selected = paths.lines().any(|line| line == entry.name);
                if ui.checkbox(&mut selected, &entry.name).changed() {
                    let mut lines: Vec<_> = paths.lines().filter(|l| *l != entry.name).collect();
                    if selected {
                        lines.push(&entry.name);
                    }
                    *paths = lines.join("\n");
                }
            }
        });
    ui.add(
        egui::TextEdit::multiline(paths)
            .desired_rows(4)
            .desired_width(420.),
    );
}

fn dependency_list(ui: &mut egui::Ui, project: &Project, prefab: &mut bool, owner_id: &mut String) {
    egui::ComboBox::from_id_salt("dependency-owner")
        .selected_text(format!(
            "{} · {}",
            if *prefab { "Prefab" } else { "Scene" },
            *owner_id
        ))
        .show_ui(ui, |ui| {
            for (is_prefab, id) in project
                .scenes()
                .map(|(id, _)| (false, id))
                .chain(project.prefabs().map(|id| (true, id)))
            {
                let kind = if is_prefab { "Prefab" } else { "Scene" };
                if ui
                    .selectable_label(
                        *prefab == is_prefab && *owner_id == id,
                        format!("{kind} · {id}"),
                    )
                    .clicked()
                {
                    *prefab = is_prefab;
                    *owner_id = id.to_owned();
                }
            }
        });
    let owner = if *prefab {
        ModelOwner::Prefab(owner_id)
    } else {
        ModelOwner::Scene(owner_id)
    };
    match project.dependencies(owner) {
        Ok(items) if items.is_empty() => {
            ui.label("No shared assets.");
        }
        Ok(items) => {
            egui::ScrollArea::vertical()
                .max_height(300.)
                .show(ui, |ui| {
                    for (asset, name) in items {
                        ui.label(format!("{asset} · {name}"));
                    }
                });
        }
        Err(error) => {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }
}

struct Dialog {
    action: Action,
    path: String,
    scene_id: String,
    dependencies: Option<Project>,
    prefab_dependencies: bool,
    shader_source: String,
    presentation: Option<UiDesigner>,
    script: Option<ScriptEditor>,
    usage: Option<(Project, SharedAsset, Vec<String>)>,
    error: Option<String>,
    profile: jobs::Profile,
    target: Option<jobs::Target>,
    application: Application,
}

#[derive(Copy, Clone, PartialEq)]
enum TransformTool {
    Select,
    Move,
    Rotate,
    Scale,
}

#[derive(Copy, Clone, PartialEq)]
enum DockTab {
    Project,
    Assets,
    Console,
}

#[derive(Copy, Clone)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Import,
    Undo,
    Redo,
    Duplicate,
    Delete,
    Frame,
    Play,
    Pause,
    Step,
    Tool(TransformTool),
}

impl Command {
    fn title(self) -> &'static str {
        match self {
            Self::New => "New scene",
            Self::Open => "Open…",
            Self::Save => "Save",
            Self::SaveAs => "Save as…",
            Self::Import => "Import model…",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Duplicate => "Duplicate",
            Self::Delete => "Delete",
            Self::Frame => "Frame selection",
            Self::Play => "Play / Stop",
            Self::Pause => "Pause / Resume",
            Self::Step => "Step one frame",
            Self::Tool(TransformTool::Select) => "Select",
            Self::Tool(TransformTool::Move) => "Move",
            Self::Tool(TransformTool::Rotate) => "Rotate",
            Self::Tool(TransformTool::Scale) => "Scale",
        }
    }

    fn key(self) -> egui::KeyboardShortcut {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let command = Modifiers::COMMAND;
        let shift = command | Modifiers::SHIFT;
        let plain = Modifiers::NONE;
        let (mods, key) = match self {
            Self::New => (command, Key::N),
            Self::Open => (command, Key::O),
            Self::Save => (command, Key::S),
            Self::SaveAs => (shift, Key::S),
            Self::Import => (command, Key::I),
            Self::Undo => (command, Key::Z),
            Self::Redo => (shift, Key::Z),
            Self::Duplicate => (command, Key::D),
            Self::Delete => (plain, Key::Delete),
            Self::Frame => (plain, Key::F),
            Self::Play => (plain, Key::F5),
            Self::Pause => (plain, Key::F6),
            Self::Step => (plain, Key::F7),
            Self::Tool(TransformTool::Select) => (plain, Key::Q),
            Self::Tool(TransformTool::Move) => (plain, Key::W),
            Self::Tool(TransformTool::Rotate) => (plain, Key::E),
            Self::Tool(TransformTool::Scale) => (plain, Key::R),
        };
        KeyboardShortcut::new(mods, key)
    }

    fn menu_button(self, ui: &egui::Ui) -> egui::Button<'static> {
        egui::Button::new(self.title()).shortcut_text(ui.ctx().format_shortcut(&self.key()))
    }
}

const TRANSFORM_TOOLS: [TransformTool; 4] = [
    TransformTool::Select,
    TransformTool::Move,
    TransformTool::Rotate,
    TransformTool::Scale,
];

const EDIT_KEYS: [Command; 14] = [
    Command::New,
    Command::Open,
    Command::SaveAs,
    Command::Save,
    Command::Import,
    Command::Redo,
    Command::Undo,
    Command::Duplicate,
    Command::Delete,
    Command::Frame,
    Command::Tool(TransformTool::Select),
    Command::Tool(TransformTool::Move),
    Command::Tool(TransformTool::Rotate),
    Command::Tool(TransformTool::Scale),
];

struct Browser {
    directory: PathBuf,
    filter: String,
    entries: Vec<project::Entry>,
    error: Option<String>,
}

impl Browser {
    fn new() -> Self {
        let mut browser = Self {
            directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            filter: String::new(),
            entries: Vec::new(),
            error: None,
        };
        browser.refresh();
        browser
    }

    fn refresh(&mut self) {
        match project::entries(&self.directory) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(error);
            }
        }
    }
}

struct Editor {
    scene: Scene,
    prepared: PreparedScene,
    selected: Option<usize>,
    preview: Option<GamePlayback>,
    overlay: Option<Overlay>,
    paused: bool,
    step_preview: bool,
    project: Option<OpenProject>,
    project_dirty: bool,
    view: View,
    viewport: egui::Rect,
    pixels_per_point: f32,
    ui_scale: u16,
    ui_scale_path: Option<PathBuf>,
    path: Option<PathBuf>,
    dialog: Option<Dialog>,
    dirty: bool,
    saved: Scene,
    history: project::History,
    scene_epoch: u64,
    pointer_edit: bool,
    dock: DockTab,
    collapsed: HashSet<u64>,
    focus_viewport: bool,
    show_hierarchy: bool,
    show_inspector: bool,
    show_project: bool,
    swap_docks: bool,
    transform_tool: TransformTool,
    object_filter: String,
    asset_filter: String,
    browser: Browser,
    build: Option<jobs::Job>,
    build_report: Option<jobs::Report>,
    last_build: Option<jobs::Job>,
    script_errors: Vec<String>,
    physics_events: Vec<String>,
    show_colliders: bool,
    game_keyboard: bool,
    grid: bool,
    status: String,
}

impl Editor {
    fn new() -> Self {
        let scene = Scene::rendering_demo();
        let ui_scale_path = ui_scale_path();
        let ui_scale = ui_scale_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|value| value.trim().parse().ok())
            .filter(|scale| UI_SCALES.contains(scale))
            .unwrap_or(DEFAULT_UI_SCALE);
        Self {
            prepared: PreparedScene::new(&scene),
            view: scene.camera.clone(),
            saved: scene.clone(),
            scene,
            selected: Some(1),
            preview: None,
            overlay: None,
            paused: false,
            step_preview: false,
            project: None,
            project_dirty: false,
            viewport: egui::Rect::NOTHING,
            pixels_per_point: 1.,
            ui_scale,
            ui_scale_path,
            path: None,
            dialog: None,
            dirty: false,
            history: project::History::default(),
            scene_epoch: 0,
            pointer_edit: false,
            dock: DockTab::Project,
            collapsed: HashSet::new(),
            focus_viewport: false,
            show_hierarchy: true,
            show_inspector: true,
            show_project: true,
            swap_docks: false,
            transform_tool: TransformTool::Move,
            object_filter: String::new(),
            asset_filter: String::new(),
            browser: Browser::new(),
            build: None,
            build_report: None,
            last_build: None,
            script_errors: Vec::new(),
            physics_events: Vec::new(),
            show_colliders: true,
            game_keyboard: false,
            grid: true,
            status: "Light & Motion demo loaded".into(),
        }
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if let Some(job) = &mut self.build {
            if let Some(report) = job.status().cloned() {
                self.status = format!("Build {}", report.state);
                self.build_report = Some(report);
                self.last_build = self.build.take();
                self.open_console();
                self.browser.refresh();
            } else {
                self.status = format!("Build · {}", job.stage().label());
            }
        }
        if let Some(scale) = ctx.input_mut(|input| {
            use egui::{Key, Modifiers};
            if input.consume_key(Modifiers::COMMAND, Key::Num0) {
                Some(DEFAULT_UI_SCALE)
            } else if input.consume_key(Modifiers::COMMAND, Key::Plus)
                || input.consume_key(Modifiers::COMMAND, Key::Equals)
            {
                UI_SCALES.into_iter().find(|&scale| scale > self.ui_scale)
            } else if input.consume_key(Modifiers::COMMAND, Key::Minus) {
                UI_SCALES
                    .into_iter()
                    .rev()
                    .find(|&scale| scale < self.ui_scale)
            } else {
                None
            }
        }) {
            self.set_ui_scale(scale);
        }
        let before = self.scene.clone();
        let epoch = self.scene_epoch;
        let mut navigated_history = self.shortcuts(ctx);
        self.pixels_per_point = ctx.pixels_per_point();
        let playing = self.preview.is_some();
        egui::TopBottomPanel::top("main-menu")
            .frame(compact_frame().fill(HEADER))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("FORGE3D").size(17.).strong().color(ACCENT));
                    ui.separator();
                    ui.add_enabled_ui(!playing, |ui| {
                        ui.menu_button("File", |ui| {
                            for command in [
                                Command::New,
                                Command::Open,
                                Command::Save,
                                Command::SaveAs,
                                Command::Import,
                            ] {
                                if ui.add(command.menu_button(ui)).clicked() {
                                    self.command(command);
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if ui.button("Export Rust…").clicked() {
                                self.prompt(Action::Export);
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("Edit", |ui| {
                            for (command, enabled) in [
                                (Command::Undo, self.history.can_undo()),
                                (Command::Redo, self.history.can_redo()),
                                (Command::Duplicate, self.selected.is_some()),
                                (Command::Delete, self.selected.is_some()),
                            ] {
                                if ui.add_enabled(enabled, command.menu_button(ui)).clicked() {
                                    navigated_history |= self.command(command);
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.menu_button("Project", |ui| self.project_menu(ui));
                    });
                    ui.menu_button("Workspace", |ui| {
                        ui.checkbox(&mut self.focus_viewport, "Focus viewport");
                        ui.checkbox(&mut self.show_hierarchy, "Hierarchy");
                        ui.checkbox(&mut self.show_inspector, "Inspector");
                        ui.checkbox(&mut self.show_project, "Project dock");
                        if ui.button("Show console").clicked() {
                            self.open_console();
                            ui.close_menu();
                        }
                        ui.checkbox(&mut self.show_colliders, "Collider guides");
                        ui.checkbox(&mut self.game_keyboard, "Send all keys to Play")
                            .on_hover_text("Use the toolbar to stop, pause, or step in Play.");
                        ui.checkbox(&mut self.swap_docks, "Swap side panels");
                        ui.separator();
                        ui.menu_button(format!("UI scale ({}%)", self.ui_scale), |ui| {
                            for scale in UI_SCALES {
                                if ui
                                    .selectable_label(self.ui_scale == scale, format!("{scale}%"))
                                    .clicked()
                                {
                                    self.set_ui_scale(scale);
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    ui.menu_button("Tools", |ui| {
                        if ui
                            .add_enabled(!playing, egui::Button::new("Game UI Editor…"))
                            .clicked()
                        {
                            self.prompt(Action::Presentation);
                            ui.close_menu();
                        }

                        if ui
                            .add_enabled(!playing, egui::Button::new("Application settings…"))
                            .clicked()
                        {
                            self.prompt(Action::Application);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(!playing, egui::Button::new("Light & Motion demo…"))
                            .clicked()
                        {
                            self.prompt(Action::RenderingDemo);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(!playing, egui::Button::new("Physics demo…"))
                            .clicked()
                        {
                            self.prompt(Action::PhysicsDemo);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                !playing && self.build.is_none(),
                                egui::Button::new("Build…"),
                            )
                            .clicked()
                        {
                            self.prompt(Action::Build);
                            ui.close_menu();
                        }
                        if let Some(job) = &mut self.build {
                            if ui.button("Cancel build").clicked() {
                                job.cancel();
                                ui.close_menu();
                            }
                        } else if self.last_build.is_some()
                            && ui
                                .add_enabled(!playing, egui::Button::new("Retry build…"))
                                .clicked()
                        {
                            self.prompt(Action::RetryBuild);
                            ui.close_menu();
                        }
                        if ui.button("Capture PNG…").clicked() {
                            self.prompt(Action::Capture);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(!playing, egui::Button::new("Test scene"))
                            .clicked()
                        {
                            self.test_scene();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        ui.strong("Keyboard shortcuts");
                        egui::Grid::new("keyboard-shortcuts")
                            .striped(true)
                            .show(ui, |ui| {
                                for command in EDIT_KEYS.into_iter().chain([
                                    Command::Play,
                                    Command::Pause,
                                    Command::Step,
                                ]) {
                                    ui.label(command.title());
                                    ui.monospace(ui.ctx().format_shortcut(&command.key()));
                                    ui.end_row();
                                }
                            });
                    });
                    ui.separator();
                    if ui
                        .button(
                            RichText::new(if self.preview.is_some() {
                                "■ Stop"
                            } else {
                                "▶ Play"
                            })
                            .strong()
                            .color(ACCENT),
                        )
                        .on_hover_text("F5")
                        .clicked()
                    {
                        self.command(Command::Play);
                    }
                    if ui
                        .add_enabled(
                            self.preview.is_some(),
                            egui::Button::new(if self.paused { "Resume" } else { "Pause" }),
                        )
                        .on_hover_text("F6")
                        .clicked()
                    {
                        self.command(Command::Pause);
                    }
                    if ui
                        .add_enabled(
                            self.preview.is_some() && self.paused,
                            egui::Button::new("Step"),
                        )
                        .on_hover_text("F7")
                        .clicked()
                    {
                        self.command(Command::Step);
                    }
                });
            });
        let playing = self.preview.is_some();
        egui::TopBottomPanel::bottom("status")
            .frame(compact_frame().fill(HEADER))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).small().color(MUTED))
                            .truncate(),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{} objects  ·  {} models",
                            self.scene.objects.len(),
                            self.scene.models.len()
                        ))
                        .small(),
                    );
                    if !playing {
                        ui.label(
                            RichText::new("RMB Orbit  ·  MMB Pan  ·  Wheel Zoom")
                                .small()
                                .color(MUTED),
                        );
                    }
                });
            });

        let panel_limit = ctx.available_rect().width()
            / if self.show_hierarchy && self.show_inspector {
                3.
            } else {
                2.
            };
        if self.show_hierarchy && !self.focus_viewport {
            let max_width = panel_limit.min(350.);
            let panel = if self.swap_docks {
                egui::SidePanel::right("hierarchy-right")
            } else {
                egui::SidePanel::left("hierarchy")
            };
            panel
                .default_width(max_width.min(254.))
                .width_range(max_width.min(182.)..=max_width)
                .frame(panel_frame())
                .show(ctx, |ui| self.hierarchy(ui, playing));
        }
        if self.show_inspector && !self.focus_viewport {
            let max_width = panel_limit.min(410.);
            let panel = if self.swap_docks {
                egui::SidePanel::left("inspector-left")
            } else {
                egui::SidePanel::right("inspector")
            };
            panel
                .default_width(max_width.min(312.))
                .width_range(max_width.min(220.)..=max_width)
                .frame(panel_frame())
                .show(ctx, |ui| self.inspector(ui, playing));
        }

        if self.show_project && !self.focus_viewport {
            egui::TopBottomPanel::bottom("project-browser")
                .resizable(true)
                .default_height(185.)
                .height_range(110.0..=400.0)
                .frame(panel_frame())
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.dock, DockTab::Project, "Project");
                        ui.selectable_value(&mut self.dock, DockTab::Assets, "Assets");
                        ui.selectable_value(&mut self.dock, DockTab::Console, "Console");
                    });
                    ui.separator();
                    match self.dock {
                        DockTab::Project => self.project_browser(ui, playing),
                        DockTab::Assets => self.assets(ui, playing),
                        DockTab::Console => self.console_panel(ui),
                    }
                });
        }
        egui::TopBottomPanel::top("viewport-tools")
            .frame(compact_frame().fill(HEADER))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(self.preview.is_none(), "Scene")
                        .clicked()
                        && self.preview.is_some()
                    {
                        self.command(Command::Play);
                    }
                    if ui
                        .selectable_label(self.preview.is_some(), "Game")
                        .clicked()
                        && self.preview.is_none()
                    {
                        self.command(Command::Play);
                    }
                    ui.separator();
                    if self.preview.is_none() {
                        for tool in TRANSFORM_TOOLS {
                            let command = Command::Tool(tool);
                            ui.selectable_value(&mut self.transform_tool, tool, command.title())
                                .on_hover_text(ui.ctx().format_shortcut(&command.key()));
                        }
                        ui.separator();
                        if ui.button("Frame").on_hover_text("F").clicked() {
                            self.frame_selection();
                        }
                        ui.checkbox(&mut self.grid, "Grid");
                    }
                    ui.toggle_value(&mut self.focus_viewport, "Focus")
                        .on_hover_text("Expand the viewport or restore your dock layout");
                });
            });
        let playing = self.preview.is_some();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                self.viewport = ui.max_rect();
                let response = ui.allocate_rect(self.viewport, egui::Sense::click_and_drag());
                if !playing && self.dialog.is_none() {
                    self.viewport_input(ui, &response);
                    response.context_menu(|ui| {
                        if self.selected.is_some() {
                            if let Some(command) = Self::object_menu(ui) {
                                self.command(command);
                            }
                            ui.separator();
                        }
                        self.context_create(ui);
                    });
                    self.transform_handles(ui);
                }
            });
        self.show_dialog(ctx);
        let pointer_down = ctx.input(|input| input.pointer.primary_down());
        if epoch == self.scene_epoch && !navigated_history && before != self.scene {
            if !self.pointer_edit {
                self.history.record(before);
            }
            self.pointer_edit = pointer_down;
        }
        if !pointer_down {
            self.pointer_edit = false;
        }
        self.dirty = self.scene != self.saved;
        // egui-miniquad only reports native DPI when it changes. Use the
        // window's current DPI so the initial scale also works on Retina.
        let dpi = macroquad::miniquad::window::dpi_scale();
        ctx.set_pixels_per_point(dpi * f32::from(self.ui_scale) / 100.);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.dialog.is_some() || self.preview.is_some() && self.game_keyboard {
            return false;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let typing = focused.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if typing {
            if self.preview.is_none() {
                let save = ctx.input_mut(|input| {
                    [Command::SaveAs, Command::Save]
                        .into_iter()
                        .find(|command| input.consume_shortcut(&command.key()))
                });
                if let Some(command) = save {
                    self.command(command);
                }
            }
            return false;
        }
        // The miniquad egui adapter doesn't forward function keys.
        for (key, command) in [
            (KeyCode::F5, Command::Play),
            (KeyCode::F6, Command::Pause),
            (KeyCode::F7, Command::Step),
        ] {
            if is_key_pressed(key) {
                return self.command(command);
            }
        }
        if self.preview.is_some() {
            return false;
        }
        let command = ctx.input_mut(|input| {
            EDIT_KEYS
                .into_iter()
                .find(|command| input.consume_shortcut(&command.key()))
                .or_else(|| {
                    input
                        .consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
                        .then_some(Command::Redo)
                })
        });
        command.is_some_and(|command| self.command(command))
    }

    fn command(&mut self, command: Command) -> bool {
        match command {
            Command::New => {
                if self.has_unsaved_changes() {
                    self.prompt(Action::New);
                } else {
                    self.new_scene();
                }
            }
            Command::Open => self.prompt(Action::Open),
            Command::Import => self.prompt(Action::Import),
            Command::Save | Command::SaveAs => {
                self.prompt(if self.project.is_some() {
                    Action::ProjectSave
                } else {
                    Action::Save
                });
                if matches!(command, Command::Save) && self.path.is_some() {
                    if let Some(mut dialog) = self.dialog.take() {
                        match self.perform(&dialog) {
                            Ok(message) => self.status = message,
                            Err(error) => {
                                dialog.error = Some(error);
                                self.dialog = Some(dialog);
                            }
                        }
                    }
                }
            }
            Command::Undo => return self.travel_history(true),
            Command::Redo => return self.travel_history(false),
            Command::Duplicate => {
                if let Some(index) = self.selected {
                    let id = self.scene.objects[index].id;
                    match project::duplicate_object(&mut self.scene, id) {
                        Ok(copy) => {
                            self.selected = self.scene.object_index(copy).ok();
                            self.status = "Duplicated object".into();
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
            Command::Delete => {
                if let Some(index) = self.selected {
                    let id = self.scene.objects[index].id;
                    match self.scene.delete_object(id, true) {
                        Ok(()) => {
                            self.selected = None;
                            self.status = "Deleted object and its children".into();
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
            Command::Frame => self.frame_selection(),
            Command::Play => {
                self.paused = false;
                self.step_preview = false;
                if self.preview.is_some() {
                    self.preview = None;
                    self.overlay = None;
                    self.prepared = PreparedScene::new(&self.scene);
                    self.status = "Play stopped".into();
                } else {
                    match self.game(true).and_then(GamePlayback::new) {
                        Ok(preview) => {
                            self.overlay = Some(Overlay::new(&preview.scene().presentation));
                            self.preview = Some(preview);
                            self.physics_events.clear();
                            self.status = "Play started".into();
                        }
                        Err(error) => {
                            self.record_script_error(error);
                            self.open_console();
                        }
                    }
                }
            }
            Command::Pause => {
                if self.preview.is_some() {
                    self.paused = !self.paused;
                    self.status = if self.paused {
                        "Game paused"
                    } else {
                        "Play resumed"
                    }
                    .into();
                }
            }
            Command::Step => self.step_preview = self.preview.is_some() && self.paused,
            Command::Tool(tool) => self.transform_tool = tool,
        }
        false
    }

    fn open_console(&mut self) {
        self.focus_viewport = false;
        self.show_project = true;
        self.dock = DockTab::Console;
    }

    fn console_panel(&mut self, ui: &mut egui::Ui) {
        let empty = self.script_errors.is_empty()
            && self.build_report.is_none()
            && self.physics_events.is_empty();
        if ui.add_enabled(!empty, egui::Button::new("Clear")).clicked() {
            self.script_errors.clear();
            self.physics_events.clear();
            self.build_report = None;
        }
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if empty {
                    ui.label("No messages.");
                }
                for event in &self.physics_events {
                    ui.monospace(event);
                }
                for error in &self.script_errors {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                if let Some(report) = &self.build_report {
                    ui.label(format!("Build {} · {}", report.state, report.stage.label()));
                    for artifact in &report.artifacts {
                        ui.monospace(artifact.display().to_string());
                    }
                    if let Some(error) = &report.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    if !report.stdout.is_empty() {
                        ui.monospace(&report.stdout);
                    }
                    if !report.stderr.is_empty() {
                        ui.monospace(&report.stderr);
                    }
                }
            });
    }

    fn set_ui_scale(&mut self, scale: u16) {
        if self.ui_scale == scale {
            return;
        }
        self.ui_scale = scale;
        if let Some(path) = &self.ui_scale_path {
            if let Err(error) = fs::create_dir_all(path.parent().expect("settings directory"))
                .and_then(|()| fs::write(path, scale.to_string()))
            {
                self.status = format!("Could not save UI scale: {error}");
            }
        }
    }

    fn project_browser(&mut self, ui: &mut egui::Ui, playing: bool) {
        let mut directory = None;
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Up").clicked() {
                directory = self.browser.directory.parent().map(Path::to_owned);
            }
            if ui.small_button("Refresh").clicked() {
                self.browser.refresh();
            }
            ui.label(
                RichText::new(self.browser.directory.display().to_string())
                    .small()
                    .color(MUTED),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.browser.filter)
                    .hint_text("Filter files")
                    .desired_width(132.),
            );
        });
        if let Some(error) = &self.browser.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        let filter = self.browser.filter.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("project-files")
            .show(ui, |ui| {
                for entry in &self.browser.entries {
                    if !entry.name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let path = self.browser.directory.join(&entry.name);
                    let label = if entry.kind == "directory" {
                        format!("{}/", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let item = ui
                        .horizontal(|ui| {
                            ui.add_sized(
                                [52., 18.],
                                egui::Label::new(
                                    RichText::new(if entry.kind == "directory" {
                                        "Folder"
                                    } else {
                                        entry.kind
                                    })
                                    .small()
                                    .color(MUTED),
                                ),
                            );
                            ui.add_enabled(
                                !playing || entry.kind == "directory",
                                egui::Button::new(label).frame(false),
                            )
                        })
                        .inner;
                    let mut activate = item.clicked();
                    item.context_menu(|ui| {
                        let label = match entry.kind {
                            "directory" | "scene" | "project" => Some("Open"),
                            "model" | "script" => Some("Import…"),
                            _ => None,
                        };
                        if let Some(label) = label {
                            if ui.button(label).clicked() {
                                activate = true;
                                ui.close_menu();
                            }
                        }
                        if ui.button("Copy path").clicked() {
                            ui.ctx().copy_text(path.display().to_string());
                            ui.close_menu();
                        }
                    });
                    if activate {
                        match entry.kind {
                            "directory" => directory = Some(path),
                            "scene" | "project" => action = Some((Action::Open, path)),
                            "model" => action = Some((Action::Import, path)),
                            "script" => action = Some((Action::ImportScript, path)),
                            _ => {}
                        }
                    }
                }
            });
        if let Some(directory) = directory {
            self.browser.directory = directory;
            self.browser.refresh();
        }
        if let Some((choice, path)) = action {
            self.prompt(choice);
            if let Some(dialog) = &mut self.dialog {
                dialog.path = path.display().to_string();
            }
        }
    }

    fn test_scene(&mut self) {
        match self.game(true).and_then(GamePlayback::new) {
            Ok(mut play) => {
                for _ in 0..60 {
                    play.tick(Vec2::ZERO, 1. / 60.);
                    play.take_events();
                }
                let errors = play.take_script_errors();
                if errors.is_empty() {
                    self.status = "Scene test passed (60 frames)".into();
                } else {
                    for error in errors {
                        self.record_script_error(error);
                    }
                    self.open_console();
                }
            }
            Err(error) => {
                self.record_script_error(error);
                self.open_console();
            }
        }
    }

    fn hierarchy(&mut self, ui: &mut egui::Ui, playing: bool) {
        ui.horizontal_wrapped(|ui| {
            section(ui, "Hierarchy");
            ui.add_enabled_ui(!playing, |ui| self.context_create(ui));
        });
        ui.add(egui::TextEdit::singleline(&mut self.object_filter).hint_text("Find objects"));
        ui.label(
            RichText::new(format!(
                "{}{}",
                self.scene.title,
                if self.has_unsaved_changes() { " *" } else { "" }
            ))
            .color(MUTED),
        );
        ui.add_enabled_ui(!playing, |ui| {
            ui.separator();
            let mut context = None;
            egui::ScrollArea::vertical()
                .id_salt("objects")
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    if self.scene.objects.is_empty() {
                        ui.label("Add an object to start.");
                    }
                    let filter = self.object_filter.to_lowercase();
                    let parents: HashSet<_> = self
                        .scene
                        .objects
                        .iter()
                        .filter_map(|object| object.parent)
                        .collect();
                    let mut hidden_depth = None;
                    for (i, depth, _) in self.scene.object_order() {
                        if hidden_depth.is_some_and(|collapsed| depth > collapsed) {
                            continue;
                        }
                        hidden_depth = None;
                        if !self.scene.objects[i].name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let object = &self.scene.objects[i];
                        let label = RichText::new(&object.name).color(if object.visible {
                            ui.visuals().text_color()
                        } else {
                            MUTED
                        });
                        let id = object.id;
                        let has_children = parents.contains(&id);
                        let mut selected = false;
                        ui.horizontal(|ui| {
                            ui.add_space((depth.min(10) * 12) as f32);
                            if has_children {
                                let marker = if self.collapsed.contains(&id) {
                                    "▸"
                                } else {
                                    "▾"
                                };
                                if ui
                                    .small_button(marker)
                                    .on_hover_text("Expand or collapse children")
                                    .clicked()
                                    && !self.collapsed.remove(&id)
                                {
                                    self.collapsed.insert(id);
                                }
                            } else {
                                ui.add_space(18.);
                            }
                            let (icon, _) =
                                ui.allocate_exact_size(egui::vec2(12., 14.), egui::Sense::hover());
                            let c = icon.center();
                            let ink = if object.visible { ACCENT } else { MUTED };
                            let pen = egui::Stroke::new(1.3_f32, ink);
                            match object.shape {
                                Shape::Sphere => {
                                    ui.painter().circle_stroke(c, 4.4, pen);
                                }
                                Shape::Cube => {
                                    ui.painter().rect_stroke(
                                        egui::Rect::from_center_size(c, egui::vec2(8., 8.)),
                                        1.,
                                        pen,
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                Shape::Plane => {
                                    ui.painter().line_segment(
                                        [c - egui::vec2(5., -2.), c + egui::vec2(5., -2.)],
                                        pen,
                                    );
                                }
                                Shape::Model(_) => {
                                    ui.painter().add(egui::Shape::closed_line(
                                        vec![
                                            c + egui::vec2(0., -5.),
                                            c + egui::vec2(5., 4.),
                                            c + egui::vec2(-5., 4.),
                                        ],
                                        pen,
                                    ));
                                }
                                Shape::Empty => {
                                    ui.painter().circle_filled(c, 1.8, ink);
                                }
                            }
                            let item = ui
                                .selectable_label(self.selected == Some(i), label)
                                .on_hover_text(format!("Object #{id}"));
                            selected = item.clicked() || item.secondary_clicked();
                            item.context_menu(|ui| {
                                if let Some(command) = Self::object_menu(ui) {
                                    context = Some((i, command));
                                }
                            });
                        });
                        if filter.is_empty() && self.collapsed.contains(&id) {
                            hidden_depth = Some(depth);
                        }
                        if selected {
                            self.selected = Some(i);
                        }
                    }
                });
            if let Some((index, command)) = context {
                self.selected = Some(index);
                self.command(command);
            }
        });
    }

    fn assets(&mut self, ui: &mut egui::Ui, playing: bool) {
        ui.add_enabled_ui(!playing, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Import model…").clicked() {
                    self.prompt(Action::Import);
                }
                if self.project.is_some() {
                    if ui.small_button("Batch import…").clicked() {
                        self.prompt(Action::BatchImport);
                    }
                    if ui.small_button("Dependencies…").clicked() {
                        self.prompt(Action::Dependencies);
                        if let (Some(open), Some(dialog)) = (&self.project, &mut self.dialog) {
                            dialog.scene_id.clone_from(&open.active);
                            match open.snapshot(&self.scene) {
                                Ok(project) => dialog.dependencies = Some(project),
                                Err(error) => dialog.error = Some(error),
                            }
                        }
                    }
                }
                let has_assets = !self.scene.models.is_empty()
                    || !self.scene.scripts.is_empty()
                    || self.project.as_ref().is_some_and(|open| {
                        !open.project.models().is_empty() || !open.project.scripts().is_empty()
                    });
                if ui
                    .add_enabled(has_assets, egui::Button::new("Where used…"))
                    .clicked()
                {
                    self.prompt(Action::AssetUsage);
                    let snapshot = match &self.project {
                        Some(open) => open.snapshot(&self.scene),
                        None => Project::new("scene", &self.scene),
                    };
                    if let Some(dialog) = &mut self.dialog {
                        match snapshot {
                            Ok(project) => {
                                if let Some(id) = project
                                    .models()
                                    .keys()
                                    .next()
                                    .copied()
                                    .map(SharedAsset::Model)
                                    .or_else(|| {
                                        project
                                            .scripts()
                                            .keys()
                                            .next()
                                            .copied()
                                            .map(SharedAsset::Script)
                                    })
                                {
                                    let rows = asset_usage_rows(&project, id);
                                    dialog.usage = Some((project, id, rows));
                                }
                            }
                            Err(error) => dialog.error = Some(error),
                        }
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut self.asset_filter).hint_text("Find assets"));
                if ui.small_button("Import .rhai…").clicked() {
                    self.prompt(Action::ImportScript);
                }
                if ui.small_button("New shared script").clicked() {
                    let name = (1..)
                        .map(|index| format!("script-{index}.rhai"))
                        .find(|name| self.scene.scripts.iter().all(|script| &script.name != name))
                        .expect("finite number of scripts");
                    self.show_script(ScriptEditor::new(
                        ScriptTarget::NewShared,
                        name,
                        scripting::DEFAULT_SCRIPT.into(),
                    ));
                }
            });
            let filter = self.asset_filter.to_lowercase();
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("asset-list")
                .show(ui, |ui| {
                    let mut edit = None;
                    for (index, script) in self.scene.scripts.iter().enumerate() {
                        if !script.name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let item = ui
                            .button(RichText::new(&script.name).monospace())
                            .on_hover_text("Edit shared script");
                        if item.clicked() {
                            edit = Some(index);
                        }
                        item.context_menu(|ui| {
                            if ui.button("Edit script…").clicked() {
                                edit = Some(index);
                                ui.close_menu();
                            }
                        });
                    }
                    if let Some(index) = edit {
                        self.open_script(ScriptTarget::Shared(index));
                    }
                    if self.scene.models.is_empty() {
                        ui.label(RichText::new("Import OBJ, FBX, glTF or GLB.").color(MUTED));
                    }
                    for i in 0..self.scene.models.len() {
                        let model = &self.scene.models[i];
                        if !model.name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        if ui
                            .button(format!("+  {}", model.name))
                            .on_hover_text("Add model instance")
                            .clicked()
                        {
                            self.add(Object::new(&model.name, Shape::Model(i)));
                        }
                    }
                });
        });
    }

    fn inspector(&mut self, ui: &mut egui::Ui, playing: bool) {
        section(ui, "Inspector");
        ui.separator();
        ui.add_enabled_ui(!playing, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let Some(index) = self.selected else {
                    self.dirty |= ui.text_edit_singleline(&mut self.scene.title).changed();
                    section(ui, "Game camera");
                    self.dirty |= vector(ui, "Target", &mut self.scene.camera.target, 0.05);
                    self.dirty |= number(ui, "Yaw", &mut self.scene.camera.yaw, -360. ..=360.);
                    self.dirty |= number(ui, "Pitch", &mut self.scene.camera.pitch, -89. ..=89.);
                    self.dirty |=
                        number(ui, "Distance", &mut self.scene.camera.distance, 0.1..=500.);
                    if ui.button("Use editor camera").clicked() {
                        self.scene.camera = self.view.clone();
                        self.dirty = true;
                    }
                    ui.separator();
                    egui::CollapsingHeader::new("Lighting")
                        .default_open(true)
                        .show(ui, |ui| {
                            let light = &mut self.scene.lighting;
                            let mut direction = light.direction;
                            if vector(ui, "Sun direction", &mut direction, 0.025)
                                && (0.00001..=1e12)
                                    .contains(&Vec3::from_array(direction).length_squared())
                            {
                                light.direction = direction;
                            }
                            ui.color_edit_button_rgb(&mut light.color);
                            number(ui, "Sun intensity", &mut light.intensity, 0. ..=50.);
                            number(ui, "Ambient", &mut light.ambient, 0. ..=3.);
                            ui.checkbox(&mut light.shadows, "Sun shadows");
                            number(ui, "Shadow coverage", &mut light.shadow_extent, 1. ..=200.);
                        });
                    ui.separator();
                    if ui.button("Edit scene color shader…").clicked() {
                        self.prompt(Action::ColorShader);
                    }
                    section(ui, "Camera script");
                    let mut controller = self.scene.camera_controller;
                    let display = controller
                        .and_then(|id| self.scene.object_index(id).ok())
                        .map_or("None", |index| self.scene.objects[index].name.as_str());
                    egui::ComboBox::from_id_salt("camera-controller")
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut controller, None, "None");
                            for object in &self.scene.objects {
                                if object.script.is_some() || object.script_asset.is_some() {
                                    ui.selectable_value(
                                        &mut controller,
                                        Some(object.id),
                                        &object.name,
                                    );
                                }
                            }
                        });
                    self.scene.camera_controller = controller;
                    ui.separator();
                    section(ui, "Physics and input");
                    vector(ui, "Gravity", &mut self.scene.gravity, 0.05);
                    let input = &mut self.scene.bindings;
                    for (name, key) in [
                        ("Forward", &mut input.forward),
                        ("Backward", &mut input.backward),
                        ("Left", &mut input.left),
                        ("Right", &mut input.right),
                    ] {
                        egui::ComboBox::from_id_salt(("key", name))
                            .selected_text(key.as_str())
                            .show_ui(ui, |ui| {
                                for &option in controls::KEY_NAMES {
                                    ui.selectable_value(key, option.into(), option);
                                }
                            });
                        ui.label(name);
                    }
                    ui.checkbox(&mut input.arrows, "Also use arrow keys");
                    return;
                };
                self.parent_inspector(ui, index);
                let object = &mut self.scene.objects[index];
                self.dirty |= ui.text_edit_singleline(&mut object.name).changed();
                self.dirty |= ui.checkbox(&mut object.visible, "Visible").changed();
                ui.separator();
                egui::CollapsingHeader::new("Transform")
                    .default_open(true)
                    .show(ui, |ui| {
                        self.dirty |= vector(ui, "Position", &mut object.position, 0.025);
                        self.dirty |= vector(ui, "Rotation (°)", &mut object.rotation, 0.5);
                        self.dirty |= vector(ui, "Scale", &mut object.scale, 0.025);
                    });
                egui::CollapsingHeader::new("Behaviors")
                    .default_open(true)
                    .show(ui, |ui| {
                        self.dirty |= number(ui, "Spin (°/s)", &mut object.spin, -720. ..=720.);
                        self.dirty |=
                            number(ui, "Movement (units/s)", &mut object.speed, 0. ..=50.);
                    });
                let rig = match object.shape {
                    Shape::Model(index) => self.scene.models[index].rig.as_deref(),
                    _ => None,
                };
                rendering_inspector(ui, object, rig);
                physics_inspector(ui, object);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Duplicate").clicked() {
                        let mut duplicate = self.scene.objects[index].clone();
                        duplicate.name.push_str(" copy");
                        duplicate.position[0] += 1.;
                        self.add(duplicate);
                    }
                    let id = self.scene.objects[index].id;
                    let has_children = self
                        .scene
                        .objects
                        .iter()
                        .any(|object| object.parent == Some(id));
                    if ui
                        .button(if has_children {
                            "Delete subtree"
                        } else {
                            "Delete"
                        })
                        .clicked()
                    {
                        match self.scene.delete_object(id, true) {
                            Ok(()) => self.selected = None,
                            Err(error) => self.status = error,
                        }
                    }
                    if ui.button("Scene settings").clicked() {
                        self.selected = None;
                    }
                });
                if self.selected == Some(index) {
                    self.script_inspector(ui, index);
                }
            });
        });
    }

    fn parent_inspector(&mut self, ui: &mut egui::Ui, index: usize) {
        let object = &self.scene.objects[index];
        let id = object.id;
        let original = object.parent;
        let mut parent = original;
        let name = original
            .and_then(|id| self.scene.object_index(id).ok())
            .map_or("Scene root", |i| self.scene.objects[i].name.as_str());
        egui::ComboBox::from_id_salt("object-parent")
            .selected_text(name)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut parent, None, "Scene root");
                for candidate in &self.scene.objects {
                    if candidate.id != id {
                        ui.selectable_value(
                            &mut parent,
                            Some(candidate.id),
                            format!("{} · #{}", candidate.name, candidate.id),
                        );
                    }
                }
            });
        if parent != original {
            if let Err(error) = self.scene.set_parent(id, parent) {
                self.status = error;
            }
        }
    }

    fn script_inspector(&mut self, ui: &mut egui::Ui, index: usize) {
        ui.separator();
        section(ui, "Script · Rhai");
        let object = &mut self.scene.objects[index];
        let original = object.script_asset;
        let mut selected = original;
        if !self.scene.scripts.is_empty() {
            let name = selected.map_or("Inline / none", |i| self.scene.scripts[i].name.as_str());
            egui::ComboBox::from_id_salt("script-asset")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, None, "Inline / none");
                    for (i, script) in self.scene.scripts.iter().enumerate() {
                        ui.selectable_value(&mut selected, Some(i), &script.name);
                    }
                });
            if original != selected {
                object.script_asset = selected;
                object.script = None;
            }
        }
        let target = selected.map(ScriptTarget::Shared).or_else(|| {
            object
                .script
                .as_ref()
                .map(|_| ScriptTarget::Inline(object.id))
        });
        if let Some(target) = target {
            if matches!(target, ScriptTarget::Shared(_)) {
                ui.label(
                    RichText::new("Shared source · affects all linked objects")
                        .small()
                        .color(MUTED),
                );
            }
            let (edit, detach) = ui
                .horizontal_wrapped(|ui| {
                    (
                        ui.button("Edit script…").clicked(),
                        ui.button("Detach script").clicked(),
                    )
                })
                .inner;
            if detach {
                object.script = None;
                object.script_asset = None;
            } else if edit {
                self.open_script(target);
            }
        } else if ui.button("+ Attach inline script").clicked() {
            let target = ScriptTarget::Inline(object.id);
            self.open_script(target);
        }
    }

    fn open_script(&mut self, target: ScriptTarget) {
        let (name, source) = match target {
            ScriptTarget::Inline(id) => {
                let Ok(index) = self.scene.object_index(id) else {
                    return;
                };
                let object = &self.scene.objects[index];
                (
                    format!("{} · inline", object.name),
                    object
                        .script
                        .as_deref()
                        .unwrap_or(scripting::DEFAULT_SCRIPT),
                )
            }
            ScriptTarget::Shared(index) => {
                let Some(asset) = self.scene.scripts.get(index) else {
                    return;
                };
                (asset.name.clone(), asset.source.as_str())
            }
            ScriptTarget::NewShared => return,
        };
        self.show_script(ScriptEditor::new(target, name, source.to_owned()));
    }

    fn show_script(&mut self, editor: ScriptEditor) {
        self.prompt(Action::Script);
        if let Some(dialog) = &mut self.dialog {
            dialog.script = Some(editor);
        }
    }

    fn record_script_error(&mut self, error: String) {
        self.status = error.clone();
        if self.script_errors.len() >= 100 {
            self.script_errors.remove(0);
        }
        self.script_errors.push(error);
    }

    fn travel_history(&mut self, undo: bool) -> bool {
        let selected_id = self.selected.map(|index| self.scene.objects[index].id);
        let old_models = self.scene.models.clone();
        let changed = if undo {
            self.history.undo(&mut self.scene)
        } else {
            self.history.redo(&mut self.scene)
        };
        if changed {
            if old_models != self.scene.models {
                self.prepared = PreparedScene::new(&self.scene);
            }
            self.selected = selected_id.and_then(|id| self.scene.object_index(id).ok());
            self.status = if undo {
                "Undid scene edit"
            } else {
                "Redid scene edit"
            }
            .into();
        }
        changed
    }

    fn transform_handles(&mut self, ui: &mut egui::Ui) {
        if self.transform_tool == TransformTool::Select {
            return;
        }
        let Some(index) = self.selected else { return };
        let world = self.scene.world_transforms();
        let (matrix, visible) = world[index];
        if !visible || self.viewport.width() < 1. || self.viewport.height() < 1. {
            return;
        }
        let parent = self.scene.objects[index]
            .parent
            .and_then(|id| self.scene.object_index(id).ok())
            .map_or(Mat4::IDENTITY, |index| world[index].0);
        let mut camera = self.view.camera();
        camera.aspect = Some(self.viewport.width() / self.viewport.height());
        let projection = camera.matrix();
        let origin = matrix.transform_point3(Vec3::ZERO);
        let Some(start) = screen_point(projection, origin, self.viewport) else {
            return;
        };
        let length = (origin.distance(camera.position) * 0.16).clamp(0.15, 40.);
        for (axis, label, color) in [
            (0, "X", Color32::LIGHT_RED),
            (1, "Y", Color32::LIGHT_GREEN),
            (2, "Z", Color32::LIGHT_BLUE),
        ] {
            let axis_vector = [Vec3::X, Vec3::Y, Vec3::Z][axis];
            let direction = if self.transform_tool == TransformTool::Scale {
                let [x, y, z] = self.scene.objects[index].rotation.map(f32::to_radians);
                parent.transform_vector3(Quat::from_euler(EulerRot::XYZ, x, y, z) * axis_vector)
            } else {
                parent.transform_vector3(axis_vector)
            };
            if direction.length_squared() < 1e-8 {
                continue;
            }
            let step = length / direction.length();
            let Some(end) = screen_point(projection, origin + direction * step, self.viewport)
            else {
                continue;
            };
            let delta = end - start;
            if delta.length_sq() < 144. || !self.viewport.contains(end) {
                continue;
            }
            let id = ui
                .id()
                .with(("translate", self.scene.objects[index].id, axis));
            let handle = ui.interact(
                egui::Rect::from_center_size(end, egui::vec2(20., 20.)),
                id,
                egui::Sense::drag(),
            );
            let painter = ui.painter().with_clip_rect(self.viewport);
            painter.line_segment([start, end], egui::Stroke::new(2.0_f32, color));
            let size = if handle.hovered() || handle.dragged() {
                10.
            } else {
                8.
            };
            match self.transform_tool {
                TransformTool::Scale => {
                    painter.rect_filled(
                        egui::Rect::from_center_size(end, egui::vec2(size * 2., size * 2.)),
                        2.,
                        color,
                    );
                }
                TransformTool::Select => continue,
                TransformTool::Move | TransformTool::Rotate => {
                    painter.circle_filled(end, size, color);
                }
            }
            if self.transform_tool == TransformTool::Rotate {
                let tangent = egui::vec2(-delta.y, delta.x).normalized() * 16.;
                painter.line_segment(
                    [end - tangent, end + tangent],
                    egui::Stroke::new(1.0_f32, color),
                );
            }
            painter.text(
                end,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::monospace(11.),
                Color32::BLACK,
            );
            if handle.dragged() {
                let motion = ui.input(|input| input.pointer.delta());
                let object = &mut self.scene.objects[index];
                match self.transform_tool {
                    TransformTool::Select => continue,
                    TransformTool::Move => {
                        object.position[axis] += motion.dot(delta) * step / delta.length_sq()
                    }
                    TransformTool::Rotate => {
                        let tangent = egui::vec2(-delta.y, delta.x).normalized();
                        object.rotation[axis] =
                            (object.rotation[axis] + motion.dot(tangent) * 0.75).rem_euclid(360.);
                    }
                    TransformTool::Scale => {
                        object.scale[axis] += motion.dot(delta) * step / delta.length_sq();
                    }
                }
            }
        }
    }

    fn viewport_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let (delta, scroll) = ui.input(|input| (input.pointer.delta(), input.raw_scroll_delta.y));
        if response.dragged_by(egui::PointerButton::Secondary) {
            self.view.yaw = (self.view.yaw - delta.x * 0.35).rem_euclid(360.);
            self.view.pitch = (self.view.pitch + delta.y * 0.35).clamp(-85., 85.);
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let camera = self.view.camera();
            let forward = (camera.target - camera.position).normalize();
            let right = forward.cross(Vec3::Y).normalize();
            let up = right.cross(forward);
            let offset = (-right * delta.x + up * delta.y) * self.view.distance * 0.002;
            self.view.target = (Vec3::from_array(self.view.target) + offset).to_array();
        }
        if response.hovered() && scroll != 0. {
            self.view.distance = (self.view.distance * (-scroll * 0.006).exp()).clamp(0.25, 500.);
        }
        if response.clicked() || response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                self.selected = pick(&self.scene, &self.view, self.viewport, pointer);
            }
        }
    }

    fn object_menu(ui: &mut egui::Ui) -> Option<Command> {
        [Command::Frame, Command::Duplicate, Command::Delete]
            .into_iter()
            .find(|command| {
                if ui.add(command.menu_button(ui)).clicked() {
                    ui.close_menu();
                    true
                } else {
                    false
                }
            })
    }

    fn context_create(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Create object", |ui| {
            for (name, shape) in [
                ("Empty", Shape::Empty),
                ("Cube", Shape::Cube),
                ("Sphere", Shape::Sphere),
                ("Plane", Shape::Plane),
            ] {
                if ui.button(name).clicked() {
                    self.add(Object::new(name, shape));
                    ui.close_menu();
                }
            }
        });
    }

    fn frame_selection(&mut self) {
        if let Some(index) = self.selected {
            let object = &self.scene.objects[index];
            let (min, max) = bounds(&self.scene, object.shape);
            let matrix = self.scene.world_transforms()[index].0;
            self.view.target = matrix.transform_point3((min + max) * 0.5).to_array();
            let size = max - min;
            let width = matrix.transform_vector3(Vec3::X * size.x).length();
            let height = matrix.transform_vector3(Vec3::Y * size.y).length();
            let depth = matrix.transform_vector3(Vec3::Z * size.z).length();
            self.view.distance = vec3(width, height, depth)
                .length()
                .mul_add(1.55, 0.1)
                .clamp(0.5, 500.);
        } else {
            self.view = self.scene.camera.clone();
        }
    }

    async fn draw(&mut self) {
        clear_background(Color::from_rgba(20, 26, 39, 255));
        if self.viewport.width() <= 1. || self.viewport.height() <= 1. {
            return;
        }
        let mut script_errors = Vec::new();
        let mut events = Vec::new();
        let overlay_scale = self.pixels_per_point / macroquad::miniquad::window::dpi_scale();
        let overlay_rect = Rect::new(
            self.viewport.left() * overlay_scale,
            self.viewport.top() * overlay_scale,
            self.viewport.width() * overlay_scale,
            self.viewport.height() * overlay_scale,
        );
        let selected = if self.preview.is_none() {
            self.selected
        } else {
            None
        };
        let show_colliders = self.preview.is_none() && self.show_colliders;
        let (scene, view, grid) = if let (Some(preview), Some(overlay)) =
            (self.preview.as_mut(), self.overlay.as_mut())
        {
            if !self.paused || std::mem::take(&mut self.step_preview) {
                let action =
                    Overlay::action(&preview.scene().presentation, overlay_rect).map(str::to_owned);
                if let Some(button) = action.as_deref() {
                    let _ = preview.ui_action(button);
                }
                let movement = if action.is_none() {
                    preview.scene().bindings.movement()
                } else {
                    Vec2::ZERO
                };
                let before = preview.current().to_owned();
                let dt = if self.paused {
                    1. / 60.
                } else {
                    get_frame_time()
                };
                let input = controls::InputControls::capture(vec2(overlay_rect.x, overlay_rect.y));
                let switched = preview.tick_input(movement, &input, dt);
                let sounds = preview.take_sounds();
                overlay
                    .play(
                        &before,
                        preview.presentation(&before),
                        sounds,
                        preview.progress().volume,
                    )
                    .await;
                if switched {
                    self.prepared = PreparedScene::new(preview.scene());
                    overlay.set_scene(&preview.scene().presentation);
                    self.status = format!("Playing scene {}", preview.current());
                }
            }
            script_errors = preview.take_script_errors();
            events = preview.take_events().0;
            let scene = preview.scene();
            (scene, scene.camera.clone(), false)
        } else {
            (&self.scene, self.view.clone(), self.grid)
        };
        let mut camera = view.camera();
        let px = self.pixels_per_point;
        let (_, framebuffer_height) = macroquad::miniquad::window::screen_size();
        camera.aspect = Some(self.viewport.width() / self.viewport.height());
        camera.viewport = Some((
            (self.viewport.left() * px).round() as i32,
            (framebuffer_height - self.viewport.bottom() * px).round() as i32,
            (self.viewport.width() * px).round() as i32,
            (self.viewport.height() * px).round() as i32,
        ));
        self.prepared.draw(scene, &camera, grid, selected);
        if show_colliders {
            physics::draw_gizmos(scene, &camera);
        }
        for event in events {
            let name = |id| {
                scene
                    .object_index(id)
                    .ok()
                    .map_or("?", |index| scene.objects[index].name.as_str())
            };
            if self.physics_events.len() >= 100 {
                self.physics_events.remove(0);
            }
            self.physics_events.push(format!(
                "{} · {} / {}",
                event.kind.as_str(),
                name(event.a),
                name(event.b)
            ));
        }
        if let (Some(preview), Some(overlay)) = (&self.preview, &self.overlay) {
            overlay.draw(
                &preview.scene().presentation,
                overlay_rect,
                preview.progress(),
            );
        }
        for error in script_errors {
            self.record_script_error(error);
        }
    }

    fn add(&mut self, object: Object) {
        match self.scene.add_object(object) {
            Ok(_) => self.selected = Some(self.scene.objects.len() - 1),
            Err(error) => self.status = error,
        }
    }

    fn project_menu(&mut self, ui: &mut egui::Ui) {
        if self.project.is_none() && ui.button("Create from current scene…").clicked() {
            self.prompt(Action::ProjectNew);
            ui.close_menu();
        }
        if ui.button("Open Workshop Run…").clicked() {
            self.prompt(Action::CampaignDemo);
            ui.close_menu();
        }
        if ui.button("Open two-scene demo…").clicked() {
            self.prompt(Action::ProjectDemo);
            ui.close_menu();
        }
        let Some(open) = &self.project else {
            return;
        };
        let active = open.active.clone();
        let start = open.project.start().to_owned();
        let scenes: Vec<_> = open
            .project
            .scenes()
            .map(|(id, title)| (id.to_owned(), title.to_owned()))
            .collect();
        let prefabs: Vec<_> = open.project.prefabs().map(str::to_owned).collect();
        let models: Vec<_> = open
            .project
            .models()
            .iter()
            .map(|(&id, model)| (id, model.name.clone()))
            .collect();
        let scripts: Vec<_> = open
            .project
            .scripts()
            .iter()
            .map(|(&id, script)| (id, script.name.clone()))
            .collect();
        ui.separator();
        ui.label(format!("Starting scene: {start}"));
        if ui.button("Save project…").clicked() {
            self.prompt(Action::ProjectSave);
            ui.close_menu();
        }
        if ui.button("Add scene…").clicked() {
            self.prompt(Action::ProjectAdd);
            ui.close_menu();
        }
        ui.menu_button(format!("Edit scene ({active})"), |ui| {
            for (id, title) in scenes {
                if ui
                    .selectable_label(id == active, format!("{id} · {title}"))
                    .clicked()
                    && id != active
                {
                    let result = self.select_project_scene(&id);
                    self.status = result.map_or_else(|err| err, |()| format!("Editing scene {id}"));
                    ui.close_menu();
                }
            }
        });
        if ui
            .add_enabled(
                active != start,
                egui::Button::new("Use active scene at launch"),
            )
            .clicked()
        {
            let result = self
                .project
                .as_mut()
                .ok_or("Open a project first.")
                .and_then(|open| {
                    open.project
                        .set_start(&active)
                        .map_err(|_| "Unknown active scene.")
                });
            match result {
                Ok(()) => {
                    self.project_dirty = true;
                    self.status = format!("Starting scene set to {active}");
                }
                Err(error) => self.status = error.into(),
            }
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.selected.is_some(),
                egui::Button::new("Create prefab from selection…"),
            )
            .clicked()
        {
            self.prompt(Action::PrefabCreate);
            ui.close_menu();
        }
        ui.add_enabled_ui(!prefabs.is_empty(), |ui| {
            ui.menu_button("Insert prefab", |ui| {
                for id in prefabs {
                    if ui.button(&id).clicked() {
                        let result = self.insert_prefab(&id);
                        self.status =
                            result.map_or_else(|err| err, |()| format!("Inserted prefab {id}"));
                        ui.close_menu();
                    }
                }
            });
        });
        ui.add_enabled_ui(!(models.is_empty() && scripts.is_empty()), |ui| {
            ui.menu_button("Shared assets", |ui| {
                for (id, name) in models {
                    if ui.button(format!("Model {id} · {name}")).clicked() {
                        let result = self.use_shared_model(id, name);
                        self.status = result.unwrap_or_else(|err| err);
                        ui.close_menu();
                    }
                }
                for (id, name) in scripts {
                    if ui.button(format!("Script {id} · {name}")).clicked() {
                        let result = self
                            .project
                            .as_ref()
                            .ok_or("Open a project first.".into())
                            .and_then(|open| open.project.use_script(id, &mut self.scene));
                        self.status =
                            result.map_or_else(|err| err, |_| format!("Added script {name}"));
                        ui.close_menu();
                    }
                }
            });
        });
    }

    fn has_unsaved_changes(&self) -> bool {
        self.dirty || self.project_dirty
    }

    fn open_project(&mut self, project: Project, path: Option<PathBuf>) -> Result<(), String> {
        let open = OpenProject::open(project);
        let scene = open.project.scene(&open.active)?;
        self.project_dirty = path.is_none();
        self.project = Some(open);
        self.replace_scene(scene, path);
        Ok(())
    }

    fn select_project_scene(&mut self, id: &str) -> Result<(), String> {
        let open = self.project.as_mut().ok_or("Open a project first.")?;
        let scene = open.select(id, &self.scene)?;
        self.project_dirty |= self.dirty;
        self.replace_scene(scene, self.path.clone());
        Ok(())
    }

    fn insert_prefab(&mut self, id: &str) -> Result<(), String> {
        let open = self.project.as_ref().ok_or("Open a project first.")?;
        let root = open.project.instantiate(id, &mut self.scene, None)?;
        self.selected = Some(self.scene.object_index(root)?);
        self.prepared = PreparedScene::new(&self.scene);
        Ok(())
    }

    fn use_shared_model(&mut self, id: u64, name: String) -> Result<String, String> {
        let open = self.project.as_ref().ok_or("Open a project first.")?;
        let index = open.project.use_model(id, &mut self.scene)?;
        self.scene
            .add_object(Object::new(&name, Shape::Model(index)))?;
        self.selected = Some(self.scene.objects.len() - 1);
        self.prepared = PreparedScene::new(&self.scene);
        Ok(format!("Added shared model {name}"))
    }

    fn game(&self, play_active: bool) -> Result<Game, String> {
        match &self.project {
            Some(open) => open
                .snapshot(&self.scene)?
                .game(play_active.then_some(&open.active)),
            None => Ok(Game::single(self.scene.clone())),
        }
    }

    fn detach_project(&mut self) {
        self.project = None;
        self.project_dirty = false;
    }

    fn replace_scene(&mut self, scene: Scene, path: Option<PathBuf>) {
        self.prepared = PreparedScene::new(&scene);
        self.view = scene.camera.clone();
        self.saved = scene.clone();
        self.scene = scene;
        self.history = project::History::default();
        self.scene_epoch += 1;
        self.pointer_edit = false;
        self.collapsed.clear();
        self.selected = None;
        if let Some(directory) = path
            .as_deref()
            .and_then(|path| path.canonicalize().ok())
            .and_then(|path| path.parent().map(Path::to_owned))
        {
            self.browser.directory = directory;
            self.browser.refresh();
        }
        self.path = path;
        self.dirty = false;
    }

    fn new_scene(&mut self) {
        self.detach_project();
        self.replace_scene(Scene::empty(), None);
        self.status = "New scene".into();
    }

    fn prompt(&mut self, action: Action) {
        let path = match action {
            Action::New
            | Action::PhysicsDemo
            | Action::RenderingDemo
            | Action::Application
            | Action::ProjectDemo
            | Action::CampaignDemo
            | Action::Presentation
            | Action::ColorShader
            | Action::Dependencies
            | Action::AssetUsage
            | Action::Script
            | Action::ProjectAdd => Path::new(""),
            Action::Open | Action::Save => self
                .path
                .as_deref()
                .unwrap_or(Path::new("scene.forge.json")),
            Action::BatchImport | Action::Import | Action::ImportScript | Action::Reimport(_) => {
                Path::new("")
            }
            Action::Export => Path::new("game-export"),
            Action::Build => Path::new("game-build"),
            Action::RetryBuild => Path::new("game-build-retry"),
            Action::Capture => Path::new("capture.png"),
            Action::ProjectNew => Path::new("game.forge-project.json"),
            Action::ProjectSave => self
                .path
                .as_deref()
                .unwrap_or(Path::new("game.forge-project.json")),
            Action::PrefabCreate => Path::new("prefab"),
        };
        self.dialog = Some(Dialog {
            action,
            path: path.display().to_string(),
            scene_id: String::new(),
            dependencies: None,
            prefab_dependencies: false,
            shader_source: if matches!(action, Action::ColorShader) {
                self.scene.lighting.color_shader.clone()
            } else {
                String::new()
            },
            presentation: matches!(action, Action::Presentation)
                .then(|| UiDesigner::new(self.scene.presentation.clone())),
            script: None,
            usage: None,
            error: None,
            profile: jobs::Profile::Debug,
            target: None,
            application: self.scene.application.clone(),
        });
    }

    fn show_dialog(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some()
            && !ctx.memory(|memory| memory.any_popup_open())
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.dialog = None;
            return;
        }
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut submit = false;
        let mut open = true;
        let editing = matches!(
            dialog.action,
            Action::Presentation | Action::Script | Action::ColorShader
        );
        let window = egui::Window::new(dialog.action.label())
            .default_width(if editing {
                ctx.screen_rect().width().min(840.) - 32.
            } else {
                420.
            })
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .resizable(editing)
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(dialog.action.hint());
                if !matches!(
                    dialog.action,
                    Action::New
                        | Action::PhysicsDemo
                        | Action::RenderingDemo
                        | Action::Application
                        | Action::ProjectDemo
                        | Action::CampaignDemo
                        | Action::Presentation
                        | Action::ColorShader
                        | Action::Dependencies
                        | Action::AssetUsage
                        | Action::Script
                ) {
                    if matches!(dialog.action, Action::BatchImport) {
                        batch_import_fields(ui, &self.browser, &mut dialog.path);
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut dialog.path).desired_width(420.));
                    }
                }
                if matches!(dialog.action, Action::ColorShader) {
                    ui.add(
                        egui::TextEdit::multiline(&mut dialog.shader_source)
                            .code_editor()
                            .desired_width(ui.available_width())
                            .desired_rows(8),
                    );
                    if ui.button("Pulse example").clicked() {
                        dialog.shader_source = lighting::PULSE_SHADER.into();
                    }
                    ui.label("Enable Use scene color shader on the object's material. An empty shader uses the standard color.");
                }
                if let Some(project) = &dialog.dependencies {
                    dependency_list(ui, project, &mut dialog.prefab_dependencies, &mut dialog.scene_id);
                }
                if let Some((project, selected, rows)) = &mut dialog.usage {
                    let before = *selected;
                    egui::ComboBox::from_id_salt("model-usage")
                        .selected_text(match *selected {
                            SharedAsset::Model(id) => format!("Model · {} #{id}", project.models()[&id].name),
                            SharedAsset::Script(id) => format!("Script · {} #{id}", project.scripts()[&id].name),
                        })
                        .show_ui(ui, |ui| {
                            for (id, model) in project.models() {
                                ui.selectable_value(selected, SharedAsset::Model(*id), format!("Model · {} #{id}", model.name));
                            }
                            for (id, script) in project.scripts() {
                                ui.selectable_value(selected, SharedAsset::Script(*id), format!("Script · {} #{id}", script.name));
                            }
                        });
                    if *selected != before {
                        *rows = asset_usage_rows(project, *selected);
                    }
                    let action = Action::Reimport(*selected);
                    if self.project.is_some() && ui.button(format!("{}…", action.label())).clicked() {
                        self.prompt(action);
                    }
                    if rows.is_empty() {
                        ui.label("Not used in this project.");
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(300.)
                            .show(ui, |ui| {
                                for row in rows.iter() {
                                    ui.label(row);
                                }
                            });
                    }
                }
                if let Action::Reimport(id) = dialog.action {
                    ui.label(format!("Update {id} across all scenes and prefabs."));
                }
                if let Some(editor) = &mut dialog.presentation {
                    let aspect = if self.scene.application.orientation == Orientation::Portrait {
                        9. / 16.
                    } else {
                        self.viewport.aspect_ratio().clamp(0.3, 5.)
                    };
                    egui::ScrollArea::vertical()
                        .max_height(ctx.screen_rect().height() * 0.67)
                        .show(ui, |ui| editor.show(ui, aspect));
                }
                if let Some(editor) = &mut dialog.script {
                    editor.show(ui);
                }
                if matches!(dialog.action, Action::ProjectAdd) {
                    ui.label("Scene ID");
                    ui.text_edit_singleline(&mut dialog.scene_id);
                }
                if matches!(dialog.action, Action::Build) {
                    ui.horizontal_wrapped(|ui| {
                        for (name, profile) in [
                            ("Check", jobs::Profile::Check),
                            ("Test", jobs::Profile::Test),
                            ("Debug", jobs::Profile::Debug),
                            ("Release", jobs::Profile::Release),
                            ("APK debug", jobs::Profile::AndroidDebug),
                            ("APK release", jobs::Profile::AndroidRelease),
                            ("AAB release", jobs::Profile::AndroidBundle),
                        ] {
                            ui.selectable_value(&mut dialog.profile, profile, name);
                        }
                    });
                }
                if matches!(dialog.action, Action::Build) {
                    if matches!(
                        dialog.profile,
                        jobs::Profile::AndroidDebug
                            | jobs::Profile::AndroidRelease
                            | jobs::Profile::AndroidBundle
                    ) {
                        dialog.target = None;
                    } else {
                        egui::ComboBox::from_id_salt("build-target")
                            .selected_text(dialog.target.map_or("Host", jobs::Target::triple))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut dialog.target, None, "Host");
                                for (name, target) in [
                                    ("Windows x64 (MSVC)", jobs::Target::WindowsX64),
                                    ("macOS Apple Silicon", jobs::Target::MacosArm64),
                                    ("Linux x64", jobs::Target::LinuxX64),
                                ] {
                                    ui.selectable_value(&mut dialog.target, Some(target), name);
                                }
                            });
                    }
                }
                if matches!(dialog.action, Action::Application) {
                    application_inspector(ui, &mut dialog.application);
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                let discarding = self.has_unsaved_changes()
                    && matches!(
                        dialog.action,
                        Action::New
                            | Action::PhysicsDemo
                            | Action::RenderingDemo
                            | Action::Open
                            | Action::ProjectDemo
                            | Action::CampaignDemo
                    );
                if discarding {
                    ui.colored_label(Color32::YELLOW, "This will discard unsaved changes.");
                }
                submit = ui
                    .button(if discarding {
                        "Discard changes and continue"
                    } else if matches!(dialog.action, Action::AssetUsage | Action::Dependencies) {
                        "Close"
                    } else if matches!(
                        dialog.action,
                        Action::Application | Action::Presentation | Action::Script | Action::ColorShader
                    ) {
                        "Apply"
                    } else {
                        dialog.action.label()
                    })
                    .clicked();
            });
        if let Some(window) = window {
            ctx.memory_mut(|memory| memory.set_modal_layer(window.response.layer_id));
        }
        if self.dialog.is_some() {
            return;
        }
        if submit {
            match self.perform(&dialog) {
                Ok(message) => {
                    self.status = message;
                    return;
                }
                Err(error) => dialog.error = Some(error),
            }
        }
        self.dialog = open.then_some(dialog);
    }

    fn perform(&mut self, dialog: &Dialog) -> Result<String, String> {
        let action = dialog.action;
        let path = Path::new(dialog.path.trim());
        if !matches!(
            action,
            Action::New
                | Action::PhysicsDemo
                | Action::RenderingDemo
                | Action::Application
                | Action::ProjectDemo
                | Action::CampaignDemo
                | Action::Presentation
                | Action::ColorShader
                | Action::Dependencies
                | Action::AssetUsage
                | Action::Script
                | Action::ProjectAdd
        ) && path.as_os_str().is_empty()
        {
            return Err("Enter a path.".into());
        }
        match action {
            Action::ColorShader => {
                self.prepared.apply_color_shader(&dialog.shader_source)?;
                self.scene
                    .lighting
                    .color_shader
                    .clone_from(&dialog.shader_source);
                Ok("Scene color shader applied".into())
            }
            Action::Dependencies => Ok("Dependencies closed".into()),
            Action::BatchImport => {
                let paths: Vec<_> = dialog
                    .path
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(|p| self.browser.directory.join(p))
                    .collect();
                let open = self.project.as_mut().ok_or("Open a project first.")?;
                let mut candidate = open.snapshot(&self.scene)?;
                let imported = candidate.import_assets(&paths)?;
                open.project = candidate;
                self.project_dirty = true;
                Ok(format!(
                    "Imported {} files into Shared assets · save the project to keep them",
                    imported.len()
                ))
            }
            Action::Reimport(id) => {
                let open = self.project.as_mut().ok_or("Open a project first.")?;
                let mut candidate = open.snapshot(&self.scene)?;
                candidate.reimport_asset(id, path)?;
                let scene = candidate.scene(&open.active)?;
                open.project = candidate;
                self.prepared = PreparedScene::new(&scene);
                self.scene = scene;
                self.project_dirty = true;
                // Scene undo cannot reverse a change to the whole project library.
                self.history = project::History::default();
                self.scene_epoch += 1;
                self.pointer_edit = false;
                Ok(format!(
                    "Reimported {id} · save the project to keep the replacement"
                ))
            }
            Action::AssetUsage => Ok("Asset usage closed".into()),
            Action::New => {
                self.new_scene();
                Ok("New scene".into())
            }
            Action::PhysicsDemo => {
                self.detach_project();
                self.replace_scene(Scene::physics_demo(), None);
                self.selected = Some(1);
                Ok("Physics demo loaded".into())
            }
            Action::RenderingDemo => {
                self.detach_project();
                self.replace_scene(Scene::rendering_demo(), None);
                self.selected = Some(1);
                Ok("Light & Motion demo loaded".into())
            }
            Action::Open => {
                if path.to_string_lossy().ends_with(EXTENSION) {
                    self.open_project(Project::load(path)?, Some(path.to_owned()))?;
                } else {
                    let scene = project::load(path)?;
                    self.detach_project();
                    self.replace_scene(scene, Some(path.to_owned()));
                }
                Ok(format!("Opened {}", path.display()))
            }
            Action::Save => {
                project::save(&self.scene, path, self.path.as_deref() == Some(path))?;
                self.path = Some(path.to_owned());
                self.saved = self.scene.clone();
                Ok(format!("Saved {}", path.display()))
            }
            Action::ProjectNew | Action::ProjectSave => {
                if !path.to_string_lossy().ends_with(EXTENSION) {
                    return Err(format!("Project filenames must end in {EXTENSION}."));
                }
                if matches!(action, Action::ProjectNew) {
                    let project = Project::new("main", &self.scene)?;
                    project.save(path, false)?;
                    self.open_project(project, Some(path.to_owned()))?;
                } else {
                    let open = self.project.as_mut().ok_or("Open a project first.")?;
                    let project = open.snapshot(&self.scene)?;
                    project.save(path, self.path.as_deref() == Some(path))?;
                    open.project = project;
                    self.path = Some(path.to_owned());
                    self.saved = self.scene.clone();
                    self.project_dirty = false;
                }
                self.browser.refresh();
                Ok(format!("Saved project {}", path.display()))
            }
            Action::CampaignDemo => {
                self.open_project(Project::campaign_demo()?, None)?;
                Ok("Workshop Run opened".into())
            }
            Action::Presentation => {
                let editor = dialog
                    .presentation
                    .as_ref()
                    .ok_or("UI editor is unavailable.")?;
                editor.draft.validate()?;
                self.scene.presentation = editor.draft.clone();
                Ok("Game UI and audio updated".into())
            }
            Action::Script => {
                let editor = dialog
                    .script
                    .as_ref()
                    .ok_or("Script editor is unavailable.")?;
                scripting::validate_source(&editor.source)?;
                match editor.target {
                    ScriptTarget::Inline(id) => {
                        let index = self.scene.object_index(id)?;
                        let object = &mut self.scene.objects[index];
                        if object.script_asset.is_some() {
                            return Err("Object now uses a shared script.".into());
                        }
                        object.script = Some(editor.source.clone());
                    }
                    ScriptTarget::Shared(index) => {
                        self.scene
                            .scripts
                            .get_mut(index)
                            .ok_or("Script asset was removed.")?
                            .source = editor.source.clone();
                    }
                    ScriptTarget::NewShared => {
                        let name = editor.name.trim();
                        if name.is_empty() {
                            return Err("Enter a name for the shared script.".into());
                        }
                        if self.scene.scripts.iter().any(|script| script.name == name) {
                            return Err("A script with this name already exists.".into());
                        }
                        self.scene.scripts.push(ScriptAsset {
                            name: name.to_owned(),
                            source: editor.source.clone(),
                        });
                    }
                }
                Ok("Script compiled and applied".into())
            }
            Action::ProjectDemo => {
                self.open_project(Project::demo()?, None)?;
                Ok("Two Workshops project opened".into())
            }
            Action::ProjectAdd => {
                let id = dialog.scene_id.trim();
                let scene = if path.as_os_str().is_empty() {
                    let mut scene = Scene::empty();
                    scene.title = id.into();
                    scene.application = self.scene.application.clone();
                    scene
                } else {
                    project::load(path)?
                };
                let open = self.project.as_mut().ok_or("Open a project first.")?;
                let mut candidate = open.snapshot(&self.scene)?;
                candidate.add_scene(id, &scene)?;
                open.project = candidate;
                open.active = id.into();
                self.project_dirty = true;
                self.replace_scene(scene, self.path.clone());
                Ok(format!("Added scene {id}"))
            }
            Action::PrefabCreate => {
                let id = dialog.path.trim();
                let index = self.selected.ok_or("Select an object first.")?;
                let open = self.project.as_mut().ok_or("Open a project first.")?;
                open.project
                    .create_prefab(id, &self.scene, self.scene.objects[index].id)?;
                self.project_dirty = true;
                Ok(format!("Created prefab {id}"))
            }
            Action::Import => {
                let model = import::model(path)?;
                let name = model.name.clone();
                let index = self.scene.models.len();
                self.scene.models.push(model);
                self.prepared = PreparedScene::new(&self.scene);
                self.add(Object::new(&name, Shape::Model(index)));
                self.frame_selection();
                Ok(format!("Imported {name}"))
            }
            Action::ImportScript => {
                let script = import::script(path)?;
                let name = script.name.clone();
                self.scene.scripts.push(script);
                Ok(format!("Imported shared script {name}"))
            }
            Action::Build => {
                if self.build.is_some() {
                    return Err("A build is already running.".into());
                }
                self.build = Some(jobs::Job::start_game(
                    self.game(false)?,
                    path,
                    dialog.profile,
                    dialog.target,
                )?);
                self.build_report = None;
                Ok(format!("Building {}", path.display()))
            }
            Action::RetryBuild => {
                if self.build.is_some() {
                    return Err("A build is already running.".into());
                }
                let previous = self.last_build.as_mut().ok_or("No previous build.")?;
                self.build = Some(previous.retry(path)?);
                self.build_report = None;
                Ok(format!("Retrying {}", path.display()))
            }
            Action::Application => {
                dialog.application.validate()?;
                self.scene.application = dialog.application.clone();
                Ok("Application settings applied".into())
            }
            Action::Capture => {
                if path.extension().is_none_or(|extension| extension != "png") {
                    return Err("Choose a .png filename.".into());
                }
                let scene = self
                    .preview
                    .as_ref()
                    .map_or(&self.scene, GamePlayback::scene);
                project::write_new(path, &capture::png(scene, 960, 540)?)?;
                Ok(format!("Captured {}", path.display()))
            }
            Action::Export => {
                export::collection(&self.game(false)?, path)?;
                Ok(format!("Rust project exported to {}", path.display()))
            }
        }
    }
}

fn application_inspector(ui: &mut egui::Ui, app: &mut Application) {
    egui::Grid::new("application-settings")
        .num_columns(2)
        .spacing([12., 8.])
        .show(ui, |ui| {
            ui.label("App title");
            ui.add(egui::TextEdit::singleline(&mut app.title).hint_text("Use scene title"));
            ui.end_row();
            ui.label("Application ID");
            ui.text_edit_singleline(&mut app.identifier);
            ui.end_row();
            ui.label("Game version");
            ui.text_edit_singleline(&mut app.version);
            ui.end_row();
            ui.label("Android version code");
            ui.add(egui::DragValue::new(&mut app.version_code).range(1..=2_100_000_000));
            ui.end_row();
            ui.label("Desktop window");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut app.width).range(320..=7680));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut app.height).range(240..=4320));
            });
            ui.end_row();
            ui.label("Android orientation");
            egui::ComboBox::from_id_salt("app-orientation")
                .selected_text(format!("{:?}", app.orientation))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.orientation, Orientation::Landscape, "Landscape");
                    ui.selectable_value(&mut app.orientation, Orientation::Portrait, "Portrait");
                    ui.selectable_value(&mut app.orientation, Orientation::Sensor, "Sensor");
                });
            ui.end_row();
        });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut app.resizable, "Resizable desktop window");
        ui.checkbox(&mut app.fullscreen, "Desktop fullscreen");
        ui.checkbox(&mut app.android_fullscreen, "Android fullscreen");
    });
}

fn rendering_inspector(ui: &mut egui::Ui, object: &mut Object, rig: Option<&animation::Rig>) {
    if let Some(rig) = rig.filter(|rig| !rig.clips.is_empty()) {
        egui::CollapsingHeader::new("Animation")
            .default_open(true)
            .show(ui, |ui| {
                let mut selected = object.animation.as_ref().map(|player| player.clip.clone());
                egui::ComboBox::from_id_salt("animation-clip")
                    .selected_text(selected.as_deref().unwrap_or("Bind pose"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, None, "Bind pose");
                        for clip in &rig.clips {
                            ui.selectable_value(&mut selected, Some(clip.name.clone()), &clip.name);
                        }
                    });
                if selected.as_deref()
                    != object.animation.as_ref().map(|player| player.clip.as_str())
                {
                    object.animation = selected.map(|clip| animation::Player {
                        clip,
                        ..animation::Player::default()
                    });
                }
                if let Some(player) = &mut object.animation {
                    let duration = rig
                        .clips
                        .iter()
                        .find(|clip| clip.name == player.clip)
                        .map_or(0., |clip| clip.duration);
                    number(ui, "Start time (s)", &mut player.time, 0. ..=duration);
                    number(ui, "Speed", &mut player.speed, -10. ..=10.);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut player.playing, "Play");
                        ui.checkbox(&mut player.looping, "Loop");
                    });
                }
            });
    }
    egui::CollapsingHeader::new("Material and light")
        .default_open(object.light.is_some())
        .show(ui, |ui| {
            ui.checkbox(&mut object.cast_shadows, "Cast shadows");
            let mut material = object.surface.is_some();
            if ui.checkbox(&mut material, "Override material").changed() {
                object.surface = material.then(lighting::Surface::default);
            }
            if let Some(surface) = &mut object.surface {
                let mut rgb = [
                    surface.base_color[0],
                    surface.base_color[1],
                    surface.base_color[2],
                ];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    surface.base_color[..3].copy_from_slice(&rgb);
                }
                number(ui, "Metallic", &mut surface.metallic, 0. ..=1.);
                number(ui, "Roughness", &mut surface.roughness, 0. ..=1.);
                vector(ui, "Emissive", &mut surface.emissive, 0.025);
                surface.emissive = surface.emissive.map(|value| value.clamp(0., 50.));
                ui.checkbox(&mut surface.unlit, "Unlit");
                ui.checkbox(&mut surface.use_color_shader, "Use scene color shader");
            }
            let mut point = object.light.is_some();
            if ui.checkbox(&mut point, "Point light").changed() {
                object.light = point.then(lighting::PointLight::default);
            }
            if let Some(light) = &mut object.light {
                ui.color_edit_button_rgb(&mut light.color);
                number(ui, "Power", &mut light.intensity, 0. ..=200.);
                number(ui, "Range", &mut light.range, 0.01..=200.);
            }
        });
}

fn physics_inspector(ui: &mut egui::Ui, object: &mut Object) {
    use physics::{Body, BodyKind, ColliderKind};
    egui::CollapsingHeader::new("Physics")
        .default_open(object.body.is_some())
        .show(ui, |ui| {
            let mut enabled = object.body.is_some();
            if ui
                .checkbox(&mut enabled, "Collider and rigid body")
                .changed()
            {
                object.body = enabled.then(|| {
                    let kind = if object.parent.is_none() {
                        BodyKind::Dynamic
                    } else {
                        BodyKind::Static
                    };
                    let collider = if object.shape == Shape::Sphere {
                        ColliderKind::Sphere
                    } else {
                        ColliderKind::Box
                    };
                    Body {
                        kind,
                        collider,
                        ..Body::default()
                    }
                });
            }
            let Some(body) = &mut object.body else { return };
            egui::ComboBox::from_id_salt("body-kind")
                .selected_text(format!("{:?}", body.kind))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut body.kind, BodyKind::Static, "Static");
                    ui.add_enabled_ui(object.parent.is_none(), |ui| {
                        ui.selectable_value(&mut body.kind, BodyKind::Dynamic, "Dynamic");
                        if ui
                            .selectable_value(
                                &mut body.kind,
                                BodyKind::Character,
                                "Character controller",
                            )
                            .changed()
                        {
                            *body = Body::character();
                            if object.speed == 0. {
                                object.speed = 3.;
                            }
                        }
                    });
                    ui.selectable_value(&mut body.kind, BodyKind::Kinematic, "Kinematic");
                });
            if body.kind == BodyKind::Character {
                number(ui, "Capsule diameter", &mut body.size[0], 0.001..=10_000.);
                number(ui, "Capsule height", &mut body.size[1], 0.001..=10_000.);
                number(ui, "Jump height", &mut body.jump_height, 0. ..=100.);
                number(
                    ui,
                    "Max slope (degrees)",
                    &mut body.max_slope_degrees,
                    0. ..=89.,
                );
                number(ui, "Gravity scale", &mut body.gravity_scale, 0. ..=10.);
                return;
            }
            egui::ComboBox::from_id_salt("collider-kind")
                .selected_text(format!("{:?}", body.collider))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut body.collider, ColliderKind::Box, "Box");
                    ui.selectable_value(&mut body.collider, ColliderKind::Sphere, "Sphere");
                    ui.selectable_value(&mut body.collider, ColliderKind::Capsule, "Capsule");
                });
            ui.checkbox(&mut body.sensor, "Trigger (sensor)");
            if body.collider == ColliderKind::Sphere {
                number(ui, "Diameter", &mut body.size[0], 0.001..=10_000.);
            } else {
                ui.label("Collider dimensions");
                ui.horizontal_wrapped(|ui| {
                    for (axis, size) in ["X ", "Y ", "Z "].into_iter().zip(&mut body.size) {
                        ui.add(
                            egui::DragValue::new(size)
                                .prefix(axis)
                                .range(0.001..=10_000.)
                                .speed(0.025),
                        );
                    }
                });
            }
            number(ui, "Friction", &mut body.friction, 0. ..=2.);
            number(ui, "Restitution", &mut body.restitution, 0. ..=1.);
            if body.kind == BodyKind::Dynamic {
                number(ui, "Gravity scale", &mut body.gravity_scale, -10. ..=10.);
                ui.checkbox(&mut body.lock_rotation, "Lock rotation");
            }
        });
}

fn compact_frame() -> egui::Frame {
    panel_frame().inner_margin(egui::Margin::symmetric(10, 5))
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0_f32, LINE))
        .inner_margin(8.)
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(16.).strong());
}

fn vector(ui: &mut egui::Ui, label: &str, components: &mut [f32; 3], speed: f32) -> bool {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        components
            .iter_mut()
            .zip(["X ", "Y ", "Z "])
            .fold(false, |changed, (value, axis)| {
                ui.add(egui::DragValue::new(value).prefix(axis).speed(speed))
                    .changed()
                    || changed
            })
    })
    .inner
}

fn number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.1).range(range))
            .changed()
    })
    .inner
}

fn bounds(scene: &Scene, shape: Shape) -> (Vec3, Vec3) {
    match shape {
        Shape::Model(index) => scene.models[index]
            .parts
            .iter()
            .flat_map(|part| &part.vertices)
            .map(|vertex| Vec3::from_array(vertex.position))
            .fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(min, max), p| (min.min(p), max.max(p)),
            ),
        Shape::Plane => (vec3(-0.5, -0.005, -0.5), vec3(0.5, 0.005, 0.5)),
        _ => (Vec3::splat(-0.5), Vec3::splat(0.5)),
    }
}

fn pick(scene: &Scene, view: &View, rect: egui::Rect, pointer: egui::Pos2) -> Option<usize> {
    let mut camera = view.camera();
    camera.aspect = Some(rect.width() / rect.height());
    let projection = camera.matrix().inverse();
    let ndc = vec3(
        2. * (pointer.x - rect.left()) / rect.width() - 1.,
        1. - 2. * (pointer.y - rect.top()) / rect.height(),
        1.,
    );
    let ray = (projection.project_point3(ndc) - camera.position).normalize_or_zero();
    scene
        .world_transforms()
        .into_iter()
        .enumerate()
        .filter_map(|(index, (matrix, visible))| {
            let object = &scene.objects[index];
            if !visible || object.shape == Shape::Empty || matrix.determinant().abs() < 1e-8 {
                return None;
            }
            let inverse = matrix.inverse();
            let (min, max) = bounds(scene, object.shape);
            let origin = inverse.transform_point3(camera.position);
            let direction = inverse.transform_vector3(ray);
            let (mut near, mut far) = (0_f32, f32::INFINITY);
            for axis in 0..3 {
                if direction[axis].abs() < 1e-8 {
                    if origin[axis] < min[axis] || origin[axis] > max[axis] {
                        return None;
                    }
                    continue;
                }
                let first = (min[axis] - origin[axis]) / direction[axis];
                let second = (max[axis] - origin[axis]) / direction[axis];
                near = near.max(first.min(second));
                far = far.min(first.max(second));
            }
            (near <= far).then_some((index, near))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

fn screen_point(projection: Mat4, point: Vec3, rect: egui::Rect) -> Option<egui::Pos2> {
    let clip = projection * point.extend(1.);
    if clip.w <= 0. {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(egui::pos2(
        rect.left() + (ndc.x + 1.) * rect.width() * 0.5,
        rect.top() + (1. - ndc.y) * rect.height() * 0.5,
    ))
}
