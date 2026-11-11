//! The circuit editor: free-space optics, component boxes, waveguides and
//! electrical signals on one grid, with Blender-style controls.
//!
//! - [`model`] — layout data and file format
//! - [`view`] — pan/zoom transform
//! - [`editor`] — selection, grab, undo (no UI)
//! - [`input`] — keymap
//! - [`render`] — drawing primitives
//! - [`canvas`] — hit testing, drawing and mouse interaction on the grid
//! - [`library`] — the parts library, add menu and parameter widgets
//! - [`examples`] — ready-made layouts, components and demos

pub mod bridge;
pub mod canvas;
pub mod editor;
pub mod examples;
pub mod input;
pub mod library;
pub mod model;
pub mod reference;
pub mod render;
pub mod sweep_ui;
pub mod view;

use std::hash::{Hash, Hasher};

use egui::{Id, Pos2, Rect, Ui};

pub use model::{GRID, Layout, Line, OpticComponentKind, PlacedComponent};

use super::chips::ChipBrowser;
use super::circuit::sim::{self, Machine, NoDocuments, Options, Outcome};
use super::prefs::Preferences;
use super::project::{Catalog, DocId};
use super::ui_kit;
use editor::{Axis, Editor};
use input::{Action, SHORTCUTS, shortcut};
use model::{BlockRef, Builtin, Item, PortCounts, Terminal, box_geometry};
use view::View;

pub use crate::panels::Request;
pub use canvas::Hit;
pub use library::Placeable;

#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    Place(Placeable),
    DrawLine,
}

/// Last simulation and what it was computed from.
#[derive(Clone, Debug)]
pub struct SimCache {
    pub key: u64,
    pub outcome: Outcome,
}

/// Context menu (right click) or add menu (Shift+A).
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub screen: Pos2,
    pub world: Pos2,
    pub target: Option<Hit>,
    pub search: String,
    /// Category whose submenu is open, and the screen y of its row.
    pub open_section: Option<(usize, f32)>,
    opened_at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuChoice {
    Rotate(i32),
    Duplicate,
    Delete,
    Inspect,
    Sweep,
    Add(Placeable),
    DeleteLines,
    DeleteWire(WireRef),
}

/// A selected or hovered connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireRef {
    /// Index into [`Layout::wires`], then on into [`Layout::feeds`].
    Waveguide(usize),
    /// Index into [`Layout::signals`].
    Signal(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Box { start: Pos2, extend: bool },
    Line { start: Pos2 },
    /// A wire being drawn from a port; the other end follows the mouse.
    Connect(Hit),
}

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
    /// Library entry being dragged onto the canvas.
    pub dragging_new: Option<Placeable>,
    pub selected_wire: Option<WireRef>,
    pub simulation_active: bool,
    /// Physical size of one grid cell in micrometres.
    pub cell_um: f64,
    pub sim: Option<SimCache>,
    /// Register contents and clock count.
    pub machine: Machine,
    /// Clock ticking by itself.
    pub running: bool,
    last_tick: f64,
    /// Components of the project, set by the host each frame.
    pub catalog: Catalog,
    /// The document being edited, to refuse placing it inside itself.
    pub doc_id: Option<DocId>,
    pub search: String,
    /// How things look; set by the host each frame.
    pub prefs: Preferences,
    pub sweep: sweep_ui::SweepState,
    /// The categorised browser of every part and component.
    pub show_browser: bool,
    /// The script being edited in the inspector: its box, the text, and why
    /// the last Apply failed.
    pub script_edit: Option<(model::InstanceId, String, Option<String>)>,
    /// Folder shown in the browser, an index into [`library::SECTIONS`].
    pub browser_section: usize,
    pub browser_search: String,
    pub show_toolbar: bool,
    pub show_properties: bool,
    pub show_shortcuts: bool,
    pub show_examples: bool,
    pub examples_tab: ExamplesTab,
    pub menu: Option<Menu>,
    /// Offer to insert a converter after a light-to-signal drop.
    pub bridge: Option<bridge::Offer>,
    /// Transient message and the time it expires.
    pub status: Option<(String, f64)>,
    pub chips: ChipBrowser,
    /// Hosted inside the workspace: file actions become [`Request`]s.
    pub embedded: bool,
    pub requests: Vec<Request>,
    drag: Drag,
    /// A grab ended with a mouse press: ignore the click it completes.
    swallow_click: bool,
    canvas: Option<Rect>,
    /// The canvas widget, which takes keyboard focus when clicked.
    canvas_id: Option<Id>,
    /// Wiring from the keyboard: the item the wire starts at.
    wiring_from: Option<Item>,
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
            selected_wire: None,
            simulation_active: true,
            cell_um: 10.0,
            sim: None,
            machine: Machine::default(),
            running: false,
            last_tick: 0.0,
            catalog: Catalog::default(),
            doc_id: None,
            search: String::new(),
            prefs: Preferences::default(),
            sweep: sweep_ui::SweepState::default(),
            show_browser: false,
            script_edit: None,
            browser_section: 0,
            browser_search: String::new(),
            show_toolbar: true,
            show_properties: true,
            show_shortcuts: false,
            show_examples: false,
            examples_tab: ExamplesTab::Layouts,
            menu: None,
            bridge: None,
            status: None,
            chips: ChipBrowser::default(),
            embedded: false,
            requests: Vec::new(),
            drag: Drag::None,
            swallow_click: false,
            canvas: None,
            canvas_id: None,
            wiring_from: None,
            mouse_screen: None,
            mouse_world: None,
            frame_pending: false,
            time: 0.0,
        }
    }
}

