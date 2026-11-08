//! Free-space optics editor with Blender-style controls.
//!
//! - [`model`] — layout data and file format
//! - [`view`] — pan/zoom transform
//! - [`editor`] — selection, grab, undo (no UI)
//! - [`input`] — keymap
//! - [`render`] — drawing
//! - [`examples`] — ready-made layouts

pub mod editor;
pub mod examples;
pub mod input;
pub mod model;
pub mod render;
pub mod view;

use egui::{Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};

pub use model::{GRID, Layout, Line, OpticComponentKind, PlacedComponent};

use super::beam_sim::{self, LayoutSimulation};
use super::chips::ChipBrowser;
use super::ui_kit;
use editor::{Axis, Editor};
use input::{Action, SHORTCUTS, shortcut};
use view::View;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Place(OpticComponentKind),
    DrawLine,
}

/// Last simulation and the inputs it was computed from.
pub struct SimCache {
    pub components: Vec<PlacedComponent>,
    pub cell_um: f64,
    pub result: LayoutSimulation,
}

/// Context menu (right click) or add menu (Shift+A).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Menu {
    pub screen: Pos2,
    pub world: Pos2,
    pub target: Option<usize>,
    opened_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuChoice {
    Rotate(i32),
    Duplicate,
    Delete,
    Add(OpticComponentKind),
    DeleteLines,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Box { start: Pos2, extend: bool },
    Line { start: Pos2 },
}

pub use crate::panels::Request;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExamplesTab {
    Layouts,
    Chips,
}

pub struct OpticsPanel {
    pub layout: Layout,
    pub editor: Editor,
    pub view: View,
    pub tool: Option<Tool>,
    /// Component being dragged in from the toolbar.
    pub dragging_new: Option<OpticComponentKind>,
    pub simulation_active: bool,
    /// Physical size of one grid cell in micrometres.
    pub cell_um: f64,
    pub sim_cache: Option<SimCache>,
    pub show_toolbar: bool,
    pub show_properties: bool,
    pub show_shortcuts: bool,
    pub show_examples: bool,
    pub examples_tab: ExamplesTab,
    pub menu: Option<Menu>,
    /// Transient message and the time it expires.
    pub status: Option<(String, f64)>,
    pub chips: ChipBrowser,
    /// Hosted inside the workspace: file actions become [`Request`]s.
    pub embedded: bool,
    pub requests: Vec<Request>,
    drag: Drag,
    canvas: Option<Rect>,
    mouse_screen: Option<Pos2>,
    mouse_world: Option<Pos2>,
    frame_pending: bool,
    time: f64,
}

impl Default for OpticsPanel {
    fn default() -> Self {
        let layout = Layout::default();
        Self {
            editor: Editor::new(&layout),
            layout,
            view: View::default(),
            tool: None,
            dragging_new: None,
            simulation_active: false,
            cell_um: 10.0,
            sim_cache: None,
            show_toolbar: true,
            show_properties: true,
            show_shortcuts: false,
            show_examples: false,
            examples_tab: ExamplesTab::Layouts,
            menu: None,
            status: None,
            chips: ChipBrowser::default(),
            embedded: false,
            requests: Vec::new(),
            drag: Drag::None,
            canvas: None,
            mouse_screen: None,
            mouse_world: None,
            frame_pending: false,
            time: 0.0,
        }
    }
}

impl OpticsPanel {
    /// An embedded editor for an existing layout.
    pub fn with_layout(layout: Layout, cell_um: f64) -> Self {
        Self {
            editor: Editor::new(&layout),
            layout,
            cell_um,
            embedded: true,
            frame_pending: true,
            simulation_active: true,
            ..Self::default()
        }
    }

    /// Canvas area from the last frame.
    pub fn canvas_rect(&self) -> Option<Rect> {
        self.canvas
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        let message = message.into();
        tracing::info!("{message}");
        self.status = Some((message, self.time + 3.0));
    }

    // ---- simulation -------------------------------------------------------

