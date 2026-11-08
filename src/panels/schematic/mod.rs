//! Schematic editor: blocks and components placed freely, connected by waveguides.

pub mod examples;
pub mod geometry;
pub mod model;
pub mod sim;

use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use super::history::History;
use super::optics::editor::Axis;
use super::optics::input::{Action, shortcut};
use super::optics::render::wavelength_color;
use super::optics::view::{self, View};
use super::project::{BlockDisplay, Catalog, DocId};
use super::selection::Selection;
use super::{Request, ui_kit};
use geometry::Shape;
use model::{BlockRef, Builtin, Category, PortRef, Schematic, WireModel, snap};
use sim::SchematicSim;

/// A wire being drawn: the fixed end, the other follows the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Output(PortRef),
    Input(PortRef),
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    None,
    Box { start: Pos2, extend: bool },
    Wire(Anchor),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub start: Pos2,
    pub originals: Vec<(usize, Pos2)>,
    pub axis: Axis,
    pub tweak: bool,
    /// Started by Shift+D: cancelling removes the copies and re-selects these.
    pub duplicated_from: Option<Vec<usize>>,
}

/// Add menu (Shift+A, right click on empty space) or context menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub screen: Pos2,
    pub world: Pos2,
    pub target: Option<usize>,
    pub search: String,
    opened_at: f64,
}

/// What the cursor is over, most specific first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Output(PortRef),
    Input(PortRef),
    Instance(usize),
    Wire(usize),
    Empty,
}

pub struct SchematicPanel {
    pub schematic: Schematic,
    pub history: History<Schematic>,
    pub selection: Selection,
    pub selected_wire: Option<usize>,
    pub view: View,
    /// Block placed by clicking on empty canvas.
    pub tool: Option<BlockRef>,
    pub dragging_new: Option<BlockRef>,
    pub simulation_active: bool,
    pub sim: Option<SchematicSim>,
    /// Project fingerprint the simulation was computed for.
    pub sim_key: Option<u64>,
    pub status: Option<(String, f64)>,
    pub menu: Option<Menu>,
    pub requests: Vec<Request>,
    pub grab: Option<Grab>,
    /// The document being edited, to refuse placing it inside itself.
    pub doc_id: Option<DocId>,
    pub search: String,
    drag: Drag,
    canvas: Option<Rect>,
    mouse_screen: Option<Pos2>,
    mouse_world: Option<Pos2>,
    frame_pending: bool,
    time: f64,
}

impl Default for SchematicPanel {
    fn default() -> Self {
        Self::with_schematic(Schematic::default())
    }
}

impl SchematicPanel {
    pub fn with_schematic(schematic: Schematic) -> Self {
        Self {
            history: History::new(&schematic),
            frame_pending: !schematic.instances.is_empty(),
            schematic,
            selection: Selection::default(),
            selected_wire: None,
            view: View::default(),
            tool: None,
            dragging_new: None,
            simulation_active: true,
            sim: None,
            sim_key: None,
            status: None,
            menu: None,
            requests: Vec::new(),
            grab: None,
            doc_id: None,
            search: String::new(),
            drag: Drag::None,
            canvas: None,
            mouse_screen: None,
            mouse_world: None,
            time: 0.0,
        }
    }

    pub fn canvas_rect(&self) -> Option<Rect> {
        self.canvas
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        let message = message.into();
        tracing::info!("{message}");
        self.status = Some((message, self.time + 3.0));
    }

    pub fn shapes(&self, catalog: &Catalog) -> Vec<Shape> {
        self.schematic
            .instances
            .iter()
            .map(|inst| {
                let d = catalog.display(&inst.block);
                geometry::shape(inst.pos, &inst.block, d.inputs.len(), d.outputs.len())
            })
            .collect()
    }

    /// What is under `screen`.
    pub fn hit(&self, catalog: &Catalog, canvas: Rect, screen: Pos2) -> Hit {
        let shapes = self.shapes(catalog);
        let to_screen = |p: Pos2| self.view.to_screen(canvas, p);
        for (i, shape) in shapes.iter().enumerate().rev() {
            let instance = self.schematic.instances[i].id;
            for (k, p) in shape.outputs.iter().enumerate() {
                if to_screen(*p).distance(screen) <= geometry::PORT_HIT {
                    return Hit::Output(PortRef { instance, port: k });
                }
            }
            for (k, p) in shape.inputs.iter().enumerate() {
                if to_screen(*p).distance(screen) <= geometry::PORT_HIT {
                    return Hit::Input(PortRef { instance, port: k });
                }
            }
        }
        let world = self.view.to_world(canvas, screen);
        if let Some(i) = shapes.iter().rposition(|s| s.rect.contains(world)) {
            return Hit::Instance(i);
        }
        for (k, (a, b)) in self.wire_endpoints(&shapes).into_iter().enumerate() {
            if let (Some(a), Some(b)) = (a, b)
                && geometry::distance_to_wire(screen, to_screen(a), to_screen(b)) < 5.0
            {
                return Hit::Wire(k);
            }
        }
        Hit::Empty
    }

    /// World positions of each wire's ends, `None` where the port is gone.
    fn wire_endpoints(&self, shapes: &[Shape]) -> Vec<(Option<Pos2>, Option<Pos2>)> {
        let shape_of = |id| self.schematic.index_of(id).map(|i| &shapes[i]);
        self.schematic
            .wires
            .iter()
            .map(|w| {
                (
                    shape_of(w.from.instance).and_then(|s| s.outputs.get(w.from.port).copied()),
                    shape_of(w.to.instance).and_then(|s| s.inputs.get(w.to.port).copied()),
                )
            })
            .collect()
    }

    fn port_world(&self, shapes: &[Shape], anchor: Anchor) -> Option<Pos2> {
        let (port, outputs) = match anchor {
            Anchor::Output(p) => (p, true),
            Anchor::Input(p) => (p, false),
        };
        let shape = &shapes[self.schematic.index_of(port.instance)?];
        if outputs {
            shape.outputs.get(port.port).copied()
        } else {
            shape.inputs.get(port.port).copied()
        }
    }

    // ---- editing ----------------------------------------------------------