/// Order-independent hash of register contents and the clock.
fn hash_machine(machine: &Machine, hasher: &mut impl Hasher) {
    machine.tick.hash(hasher);
    for v in &machine.inputs {
        v.to_bits().hash(hasher);
    }
    let mut entries: u64 = 0;
    for ((path, id), bits) in &machine.registers {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        id.hash(&mut h);
        for b in bits {
            b.to_bits().hash(&mut h);
        }
        entries = entries.wrapping_add(h.finish());
    }
    entries.hash(hasher);
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

    /// Port counts of a block, for sizing boxes.
    pub fn counts(&self, block: &BlockRef) -> PortCounts {
        self.catalog.counts(block)
    }

    // ---- simulation -------------------------------------------------------

    /// Simulation key for this document inside a project with the given
    /// fingerprint (which covers every document's layout and scale).
    pub fn simulation_key(&self, project_fingerprint: u64) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        project_fingerprint.hash(&mut h);
        hash_machine(&self.machine, &mut h);
        h.finish()
    }

    /// Key for a panel on its own: the layout, scale and machine.
    fn standalone_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        postcard::to_allocvec(&self.layout).expect("serialisable").hash(&mut h);
        self.cell_um.to_bits().hash(&mut h);
        hash_machine(&self.machine, &mut h);
        h.finish()
    }

    pub fn is_stale(&self, key: u64) -> bool {
        self.sim.as_ref().is_none_or(|s| s.key != key)
    }

    pub fn set_outcome(&mut self, key: u64, outcome: Outcome) {
        if let Some(e) = &outcome.error {
            tracing::warn!("Simulation failed: {e}");
        }
        self.sim = Some(SimCache { key, outcome });
    }

    /// Re-solve a standalone layout (no components from a project) when it changed.
    pub fn refresh_simulation(&mut self) {
        let key = self.standalone_key();
        if !self.is_stale(key) {
            return;
        }
        let start = web_time::Instant::now();
        let outcome = sim::simulate(&self.layout, self.cell_um, &NoDocuments, &self.machine, None, &Options::default());
        tracing::debug!(
            "Simulated {} parts and {} boxes in {:?} ({} rounds)",
            self.layout.components.len(),
            self.layout.instances.len(),
            start.elapsed(),
            outcome.iterations
        );
        self.set_outcome(key, outcome);
    }

    /// The current simulation result, when simulating.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.sim.as_ref().filter(|_| self.simulation_active).map(|s| &s.outcome)
    }

    pub fn detector_reading(&self, index: usize) -> Option<f64> {
        self.outcome()?.reading(index)
    }

    /// Advance the clock: registers store their inputs. Embedded panels ask
    /// the host, which knows the components.
    pub fn tick(&mut self) {
        if self.embedded {
            self.requests.push(Request::Tick);
            return;
        }
        self.refresh_simulation();
        let outcome = self.sim.as_ref().map(|s| s.outcome.clone()).unwrap_or_default();
        sim::clock(&self.layout, &NoDocuments, &mut self.machine, &outcome);
    }

    /// Clear registers and the clock (pin values stay).
    pub fn reset_clock(&mut self) {
        self.machine = Machine {
            inputs: std::mem::take(&mut self.machine.inputs),
            ..Machine::default()
        };
        self.running = false;
    }

    /// Value set by hand on signal-in pin part `part`, if it is one.
    pub fn pin_value(&self, part: usize) -> Option<f64> {
        let k = sim::interface(&self.layout).signal_in.iter().position(|&i| i == part)?;
        Some(self.machine.inputs.get(k).copied().unwrap_or(0.0))
    }

    /// Flip a signal-in pin between 0 and 1. Returns false for other parts.
    pub fn toggle_pin(&mut self, part: usize) -> bool {
        let pins = sim::interface(&self.layout).signal_in;
        let Some(k) = pins.iter().position(|&i| i == part) else { return false };
        self.machine.inputs.resize(pins.len().max(self.machine.inputs.len()), 0.0);
        let v = &mut self.machine.inputs[k];
        *v = if *v >= 0.5 { 0.0 } else { 1.0 };
        true
    }

    // ---- documents --------------------------------------------------------

    pub fn new_layout(&mut self) {
        self.editor.cancel_grab(&mut self.layout);
        self.layout = Layout::default();
        self.editor.clear_selection();
        self.selected_wire = None;
        self.reset_clock();
        self.view = View::default();
        self.notify("New layout");
    }

    /// Show the canvas at `view` (a restored session), instead of framing
    /// the layout when it's first drawn.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.frame_pending = false;
    }

    /// Replace the layout (loading a file or an example).
    pub fn set_layout(&mut self, layout: Layout, cell_um: f64) {
        self.editor.cancel_grab(&mut self.layout);
        self.layout = layout;
        self.cell_um = cell_um;
        self.editor.clear_selection();
        self.selected_wire = None;
        self.reset_clock();
        self.frame_pending = true;
    }

    /// Save as a Shine project holding just this layout.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let mut project = crate::panels::project::Project::new();
        project.add_layout("Layout", self.layout.clone(), self.cell_um);
        std::fs::write(path, project.to_bytes()).map_err(|e| e.to_string())
    }

    /// Load the first document of a Shine project, or a JSON layout from an earlier version.
    pub fn load_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let (layout, cell_um) = if crate::panels::project::Project::is_project(&bytes) {
            let project = crate::panels::project::Project::from_bytes(&bytes)?;
            let doc = project.documents.first().ok_or("the project is empty")?;
            (doc.layout().clone(), Some(doc.panel().cell_um))
        } else {
            model::from_legacy_json(&String::from_utf8_lossy(&bytes))?
        };
        self.set_layout(layout, cell_um.unwrap_or(self.cell_um));
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
            .add_filter("Shine project or JSON layout", &[super::file_format::EXTENSION, "json"])
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
        self.set_layout((example.build)(), example.cell_um);
        self.simulation_active = true;
        self.notify(format!("Loaded example: {}", example.name));
    }

    // ---- actions ----------------------------------------------------------

    /// Bounds of everything, or of the selection, in world units.
    pub fn bounds(&self, selected_only: bool) -> Option<Rect> {
        let parts = if selected_only {
            self.layout.bounds(self.editor.selection.iter())
        } else {
            self.layout.all_bounds()
        };
        let boxes = self
            .layout
            .instances
            .iter()
            .enumerate()
            .filter(|(i, _)| !selected_only || self.editor.boxes.contains(*i))
            .map(|(_, inst)| box_geometry(inst.cell, self.counts(&inst.block)).rect.expand(GRID * 0.5))
            .reduce(|a, b| a.union(b));
        match (parts, boxes) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        }
    }

    fn frame(&mut self, selected_only: bool) {
        match (self.canvas, self.bounds(selected_only)) {
            (Some(canvas), Some(bounds)) => self.view.frame(canvas, bounds, 40.0),
            (_, None) if !selected_only => self.view = View::default(),
            (None, Some(_)) => self.frame_pending = true,
            (_, None) => {}
        }
    }

    fn confirm_grab(&mut self) {
        let catalog = &self.catalog;
        if let Err(e) = self.editor.confirm_grab(&mut self.layout, &|b| catalog.counts(b)) {
            self.notify(format!("Move cancelled: {e}"));
        }
    }

    /// The component behind the selected box, if any.
    pub fn selected_component(&self) -> Option<DocId> {
        let i = self.editor.boxes.active()?;
        match self.layout.instances.get(i)?.block {
            BlockRef::Component(id) => Some(id),
            BlockRef::Builtin(_) => None,
        }
    }

    pub fn delete_wire(&mut self, wire: WireRef) {
        match wire {
            WireRef::Waveguide(w) if w < self.layout.wires.len() => {
                self.layout.wires.remove(w);
            }
            WireRef::Waveguide(w) if w - self.layout.wires.len() < self.layout.feeds.len() => {
                self.layout.feeds.remove(w - self.layout.wires.len());
            }
            WireRef::Signal(w) if w < self.layout.signals.len() => {
                self.layout.signals.remove(w);
            }
            _ => {}
        }
        self.selected_wire = None;
    }

    pub fn apply(&mut self, action: Action) {
        let mouse = self
            .mouse_world
            .or_else(|| self.canvas.map(|c| self.view.to_world(c, c.center())))
            .unwrap_or(Pos2::ZERO);
        match action {
            Action::SelectAll => self.editor.select_all(&self.layout),
            Action::DeselectAll => {
                self.editor.clear_selection();
                self.selected_wire = None;
            }
            Action::InvertSelection => self.editor.invert(&self.layout),
            Action::Delete => {
                if let Some(w) = self.selected_wire {
                    self.delete_wire(w);
                    self.notify("Deleted the connection");
                    return;
                }
                let n = self.editor.delete_selected(&mut self.layout);
                if n > 0 {
                    self.notify(format!("Deleted {n} item(s)"));
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
            Action::ToggleProperties if self.embedded => self.requests.push(Request::ToggleInspector),
            Action::ShowShortcuts if self.embedded => self.requests.push(Request::ShowShortcuts),
            Action::ToggleToolbar => self.show_toolbar = !self.show_toolbar,
            Action::ToggleProperties => self.show_properties = !self.show_properties,
            Action::ShowShortcuts => self.show_shortcuts = !self.show_shortcuts,
            Action::Inspect => match self.selected_component() {
                Some(id) if self.embedded => self.requests.push(Request::Inspect(id)),
                Some(_) => self.notify("Components open in the workspace"),
                None => self.notify("Select a component box to open it"),
            },
            Action::AddMenu => self.open_menu(None),
            Action::Save if self.embedded => self.requests.push(Request::Save),
            Action::Open if self.embedded => self.requests.push(Request::Open),
            Action::New if self.embedded => self.requests.push(Request::New),
            Action::Save => self.save_dialog(),
            Action::Open => self.open_dialog(),
            Action::New => self.new_layout(),
            Action::ConstrainX => self.editor.constrain(Axis::X),
            Action::ConstrainY => self.editor.constrain(Axis::Y),
            Action::Confirm if self.wiring_from.is_some() => self.finish_keyboard_wire(),
            Action::Confirm => self.confirm_grab(),
            Action::Wire => match self.wiring_from {
                Some(_) => self.finish_keyboard_wire(),
                None => match self.selected_item() {
                    Some(item) => {
                        self.wiring_from = Some(item);
                        self.notify(format!(
                            "Wiring from {}: select where it goes with ] / [, then Enter (Esc cancels)",
                            self.item_name(item)
                        ));
                    }
                    None => self.notify("Select the part or box to wire from"),
                },
            },
            Action::Nudge(dx, dy) => {
                if self.editor.grab.is_some() {
                    self.editor.nudge_grab(&mut self.layout, (dx, dy));
                } else if !self.editor.is_empty() {
                    // A grab moved by the keys and confirmed: the same checks
                    // for room as moving with the mouse.
                    self.editor.begin_grab(&self.layout, mouse, false);
                    self.editor.nudge_grab(&mut self.layout, (dx, dy));
                    self.confirm_grab();
                } else {
                    // The view moves the other way, like scrolling.
                    self.view.pan_by(-egui::vec2(dx as f32, dy as f32) * 3.0 * GRID * self.view.zoom);
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                if let Some(canvas) = self.canvas {
                    let factor = if action == Action::ZoomIn { 1.25 } else { 0.8 };
                    self.view.zoom_at(canvas, canvas.center(), factor);
                }
            }
            Action::SelectNext => self.select_step(1),
            Action::SelectPrevious => self.select_step(-1),
            Action::Cancel => {
                if self.wiring_from.take().is_some() {
                    self.notify("Wiring cancelled");
                } else if self.editor.grab.is_some() {
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

    /// The active part or box.
    fn selected_item(&self) -> Option<Item> {
        match (self.editor.selection.active(), self.editor.boxes.active()) {
            (_, Some(b)) => self.layout.instances.get(b).map(|i| Item::Box(i.id)),
            (Some(i), None) => Some(Item::Part(i)),
            _ => None,
        }
    }

    /// Finish a keyboard wire at the selected item: the first free output of
    /// the start that matches a free input of the end, light before signals.
    fn finish_keyboard_wire(&mut self) {
        let Some(from) = self.wiring_from.take() else { return };
        let Some(to) = self.selected_item() else {
            self.notify("Select the part or box to wire to");
            self.wiring_from = Some(from);
            return;
        };
        let layout = &self.layout;
        let counts = |item: Item| match item {
            Item::Box(id) => layout.instance(id).map(|inst| self.catalog.counts(&inst.block)).unwrap_or_default(),
            Item::Part(i) => layout.components.get(i).map_or_else(PortCounts::default, |c| PortCounts {
                signal_in: c.kind.signal_inputs().len(),
                signal_out: c.kind.signal_outputs().len(),
                ..Default::default()
            }),
        };
        let (out, inp) = (counts(from), counts(to));
        // Light: a laser's output or a box's light outputs, into a box's light inputs.
        let light_from: Vec<Hit> = match from {
            Item::Part(i) if matches!(layout.components.get(i).map(|c| c.kind), Some(OpticComponentKind::Laser { .. })) => {
                if layout.feed_of(i).is_none() { vec![Hit::LaserOut(i)] } else { Vec::new() }
            }
            Item::Box(id) => (0..out.optical_out)
                .map(|port| model::PortRef { instance: id, port })
                .filter(|p| !layout.wires.iter().any(|w| w.from == *p))
                .map(Hit::LightOut)
                .collect(),
            _ => Vec::new(),
        };
        let light_to: Option<Hit> = match to {
            Item::Box(id) => (0..inp.optical_in)
                .map(|port| model::PortRef { instance: id, port })
                .find(|p| !layout.wires.iter().any(|w| w.to == *p) && !layout.feeds.iter().any(|f| f.to == *p))
                .map(Hit::LightIn),
            Item::Part(_) => None,
        };
        // Signals: an output may drive many inputs, but prefer an unused one.
        let mut signal_from: Vec<Terminal> = (0..out.signal_out).map(|port| Terminal { item: from, port }).collect();
        signal_from.sort_by_key(|t| layout.signals.iter().any(|w| w.from == *t));
        let signal_to = (0..inp.signal_in).map(|port| Terminal { item: to, port }).find(|t| layout.signal_source(*t).is_none());
        let pair = match (light_from.first(), light_to, signal_from.first(), signal_to) {
            (Some(&a), Some(b), _, _) => Some((a, b)),
            (_, _, Some(&a), Some(b)) => Some((Hit::SignalOut(a), Hit::SignalIn(b))),
            _ => None,
        };
        let (from_name, to_name) = (self.item_name(from), self.item_name(to));
        match pair {
            Some((a, b)) if from != to => {
                self.connect(a, b);
                self.notify(format!("Wired {from_name} to {to_name}"));
            }
            Some(_) => self.notify("Choose another item to wire to"),
            None => self.notify(format!("No free matching ports from {from_name} to {to_name}")),
        }
    }

    /// Select the part or box after (or before) the active one, in order:
    /// parts, then boxes. Brings it into view.
    fn select_step(&mut self, step: i32) {
        let parts = self.layout.components.len();
        let total = parts + self.layout.instances.len();
        if total == 0 {
            self.notify("Nothing to select");
            return;
        }
        let current = match (self.editor.selection.active(), self.editor.boxes.active()) {
            (Some(i), _) if self.editor.boxes.is_empty() => Some(i),
            (_, Some(b)) => Some(parts + b),
            _ => None,
        };
        let next = match current {
            Some(k) => (k as i32 + step).rem_euclid(total as i32) as usize,
            None if step > 0 => 0,
            None => total - 1,
        };
        self.editor.clear_selection();
        self.selected_wire = None;
        if next < parts {
            self.editor.selection.select_only(next);
        } else {
            self.editor.boxes.select_only(next - parts);
        }
        if let (Some(canvas), Some(bounds)) = (self.canvas, self.bounds(true))
            && !self.view.visible(canvas).contains_rect(bounds)
        {
            self.view.pan = canvas.size() / 2.0 - bounds.center().to_vec2() * self.view.zoom;
        }
    }

    fn open_menu(&mut self, target: Option<Hit>) {
        // From the keyboard, with the pointer elsewhere: next to the
        // selection, or in the middle of the view.
        let fallback = self.canvas.map(|canvas| {
            let world = self.bounds(true).map_or_else(|| self.view.to_world(canvas, canvas.center()), |b| b.right_center() + egui::vec2(GRID, 0.0));
            (self.view.to_screen(canvas, world), world)
        });
        let Some((screen, world)) = self.mouse_screen.zip(self.mouse_world).or(fallback) else {
            return;
        };
        self.menu = Some(Menu {
            screen,
            world,
            target,
            search: String::new(),
            open_section: None,
            opened_at: self.time,
        });
    }

    /// Put a library entry at `world`.
    pub fn place(&mut self, item: Placeable, world: Pos2) {
        let catalog = &self.catalog;
        let counts = |b: &BlockRef| catalog.counts(b);
        let placed = match item {
            Placeable::Part(mut kind) => {
                if let OpticComponentKind::Laser { wavelength, .. } = &mut kind {
                    *wavelength = self.prefs.default_wavelength_nm;
                }
                self.editor.place(&mut self.layout, kind, world, &counts).is_some()
            }
            Placeable::Block(BlockRef::Component(id)) if self.doc_id.is_some_and(|me| !catalog.can_place(id, me)) => {
                self.notify("A component can't contain itself");
                return;
            }
            Placeable::Block(block) => self.editor.place_block(&mut self.layout, block, world, &counts).is_some(),
            Placeable::Asset(asset) => {
                if self.embedded {
                    self.requests.push(Request::PlaceAsset { asset, cell: model::cell_of(world) });
                } else {
                    self.notify("Library components need a project: use the workspace");
                }
                return;
            }
        };
        if !placed {
            self.notify("Not enough room there");
        }
    }

    pub fn apply_menu_choice(&mut self, choice: MenuChoice, world: Pos2) {
        match choice {
            MenuChoice::Rotate(steps) => self.editor.rotate_selected(&mut self.layout, steps),
            MenuChoice::Duplicate => self.apply(Action::Duplicate),
            MenuChoice::Delete => self.apply(Action::Delete),
            MenuChoice::Inspect => self.apply(Action::Inspect),
            MenuChoice::Sweep => {
                let item = match self.menu.as_ref().and_then(|m| m.target) {
                    Some(Hit::Part(i)) => Some(Item::Part(i)),
                    Some(Hit::Box(b)) => self.layout.instances.get(b).map(|i| Item::Box(i.id)),
                    _ => None,
                };
                self.open_sweep(item);
            }
            MenuChoice::Add(item) => self.place(item, world),
            MenuChoice::DeleteLines => {
                let cell = model::cell_of(world);
                self.layout
                    .lines
                    .retain(|l| model::cell_of(l.start) != cell && model::cell_of(l.end) != cell);
            }
            MenuChoice::DeleteWire(w) => self.delete_wire(w),
        }
        self.menu = None;
    }

    /// Drop connections to ports that no longer exist (after a block's
    /// parameters changed its port count).
    pub fn prune_connections(&mut self) {
        let catalog = &self.catalog;
        let layout = &self.layout;
        let fits = |t: &Terminal, output: bool| match t.item {
            Item::Part(i) => layout.components.get(i).is_some_and(|c| {
                t.port
                    < if output {
                        c.kind.signal_outputs().len()
                    } else {
                        c.kind.signal_inputs().len()
                    }
            }),
            Item::Box(id) => layout.instance(id).is_some_and(|inst| {
                let c = catalog.counts(&inst.block);
                t.port < if output { c.signal_out } else { c.signal_in }
            }),
        };
        let signals: Vec<_> = layout.signals.iter().filter(|w| fits(&w.from, true) && fits(&w.to, false)).copied().collect();
        let light_fits = |p: &model::PortRef, output: bool| {
            layout.instance(p.instance).is_some_and(|inst| {
                let c = catalog.counts(&inst.block);
                p.port < if output { c.optical_out } else { c.optical_in }
            })
        };
        let wires: Vec<_> = layout.wires.iter().filter(|w| light_fits(&w.from, true) && light_fits(&w.to, false)).cloned().collect();
        let is_laser = |i: usize| matches!(layout.components.get(i).map(|c| c.kind), Some(model::OpticComponentKind::Laser { .. }));
        let feeds: Vec<_> = layout.feeds.iter().filter(|f| is_laser(f.laser) && light_fits(&f.to, false)).copied().collect();
        if signals.len() != self.layout.signals.len() || wires.len() != self.layout.wires.len() || feeds.len() != self.layout.feeds.len() {
            self.layout.signals = signals;
            self.layout.wires = wires;
            self.layout.feeds = feeds;
            self.selected_wire = None;
        }
    }

    // ---- UI ---------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.begin_frame(ui);
        if self.show_toolbar {
            egui::Panel::left("optics_toolbar")
                .resizable(false)
                .exact_size(220.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.palette_ui(ui));
                });
        }
        if self.show_properties {
            egui::Panel::right("optics_properties").default_size(270.0).show(ui, |ui| {
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

    /// Per-frame bookkeeping before any part of the editor is drawn. A
    /// standalone panel simulates here; an embedded one is simulated by its
    /// host right after.
    pub fn begin_frame(&mut self, ui: &Ui) {
        self.time = ui.input(|i| i.time);
        if self.status.as_ref().is_some_and(|s| s.1 < self.time) {
            self.status = None;
        }
        self.handle_keys(ui);
        if self.running {
            if self.time - self.last_tick >= self.prefs.tick_seconds {
                self.last_tick = self.time;
                self.tick();
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(self.prefs.tick_seconds));
        }
        if self.simulation_active && !self.embedded {
            self.refresh_simulation();
        }
    }

    /// Records one undo step per settled interaction.
    pub fn end_frame(&mut self, ui: &Ui) {
        if self.editor.grab.is_none() && !ui.input(|i| i.pointer.any_down()) {
            self.editor.history.commit(&self.layout);
        }
    }

    /// Floating UI drawn above everything (context and add menus).
    pub fn overlays(&mut self, ctx: &egui::Context) {
        self.menu_ui(ctx);
        self.bridge_ui(ctx);
        self.browser_ui(ctx);
        self.sweep_ui(ctx);
    }

    /// One line for the status bar: transient message, or mode and hints.
    pub fn status_text(&self) -> String {
        match &self.status {
            Some((message, _)) => message.clone(),
            None => self.header_text(),
        }
    }

    pub fn handle_keys(&mut self, ui: &Ui) {
        for (key, modifiers) in ui_kit::shortcut_keys(ui.ctx(), self.canvas_id) {
            if let Some(action) = shortcut(key, modifiers, self.editor.grab.is_some()) {
                // Tab opens a selected component; otherwise it moves the
                // keyboard focus on to the next control, as everywhere.
                if action == Action::Inspect {
                    if self.selected_component().is_none() {
                        continue;
                    }
                    ui_kit::keep_focus_on_tab(ui.ctx());
                }
                self.apply(action);
            }
        }
    }

    /// What a screen reader says for the canvas: what's on it and what's
    /// selected, as `]` / `[` step through it.
    pub fn canvas_description(&self) -> String {
        let mut text = format!(
            "Canvas, {} parts and {} boxes",
            self.layout.components.len(),
            self.layout.instances.len()
        );
        let selected = match (self.editor.selection.active(), self.editor.boxes.active()) {
            (_, Some(b)) => self.layout.instances.get(b).map(|inst| {
                format!("{} at cell {}, {}", self.catalog.display(&inst.block).name, inst.cell.0, inst.cell.1)
            }),
            (Some(i), None) => self.layout.components.get(i).map(|c| {
                let (ci, cj) = model::cell_of(c.pos);
                format!("{} at cell {ci}, {cj}", c.kind.name())
            }),
            _ => None,
        };
        if let Some(from) = self.wiring_from {
            text += &format!(". Wiring from {}", self.item_name(from));
        }
        match (selected, self.editor.len()) {
            (Some(item), 1) => text += &format!(". Selected: {item}"),
            (Some(item), n) => text += &format!(". {n} selected, last {item}"),
            (None, _) => text += ". Nothing selected; ] selects the first item, Shift+A adds a part",
        }
        text
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
        if let Drag::Connect(from) = self.drag {
            return match from {
                Hit::LightOut(_) | Hit::LightIn(_) | Hit::LaserOut(_) => "Drop on a light port to connect with a waveguide".to_owned(),
                _ => "Drop on a signal terminal to connect".to_owned(),
            };
        }
        let cell = self
            .mouse_world
            .map(|w| {
                let (i, j) = model::cell_of(w);
                format!("  ·  cell {i}, {j}")
            })
            .unwrap_or_default();
        let tool = match &self.tool {
            Some(Tool::Place(item)) => format!("  ·  placing {}", item.name(&self.catalog)),
            Some(Tool::DrawLine) => "  ·  line tool".to_owned(),
            None => String::new(),
        };
        let clock = if self.machine.tick > 0 || self.running {
            format!("  ·  tick {}", self.machine.tick)
        } else {
            String::new()
        };
        format!(
            "{:.0}%{cell}  ·  {} selected{tool}{clock}  ·  F1 shortcuts",
            self.view.zoom * 100.0,
            self.editor.len()
        )
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.show_shortcuts;
        crate::panels::ui_kit::window(ctx, "Keyboard & mouse")
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
        crate::panels::ui_kit::window(ctx, "Examples library")
            .id(Id::unique("optics_examples"))
            .open(&mut open)
            .default_size([760.0, 520.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.examples_tab, ExamplesTab::Layouts, "Free-space layouts");
                    ui.selectable_value(&mut self.examples_tab, ExamplesTab::Chips, "Chip circuits");
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

/// Number on a box's first `bits` signal inputs (MSB first).
pub fn input_number(layout: &Layout, outcome: &Outcome, id: model::InstanceId, bits: usize) -> u64 {
    let values: Vec<f64> = (0..bits).map(|k| outcome.input(layout, Terminal { item: Item::Box(id), port: k })).collect();
    sim::bits_to_number(&values)
}

/// Number on a box's first `bits` signal outputs (MSB first).
pub fn output_number(outcome: &Outcome, id: model::InstanceId, bits: usize) -> u64 {
    let values: Vec<f64> = (0..bits).map(|k| outcome.signal(Item::Box(id), k)).collect();
    sim::bits_to_number(&values)
}

/// Whether a block is one of the electronics blocks that show a number.
pub fn shows_number(block: &BlockRef) -> bool {
    matches!(
        block,
        BlockRef::Builtin(Builtin::NumberInput { .. } | Builtin::Display { .. } | Builtin::Register { .. })
    )
}

#[cfg(test)]
mod tests;