    /// Re-solve only when the layout or the physical scale changed.
    pub fn refresh_simulation(&mut self) {
        let stale = self
            .sim_cache
            .as_ref()
            .is_none_or(|c| c.components != self.layout.components || c.cell_um != self.cell_um);
        if !stale {
            return;
        }
        let start = web_time::Instant::now();
        let result =
            beam_sim::simulate_layout(&self.layout.components, GRID, self.cell_um, GRID * 500.0);
        match &result.error {
            Some(e) => tracing::warn!("Simulation failed: {e}"),
            None => tracing::debug!(
                "Simulated {} components, {} beam segments in {:?}",
                self.layout.components.len(),
                result.beams.len(),
                start.elapsed()
            ),
        }
        self.sim_cache = Some(SimCache {
            components: self.layout.components.clone(),
            cell_um: self.cell_um,
            result,
        });
    }

    pub fn detector_reading(&self, index: usize) -> Option<f64> {
        let cache = self.sim_cache.as_ref()?;
        cache
            .result
            .detector_power
            .iter()
            .find(|d| d.0 == index)
            .map(|d| d.1)
    }

    // ---- documents --------------------------------------------------------

    pub fn new_layout(&mut self) {
        self.editor.cancel_grab(&mut self.layout);
        self.layout = Layout::default();
        self.editor.selection.clear();
        self.view = View::default();
        self.notify("New layout");
    }