    /// Place `block` at `world` unless it would make the document contain itself.
    pub fn place(&mut self, catalog: &Catalog, block: BlockRef, world: Pos2) -> Option<usize> {
        if let (BlockRef::Component(id), Some(me)) = (&block, self.doc_id)
            && !catalog.can_place(*id, me)
        {
            self.notify("A component can't contain itself");
            return None;
        }
        let d = catalog.display(&block);
        let size = geometry::box_size(d.inputs.len(), d.outputs.len(), false);
        self.schematic.add(block, world - size / 2.0);
        let i = self.schematic.instances.len() - 1;
        self.selection.select_only(i);
        self.selected_wire = None;
        Some(i)
    }

    pub fn delete_selected(&mut self) -> usize {
        if let Some(w) = self.selected_wire.take() {
            if w < self.schematic.wires.len() {
                self.schematic.wires.remove(w);
                return 1;
            }
            return 0;
        }
        let doomed: Vec<usize> = self.selection.iter().collect();
        self.schematic.remove(&doomed);
        self.selection.clear();
        doomed.len()
    }

    fn begin_grab(&mut self, mouse: Pos2, tweak: bool) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        self.grab = Some(Grab {
            start: mouse,
            originals: self
                .selection
                .iter()
                .map(|i| (i, self.schematic.instances[i].pos))
                .collect(),
            axis: Axis::Free,
            tweak,
            duplicated_from: None,
        });
        true
    }

    fn update_grab(&mut self, mouse: Pos2) {
        let Some(g) = &self.grab else { return };
        let mut delta = mouse - g.start;
        match g.axis {
            Axis::Free => {}
            Axis::X => delta.y = 0.0,
            Axis::Y => delta.x = 0.0,
        }
        for &(i, original) in &g.originals {
            if let Some(inst) = self.schematic.instances.get_mut(i) {
                inst.pos = snap(original + delta);
            }
        }
    }

    pub fn cancel_grab(&mut self) {
        let Some(g) = self.grab.take() else { return };
        if let Some(sources) = g.duplicated_from {
            let copies: Vec<usize> = g.originals.iter().map(|o| o.0).collect();
            self.schematic.remove(&copies);
            self.selection.clear();
            for i in sources {
                self.selection.add(i);
            }
        } else {
            for (i, original) in g.originals {
                if let Some(inst) = self.schematic.instances.get_mut(i) {
                    inst.pos = original;
                }
            }
        }
    }

    fn duplicate(&mut self, mouse: Pos2) -> bool {
        let sources: Vec<usize> = self.selection.iter().collect();
        if sources.is_empty() {
            return false;
        }
        let copies = self.schematic.duplicate(&sources, Vec2::ZERO);
        self.selection.clear();
        for i in copies {
            self.selection.add(i);
        }
        self.begin_grab(mouse, false);
        if let Some(g) = &mut self.grab {
            g.duplicated_from = Some(sources);
        }
        true
    }

    fn frame(&mut self, catalog: &Catalog, selected_only: bool) {
        let shapes = self.shapes(catalog);
        let bounds = shapes
            .iter()
            .enumerate()
            .filter(|(i, _)| !selected_only || self.selection.contains(*i))
            .map(|(_, s)| s.rect)
            .reduce(|a, b| a.union(b));
        match (self.canvas, bounds) {
            (Some(canvas), Some(bounds)) => self.view.frame(canvas, bounds, 60.0),
            (_, None) if !selected_only => self.view = View::default(),
            (None, Some(_)) => self.frame_pending = true,
            (_, None) => {}
        }
    }

    fn active_component(&self) -> Option<DocId> {
        let i = self.selection.active()?;
        match self.schematic.instances.get(i)?.block {
            BlockRef::Component(id) => Some(id),
            BlockRef::Builtin(_) => None,
        }
    }

    pub fn apply(&mut self, catalog: &Catalog, action: Action) {
        let mouse = self
            .mouse_world
            .or_else(|| self.canvas.map(|c| self.view.to_world(c, c.center())))
            .unwrap_or(Pos2::ZERO);
        let count = self.schematic.instances.len();
        match action {
            Action::SelectAll => {
                self.selection.select_all(count);
                self.selected_wire = None;
            }
            Action::DeselectAll => {
                self.selection.clear();
                self.selected_wire = None;
            }
            Action::InvertSelection => self.selection.invert(count),
            Action::Delete => {
                self.delete_selected();
            }
            Action::Duplicate => {
                if !self.duplicate(mouse) {
                    self.notify("Nothing selected to duplicate");
                }
            }
            Action::Grab => {
                if !self.begin_grab(mouse, false) {
                    self.notify("Nothing selected to move");
                }
            }
            Action::RotateCw | Action::RotateCcw => {
                self.notify("Blocks keep inputs on the left and outputs on the right")
            }
            Action::Undo | Action::Redo => {
                self.cancel_grab();
                let changed = if action == Action::Undo {
                    self.history.undo(&mut self.schematic)
                } else {
                    self.history.redo(&mut self.schematic)
                };
                if !changed {
                    self.notify(if action == Action::Undo {
                        "Nothing to undo"
                    } else {
                        "Nothing to redo"
                    });
                }
                self.selection.retain_valid(self.schematic.instances.len());
                self.selected_wire = None;
            }
            Action::FrameAll => self.frame(catalog, false),
            Action::FrameSelected => self.frame(catalog, true),
            Action::ToggleSimulation => self.simulation_active = !self.simulation_active,
            Action::ToggleToolbar => self.requests.push(Request::ToggleLibrary),
            Action::ToggleProperties => self.requests.push(Request::ToggleInspector),
            Action::ShowShortcuts => self.requests.push(Request::ShowShortcuts),
            Action::Save => self.requests.push(Request::Save),
            Action::Open => self.requests.push(Request::Open),
            Action::New => self.requests.push(Request::New),
            Action::AddMenu => self.open_menu(None),
            Action::ConstrainX | Action::ConstrainY => {
                let axis = if action == Action::ConstrainX {
                    Axis::X
                } else {
                    Axis::Y
                };
                if let Some(g) = &mut self.grab {
                    g.axis = if g.axis == axis { Axis::Free } else { axis };
                }
                self.update_grab(mouse);
            }
            Action::Confirm => self.grab = None,
            Action::Cancel => {
                if self.grab.is_some() {
                    self.cancel_grab();
                } else {
                    self.menu = None;
                    self.tool = None;
                    self.dragging_new = None;
                    self.drag = Drag::None;
                }
            }
            Action::Inspect => match self.active_component() {
                Some(id) => self.requests.push(Request::Inspect(id)),
                None => self.notify("Select a component to open it"),
            },
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
            search: String::new(),
            opened_at: self.time,
        });
    }

    // ---- frame ------------------------------------------------------------

    pub fn begin_frame(&mut self, ui: &Ui, catalog: &Catalog) {
        self.time = ui.input(|i| i.time);
        if self.status.as_ref().is_some_and(|s| s.1 < self.time) {
            self.status = None;
        }
        for (key, modifiers) in ui_kit::shortcut_keys(ui.ctx()) {
            if let Some(action) = shortcut(key, modifiers, self.grab.is_some()) {
                self.apply(catalog, action);
            }
        }
    }

    pub fn end_frame(&mut self, ui: &Ui) {
        if self.grab.is_none() && self.drag == Drag::None && !ui.input(|i| i.pointer.any_down()) {
            self.history.commit(&self.schematic);
        }
    }

    pub fn status_text(&self) -> String {
        if let Some((message, _)) = &self.status {
            return message.clone();
        }
        if self.grab.is_some() {
            return "Move  ·  X/Y constrain  ·  LMB/Enter confirm  ·  RMB/Esc cancel".to_owned();
        }
        if let Drag::Wire(_) = self.drag {
            return "Drop on a port to connect  ·  release elsewhere to cancel".to_owned();
        }
        let selected = match (self.selection.len(), self.selected_wire) {
            (_, Some(_)) => "waveguide selected".to_owned(),
            (0, None) => "drag from a port to draw a waveguide".to_owned(),
            (n, None) => format!("{n} selected"),
        };
        format!(
            "{:.0}%  ·  {selected}  ·  Tab opens a component  ·  F1 shortcuts",
            self.view.zoom * 100.0
        )
    }

    // ---- library ----------------------------------------------------------

    fn matches_search(search: &str, name: &str, description: &str) -> bool {
        let search = search.trim().to_lowercase();
        search.is_empty()
            || name.to_lowercase().contains(&search)
            || description.to_lowercase().contains(&search)
    }

    /// Library rows for every block and component matching `search`.
    pub fn library_items(catalog: &Catalog, search: &str) -> Vec<(Option<Category>, BlockRef)> {
        let mut items: Vec<(Option<Category>, BlockRef)> = Builtin::palette()
            .into_iter()
            .filter(|b| {
                let info = b.info();
                Self::matches_search(search, info.name, info.description)
            })
            .map(|b| (Some(b.info().category), BlockRef::Builtin(b)))
            .collect();
        items.extend(
            catalog
                .components()
                .filter(|e| Self::matches_search(search, &e.name, &e.description))
                .map(|e| (None, BlockRef::Component(e.id))),
        );
        items
    }

    fn library_row(&mut self, ui: &mut Ui, catalog: &Catalog, block: BlockRef) {
        let d = catalog.display(&block);
        let placeable = match (&block, self.doc_id) {
            (BlockRef::Component(id), Some(me)) => catalog.can_place(*id, me),
            _ => true,
        };
        let selected = self.tool.as_ref() == Some(&block);
        let fill = badge_fill(&d);
        let response = ui.add_enabled_ui(placeable, |ui| {
            ui_kit::item_row(ui, selected, &d.name, |painter, rect| {
                ui_kit::badge(painter, rect.shrink(1.0), &d.initials, fill, Color32::WHITE);
            })
        });
        let response = response.inner;
        if response.drag_started() {
            self.dragging_new = Some(block.clone());
        }
        if response.clicked() {
            self.tool = if selected { None } else { Some(block) };
        }
        let tip = if placeable {
            library_tooltip(&d)
        } else {
            format!("{}\n\nCan't be placed here: it uses this document.", d.name)
        };
        response.on_hover_text(tip);
    }

    pub fn palette_ui(&mut self, ui: &mut Ui, catalog: &Catalog) {
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search blocks")
                .desired_width(f32::INFINITY),
        );
        let search = self.search.clone();
        let items = Self::library_items(catalog, &search);
        let mut current: Option<Option<Category>> = None;
        for (category, block) in items {
            if current != Some(category) {
                ui_kit::section(ui, category.map_or("My components", Category::title));
                current = Some(category);
            }
            self.library_row(ui, catalog, block);
        }
        if catalog.components().next().is_none() && search.is_empty() {
            ui_kit::section(ui, "My components");
            ui.label(
                egui::RichText::new("Save any layout or schematic as a component (Component menu) to reuse it here.")
                    .small()
                    .weak(),
            );
        }
    }

    // ---- inspector --------------------------------------------------------

    pub fn inspector_ui(&mut self, ui: &mut Ui, catalog: &Catalog) {
        ui_kit::section(ui, "Selection");
        if let Some(w) = self
            .selected_wire
            .filter(|&w| w < self.schematic.wires.len())
        {
            self.wire_inspector(ui, catalog, w);
        } else if let Some(i) = self
            .selection
            .active()
            .filter(|&i| i < self.schematic.instances.len())
        {
            self.instance_inspector(ui, catalog, i);
        } else {
            ui.label(egui::RichText::new("Nothing selected").weak());
            ui.label(
                egui::RichText::new("Click a block or a waveguide.")
                    .small()
                    .weak(),
            );
        }

        ui_kit::section(ui, "Waveguides");
        let settings = &mut self.schematic.wire_settings;
        let mut geometric = matches!(settings.model, WireModel::Geometric { .. });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut geometric, false, "Ideal")
                .on_hover_text("No length: results don't depend on where blocks sit");
            ui.selectable_value(&mut geometric, true, "From drawing")
                .on_hover_text("Length follows the drawn waveguide");
        });
        match (geometric, settings.model) {
            (true, WireModel::Ideal) => settings.model = WireModel::Geometric { um_per_unit: 0.5 },
            (false, WireModel::Geometric { .. }) => settings.model = WireModel::Ideal,
            _ => {}
        }
        egui::Grid::new("wire_settings")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                if let WireModel::Geometric { um_per_unit } = &mut settings.model {
                    ui.label("Scale");
                    ui.add(
                        egui::DragValue::new(um_per_unit)
                            .range(0.001..=1000.0)
                            .speed(0.01)
                            .suffix(" µm/unit"),
                    );
                    ui.end_row();
                }
                ui.label("Index n_eff");
                ui.add(
                    egui::DragValue::new(&mut settings.n_eff)
                        .range(1.0..=4.0)
                        .speed(0.01),
                );
                ui.end_row();
                ui.label("Loss");
                ui.add(
                    egui::DragValue::new(&mut settings.loss_db_per_cm)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .suffix(" dB/cm"),
                );
                ui.end_row();
            });

        ui_kit::section(ui, "Simulation");
        ui.checkbox(&mut self.simulation_active, "Live simulation")
            .on_hover_text("Space");
        let Some(sim) = self.sim.as_ref().filter(|_| self.simulation_active) else {
            return;
        };
        if let Some(e) = &sim.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        let summary = if sim.wavelengths.is_empty() {
            "No light — add a laser".to_owned()
        } else {
            format!(
                "{} nm · {} circuit nodes",
                sim.wavelengths
                    .iter()
                    .map(|w| format!("{w:.1}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                sim.nodes
            )
        };
        ui.label(egui::RichText::new(summary).small().weak());

        let readings = sim.readings.clone();
        if readings.is_empty() {
            return;
        }
        ui_kit::section(ui, "Readouts");
        let scale = readings.iter().map(|r| r.1).fold(1e-12, f64::max);
        egui::Grid::new("schematic_readouts")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for (id, power) in readings {
                    let Some(i) = self.schematic.index_of(id) else {
                        continue;
                    };
                    let name = match &self.schematic.instances[i].block {
                        BlockRef::Builtin(Builtin::OutputPin { name }) => format!("Out “{name}”"),
                        _ => format!("Detector {}", id.0 + 1),
                    };
                    if ui
                        .selectable_label(self.selection.contains(i), name)
                        .clicked()
                    {
                        self.selection.select_only(i);
                        self.selected_wire = None;
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

    fn wire_inspector(&mut self, ui: &mut Ui, catalog: &Catalog, w: usize) {
        let wire = &self.schematic.wires[w];
        let end = |port: PortRef, output: bool| {
            let Some(inst) = self.schematic.instance(port.instance) else {
                return "?".to_owned();
            };
            let d = catalog.display(&inst.block);
            let names = if output { &d.outputs } else { &d.inputs };
            format!(
                "{} · {}",
                d.initials,
                names
                    .get(port.port)
                    .cloned()
                    .unwrap_or_else(|| "missing".into())
            )
        };
        ui.strong("Waveguide");
        ui.label(format!(
            "{}  →  {}",
            end(wire.from, true),
            end(wire.to, false)
        ));
        let power = self.sim.as_ref().map(|s| s.wire_power(w));
        let wire = &mut self.schematic.wires[w];
        let mut fixed = wire.length_um.is_some();
        egui::Grid::new("wire_params")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Length");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut fixed, "fixed");
                    if let Some(length) = &mut wire.length_um {
                        ui.add(
                            egui::DragValue::new(length)
                                .range(0.0..=1e6)
                                .speed(0.05)
                                .suffix(" µm"),
                        );
                    }
                });
                ui.end_row();
                if let Some(p) = power.filter(|_| self.simulation_active) {
                    ui.label("Power");
                    ui.monospace(format!("{p:.4}"));
                    ui.end_row();
                }
            });
        match (fixed, wire.length_um) {
            (true, None) => wire.length_um = Some(10.0),
            (false, Some(_)) => wire.length_um = None,
            _ => {}
        }
        if ui.small_button("Delete").on_hover_text("X").clicked() {
            self.delete_selected();
        }
    }

    fn instance_inspector(&mut self, ui: &mut Ui, catalog: &Catalog, i: usize) {
        let d = catalog.display(&self.schematic.instances[i].block);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(30.0, 20.0), Sense::hover());
            ui_kit::badge(
                ui.painter(),
                rect,
                &d.initials,
                badge_fill(&d),
                Color32::WHITE,
            );
            ui.strong(&d.name);
        });
        if self.selection.len() > 1 {
            ui.label(
                egui::RichText::new(format!(
                    "{} selected · showing the last clicked",
                    self.selection.len()
                ))
                .small()
                .weak(),
            );
        }
        if !d.description.is_empty() {
            ui.label(egui::RichText::new(&d.description).weak());
        }
        let id = self.schematic.instances[i].id;
        if let BlockRef::Builtin(b) = &mut self.schematic.instances[i].block {
            builtin_params_ui(ui, b);
        }
        if let Some(component) = d.component
            && ui.button("Open definition").on_hover_text("Tab").clicked()
        {
            self.requests.push(Request::Inspect(component));
        }
        if !d.inputs.is_empty() || !d.outputs.is_empty() {
            ui_kit::section(ui, "Ports");
            let power = |k: usize| {
                self.sim
                    .as_ref()
                    .filter(|_| self.simulation_active)
                    .and_then(|s| s.port_power.get(&(id, k)).copied())
            };
            egui::Grid::new("ports")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    for name in &d.inputs {
                        ui.label(egui::RichText::new("in").weak());
                        ui.label(name);
                        ui.end_row();
                    }
                    for (k, name) in d.outputs.iter().enumerate() {
                        ui.label(egui::RichText::new("out").weak());
                        ui.horizontal(|ui| {
                            ui.label(name);
                            if let Some(p) = power(k) {
                                ui.monospace(format!("{p:.4}"));
                            }
                        });
                        ui.end_row();
                    }
                });
        }
        if ui.small_button("Delete").on_hover_text("X").clicked() {
            self.delete_selected();
        }
    }

    // ---- canvas -----------------------------------------------------------

    pub fn canvas_ui(&mut self, ui: &mut Ui, catalog: &Catalog) {
        let theme = ui_kit::CanvasTheme::from_visuals(ui.visuals());
        egui::Frame::canvas(ui.style())
            .fill(theme.background)
            .inner_margin(0.0)
            .show(ui, |ui| self.canvas_contents(ui, catalog, &theme));
    }

    fn canvas_contents(&mut self, ui: &mut Ui, catalog: &Catalog, theme: &ui_kit::CanvasTheme) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = Some(rect);
        if self.frame_pending {
            self.frame_pending = false;
            self.frame(catalog, false);
        }
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p));
        self.mouse_screen = pointer;
        view::navigate(&mut self.view, ui, rect, &response, pointer);
        self.mouse_world = pointer.map(|p| self.view.to_world(rect, p));
        self.interact(ui, catalog, rect, &response, pointer);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.background);
        super::optics::render::draw_grid(&painter, rect, &self.view, theme);
        if self.schematic.instances.is_empty() && self.dragging_new.is_none() {
            ui_kit::empty_hint(
                &painter,
                rect,
                theme,
                "Empty schematic",
                "Drag blocks from the library, or press Shift+A",
            );
        }

        let shapes = self.shapes(catalog);
        let hover = pointer.map(|p| self.hit(catalog, rect, p));
        let to_screen = |p: Pos2| self.view.to_screen(rect, p);
        let zoom = self.view.zoom;

        // Waveguides.
        for (k, (a, b)) in self.wire_endpoints(&shapes).into_iter().enumerate() {
            let selected = self.selected_wire == Some(k);
            let hovered = hover == Some(Hit::Wire(k));
            let (Some(a), Some(b)) = (a, b) else {
                // A port vanished after its component changed: show what is left.
                let end = a.or(b).map(to_screen);
                if let Some(end) = end {
                    painter.circle_stroke(end, 6.0, Stroke::new(2.0, ui.visuals().error_fg_color));
                }
                continue;
            };
            let color = self.wire_color(k, theme);
            let width = if selected || hovered { 3.0 } else { 2.0 };
            let points: Vec<Pos2> = geometry::wire_polyline(a, b, 32)
                .into_iter()
                .map(to_screen)
                .collect();
            if selected {
                painter.add(egui::Shape::line(
                    points.clone(),
                    Stroke::new(width + 3.0, theme.selection.gamma_multiply(0.5)),
                ));
            }
            painter.add(egui::Shape::line(points, Stroke::new(width, color)));
        }
        if let (Drag::Wire(anchor), Some(p)) = (&self.drag, pointer)
            && let Some(start) = self.port_world(&shapes, *anchor)
        {
            let (a, b) = match anchor {
                Anchor::Output(_) => (to_screen(start), p),
                Anchor::Input(_) => (p, to_screen(start)),
            };
            let curve: Vec<Pos2> = geometry::wire_polyline(
                self.view.to_world(rect, a),
                self.view.to_world(rect, b),
                24,
            )
            .into_iter()
            .map(to_screen)
            .collect();
            painter.add(egui::Shape::dashed_line(
                &curve,
                Stroke::new(2.0, theme.weak_text),
                6.0,
                4.0,
            ));
        }

        // Blocks.
        for (i, (inst, shape)) in self.schematic.instances.iter().zip(&shapes).enumerate() {
            let r = Rect::from_min_max(to_screen(shape.rect.min), to_screen(shape.rect.max));
            if !rect.intersects(r.expand(20.0)) {
                continue;
            }
            let d = catalog.display(&inst.block);
            let selected = self.selection.contains(i);
            let active = self.selection.active() == Some(i);
            draw_block(&painter, r, &inst.block, &d, theme, zoom);
            if selected {
                painter.rect_stroke(
                    r.expand(2.0),
                    6.0,
                    Stroke::new(
                        if active { 2.0 } else { 1.5 },
                        if active {
                            theme.active
                        } else {
                            theme.selection
                        },
                    ),
                    egui::StrokeKind::Outside,
                );
            }
            for (k, p) in shape.inputs.iter().enumerate() {
                let port = PortRef {
                    instance: inst.id,
                    port: k,
                };
                let connected = self.schematic.wire_into(port).is_some();
                draw_port(
                    &painter,
                    to_screen(*p),
                    false,
                    connected,
                    hover == Some(Hit::Input(port)),
                    theme,
                );
            }
            for (k, p) in shape.outputs.iter().enumerate() {
                let port = PortRef {
                    instance: inst.id,
                    port: k,
                };
                let connected = self.schematic.wire_from(port).is_some();
                draw_port(
                    &painter,
                    to_screen(*p),
                    true,
                    connected,
                    hover == Some(Hit::Output(port)),
                    theme,
                );
            }
            if zoom >= 0.7
                && !matches!(
                    inst.block,
                    BlockRef::Builtin(Builtin::InputPin { .. } | Builtin::OutputPin { .. })
                )
            {
                painter.text(
                    r.center_bottom() + Vec2::new(0.0, 3.0),
                    Align2::CENTER_TOP,
                    &d.name,
                    FontId::proportional(11.0),
                    theme.weak_text,
                );
            }
            if let Some(reading) = self
                .sim
                .as_ref()
                .filter(|_| self.simulation_active)
                .and_then(|s| s.reading(inst.id))
            {
                let at = if zoom >= 0.7 {
                    r.center_bottom() + Vec2::new(0.0, 16.0)
                } else {
                    r.center_bottom() + Vec2::new(0.0, 3.0)
                };
                painter.text(
                    at,
                    Align2::CENTER_TOP,
                    format!("{reading:.3}"),
                    FontId::monospace(11.0),
                    theme.text,
                );
            }
        }

        // Placement preview.
        if let (Some(block), Some(world)) = (
            self.dragging_new.as_ref().or(self.tool.as_ref()),
            self.mouse_world,
        ) && hover == Some(Hit::Empty)
        {
            let d = catalog.display(block);
            let size = geometry::box_size(d.inputs.len(), d.outputs.len(), false);
            let ghost = geometry::shape(
                snap(world - size / 2.0),
                block,
                d.inputs.len(),
                d.outputs.len(),
            );
            let r = Rect::from_min_max(to_screen(ghost.rect.min), to_screen(ghost.rect.max));
            painter.rect_stroke(
                r,
                6.0,
                Stroke::new(1.5, theme.weak_text),
                egui::StrokeKind::Inside,
            );
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                &d.initials,
                FontId::proportional(14.0 * zoom.clamp(0.5, 2.0)),
                theme.weak_text,
            );
        }

        if let (Drag::Box { start, .. }, Some(p)) = (&self.drag, pointer) {
            let area = Rect::from_two_pos(to_screen(*start), p);
            painter.rect(
                area,
                0.0,
                Color32::from_rgba_unmultiplied(80, 130, 255, 40),
                Stroke::new(1.0, Color32::from_rgb(80, 130, 255)),
                egui::StrokeKind::Inside,
            );
        }

        if self.grab.is_none()
            && self.drag == Drag::None
            && let Some(hit) = hover
            && let Some(text) = self.hover_text(catalog, hit)
        {
            response.clone().on_hover_text_at_pointer(text);
        }
    }

    fn wire_color(&self, wire: usize, theme: &ui_kit::CanvasTheme) -> Color32 {
        let base = theme.major_line.gamma_multiply(1.4);
        let Some(sim) = self
            .sim
            .as_ref()
            .filter(|s| self.simulation_active && s.error.is_none())
        else {
            return base;
        };
        let Some(fields) = sim.wire_fields.get(wire) else {
            return base;
        };
        let best = fields
            .iter()
            .zip(&sim.wavelengths)
            .max_by(|a, b| a.0.norm_sqr().total_cmp(&b.0.norm_sqr()));
        match best {
            Some((field, &wl)) if field.norm_sqr() > 1e-9 => {
                wavelength_color(wl).gamma_multiply((field.norm() as f32).clamp(0.3, 1.0))
            }
            _ => base,
        }
    }

    pub fn hover_text(&self, catalog: &Catalog, hit: Hit) -> Option<String> {
        let power = |instance, k: usize| {
            self.sim
                .as_ref()
                .filter(|_| self.simulation_active)
                .and_then(|s| s.port_power.get(&(instance, k)).copied())
        };
        match hit {
            Hit::Instance(i) => {
                let d = catalog.display(&self.schematic.instances[i].block);
                let mut text = format!("{}\n{}", d.name, d.description);
                let ports = |names: &[String]| {
                    if names.is_empty() {
                        "—".to_owned()
                    } else {
                        names.join(", ")
                    }
                };
                text += &format!(
                    "\n\nInputs: {}\nOutputs: {}",
                    ports(&d.inputs),
                    ports(&d.outputs)
                );
                if d.component.is_some() {
                    text += "\n\nTab or double-click to open its definition";
                }
                Some(text.trim().to_owned())
            }
            Hit::Output(p) | Hit::Input(p) => {
                let inst = self.schematic.instance(p.instance)?;
                let d = catalog.display(&inst.block);
                let output = matches!(hit, Hit::Output(_));
                let name = if output {
                    d.outputs.get(p.port)
                } else {
                    d.inputs.get(p.port)
                }
                .cloned()
                .unwrap_or_default();
                let mut text = format!(
                    "{} {} “{name}”",
                    d.initials,
                    if output { "output" } else { "input" }
                );
                if let Some(pw) = power(p.instance, p.port).filter(|_| output) {
                    text += &format!("\nPower {pw:.4}");
                }
                Some(text)
            }
            Hit::Wire(w) => {
                let wire = self.schematic.wires.get(w)?;
                let length = match (wire.length_um, self.schematic.wire_settings.model) {
                    (Some(l), _) => format!("{l:.2} µm (fixed)"),
                    (None, WireModel::Ideal) => "ideal".to_owned(),
                    (None, WireModel::Geometric { .. }) => "from drawing".to_owned(),
                };
                let mut text = format!("Waveguide · {length}");
                if let Some(sim) = self.sim.as_ref().filter(|_| self.simulation_active) {
                    for (field, wl) in sim
                        .wire_fields
                        .get(w)
                        .into_iter()
                        .flatten()
                        .zip(&sim.wavelengths)
                    {
                        text += &format!(
                            "\n{wl:.1} nm: power {:.4}, phase {:.3} π",
                            field.norm_sqr(),
                            field.arg().rem_euclid(std::f64::consts::TAU) / std::f64::consts::PI
                        );
                    }
                }
                Some(text)
            }
            Hit::Empty => None,
        }
    }

    fn interact(
        &mut self,
        ui: &Ui,
        catalog: &Catalog,
        rect: Rect,
        response: &egui::Response,
        pointer: Option<Pos2>,
    ) {
        let shift = ui.input(|i| i.modifiers.shift);

        if let Some(g) = &self.grab {
            let tweak = g.tweak;
            if let Some(world) = self.mouse_world {
                self.update_grab(world);
            }
            let (pressed, released, cancel) = ui.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Primary),
                    i.pointer.button_released(egui::PointerButton::Primary),
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                )
            });
            if cancel {
                self.cancel_grab();
            } else if (tweak && released) || (!tweak && pressed && pointer.is_some()) {
                self.grab = None;
            }
            return;
        }

        if let Some(block) = self.dragging_new.clone()
            && ui.input(|i| i.pointer.any_released())
        {
            self.dragging_new = None;
            if let Some(world) = self.mouse_world {
                self.place(catalog, block, world);
            }
        }

        // Finish drags wherever the button is released, even over another panel.
        if self.drag != Drag::None && !ui.input(|i| i.pointer.primary_down()) {
            let end = ui.input(|i| i.pointer.latest_pos());
            match (std::mem::replace(&mut self.drag, Drag::None), end) {
                (Drag::Box { start, extend }, Some(end)) => {
                    let area = Rect::from_two_pos(start, self.view.to_world(rect, end));
                    if !extend {
                        self.selection.clear();
                    }
                    self.selected_wire = None;
                    for (i, shape) in self.shapes(catalog).iter().enumerate() {
                        if area.intersects(shape.rect) {
                            self.selection.add(i);
                        }
                    }
                }
                (Drag::Wire(anchor), Some(end)) if rect.contains(end) => {
                    match (anchor, self.hit(catalog, rect, end)) {
                        (Anchor::Output(from), Hit::Input(to))
                        | (Anchor::Input(to), Hit::Output(from)) => {
                            self.schematic.connect(from, to);
                            self.selected_wire = Some(self.schematic.wires.len() - 1);
                            self.selection.clear();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let Some(pointer) = pointer else { return };
        let world = self.view.to_world(rect, pointer);

        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
            match self.hit(catalog, rect, origin) {
                Hit::Output(p) => self.drag = Drag::Wire(Anchor::Output(p)),
                Hit::Input(p) => {
                    // Pick up the existing waveguide from its source, like node editors.
                    if let Some(w) = self.schematic.wire_into(p) {
                        let from = self.schematic.wires.remove(w).from;
                        self.drag = Drag::Wire(Anchor::Output(from));
                    } else {
                        self.drag = Drag::Wire(Anchor::Input(p));
                    }
                }
                Hit::Instance(i) => {
                    if !self.selection.contains(i) {
                        self.selection.select_only(i);
                    }
                    self.selected_wire = None;
                    self.begin_grab(self.view.to_world(rect, origin), true);
                    self.update_grab(world);
                }
                Hit::Wire(_) | Hit::Empty => {
                    self.drag = Drag::Box {
                        start: self.view.to_world(rect, origin),
                        extend: shift,
                    }
                }
            }
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            self.menu = None;
            match self.hit(catalog, rect, pointer) {
                Hit::Instance(i) => {
                    self.selected_wire = None;
                    if shift {
                        self.selection.toggle(i);
                    } else {
                        self.selection.select_only(i);
                    }
                    if response.double_clicked()
                        && let BlockRef::Component(id) = self.schematic.instances[i].block
                    {
                        self.requests.push(Request::Inspect(id));
                    }
                }
                Hit::Wire(w) => {
                    self.selection.clear();
                    self.selected_wire = Some(w);
                }
                Hit::Empty => {
                    if let Some(block) = self.tool.clone() {
                        self.place(catalog, block, world);
                    } else if !shift {
                        self.selection.clear();
                        self.selected_wire = None;
                    }
                }
                _ => {}
            }
        }

        if response.clicked_by(egui::PointerButton::Secondary) {
            let target = match self.hit(catalog, rect, pointer) {
                Hit::Instance(i) => {
                    if !self.selection.contains(i) {
                        self.selection.select_only(i);
                    }
                    Some(i)
                }
                _ => None,
            };
            self.open_menu(target);
        }
    }

    pub fn overlays(&mut self, ctx: &egui::Context, catalog: &Catalog) {
        let Some(mut menu) = self.menu.clone() else {
            return;
        };
        let focus_search = menu.opened_at == self.time;
        let mut choice: Option<MenuChoice> = None;
        let shown = egui::Area::new(Id::unique("schematic_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    if let Some(i) = menu.target.filter(|&i| i < self.schematic.instances.len()) {
                        let d = catalog.display(&self.schematic.instances[i].block);
                        ui.label(egui::RichText::new(&d.name).strong());
                        if let Some(id) = d.component
                            && ui.button("Open definition").clicked()
                        {
                            choice = Some(MenuChoice::Inspect(id));
                        }
                        if ui.button("Duplicate").clicked() {
                            choice = Some(MenuChoice::Duplicate);
                        }
                        if ui.button("Disconnect").clicked() {
                            choice = Some(MenuChoice::Disconnect(i));
                        }
                        if ui.button("Delete").clicked() {
                            choice = Some(MenuChoice::Delete);
                        }
                        return;
                    }
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut menu.search)
                            .hint_text("Add…")
                            .desired_width(f32::INFINITY),
                    );
                    if focus_search {
                        search.request_focus();
                    }
                    let items = Self::library_items(catalog, &menu.search);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (_, block) in &items {
                                if let (BlockRef::Component(id), Some(me)) = (block, self.doc_id)
                                    && !catalog.can_place(*id, me)
                                {
                                    continue;
                                }
                                let d = catalog.display(block);
                                if ui
                                    .selectable_label(
                                        false,
                                        format!("{:<4} {}", d.initials, d.name),
                                    )
                                    .clicked()
                                {
                                    choice = Some(MenuChoice::Add(block.clone()));
                                }
                            }
                        });
                    if search.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && let Some((_, block)) = items.first()
                    {
                        choice = Some(MenuChoice::Add(block.clone()));
                    }
                });
            });
        match choice {
            Some(c) => self.apply_menu_choice(catalog, c, menu.world),
            None => {
                let clicked_outside = menu.opened_at < self.time
                    && ctx.input(|i| i.pointer.any_pressed())
                    && !shown.response.contains_pointer();
                self.menu = if clicked_outside { None } else { Some(menu) };
            }
        }
    }

    pub fn apply_menu_choice(&mut self, catalog: &Catalog, choice: MenuChoice, world: Pos2) {
        self.menu = None;
        match choice {
            MenuChoice::Add(block) => {
                self.place(catalog, block, world);
            }
            MenuChoice::Inspect(id) => self.requests.push(Request::Inspect(id)),
            MenuChoice::Duplicate => self.apply(catalog, Action::Duplicate),
            MenuChoice::Delete => {
                self.selected_wire = None;
                self.delete_selected();
            }
            MenuChoice::Disconnect(i) => {
                if let Some(id) = self.schematic.instances.get(i).map(|inst| inst.id) {
                    self.schematic
                        .wires
                        .retain(|w| w.from.instance != id && w.to.instance != id);
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuChoice {
    Add(BlockRef),
    Inspect(DocId),
    Duplicate,
    Delete,
    Disconnect(usize),
}

fn badge_fill(d: &BlockDisplay) -> Color32 {
    if d.component.is_some() {
        ui_kit::accent(&d.name)
    } else {
        Color32::from_rgb(88, 96, 112)
    }
}

fn library_tooltip(d: &BlockDisplay) -> String {
    let ports = |names: &[String]| {
        if names.is_empty() {
            "—".to_owned()
        } else {
            names.join(", ")
        }
    };
    format!(
        "{}\n{}\n\nInputs: {}\nOutputs: {}",
        d.name,
        if d.description.is_empty() {
            "No description"
        } else {
            &d.description
        },
        ports(&d.inputs),
        ports(&d.outputs)
    )
}

fn draw_block(
    painter: &egui::Painter,
    r: Rect,
    block: &BlockRef,
    d: &BlockDisplay,
    theme: &ui_kit::CanvasTheme,
    zoom: f32,
) {
    let dark = theme.background.r() < 100;
    let body = if dark {
        Color32::from_rgb(43, 45, 50)
    } else {
        Color32::WHITE
    };
    let border = theme.major_line;
    let rounding = 5.0 * zoom.clamp(0.5, 1.5);
    match block {
        BlockRef::Builtin(Builtin::InputPin { name } | Builtin::OutputPin { name }) => {
            let fill = if geometry::is_input_pin(block) {
                Color32::from_rgb(0, 137, 123)
            } else {
                Color32::from_rgb(123, 31, 162)
            };
            painter.rect_filled(r, r.height() / 2.0, fill);
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                name,
                FontId::proportional((12.0 * zoom).clamp(6.0, 24.0)),
                Color32::WHITE,
            );
        }
        _ => {
            painter.rect(
                r,
                rounding,
                body,
                Stroke::new(1.0, border),
                egui::StrokeKind::Inside,
            );
            let text_color = if d.component.is_some() {
                let accent = ui_kit::accent(&d.name);
                // Components get an accent stripe so they read differently from built-in blocks.
                painter.rect_filled(
                    Rect::from_min_max(
                        r.left_top(),
                        egui::pos2(r.left() + 4.0 * zoom.clamp(0.5, 1.5), r.bottom()),
                    ),
                    egui::CornerRadius {
                        nw: rounding as u8,
                        sw: rounding as u8,
                        ne: 0,
                        se: 0,
                    },
                    accent,
                );
                accent
            } else {
                theme.text
            };
            let size = (15.0 * zoom).clamp(5.0, 40.0)
                * if d.initials.chars().count() > 2 {
                    0.85
                } else {
                    1.0
                };
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                &d.initials,
                FontId::proportional(size),
                text_color,
            );
        }
    }
}

fn draw_port(
    painter: &egui::Painter,
    at: Pos2,
    output: bool,
    connected: bool,
    hovered: bool,
    theme: &ui_kit::CanvasTheme,
) {
    let radius = if hovered { 6.0 } else { 4.5 };
    let color = if hovered {
        theme.active
    } else {
        theme.text.gamma_multiply(0.75)
    };
    if output || connected {
        painter.circle_filled(at, radius, color);
    } else {
        painter.circle(at, radius, theme.background, Stroke::new(1.5, color));
    }
}

/// Parameter widgets for a built-in block; returns whether anything changed.
pub fn builtin_params_ui(ui: &mut Ui, block: &mut Builtin) -> bool {
    let mut changed = false;
    let tau = std::f64::consts::TAU;
    egui::Grid::new("block_params")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let mut row =
                |ui: &mut Ui, label: &str, widget: &mut dyn FnMut(&mut Ui) -> egui::Response| {
                    ui.label(label);
                    changed |= widget(ui).changed();
                    ui.end_row();
                };
            match block {
                Builtin::Laser {
                    amplitude,
                    phase,
                    wavelength_nm,
                } => {
                    row(ui, "Amplitude", &mut |ui| {
                        ui.add(egui::Slider::new(amplitude, 0.0..=2.0))
                    });
                    row(ui, "Phase", &mut |ui| {
                        ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad"))
                    });
                    row(ui, "Wavelength", &mut |ui| {
                        ui.add(
                            egui::DragValue::new(wavelength_nm)
                                .range(200.0..=2000.0)
                                .speed(0.1)
                                .suffix(" nm"),
                        )
                    });
                }
                Builtin::InputPin { name } | Builtin::OutputPin { name } => {
                    row(ui, "Name", &mut |ui| ui.text_edit_singleline(name));
                }
                Builtin::Coupler { coupling } => {
                    row(ui, "Coupling", &mut |ui| {
                        ui.add(egui::Slider::new(coupling, 0.0..=1.0))
                    });
                }
                Builtin::PhaseShifter { phase } => {
                    row(ui, "Phase", &mut |ui| {
                        ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad"))
                    });
                }
                Builtin::Attenuator { loss_db } => {
                    row(ui, "Loss", &mut |ui| {
                        ui.add(
                            egui::DragValue::new(loss_db)
                                .range(0.0..=100.0)
                                .speed(0.1)
                                .suffix(" dB"),
                        )
                    });
                }
                Builtin::DelayLine { length_um, n_eff } => {
                    row(ui, "Length", &mut |ui| {
                        ui.add(
                            egui::DragValue::new(length_um)
                                .range(0.0..=1e6)
                                .speed(0.05)
                                .suffix(" µm"),
                        )
                    });
                    row(ui, "n_eff", &mut |ui| {
                        ui.add(egui::DragValue::new(n_eff).range(1.0..=4.0).speed(0.01))
                    });
                }
                Builtin::Mzi { theta, phi } => {
                    row(ui, "θ (internal)", &mut |ui| {
                        ui.add(egui::Slider::new(theta, 0.0..=tau).suffix(" rad"))
                    });
                    row(ui, "φ (input)", &mut |ui| {
                        ui.add(egui::Slider::new(phi, 0.0..=tau).suffix(" rad"))
                    });
                }
                Builtin::Ring {
                    coupling,
                    length_um,
                    n_eff,
                    loss_db_per_cm,
                }
                | Builtin::AddDropRing {
                    coupling,
                    length_um,
                    n_eff,
                    loss_db_per_cm,
                } => {
                    row(ui, "Coupling", &mut |ui| {
                        ui.add(egui::Slider::new(coupling, 0.0..=1.0))
                    });
                    row(ui, "Length", &mut |ui| {
                        ui.add(
                            egui::DragValue::new(length_um)
                                .range(0.1..=1e5)
                                .speed(0.01)
                                .suffix(" µm"),
                        )
                    });
                    row(ui, "n_eff", &mut |ui| {
                        ui.add(egui::DragValue::new(n_eff).range(1.0..=4.0).speed(0.01))
                    });
                    row(ui, "Loss", &mut |ui| {
                        ui.add(
                            egui::DragValue::new(loss_db_per_cm)
                                .range(0.0..=1000.0)
                                .speed(0.1)
                                .suffix(" dB/cm"),
                        )
                    });
                }
                Builtin::SplitterTree { levels } => {
                    row(ui, "Levels", &mut |ui| {
                        ui.add(egui::Slider::new(levels, 0..=5))
                    });
                }
                Builtin::SwitchTree { levels, address } => {
                    row(ui, "Levels", &mut |ui| {
                        ui.add(egui::Slider::new(levels, 1..=5))
                    });
                    *address = (*address).min((1 << *levels) - 1);
                    let max = (1 << *levels) - 1;
                    row(ui, "Address", &mut |ui| {
                        ui.add(egui::Slider::new(address, 0..=max))
                    });
                }
                Builtin::Dft { n } => {
                    row(ui, "Modes", &mut |ui| ui.add(egui::Slider::new(n, 2..=16)));
                }
                Builtin::Detector | Builtin::YSplitter | Builtin::YCombiner | Builtin::Crossing => {
                }
            }
        });
    changed
}

#[cfg(test)]
mod tests;
