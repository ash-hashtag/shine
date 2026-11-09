//! The application shell: menu bar, document tabs, library, inspector and status bar.

use std::path::Path;

use egui::{Color32, Key, Ui};

use super::chips::ChipBrowser;
use super::file_format;
use super::circuit::sim;
use super::optics::examples::{self, demos, layout_examples};
use super::optics::input::{Action, SHORTCUTS};
use super::optics::model::{self, BlockRef, Layout, OpticComponentKind, PlacedComponent, cell_center};
use super::optics::{OpticsPanel, Placeable};
use super::project::{Catalog, ComponentMeta, DocId, Project};
use super::prefs::{LightColors, PowerUnit, Preferences, Theme, WAVELENGTH_PRESETS};
use super::{Request, ui_kit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExamplesTab {
    Demos,
    Layouts,
    Chips,
}

/// Files (name, bytes) handed over from asynchronous browser file pickers.
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// "Save as component" / rename dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentDialog {
    pub doc: DocId,
    pub name: String,
    pub initials: String,
    pub description: String,
    /// Initials follow the name until edited by hand.
    pub initials_edited: bool,
    /// Publish (or keep) the document as a component; otherwise only rename.
    pub component: bool,
}

/// Above this, a simulation is too slow to repeat on every mouse move.
const SMOOTH_DRAG_BUDGET_MS: f64 = 8.0;
/// How often a slow design re-simulates while something is being dragged.
const DRAG_SIMULATIONS_PER_SECOND: f64 = 10.0;

pub use super::perf::FrameStats;

pub struct Workspace {
    pub project: Project,
    pub tabs: Vec<DocId>,
    pub active: Option<DocId>,
    pub show_library: bool,
    pub show_inspector: bool,
    pub show_shortcuts: bool,
    pub show_examples: bool,
    pub examples_tab: ExamplesTab,
    pub chips: ChipBrowser,
    pub dialog: Option<DocumentDialog>,
    pub status: Option<(String, f64)>,
    /// Files read asynchronously (browser file picker), opened next frame.
    pub incoming: Inbox,
    /// How things look and behave; saved between sessions by the app.
    pub prefs: Preferences,
    pub show_preferences: bool,
    /// How long frames take, for the status bar.
    pub stats: FrameStats,
    /// The project's fingerprint when it was last saved or opened.
    saved_fingerprint: u64,
    /// A project waiting to be opened until the user decides about unsaved changes.
    pub pending_open: Option<PendingOpen>,
    /// Window title last set (desktop).
    title: String,
    /// The window is closing (desktop): the session is saved as a clean exit.
    pub closing: bool,
    /// The report of a crash in the last run (desktop), shown until dismissed.
    pub crash_report: Option<String>,
    /// Help → About.
    pub about: super::about::About,
    /// An image to save from the next screenshot: file name and area (points).
    pub pending_image: Option<(&'static str, egui::Rect)>,
    /// Where the screenshot lands when it's taken.
    pub screenshot: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
    /// Theme last applied to the context.
    applied_theme: Option<Theme>,
    ctx: Option<egui::Context>,
    time: f64,
}

impl Default for Workspace {
    fn default() -> Self {
        let mut ws = Self {
            project: Project::new(),
            tabs: Vec::new(),
            active: None,
            show_library: true,
            show_inspector: true,
            show_shortcuts: false,
            show_examples: false,
            examples_tab: ExamplesTab::Demos,
            chips: ChipBrowser::default(),
            dialog: None,
            status: None,
            incoming: Default::default(),
            prefs: Preferences::default(),
            show_preferences: false,
            stats: FrameStats::default(),
            saved_fingerprint: 0,
            pending_open: None,
            title: String::new(),
            closing: false,
            crash_report: None,
            about: Default::default(),
            pending_image: None,
            screenshot: Default::default(),
            applied_theme: None,
            ctx: None,
            time: 0.0,
        };
        ws.new_layout();
        ws.mark_saved();
        ws
    }
}

/// A project file to open once the user has decided what to do with unsaved
/// changes.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingOpen {
    pub name: String,
    pub bytes: Vec<u8>,
    pub path: Option<std::path::PathBuf>,
}

/// What to do with unsaved changes before opening another project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

impl Workspace {
    pub fn notify(&mut self, message: impl Into<String>) {
        let message = message.into();
        tracing::info!("{message}");
        self.status = Some((message, self.time + 4.0));
    }

    // ---- documents and tabs ------------------------------------------------

    pub fn open_tab(&mut self, id: DocId) {
        if self.project.doc(id).is_none() {
            return;
        }
        if !self.tabs.contains(&id) {
            self.tabs.push(id);
        }
        self.active = Some(id);
    }