    /// Save as a Shine project holding just this layout.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let mut project = crate::panels::project::Project::new();
        project.add_layout("Layout", self.layout.clone(), self.cell_um);
        std::fs::write(path, project.to_bytes()).map_err(|e| e.to_string())
    }

    /// Load the first layout of a Shine project, or a JSON layout from an earlier version.
    pub fn load_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let (layout, cell_um) = if crate::panels::project::Project::is_project(&bytes) {
            let project = crate::panels::project::Project::from_bytes(&bytes)?;
            project
                .documents
                .iter()
                .find_map(|d| match &d.content {
                    crate::panels::project::Content::Layout(p) => {
                        Some((p.layout.clone(), Some(p.cell_um)))
                    }
                    _ => None,
                })
                .ok_or("the project has no free-space layout")?
        } else {
            model::from_legacy_json(&String::from_utf8_lossy(&bytes))?
        };
        self.editor.cancel_grab(&mut self.layout);
        self.layout = layout;
        if let Some(cell_um) = cell_um {
            self.cell_um = cell_um;
        }
        self.editor.selection.clear();
        self.frame_pending = true;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Shine project", &[super::file_format::EXTENSION])
            .save_file()
        else {
            return;
        };
        match self.save_to(&path) {
            Ok(()) => self.notify(format!("Saved {}", path.display())),
            Err(e) => {
                tracing::error!("Failed to save {}: {e}", path.display());
                self.notify(format!("Save failed: {e}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Shine project or JSON layout",
                &[super::file_format::EXTENSION, "json"],
            )
            .pick_file()
        else {
            return;
        };
        match self.load_from(&path) {
            Ok(()) => self.notify(format!("Opened {}", path.display())),
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", path.display());
                self.notify(format!("Open failed: {e}"));
            }
        }
    }

    /// The standalone editor has no file dialogs in the browser; the
    /// workspace handles files there.
    #[cfg(target_arch = "wasm32")]
    fn save_dialog(&mut self) {
        self.notify("Use File → Save in the workspace");
    }

    #[cfg(target_arch = "wasm32")]
    fn open_dialog(&mut self) {
        self.notify("Use File → Open in the workspace");
    }

    pub fn load_example(&mut self, index: usize) {
        let Some(example) = examples::layout_examples().into_iter().nth(index) else {
            return;
        };
        self.editor.cancel_grab(&mut self.layout);
        self.layout = (example.build)();
        self.cell_um = example.cell_um;
        self.editor.selection.clear();
        self.simulation_active = true;
        self.frame_pending = true;
        self.notify(format!("Loaded example: {}", example.name));
    }

    // ---- actions ----------------------------------------------------------

    fn frame(&mut self, selected_only: bool) {
        let bounds = if selected_only {
            self.layout.bounds(self.editor.selection.iter())
        } else {
            self.layout.all_bounds()
        };
        match (self.canvas, bounds) {
            (Some(canvas), Some(bounds)) => self.view.frame(canvas, bounds, 40.0),
            (_, None) if !selected_only => self.view = View::default(),
            (None, Some(_)) => self.frame_pending = true,
            (_, None) => {}
        }
    }

    fn confirm_grab(&mut self) {
        if let Err(e) = self.editor.confirm_grab(&mut self.layout) {
            self.notify(format!("Move cancelled: {e}"));
        }
    }

    pub fn apply(&mut self, action: Action) {
        let mouse = self
            .mouse_world
            .or_else(|| self.canvas.map(|c| self.view.to_world(c, c.center())))
            .unwrap_or(Pos2::ZERO);
        let count = self.layout.components.len();
        match action {
            Action::SelectAll => self.editor.selection.select_all(count),
            Action::DeselectAll => self.editor.selection.clear(),
            Action::InvertSelection => self.editor.selection.invert(count),
            Action::Delete => {
                let n = self.editor.delete_selected(&mut self.layout);
                if n > 0 {
                    self.notify(format!("Deleted {n} component(s)"));
                }
            }
            Action::Duplicate => {
                if !self.editor.duplicate_selected(&mut self.layout, mouse) {
                    self.notify("Nothing selected to duplicate");
                }
            }
            Action::Grab => {
                if !self.editor.begin_grab(&self.layout, mouse, false) {
                    self.notify("Nothing selected to move");
                }
            }
            Action::RotateCw => self.editor.rotate_selected(&mut self.layout, 1),
            Action::RotateCcw => self.editor.rotate_selected(&mut self.layout, -1),
            Action::Undo => {
                if !self.editor.undo(&mut self.layout) {
                    self.notify("Nothing to undo");
                }
            }
            Action::Redo => {
                if !self.editor.redo(&mut self.layout) {
                    self.notify("Nothing to redo");
                }
            }
            Action::FrameAll => self.frame(false),
            Action::FrameSelected => self.frame(true),
            Action::ToggleSimulation => self.simulation_active = !self.simulation_active,
            Action::ToggleToolbar if self.embedded => self.requests.push(Request::ToggleLibrary),
            Action::ToggleProperties if self.embedded => {
                self.requests.push(Request::ToggleInspector)
            }
            Action::ShowShortcuts if self.embedded => self.requests.push(Request::ShowShortcuts),
            Action::ToggleToolbar => self.show_toolbar = !self.show_toolbar,
            Action::ToggleProperties => self.show_properties = !self.show_properties,
            Action::ShowShortcuts => self.show_shortcuts = !self.show_shortcuts,
            // Layout parts have nothing to open.
            Action::Inspect => {}
            Action::AddMenu => self.open_menu(None),
            Action::Save if self.embedded => self.requests.push(Request::Save),
            Action::Open if self.embedded => self.requests.push(Request::Open),
            Action::New if self.embedded => self.requests.push(Request::New),
            Action::Save => self.save_dialog(),
            Action::Open => self.open_dialog(),
            Action::New => self.new_layout(),
            Action::ConstrainX => self.editor.constrain(Axis::X),
            Action::ConstrainY => self.editor.constrain(Axis::Y),
            Action::Confirm => self.confirm_grab(),
            Action::Cancel => {
                if self.editor.grab.is_some() {
                    self.editor.cancel_grab(&mut self.layout);
                } else {
                    self.menu = None;
                    self.tool = None;
                    self.drag = Drag::None;
                    self.dragging_new = None;
                }
            }
        }
    }

    fn open_menu(&mut self, target: Option<usize>) {
        let (Some(screen), Some(world)) = (self.mouse_screen, self.mouse_world) else {
            return;
        };
        self.menu = Some(Menu {
            screen,
            world,
            target,
            opened_at: self.time,
        });
    }

    pub fn apply_menu_choice(&mut self, choice: MenuChoice, world: Pos2) {
        match choice {
            MenuChoice::Rotate(steps) => self.editor.rotate_selected(&mut self.layout, steps),
            MenuChoice::Duplicate => self.apply(Action::Duplicate),
            MenuChoice::Delete => self.apply(Action::Delete),
            MenuChoice::Add(kind) => {
                if self.editor.place(&mut self.layout, kind, world).is_none() {
                    self.notify("That cell is occupied");
                }
            }
            MenuChoice::DeleteLines => {
                let cell = model::cell_of(world);
                self.layout
                    .lines
                    .retain(|l| model::cell_of(l.start) != cell && model::cell_of(l.end) != cell);
            }
        }
        self.menu = None;
    }

    // ---- UI ---------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.begin_frame(ui);
        if self.show_toolbar {
            egui::Panel::left("optics_toolbar")
                .resizable(false)
                .exact_size(200.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.palette_ui(ui));
                });
        }
        if self.show_properties {
            egui::Panel::right("optics_properties")
                .default_size(250.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.inspector_ui(ui));
                });
        }
        egui::Panel::bottom("optics_status").show(ui, |ui| {
            ui.label(egui::RichText::new(self.status_text()).small());
        });
        self.canvas_ui(ui);

        let ctx = ui.ctx().clone();
        self.overlays(&ctx);
        self.windows(&ctx);
        self.end_frame(ui);
    }

    /// Per-frame bookkeeping before any part of the editor is drawn.
    pub fn begin_frame(&mut self, ui: &Ui) {
        self.time = ui.input(|i| i.time);
        if self.status.as_ref().is_some_and(|s| s.1 < self.time) {
            self.status = None;
        }
        self.handle_keys(ui);
        // Before any panel reads results, so they all see the current layout.
        if self.simulation_active {
            self.refresh_simulation();
        }
    }

    /// Records one undo step per settled interaction.
    pub fn end_frame(&mut self, ui: &Ui) {
        if self.editor.grab.is_none() && !ui.input(|i| i.pointer.any_down()) {
            self.editor.history.commit(&self.layout);
        }
    }

    /// Floating UI drawn above everything (context menu).
    pub fn overlays(&mut self, ctx: &egui::Context) {
        self.menu_ui(ctx);
    }

    /// One line for the status bar: transient message, or mode and hints.
    pub fn status_text(&self) -> String {
        match &self.status {
            Some((message, _)) => message.clone(),
            None => self.header_text(),
        }
    }

    pub fn handle_keys(&mut self, ui: &Ui) {
        for (key, modifiers) in ui_kit::shortcut_keys(ui.ctx()) {
            if let Some(action) = shortcut(key, modifiers, self.editor.grab.is_some()) {
                self.apply(action);
            }
        }
    }

    /// Parts library and tools.
    pub fn palette_ui(&mut self, ui: &mut Ui) {
        let palette = OpticComponentKind::palette();
        let groups: [(&str, &[usize]); 3] = [
            ("Sources & probes", &[0, 6]),
            ("Optics", &[1, 2, 3, 4, 5]),
            ("Component ports", &[7, 8]),
        ];
        for (title, indices) in groups {
            ui_kit::section(ui, title);
            for &k in indices {
                let kind = palette[k];
                let selected = matches!(self.tool, Some(Tool::Place(t)) if t.name() == kind.name());
                let response = ui_kit::item_row(ui, selected, kind.name(), |painter, rect| {
                    render::draw_component(painter, rect.center(), &kind, 1.0, 0.6);
                });
                if response.drag_started() {
                    self.dragging_new = Some(kind);
                }
                if response.clicked() {
                    self.tool = if selected {
                        None
                    } else {
                        Some(Tool::Place(kind))
                    };
                }
                if k >= 7 {
                    response.on_hover_text(
                        "Marks where light enters or leaves when this layout is saved as a component. \
                         Ports are numbered top to bottom.",
                    );
                }
            }
        }
        ui_kit::section(ui, "Tools");
        ui.horizontal(|ui| {
            if ui.selectable_label(self.tool.is_none(), "Select").clicked() {
                self.tool = None;
            }
            if ui
                .selectable_label(self.tool == Some(Tool::DrawLine), "Line")
                .clicked()
            {
                self.tool = Some(Tool::DrawLine);
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Drag a part onto the grid, or click it and then click the grid.")
                .small()
                .weak(),
        );
    }

    /// Selection properties, simulation settings and readouts.
    pub fn inspector_ui(&mut self, ui: &mut Ui) {
        ui_kit::section(ui, "Selection");
        let selected = self.editor.selection.len();
        match self
            .editor
            .selection
            .active()
            .filter(|&i| i < self.layout.components.len())
        {
            None => {
                ui.label(egui::RichText::new("Nothing selected").weak());
                ui.label(
                    egui::RichText::new("Click a part, or drag a box around several.")
                        .small()
                        .weak(),
                );
            }
            Some(i) => {
                let reading = self.detector_reading(i);
                let (inputs, outputs) = beam_sim::port_order(&self.layout.components);
                let component = &mut self.layout.components[i];
                let (ci, cj) = model::cell_of(component.pos);
                ui.horizontal(|ui| {
                    ui.strong(component.kind.name());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("cell {ci}, {cj}")).weak());
                    });
                });
                if selected > 1 {
                    ui.label(
                        egui::RichText::new(format!(
                            "{selected} selected · showing the last clicked"
                        ))
                        .small()
                        .weak(),
                    );
                }
                egui::Grid::new("part_params")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        match &mut component.kind {
                            OpticComponentKind::Laser {
                                amplitude,
                                phase,
                                wavelength,
                                ..
                            } => {
                                ui.label("Amplitude");
                                ui.add(egui::Slider::new(amplitude, 0.0..=2.0));
                                ui.end_row();
                                ui.label("Phase");
                                ui.add(
                                    egui::Slider::new(phase, 0.0..=std::f32::consts::TAU)
                                        .suffix(" rad"),
                                );
                                ui.end_row();
                                ui.label("Wavelength");
                                ui.add(
                                    egui::DragValue::new(wavelength)
                                        .range(200.0..=2000.0)
                                        .speed(0.5)
                                        .suffix(" nm"),
                                );
                                ui.end_row();
                            }
                            OpticComponentKind::BeamSplitter {
                                reflectivity,
                                transparency,
                                ..
                            } => {
                                ui.label("Reflectivity");
                                if ui.add(egui::Slider::new(reflectivity, 0.0..=1.0)).changed() {
                                    *transparency = 1.0 - *reflectivity;
                                }
                                ui.end_row();
                            }
                            OpticComponentKind::PhaseShifter { phase } => {
                                ui.label("Phase");
                                ui.add(
                                    egui::Slider::new(phase, 0.0..=std::f32::consts::TAU)
                                        .suffix(" rad"),
                                );
                                ui.end_row();
                            }
                            OpticComponentKind::Attenuator { transmission } => {
                                ui.label("Transmission");
                                ui.add(egui::Slider::new(transmission, 0.0..=1.0));
                                ui.end_row();
                            }
                            OpticComponentKind::Detector | OpticComponentKind::OutputPort => {
                                ui.label("Power");
                                match reading {
                                    Some(p) if self.simulation_active => {
                                        ui.monospace(format!("{p:.4}"))
                                    }
                                    _ => ui.label(egui::RichText::new("simulation off").weak()),
                                };
                                ui.end_row();
                            }
                            OpticComponentKind::InputPort { .. }
                            | OpticComponentKind::Mirror { .. }
                            | OpticComponentKind::Lens => {}
                        }
                        if let Some(k) = inputs.iter().position(|&p| p == i) {
                            ui.label("Port");
                            ui.label(format!("input {} of {}", k + 1, inputs.len()));
                            ui.end_row();
                        }
                        if let Some(k) = outputs.iter().position(|&p| p == i) {
                            ui.label("Port");
                            ui.label(format!("output {} of {}", k + 1, outputs.len()));
                            ui.end_row();
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.small_button("Rotate").on_hover_text("R").clicked() {
                        self.apply(Action::RotateCw);
                    }
                    if ui.small_button("Delete").on_hover_text("X").clicked() {
                        self.apply(Action::Delete);
                    }
                });
            }
        }

        ui_kit::section(ui, "Simulation");
        ui.checkbox(&mut self.simulation_active, "Live simulation")
            .on_hover_text("Space");
        ui.horizontal(|ui| {
            ui.label("Cell size");
            ui.add(
                egui::DragValue::new(&mut self.cell_um)
                    .range(0.01..=10_000.0)
                    .speed(0.01)
                    .suffix(" µm"),
            );
        });
        if !self.simulation_active {
            return;
        }
        let Some(cache) = &self.sim_cache else { return };
        if let Some(e) = &cache.result.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        let mut wavelengths: Vec<f64> =
            cache.result.beams.iter().map(|b| b.wavelength_nm).collect();
        wavelengths.sort_by(f64::total_cmp);
        wavelengths.dedup();
        let summary = match wavelengths.as_slice() {
            [] => "No light — add a laser".to_owned(),
            wls => format!(
                "{} beams at {} nm",
                cache.result.beams.len(),
                wls.iter()
                    .map(|w| format!("{w:.0}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        ui.label(egui::RichText::new(summary).small().weak());

        let readings = cache.result.detector_power.clone();
        if readings.is_empty() {
            return;
        }
        ui_kit::section(ui, "Readouts");
        let scale = readings.iter().map(|r| r.1).fold(1e-12, f64::max);
        egui::Grid::new("readouts")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for (i, power) in readings {
                    let Some(component) = self.layout.components.get(i) else {
                        continue;
                    };
                    let (ci, cj) = model::cell_of(component.pos);
                    let label = format!(
                        "{} {ci},{cj}",
                        if component.kind == OpticComponentKind::OutputPort {
                            "Out"
                        } else {
                            "Det"
                        }
                    );
                    if ui
                        .selectable_label(self.editor.selection.contains(i), label)
                        .clicked()
                    {
                        self.editor.selection.select_only(i);
                    }
                    ui.add(
                        egui::ProgressBar::new((power / scale) as f32)
                            .desired_width(70.0)
                            .desired_height(8.0),
                    );
                    ui.monospace(format!("{power:.4}"));
                    ui.end_row();
                }
            });
    }

    pub fn canvas_ui(&mut self, ui: &mut Ui) {
        let theme = ui_kit::CanvasTheme::from_visuals(ui.visuals());
        egui::Frame::canvas(ui.style())
            .fill(theme.background)
            .inner_margin(0.0)
            .show(ui, |ui| self.canvas_contents(ui, &theme));
    }

    fn canvas_contents(&mut self, ui: &mut Ui, theme: &ui_kit::CanvasTheme) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = Some(rect);
        if self.frame_pending {
            self.frame_pending = false;
            self.frame(false);
        }
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p));
        self.mouse_screen = pointer;
        self.mouse_world = pointer.map(|p| self.view.to_world(rect, p));

        self.navigate(ui, rect, &response, pointer);
        self.mouse_world = pointer.map(|p| self.view.to_world(rect, p));
        self.interact(ui, rect, &response, pointer);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.background);
        render::draw_grid(&painter, rect, &self.view, theme);

        if self.layout.components.is_empty()
            && self.layout.lines.is_empty()
            && self.dragging_new.is_none()
        {
            ui_kit::empty_hint(
                &painter,
                rect,
                theme,
                "Empty layout",
                "Drag parts from the library, press Shift+A, or open an example",
            );
        }

        // Hovered cell and placement preview.
        if let Some(world) = self.mouse_world
            && self.editor.grab.is_none()
        {
            let cell = Rect::from_center_size(
                self.view.to_screen(rect, model::snap(world)),
                Vec2::splat(GRID * self.view.zoom),
            );
            painter.rect_filled(cell, 2.0, theme.hover);
            let preview = self.dragging_new.or(match self.tool {
                Some(Tool::Place(kind)) => Some(kind),
                _ => None,
            });
            if let Some(kind) = preview {
                render::draw_component(&painter, cell.center(), &kind, 0.45, self.view.zoom);
            }
        }

        let line_stroke = Stroke::new(2.0, Color32::from_rgb(214, 60, 60));
        for line in &self.layout.lines {
            painter.line_segment(
                [
                    self.view.to_screen(rect, line.start),
                    self.view.to_screen(rect, line.end),
                ],
                line_stroke,
            );
        }
        if let (Drag::Line { start }, Some(world)) = (self.drag, self.mouse_world) {
            painter.line_segment(
                [
                    self.view.to_screen(rect, start),
                    self.view.to_screen(rect, model::snap(world)),
                ],
                Stroke::new(2.0, line_stroke.color.gamma_multiply(0.5)),
            );
        }

        if self.simulation_active {
            self.refresh_simulation();
            let sim = &self.sim_cache.as_ref().expect("just refreshed").result;
            let hovered = render::draw_beams(&painter, rect, &self.view, &sim.beams, pointer);
            for &(i, power) in &sim.detector_power {
                painter.text(
                    self.view.to_screen(rect, self.layout.components[i].pos)
                        + Vec2::new(0.0, 14.0 * self.view.zoom),
                    egui::Align2::CENTER_TOP,
                    format!("{power:.3}"),
                    FontId::monospace(11.0),
                    theme.text,
                );
            }
            if !hovered.is_empty() && self.editor.grab.is_none() {
                response
                    .clone()
                    .on_hover_text_at_pointer(hovered.join("\n\n"));
            }
        }

        for (i, c) in self.layout.components.iter().enumerate() {
            let at = self.view.to_screen(rect, c.pos);
            if !rect.expand(GRID * self.view.zoom).contains(at) {
                continue;
            }
            render::draw_component(&painter, at, &c.kind, 1.0, self.view.zoom);
            if self.editor.selection.contains(i) {
                let active = self.editor.selection.active() == Some(i);
                painter.rect_stroke(
                    Rect::from_center_size(at, Vec2::splat((GRID - 4.0) * self.view.zoom)),
                    4.0,
                    Stroke::new(
                        if active { 2.0 } else { 1.5 },
                        if active {
                            theme.active
                        } else {
                            theme.selection
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
            }
        }

        if let (Drag::Box { start, .. }, Some(screen)) = (self.drag, pointer) {
            let area = Rect::from_two_pos(self.view.to_screen(rect, start), screen);
            painter.rect(
                area,
                0.0,
                Color32::from_rgba_unmultiplied(80, 130, 255, 40),
                Stroke::new(1.0, Color32::from_rgb(80, 130, 255)),
                egui::StrokeKind::Inside,
            );
        }
    }

    pub fn header_text(&self) -> String {
        if let Some(g) = &self.editor.grab {
            let axis = match g.axis {
                Axis::Free => "",
                Axis::X => "  [X]",
                Axis::Y => "  [Y]",
            };
            return format!(
                "Grab  Δ {}, {} cells{axis}  ·  X/Y constrain  ·  LMB/Enter confirm  ·  RMB/Esc cancel",
                g.offset.0, g.offset.1
            );
        }
        let cell = self
            .mouse_world
            .map(|w| {
                let (i, j) = model::cell_of(w);
                format!("  ·  cell {i}, {j}")
            })
            .unwrap_or_default();
        let tool = match self.tool {
            Some(Tool::Place(k)) => format!("  ·  placing {}", k.name()),
            Some(Tool::DrawLine) => "  ·  line tool".to_owned(),
            None => String::new(),
        };
        format!(
            "{:.0}%{cell}  ·  {} selected{tool}  ·  F1 shortcuts",
            self.view.zoom * 100.0,
            self.editor.selection.len()
        )
    }

    fn navigate(&mut self, ui: &Ui, rect: Rect, response: &egui::Response, pointer: Option<Pos2>) {
        view::navigate(&mut self.view, ui, rect, response, pointer);
    }

    fn interact(&mut self, ui: &Ui, rect: Rect, response: &egui::Response, pointer: Option<Pos2>) {
        let shift = ui.input(|i| i.modifiers.shift);

        if let Some(g) = &self.editor.grab {
            let tweak = g.tweak;
            if let Some(world) = self.mouse_world {
                self.editor.update_grab(&mut self.layout, world);
            }
            let (pressed_primary, released_primary, pressed_secondary) = ui.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Primary),
                    i.pointer.button_released(egui::PointerButton::Primary),
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                )
            });
            if pressed_secondary {
                self.editor.cancel_grab(&mut self.layout);
            } else if (tweak && released_primary)
                || (!tweak && pressed_primary && pointer.is_some())
            {
                self.confirm_grab();
            }
            return;
        }

        if let Some(kind) = self.dragging_new
            && ui.input(|i| i.pointer.any_released())
        {
            self.dragging_new = None;
            if let Some(world) = self.mouse_world
                && self.editor.place(&mut self.layout, kind, world).is_none()
            {
                self.notify("That cell is occupied");
            }
        }

        // Finish drags wherever the button is released, even over another panel.
        if self.drag != Drag::None && !ui.input(|i| i.pointer.primary_down()) {
            let end = ui
                .input(|i| i.pointer.latest_pos())
                .map(|p| self.view.to_world(rect, p));
            match (std::mem::replace(&mut self.drag, Drag::None), end) {
                (Drag::Box { start, extend }, Some(end)) => {
                    self.editor
                        .box_select(&self.layout, Rect::from_two_pos(start, end), extend);
                }
                (Drag::Line { start }, Some(end))
                    if rect.contains(self.view.to_screen(rect, end)) =>
                {
                    let end = model::snap(end);
                    if end != start {
                        self.layout.lines.push(Line { start, end });
                    }
                }
                _ => {}
            }
        }

        let Some(pointer) = pointer else { return };
        let world = self.view.to_world(rect, pointer);

        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
            let start = self.view.to_world(rect, origin);
            if self.tool == Some(Tool::DrawLine) {
                self.drag = Drag::Line {
                    start: model::snap(start),
                };
            } else if let Some(i) = self.layout.component_at(start) {
                if !self.editor.selection.contains(i) {
                    self.editor.selection.select_only(i);
                }
                self.editor.begin_grab(&self.layout, start, true);
                self.editor.update_grab(&mut self.layout, world);
            } else {
                self.drag = Drag::Box {
                    start,
                    extend: shift,
                };
            }
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            self.menu = None;
            if self.layout.component_at(world).is_some() {
                self.editor.click_select(&self.layout, world, shift);
                if response.double_clicked() {
                    self.editor.rotate_selected(&mut self.layout, 1);
                }
            } else if let Some(Tool::Place(kind)) = self.tool {
                self.editor.place(&mut self.layout, kind, world);
            } else {
                self.editor.click_select(&self.layout, world, shift);
            }
        }

        if response.clicked_by(egui::PointerButton::Secondary) {
            let target = self.layout.component_at(world);
            if let Some(i) = target
                && !self.editor.selection.contains(i)
            {
                self.editor.selection.select_only(i);
            }
            self.open_menu(target);
        }
    }

    fn menu_ui(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.menu else { return };
        let has_lines = {
            let cell = model::cell_of(menu.world);
            self.layout
                .lines
                .iter()
                .any(|l| model::cell_of(l.start) == cell || model::cell_of(l.end) == cell)
        };
        let shown = egui::Area::new(Id::unique("optics_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| {
                        let mut choice = None;
                        ui.set_min_width(170.0);
                        if let Some(i) = menu.target.filter(|&i| i < self.layout.components.len()) {
                            ui.label(
                                egui::RichText::new(self.layout.components[i].kind.name()).strong(),
                            );
                            if ui.button("⟳ Rotate          R").clicked() {
                                choice = Some(MenuChoice::Rotate(1));
                            }
                            if ui.button("⟲ Rotate back   ⇧R").clicked() {
                                choice = Some(MenuChoice::Rotate(-1));
                            }
                            if ui.button("⧉ Duplicate      ⇧D").clicked() {
                                choice = Some(MenuChoice::Duplicate);
                            }
                            if ui.button("🗑 Delete           X").clicked() {
                                choice = Some(MenuChoice::Delete);
                            }
                            ui.separator();
                        }
                        ui.label(egui::RichText::new("Add").strong());
                        for kind in OpticComponentKind::palette() {
                            if ui
                                .button(format!("{} {}", kind.icon(), kind.name()))
                                .clicked()
                            {
                                choice = Some(MenuChoice::Add(kind));
                            }
                        }
                        if has_lines {
                            ui.separator();
                            if ui.button("Delete lines here").clicked() {
                                choice = Some(MenuChoice::DeleteLines);
                            }
                        }
                        choice
                    })
                    .inner
            });
        if let Some(choice) = shown.inner {
            self.apply_menu_choice(choice, menu.world);
        } else if menu.opened_at < self.time
            && ctx.input(|i| i.pointer.any_pressed())
            && !shown.response.contains_pointer()
        {
            self.menu = None;
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.show_shortcuts;
        egui::Window::new("Keyboard & mouse")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    for (keys, what) in SHORTCUTS {
                        ui.monospace(*keys);
                        ui.label(*what);
                        ui.end_row();
                    }
                });
            });
        self.show_shortcuts = open;

        let mut open = self.show_examples;
        let mut load = None;
        egui::Window::new("Examples library")
            .open(&mut open)
            .default_size([760.0, 520.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.examples_tab,
                        ExamplesTab::Layouts,
                        "Free-space layouts",
                    );
                    ui.selectable_value(
                        &mut self.examples_tab,
                        ExamplesTab::Chips,
                        "Chip circuits",
                    );
                });
                ui.separator();
                match self.examples_tab {
                    ExamplesTab::Layouts => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (i, example) in examples::layout_examples().iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.strong(example.name);
                                    if ui.button("Load").clicked() {
                                        load = Some(i);
                                    }
                                });
                                ui.label(example.description);
                                ui.separator();
                            }
                        });
                    }
                    ExamplesTab::Chips => self.chips.ui(ui),
                }
            });
        self.show_examples = open;
        if let Some(i) = load {
            self.load_example(i);
        }
    }
}

#[cfg(test)]
mod tests;