    pub fn close_tab(&mut self, id: DocId) {
        let Some(pos) = self.tabs.iter().position(|&t| t == id) else {
            return;
        };
        self.tabs.remove(pos);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(pos.min(self.tabs.len().saturating_sub(1)))
                .copied();
        }
    }

    pub fn new_layout(&mut self) -> DocId {
        let id = self.project.add_layout("Layout", Layout::default(), 10.0);
        self.project.doc_mut(id).expect("just added").panel_mut().simulation_active = self.prefs.live_simulation;
        self.open_tab(id);
        id
    }

    /// A new component with one light input and output and one signal input
    /// and output, ready to fill; opens its details dialog.
    pub fn new_component(&mut self) -> DocId {
        let mut layout = Layout::with_components(vec![
            PlacedComponent { kind: OpticComponentKind::InputPort { dir: egui::Vec2::new(1.0, 0.0) }, pos: cell_center(0, 0) },
            PlacedComponent { kind: OpticComponentKind::OutputPort, pos: cell_center(8, 0) },
            PlacedComponent { kind: OpticComponentKind::SignalIn, pos: cell_center(0, -3) },
            PlacedComponent { kind: OpticComponentKind::SignalOut, pos: cell_center(0, 3) },
        ]);
        for (i, label) in ["in", "out", "s", "q"].into_iter().enumerate() {
            layout.set_label(i, label);
        }
        let name = self.project.unique_name("Component");
        let id = self.project.add_component(&name, &ui_kit::initials(&name), "", layout, 10.0);
        self.open_tab(id);
        self.open_document_dialog(id, true);
        id
    }

    pub fn delete_document(&mut self, id: DocId) {
        let name = self
            .project
            .doc(id)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        match self.project.remove(id) {
            Ok(()) => {
                self.close_tab(id);
                self.notify(format!("Deleted “{name}”"));
            }
            Err(e) => self.notify(format!("Can't delete “{name}”: {e}")),
        }
    }

    pub fn load_layout_example(&mut self, index: usize) {
        let Some(example) = layout_examples().into_iter().nth(index) else {
            return;
        };
        let id = self
            .project
            .add_layout(example.name, (example.build)(), example.cell_um);
        self.open_tab(id);
        self.notify(format!("Opened example “{}”", example.name));
    }

    /// Open a demo circuit (adding the components it uses).
    pub fn load_demo(&mut self, index: usize) {
        let Some(example) = demos().into_iter().nth(index) else {
            return;
        };
        let id = (example.build)(&mut self.project);
        self.open_tab(id);
        self.notify(format!("Opened example “{}”", example.name));
    }

    // ---- files ------------------------------------------------------------

    /// Save to `path`. The file is written next to it under a temporary name
    /// and then renamed over it, so a failed save never leaves a damaged file.
    pub fn save_project_to(&mut self, path: &Path) -> Result<(), String> {
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".saving");
        let temporary = std::path::PathBuf::from(temporary);
        let written = std::fs::write(&temporary, self.project.to_bytes()).and_then(|()| std::fs::rename(&temporary, path));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(e.to_string());
        }
        self.project.path = Some(path.to_owned());
        self.mark_saved();
        Ok(())
    }

    /// The session to keep in the app's storage.
    pub fn session(&self) -> super::session::Session {
        super::session::Session {
            project: self.project.to_bytes(),
            path: self.project.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            saved_fingerprint: self.saved_fingerprint,
            tabs: self.tabs.iter().map(|d| d.0).collect(),
            active: self.active.map(|d| d.0),
            views: self
                .tabs
                .iter()
                .filter_map(|&id| {
                    let v = self.project.doc(id)?.panel().view;
                    Some((id.0, v.pan.x, v.pan.y, v.zoom))
                })
                .collect(),
            clean_exit: self.closing,
        }
    }

    /// Continue where the last session left off: its project, tabs, and
    /// whether it had unsaved changes.
    pub fn restore(&mut self, session: super::session::Session) -> Result<(), String> {
        let project = Project::from_bytes(&session.project)?;
        self.project = project;
        self.project.path = session.path.map(std::path::PathBuf::from);
        self.saved_fingerprint = session.saved_fingerprint;
        for (id, x, y, zoom) in session.views {
            if let Some(doc) = self.project.doc_mut(DocId(id)) {
                doc.panel_mut().set_view(super::optics::view::View { pan: egui::Vec2::new(x, y), zoom });
            }
        }
        self.tabs.clear();
        self.active = None;
        for id in session.tabs.into_iter().map(DocId) {
            if self.project.doc(id).is_some() {
                self.tabs.push(id);
            }
        }
        match session.active.map(DocId).filter(|id| self.tabs.contains(id)).or(self.tabs.first().copied()) {
            Some(id) => self.open_tab(id),
            None => {
                if let Some(first) = self.project.documents.first().map(|d| d.id) {
                    self.open_tab(first);
                }
            }
        }
        if !session.clean_exit && !cfg!(target_arch = "wasm32") {
            self.notify("Shine didn't close properly last time; your work from a few seconds before was restored");
        }
        Ok(())
    }

    /// Start over with an empty project (asking about unsaved changes).
    pub fn new_project(&mut self) {
        let mut fresh = Project::new();
        fresh.add_layout("Layout", Layout::default(), 10.0);
        self.request_open("a new project".into(), fresh.to_bytes(), None);
    }

    /// Whether the project has changed since it was last saved or opened.
    pub fn has_unsaved_changes(&self) -> bool {
        self.project.fingerprint() != self.saved_fingerprint
    }

    /// The project as it is now counts as saved.
    pub fn mark_saved(&mut self) {
        self.saved_fingerprint = self.project.fingerprint();
    }

    /// Open a file the user picked: a project replaces the current one, so
    /// with unsaved changes it waits for [`Self::resolve_unsaved`]; an old
    /// JSON layout is added as a document.
    pub fn request_open(&mut self, name: String, bytes: Vec<u8>, path: Option<std::path::PathBuf>) {
        let pending = PendingOpen { name, bytes, path };
        if Project::is_project(&pending.bytes) && self.has_unsaved_changes() {
            self.pending_open = Some(pending);
        } else {
            self.finish_open(pending);
        }
    }

    /// The user's answer about unsaved changes before opening a project.
    pub fn resolve_unsaved(&mut self, choice: UnsavedChoice) {
        let Some(pending) = self.pending_open.take() else { return };
        match choice {
            UnsavedChoice::Cancel => {}
            UnsavedChoice::Discard => self.finish_open(pending),
            UnsavedChoice::Save => {
                self.save(false);
                // Saving can be cancelled or fail: then nothing is lost.
                if self.has_unsaved_changes() && !cfg!(target_arch = "wasm32") {
                    self.notify("Not saved, so the other project wasn't opened");
                } else {
                    self.finish_open(pending);
                }
            }
        }
    }

    fn finish_open(&mut self, pending: PendingOpen) {
        let is_project = Project::is_project(&pending.bytes);
        match self.open_bytes(&pending.name, &pending.bytes) {
            Ok(()) => {
                if is_project {
                    self.project.path = pending.path;
                    self.mark_saved();
                }
                self.notify(format!("Opened {}", pending.name));
            }
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", pending.name);
                self.notify(format!("Open failed: {e}"));
            }
        }
    }

    /// Open a project, or import a JSON layout from an earlier version.
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Layout".into());
        self.open_bytes(&name, &bytes)?;
        if Project::is_project(&bytes) {
            self.project.path = Some(path.to_owned());
            self.mark_saved();
        }
        Ok(())
    }

    /// Open a Shine project, or import a JSON layout from an earlier version as `name`.
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if Project::is_project(bytes) {
            self.project = Project::from_bytes(bytes)?;
            self.tabs.clear();
            self.active = None;
            if let Some(first) = self.project.documents.first().map(|d| d.id) {
                self.open_tab(first);
            }
        } else {
            let json = std::str::from_utf8(bytes).map_err(|_| "not a Shine project".to_owned())?;
            let (layout, cell_um) = model::from_legacy_json(json)?;
            let id = self
                .project
                .add_layout(name, layout, cell_um.unwrap_or(10.0));
            self.open_tab(id);
        }
        Ok(())
    }

    /// Open an example by the slug of its name, e.g. `optical-dft`.
    pub fn open_example(&mut self, slug: &str) -> bool {
        let slugify = |name: &str| {
            name.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        };
        if let Some(i) = demos().iter().position(|e| slugify(e.name) == slug) {
            self.load_demo(i);
            return true;
        }
        if let Some(i) = layout_examples()
            .iter()
            .position(|e| slugify(e.name) == slug)
        {
            self.load_layout_example(i);
            return true;
        }
        false
    }

    /// Apply a page URL query, as used by links into the web app:
    /// `example=<slug>` opens an example, `tab=<slug>` activates a document by
    /// name, `examples=demos|layouts|chips` opens the examples window and
    /// `chip=<slug>` picks a chip-circuit example, and `sweep` opens and runs
    /// the sweep window.
    pub fn apply_url_query(&mut self, query: &str) {
        let slugify = |name: &str| {
            name.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        };
        for pair in query.trim_start_matches('?').split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "example" if !self.open_example(value) => tracing::warn!("Unknown example {value}"),
                "tab" => {
                    let id = self
                        .project
                        .documents
                        .iter()
                        .find(|d| slugify(&d.name) == value)
                        .map(|d| d.id);
                    match id {
                        Some(id) => self.open_tab(id),
                        None => tracing::warn!("No document named {value}"),
                    }
                }
                "examples" => {
                    self.show_examples = true;
                    self.examples_tab = match value {
                        "layouts" => ExamplesTab::Layouts,
                        "chips" => ExamplesTab::Chips,
                        _ => ExamplesTab::Demos,
                    };
                }
                // Open the sweep window on the active document and run it.
                "sweep" => {
                    if let Some(p) = self.active_panel() {
                        p.open_sweep(None);
                        p.run_sweep();
                    }
                }
                "chip" => {
                    match self
                        .chips
                        .examples
                        .iter()
                        .position(|e| slugify(e.name()) == value)
                    {
                        Some(i) => self.chips.select(i),
                        None => tracing::warn!("Unknown chip example {value}"),
                    }
                }
                _ => {}
            }
        }
    }

    /// Files picked in the browser arrive asynchronously; open them here.
    fn receive_files(&mut self) {
        let files: Vec<(String, Vec<u8>)> =
            std::mem::take(&mut *self.incoming.lock().expect("not poisoned"));
        for (name, bytes) in files {
            self.request_open(name, bytes, None);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, choose_path: bool) {
        let path = match (&self.project.path, choose_path) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new()
                .add_filter("Shine project", &[file_format::EXTENSION])
                .set_file_name(format!("project.{}", file_format::EXTENSION))
                .save_file()
            {
                Some(p) => p,
                None => return,
            },
        };
        match self.save_project_to(&path) {
            Ok(()) => self.notify(format!("Saved {}", path.display())),
            Err(e) => {
                tracing::error!("Failed to save {}: {e}", path.display());
                self.notify(format!("Save failed: {e}"));
            }
        }
    }

    /// In the browser, saving downloads the project file.
    #[cfg(target_arch = "wasm32")]
    fn save(&mut self, _choose_path: bool) {
        let name = format!("project.{}", file_format::EXTENSION);
        match crate::web::download(&name, &self.project.to_bytes()) {
            Ok(()) => {
                self.mark_saved();
                self.notify(format!("Downloaded {name}"));
            }
            Err(e) => self.notify(format!("Download failed: {e}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Shine project or JSON layout",
                &[file_format::EXTENSION, "json"],
            )
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let name = path.file_stem().map_or_else(|| "Layout".into(), |s| s.to_string_lossy().into_owned());
                self.request_open(name, bytes, Some(path));
            }
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", path.display());
                self.notify(format!("Open failed: {e}"));
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_dialog(&mut self) {
        let incoming = self.incoming.clone();
        let ctx = self.ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter(
                    "Shine project or JSON layout",
                    &[file_format::EXTENSION, "json"],
                )
                .pick_file()
                .await
            else {
                return;
            };
            let name = file.file_name();
            let name = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem)
                .to_owned();
            let bytes = file.read().await;
            incoming.lock().expect("not poisoned").push((name, bytes));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    // ---- components -------------------------------------------------------

    pub fn open_document_dialog(&mut self, id: DocId, component: bool) {
        let Some(doc) = self.project.doc(id) else {
            return;
        };
        let meta = doc.component.clone().unwrap_or_default();
        self.dialog = Some(DocumentDialog {
            doc: id,
            name: doc.name.clone(),
            initials: if meta.initials.is_empty() {
                ui_kit::initials(&doc.name)
            } else {
                meta.initials.clone()
            },
            description: meta.description,
            initials_edited: !meta.initials.is_empty(),
            component: component || doc.component.is_some(),
        });
    }

    pub fn apply_document_dialog(&mut self) {
        let Some(d) = self.dialog.take() else { return };
        let name = d.name.trim();
        let taken = self
            .project
            .documents
            .iter()
            .any(|doc| doc.id != d.doc && doc.name == name);
        let name = if name.is_empty() || taken {
            self.project
                .unique_name(if name.is_empty() { "Untitled" } else { name })
        } else {
            name.to_owned()
        };
        let Some(doc) = self.project.doc_mut(d.doc) else {
            return;
        };
        let newly_published = d.component && doc.component.is_none();
        doc.name = name.clone();
        if d.component {
            doc.component = Some(ComponentMeta {
                initials: d.initials.trim().chars().take(4).collect(),
                description: d.description.trim().to_owned(),
            });
        }
        if newly_published {
            self.notify(format!("“{name}” is now in My components"));
        }
    }

    pub fn unpublish(&mut self, id: DocId) {
        let users = self.project.users_of(id);
        if !users.is_empty() {
            self.notify("It is still used; remove its instances first");
            return;
        }
        if let Some(doc) = self.project.doc_mut(id) {
            doc.component = None;
        }
    }

    // ---- per-document dispatch ---------------------------------------------

    /// Send an editing action to the active document.
    pub fn apply(&mut self, action: Action) {
        let Some(doc) = self.active.and_then(|id| self.project.doc_mut(id)) else {
            return;
        };
        doc.panel_mut().apply(action);
        self.handle_requests();
    }

    fn active_panel(&mut self) -> Option<&mut OpticsPanel> {
        let id = self.active?;
        Some(self.project.doc_mut(id)?.panel_mut())
    }

    fn handle_requests(&mut self) {
        let Some(requests) = self.active_panel().map(|p| std::mem::take(&mut p.requests)) else {
            return;
        };
        for request in requests {
            match request {
                Request::New => {
                    self.new_layout();
                }
                Request::Open => self.open_dialog(),
                Request::Save => self.save(false),
                Request::ToggleLibrary => self.show_library = !self.show_library,
                Request::ToggleInspector => self.show_inspector = !self.show_inspector,
                Request::ShowShortcuts => self.show_shortcuts = !self.show_shortcuts,
                Request::Inspect(doc) => self.open_tab(doc),
                Request::NewComponent => {
                    self.new_component();
                }
                Request::PlaceExample { example, cell } => self.place_example(example, cell),
                Request::Tick => self.tick(),
                Request::RunSweep => self.run_sweep(),
                Request::SaveImage { name, rect } => self.capture(name, rect),
            }
        }
    }

    /// Add an example component to the project (once) and place it in the active document.
    pub fn place_example(&mut self, example: usize, cell: (i32, i32)) {
        let Some(into) = self.active else { return };
        let Some(component) = examples::import_component(&mut self.project, example) else {
            return;
        };
        let catalog = self.project.catalog();
        if !catalog.can_place(component, into) {
            self.notify("A component can't contain itself");
            return;
        }
        if let Some(p) = self.active_panel() {
            p.catalog = catalog;
            p.place(Placeable::Block(BlockRef::Component(component)), cell_center(cell.0, cell.1));
        }
    }

    /// Run the active editor's sweep with the project, so components work.
    pub fn run_sweep(&mut self) {
        let Some(id) = self.active else { return };
        let Some(panel) = self.project.doc(id).map(|d| d.panel()) else { return };
        let Some(spec) = panel.sweep_spec() else { return };
        let catalog = self.project.catalog();
        let layout = &panel.layout;
        let names = |inst| layout.instance(inst).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        let start = web_time::Instant::now();
        let result = super::circuit::sweep::run(layout, panel.cell_um, &self.project, &panel.machine, Some(id), &spec, &names);
        tracing::debug!("Swept {} points in {:?}", result.xs.len(), start.elapsed());
        if let Some(p) = self.active_panel() {
            p.sweep.result = Some(result);
        }
    }

    /// Advance the active document's clock: its registers store their inputs.
    pub fn tick(&mut self) {
        let Some(id) = self.active else { return };
        self.refresh_simulation();
        let Some(doc) = self.project.doc(id) else { return };
        let panel = doc.panel();
        let outcome = panel.sim.as_ref().map(|s| s.outcome.clone()).unwrap_or_default();
        let mut machine = panel.machine.clone();
        sim::clock(&panel.layout, &self.project, &mut machine, &outcome);
        if let Some(p) = self.active_panel() {
            p.machine = machine;
        }
    }

    /// Re-simulate the active document when it, anything it uses, or its
    /// registers changed. Returns whether it ran.
    pub fn refresh_simulation(&mut self) -> bool {
        let Some(id) = self.active else { return false };
        let Some(panel) = self.project.doc(id).map(|d| d.panel()) else { return false };
        if !panel.simulation_active {
            return false;
        }
        let key = panel.simulation_key(self.project.fingerprint());
        if !panel.is_stale(key) {
            return false;
        }
        // While something is being dragged, a slow design re-simulates a few
        // times a second rather than on every move, so dragging stays smooth.
        if panel.interacting()
            && self.stats.last_simulation_ms > SMOOTH_DRAG_BUDGET_MS
            && self.time - self.stats.last_simulation_at < 1.0 / DRAG_SIMULATIONS_PER_SECOND
        {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / DRAG_SIMULATIONS_PER_SECOND));
            }
            return false;
        }
        let start = web_time::Instant::now();
        let outcome = self.project.simulate(id, &panel.machine);
        tracing::debug!("Simulated {} in {:?} ({} rounds)", id.0, start.elapsed(), outcome.iterations);
        let ms = start.elapsed().as_secs_f64() * 1e3;
        self.stats.simulating += ms;
        self.stats.last_simulation_ms = ms;
        self.stats.last_simulation_at = self.time;
        if let Some(p) = self.active_panel() {
            p.set_outcome(key, outcome);
        }
        true
    }

    // ---- UI ---------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        let started = web_time::Instant::now();
        self.time = ui.input(|i| i.time);
        if self.ctx.is_none() {
            ui_kit::install_fonts(ui.ctx());
        }
        self.ctx = Some(ui.ctx().clone());
        self.apply_theme(ui.ctx());
        self.receive_files();
        self.receive_screenshot(ui);
        if self.status.as_ref().is_some_and(|s| s.1 < self.time) {
            self.status = None;
        }
        self.sync_catalog();
        // Shortcuts first, before any widget can take Tab focus.
        match self.active_panel() {
            Some(p) => p.begin_frame(ui),
            None => self.global_keys(ui),
        }
        self.handle_requests();
        self.sync_catalog();
        self.refresh_simulation();
        let catalog = self.project.catalog();

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| self.menus(ui));
        });
        egui::Panel::top("tab_strip").show(ui, |ui| self.tab_strip(ui));

        let status = self.status_text();
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(status).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.prefs.show_frame_time {
                        ui.label(egui::RichText::new(self.stats.summary()).small().monospace().weak())
                            .on_hover_text("CPU time of the last frame, drawing included, and the share spent simulating (smoothed)");
                        ui.separator();
                    }
                    ui.label(egui::RichText::new(self.file_name()).small().weak());
                    if self.has_unsaved_changes() {
                        ui.label(egui::RichText::new("● unsaved changes").small().color(ui.visuals().warn_fg_color))
                            .on_hover_text("Changed since it was last saved or opened (File → Save)");
                    }
                });
            });
        });

        match self.active {
            Some(id) => self.document_ui(ui, id, &catalog),
            None => self.empty_ui(ui),
        }
        self.handle_requests();
        self.sync_catalog();
        // Edits made this frame show on the next one.
        if self.refresh_simulation() {
            ui.ctx().request_repaint();
        }

        let ctx = ui.ctx().clone();
        self.windows(&ctx);
        self.stats.record(started.elapsed().as_secs_f64() * 1e3, self.time);
    }

    /// Give the active editor the project's components, and tell it which
    /// document it edits (it refuses to place a document inside itself).
    fn sync_catalog(&mut self) {
        let catalog = self.project.catalog();
        let id = self.active;
        let prefs = self.prefs.clone();
        if let Some(p) = self.active_panel() {
            p.catalog = catalog;
            p.doc_id = id;
            p.prefs = prefs;
        }
    }

    /// Replace the preferences (e.g. with those saved last session).
    pub fn set_preferences(&mut self, prefs: Preferences) {
        self.prefs = prefs;
        for doc in &mut self.project.documents {
            doc.panel_mut().simulation_active = self.prefs.live_simulation;
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_theme != Some(self.prefs.theme) {
            ctx.set_theme(match self.prefs.theme {
                Theme::System => egui::ThemePreference::System,
                Theme::Light => egui::ThemePreference::Light,
                Theme::Dark => egui::ThemePreference::Dark,
            });
            self.applied_theme = Some(self.prefs.theme);
        }
    }

    fn global_keys(&mut self, ui: &Ui) {
        for (key, modifiers) in ui_kit::shortcut_keys(ui.ctx()) {
            if modifiers.command {
                match key {
                    Key::N => {
                        self.new_layout();
                    }
                    Key::O => self.open_dialog(),
                    Key::S => self.save(false),
                    _ => {}
                }
            }
        }
    }

    fn status_text(&self) -> String {
        if let Some((message, _)) = &self.status {
            return message.clone();
        }
        self.active
            .and_then(|id| self.project.doc(id))
            .map(|d| d.panel().status_text())
            .unwrap_or_default()
    }

    fn document_ui(&mut self, ui: &mut Ui, id: DocId, catalog: &Catalog) {
        if self.project.doc(id).is_none() {
            self.active = None;
            return;
        }
        let mut open = None;
        let mut dialog = None;
        let mut delete = None;
        if self.show_library {
            egui::Panel::left("library")
                .default_size(230.0)
                .resizable(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(doc) = self.project.doc_mut(id) {
                            doc.panel_mut().palette_ui(ui);
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        self.project_list(ui, catalog, &mut open, &mut dialog, &mut delete);
                    });
                });
        }
        if self.show_inspector {
            egui::Panel::right("inspector")
                .default_size(270.0)
                .resizable(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.document_header(ui, id);
                        if let Some(doc) = self.project.doc_mut(id) {
                            doc.panel_mut().inspector_ui(ui);
                        }
                    });
                });
        }

        let ctx = ui.ctx().clone();
        if let Some(doc) = self.project.doc_mut(id) {
            let p = doc.panel_mut();
            p.canvas_ui(ui);
            p.overlays(&ctx);
            p.end_frame(ui);
        }

        if let Some(doc) = open {
            self.open_tab(doc);
        }
        if let Some((doc, component)) = dialog {
            self.open_document_dialog(doc, component);
        }
        if let Some(doc) = delete {
            self.delete_document(doc);
        }
    }

    /// Name of the active document and its component status.
    fn document_header(&mut self, ui: &mut Ui, id: DocId) {
        let Some(doc) = self.project.doc(id) else {
            return;
        };
        let kind = if doc.component.is_some() { "Component" } else { "Layout" };
        let interface = doc.interface();
        let component = doc.component.clone();
        let initials = doc.initials();
        ui_kit::section(ui, kind);
        ui.horizontal(|ui| {
            if component.is_some() {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(30.0, 20.0), egui::Sense::hover());
                ui_kit::badge(
                    ui.painter(),
                    rect,
                    &initials,
                    ui_kit::accent(&doc.name),
                    Color32::WHITE,
                );
            }
            ui.strong(&doc.name);
        });
        match &component {
            Some(meta) => {
                if !meta.description.is_empty() {
                    ui.label(egui::RichText::new(&meta.description).weak());
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} light in, {} out · {} signals in, {} out · used in {} document(s)",
                        interface.inputs.len(),
                        interface.outputs.len(),
                        interface.signal_inputs.len(),
                        interface.signal_outputs.len(),
                        self.project.users_of(id).len()
                    ))
                    .small()
                    .weak(),
                );
                if ui.small_button("Edit component…").clicked() {
                    self.open_document_dialog(id, true);
                }
            }
            None => {
                if ui
                    .small_button("Save as component…")
                    .on_hover_text("Reuse this layout as a box in other layouts")
                    .clicked()
                {
                    self.open_document_dialog(id, true);
                }
            }
        }
    }

    fn project_list(
        &self,
        ui: &mut Ui,
        catalog: &Catalog,
        open: &mut Option<DocId>,
        dialog: &mut Option<(DocId, bool)>,
        delete: &mut Option<DocId>,
    ) {
        ui_kit::section(ui, "Project");
        for entry in &catalog.entries {
            let active = self.active == Some(entry.id);
            let fill = if entry.is_component {
                ui_kit::accent(&entry.name)
            } else {
                Color32::from_gray(120)
            };
            let kind = "layout";
            let response = ui_kit::item_row(ui, active, &entry.name, |painter, rect| {
                ui_kit::badge(
                    painter,
                    rect.shrink(1.0),
                    &entry.initials,
                    fill,
                    Color32::WHITE,
                );
            });
            if response.clicked() {
                *open = Some(entry.id);
            }
            let tip = if entry.is_component {
                format!("{} · component {kind}\n{}", entry.name, entry.description)
            } else {
                format!("{} · {kind}", entry.name)
            };
            response.on_hover_text(tip).context_menu(|ui| {
                if ui.button("Open").clicked() {
                    *open = Some(entry.id);
                }
                if ui.button("Rename…").clicked() {
                    *dialog = Some((entry.id, false));
                }
                let label = if entry.is_component {
                    "Edit component…"
                } else {
                    "Save as component…"
                };
                if ui.button(label).clicked() {
                    *dialog = Some((entry.id, true));
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    *delete = Some(entry.id);
                }
            });
        }
    }

    fn tab_strip(&mut self, ui: &mut Ui) {
        let mut activate = None;
        let mut close = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for &id in &self.tabs {
                    let Some(doc) = self.project.doc(id) else {
                        continue;
                    };
                    let active = self.active == Some(id);
                    let text = egui::RichText::new(&doc.name);
                    let response = ui
                        .selectable_label(
                            active,
                            if doc.component.is_some() {
                                text.strong()
                            } else {
                                text
                            },
                        )
                        .on_hover_text(if doc.component.is_some() { "Component" } else { "Layout" });
                    if response.clicked() {
                        activate = Some(id);
                    }
                    if response.middle_clicked() {
                        close = Some(id);
                    }
                    if ui
                        .add(egui::Button::new(egui::RichText::new("×").weak()).frame(false))
                        .on_hover_text("Close tab")
                        .clicked()
                    {
                        close = Some(id);
                    }
                    ui.add_space(6.0);
                }
                ui.menu_button("+", |ui| {
                    if ui.button("New layout").clicked() {
                        self.new_layout();
                    }
                    if ui.button("New component").clicked() {
                        self.new_component();
                    }
                });
            });
        });
        if let Some(id) = activate {
            self.active = Some(id);
        }
        if let Some(id) = close {
            self.close_tab(id);
        }
    }

    fn menus(&mut self, ui: &mut Ui) {
        let active = self.active;
        let is_component = active
            .and_then(|id| self.project.doc(id))
            .is_some_and(|d| d.component.is_some());
        ui.menu_button("File", |ui| {
            if ui.button("New project").on_hover_text("Close this project and start an empty one").clicked() {
                self.new_project();
            }
            if ui.button("New layout").clicked() {
                self.new_layout();
            }
            if ui.button("New component").clicked() {
                self.new_component();
            }
            ui.separator();
            if ui.button("Open…").clicked() {
                self.open_dialog();
            }
            if ui.button("Save").clicked() {
                self.save(false);
            }
            if ui.button("Save as…").clicked() {
                self.save(true);
            }
            ui.separator();
            if ui.button("Examples…").clicked() {
                self.show_examples = true;
            }
        });
        ui.menu_button("Edit", |ui| {
            for (label, action) in [
                ("Undo", Action::Undo),
                ("Redo", Action::Redo),
                ("Select all", Action::SelectAll),
                ("Duplicate", Action::Duplicate),
                ("Delete", Action::Delete),
            ] {
                if ui
                    .add_enabled(active.is_some(), egui::Button::new(label))
                    .clicked()
                {
                    self.apply(action);
                }
            }
            ui.separator();
            if ui.button("Preferences…").clicked() {
                self.show_preferences = true;
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.show_library, "Library");
            ui.checkbox(&mut self.show_inspector, "Inspector");
            if let Some(on) = self.simulation_flag() {
                ui.checkbox(on, "Live simulation");
            }
            ui.separator();
            if ui.button("Frame all").clicked() {
                self.apply(Action::FrameAll);
            }
            if ui.button("Frame selected").clicked() {
                self.apply(Action::FrameSelected);
            }
            ui.separator();
            let canvas = self.active.and_then(|id| self.project.doc(id)).and_then(|d| d.panel().canvas_rect());
            if ui.add_enabled(canvas.is_some(), egui::Button::new("Save canvas image…")).on_hover_text("The canvas as a PNG image").clicked()
                && let Some(rect) = canvas
            {
                self.capture("canvas.png", rect);
            }
            ui.checkbox(&mut self.prefs.show_performance_graph, "Performance graph")
                .on_hover_text("Frames per second and frame times, live");
            if ui.button("Keyboard & mouse").clicked() {
                self.show_shortcuts = true;
            }
        });
        ui.menu_button("Analysis", |ui| {
            if ui
                .add_enabled(active.is_some(), egui::Button::new("Sweep…"))
                .on_hover_text("Plot every output over wavelengths or one parameter")
                .clicked()
                && let Some(p) = self.active_panel()
            {
                let selected = p
                    .editor
                    .boxes
                    .active()
                    .and_then(|b| p.layout.instances.get(b).map(|i| super::optics::model::Item::Box(i.id)))
                    .or(p.editor.selection.active().map(super::optics::model::Item::Part));
                p.open_sweep(selected);
            }
        });
        ui.menu_button("Component", |ui| {
            let label = if is_component {
                "Edit component…"
            } else {
                "Save as component…"
            };
            if ui
                .add_enabled(active.is_some(), egui::Button::new(label))
                .clicked()
                && let Some(id) = active
            {
                self.open_document_dialog(id, true);
            }
            if ui
                .add_enabled(is_component, egui::Button::new("Stop sharing as component"))
                .clicked()
                && let Some(id) = active
            {
                self.unpublish(id);
            }
            if ui
                .add_enabled(active.is_some(), egui::Button::new("Rename…"))
                .clicked()
                && let Some(id) = active
            {
                self.open_document_dialog(id, false);
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Keyboard & mouse").clicked() {
                self.show_shortcuts = true;
            }
            if ui.button("Examples").clicked() {
                self.show_examples = true;
            }
            ui.separator();
            if ui.button("About Shine").clicked() {
                self.about.open = true;
            }
        });

        // The one control people look for first: always visible, top right.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(on) = self.simulation_flag() {
                let (label, tip) = if *on {
                    (
                        "■  Stop simulation",
                        "Simulation is live: results update as you edit (Space)",
                    )
                } else {
                    (
                        "▶  Simulate",
                        "Solve the circuit and show the light (Space)",
                    )
                };
                if ui
                    .add(egui::Button::new(label).selected(*on))
                    .on_hover_text(tip)
                    .clicked()
                {
                    *on = !*on;
                }
            }
        });
    }

    /// The active document's live-simulation switch.
    pub fn simulation_flag(&mut self) -> Option<&mut bool> {
        Some(&mut self.project.doc_mut(self.active?)?.panel_mut().simulation_active)
    }

    fn empty_ui(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading("No document open");
                ui.add_space(8.0);
                if ui.button("New layout").clicked() {
                    self.new_layout();
                }
                if ui.button("New component").clicked() {
                    self.new_component();
                }
                if ui.button("Examples…").clicked() {
                    self.show_examples = true;
                }
            });
        });
    }

    /// Save the area `rect` of the window as a PNG: asks for a screenshot,
    /// which arrives a frame or two later (see [`Self::receive_screenshot`]).
    pub fn capture(&mut self, name: &'static str, rect: egui::Rect) {
        self.pending_image = Some((name, rect));
        if let Some(ctx) = &self.ctx {
            let slot = self.screenshot.clone();
            let repaint = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
                *slot.lock().expect("not poisoned") = Some(image);
                repaint.request_repaint();
            })));
        }
    }

    /// A screenshot for [`Self::capture`]: crop it and save it.
    fn receive_screenshot(&mut self, ui: &Ui) {
        let Some((name, rect)) = self.pending_image else { return };
        let Some(image) = self.screenshot.lock().expect("not poisoned").take() else { return };
        self.pending_image = None;
        let cropped = super::export::crop(&image, rect, ui.ctx().pixels_per_point());
        let saved = super::export::png(&cropped).and_then(|bytes| super::export::save(name, "PNG image", "png", &bytes));
        match saved {
            Ok(Some(to)) => self.notify(format!("Saved {to}")),
            Ok(None) => {}
            Err(e) => {
                tracing::error!("Image not saved: {e}");
                self.notify(format!("Image not saved: {e}"));
            }
        }
    }

    /// The project's file name, or "Unsaved project".
    pub fn file_name(&self) -> String {
        self.project
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Unsaved project".into(), |f| f.to_string_lossy().into_owned())
    }

    /// Asks what to do with unsaved changes before another project replaces
    /// this one.
    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_open else { return };
        let question = format!("Save the changes to “{}” before opening “{}”?", self.file_name(), pending.name);
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("unsaved_dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Unsaved changes");
            ui.add_space(6.0);
            ui.label(question);
            ui.label(egui::RichText::new("Opening a project replaces the one that's open.").small().weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(if cfg!(target_arch = "wasm32") { "Download and open" } else { "Save and open" }).clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
                if ui.button("Open without saving").clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(UnsavedChoice::Cancel);
        }
        if let Some(choice) = choice {
            self.resolve_unsaved(choice);
        }
    }

    /// The desktop window's title: the file, and a dot when it has unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let dot = if self.has_unsaved_changes() { " ●" } else { "" };
        let title = format!("{}{dot} - Shine", self.file_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// After a crash: what happened, and a one-click report.
    fn crash_window(&mut self, ctx: &egui::Context) {
        let Some(report) = self.crash_report.clone() else { return };
        let mut open = true;
        let mut dismiss = false;
        ui_kit::window(ctx, "Shine crashed last time").open(&mut open).default_width(560.0).show(ctx, |ui| {
            ui.label("Sorry about that. Your work up to a few seconds before was restored. Reporting it helps fix it; the report has no project data.");
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&report).monospace().small()).wrap());
            });
            ui.horizontal(|ui| {
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Report on GitHub").on_hover_text("Opens a new issue with this report").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(crate::crash::issue_url(&report)));
                }
                if ui.button("Copy details").clicked() {
                    ctx.copy_text(report.clone());
                }
                if ui.button("Close").clicked() {
                    dismiss = true;
                }
            });
        });
        if !open || dismiss {
            self.crash_report = None;
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if self.about.open {
            self.about.ui(ctx);
        }
        self.crash_window(ctx);
        self.unsaved_dialog(ctx);
        if !cfg!(target_arch = "wasm32") {
            self.update_title(ctx);
        }
        let mut open = self.prefs.show_performance_graph;
        ui_kit::window(ctx, "Performance")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| self.stats.graph_ui(ui));
        self.prefs.show_performance_graph = open;
        if open {
            // Keep the graph scrolling while it's open.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        let mut open = self.show_shortcuts;
        ui_kit::window(ctx, "Keyboard & mouse")
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
        let mut load_layout = None;
        let mut load_demo = None;
        ui_kit::window(ctx, "Examples")
            .open(&mut open)
            .default_size([760.0, 520.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.examples_tab, ExamplesTab::Demos, "Circuits");
                    ui.selectable_value(
                        &mut self.examples_tab,
                        ExamplesTab::Layouts,
                        "Free-space optics",
                    );
                    ui.selectable_value(
                        &mut self.examples_tab,
                        ExamplesTab::Chips,
                        "Chip circuits",
                    );
                });
                ui.separator();
                let list = |ui: &mut Ui,
                            items: Vec<(&'static str, &'static str)>,
                            chosen: &mut Option<usize>| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (i, (name, description)) in items.into_iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.strong(name);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Open").clicked() {
                                            *chosen = Some(i);
                                        }
                                    },
                                );
                            });
                            ui.label(egui::RichText::new(description).weak());
                            ui.add_space(6.0);
                        }
                    });
                };
                match self.examples_tab {
                    ExamplesTab::Layouts => list(
                        ui,
                        layout_examples()
                            .iter()
                            .map(|e| (e.name, e.description))
                            .collect(),
                        &mut load_layout,
                    ),
                    ExamplesTab::Demos => list(
                        ui,
                        demos().iter().map(|e| (e.name, e.description)).collect(),
                        &mut load_demo,
                    ),
                    ExamplesTab::Chips => self.chips.ui(ui),
                }
            });
        self.show_examples = open;
        if let Some(i) = load_layout {
            self.load_layout_example(i);
        }
        if let Some(i) = load_demo {
            self.load_demo(i);
        }

        self.dialog_ui(ctx);
        self.preferences_ui(ctx);
    }

    fn preferences_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.show_preferences;
        let mut reset = false;
        ui_kit::window(ctx, "Preferences")
            .open(&mut open)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                let p = &mut self.prefs;
                ui_kit::section(ui, "Appearance");
                egui::Grid::new("prefs_appearance").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut p.theme, Theme::System, "System");
                        ui.selectable_value(&mut p.theme, Theme::Light, "Light");
                        ui.selectable_value(&mut p.theme, Theme::Dark, "Dark");
                    });
                    ui.end_row();
                    ui.label("Grid");
                    ui.checkbox(&mut p.show_grid, "Show the grid");
                    ui.end_row();
                    ui.label("Boxes");
                    ui.checkbox(&mut p.show_port_names, "Show port names");
                    ui.end_row();
                    ui.label("Performance");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut p.show_frame_time, "FPS and frame time in the status bar");
                        ui.checkbox(&mut p.show_performance_graph, "Performance graph");
                    });
                    ui.end_row();
                });
                ui_kit::section(ui, "Light");
                egui::Grid::new("prefs_light").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Colours");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut p.light_colors, LightColors::Spectral, "Real colours where visible")
                            .on_hover_text("380–780 nm in their real colour; infrared and ultraviolet each get a distinct false colour");
                        ui.radio_value(&mut p.light_colors, LightColors::Distinct, "A distinct colour per wavelength");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut p.light_colors, LightColors::Single, "One colour");
                            ui.color_edit_button_srgb(&mut p.single_color);
                        });
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut p.laser_colors, "Use a laser's own colour when it has one");
                    ui.end_row();
                    ui.label("Channels");
                    ui.checkbox(&mut p.channel_strands, "Draw each wavelength as its own strand");
                    ui.end_row();
                    ui.label("Beams");
                    ui.checkbox(&mut p.beam_arrows, "Direction arrows");
                    ui.end_row();
                    ui.label("Readings");
                    ui.checkbox(&mut p.show_readings, "Show detector powers on the canvas");
                    ui.end_row();
                    ui.label("Power in");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut p.power_unit, PowerUnit::MilliWatt, "mW");
                        ui.selectable_value(&mut p.power_unit, PowerUnit::Dbm, "dBm");
                    })
                    .response
                    .on_hover_text("For readings and laser power. A laser of amplitude 1 emits 1 mW (0 dBm).");
                    ui.end_row();
                    ui.label("New lasers");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut p.default_wavelength_nm).range(200.0..=2000.0).suffix(" nm"));
                        egui::ComboBox::from_id_salt("prefs_wavelength")
                            .selected_text("Presets")
                            .show_ui(ui, |ui| {
                                for &(nm, what) in WAVELENGTH_PRESETS {
                                    ui.selectable_value(&mut p.default_wavelength_nm, nm, format!("{nm:.0} nm  {what}"));
                                }
                            });
                    });
                    ui.end_row();
                });
                ui_kit::section(ui, "Simulation");
                egui::Grid::new("prefs_sim").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("New documents");
                    ui.checkbox(&mut p.live_simulation, "Simulate live");
                    ui.end_row();
                    ui.label("Clock");
                    ui.add(egui::Slider::new(&mut p.tick_seconds, 0.05..=2.0).suffix(" s per tick").logarithmic(true));
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset to defaults").clicked() {
                        reset = true;
                    }
                    ui.label(egui::RichText::new("Saved automatically").small().weak());
                });
            });
        if reset {
            self.prefs = Preferences::default();
        }
        self.show_preferences = open;
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.clone() else {
            return;
        };
        let interface = self
            .project
            .doc(d.doc)
            .map(|doc| doc.interface());
        let mut save = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("document_dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(if d.component { "Component" } else { "Rename" });
            ui.add_space(6.0);
            egui::Grid::new("dialog_fields").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Name");
                let name = ui.text_edit_singleline(&mut d.name);
                if name.changed() && !d.initials_edited {
                    d.initials = ui_kit::initials(&d.name);
                }
                ui.end_row();
                if d.component {
                    ui.label("Initials");
                    if ui.add(egui::TextEdit::singleline(&mut d.initials).char_limit(4).desired_width(60.0)).changed() {
                        d.initials_edited = true;
                    }
                    ui.end_row();
                    ui.label("Description");
                    ui.add(egui::TextEdit::multiline(&mut d.description).desired_rows(3).hint_text("Shown when hovering an instance"));
                    ui.end_row();
                }
            });
            if d.component
                && let Some(interface) = &interface
            {
                ui.add_space(6.0);
                if interface.is_empty() {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "No ports yet. Add Input/Output port parts (light) or Signal in/out parts (electrical) so the box can be connected.",
                    );
                } else {
                    let join = |v: &[String]| if v.is_empty() { "—".to_owned() } else { v.join(", ") };
                    ui.label(
                        egui::RichText::new(format!(
                            "Light in: {}\nLight out: {}\nSignals in: {}\nSignals out: {}",
                            join(&interface.inputs),
                            join(&interface.outputs),
                            join(&interface.signal_inputs),
                            join(&interface.signal_outputs)
                        ))
                        .weak(),
                    );
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        self.dialog = Some(d);
        if save {
            self.apply_document_dialog();
        } else if cancel {
            self.dialog = None;
        }
    }
}

#[cfg(test)]
mod tests;
