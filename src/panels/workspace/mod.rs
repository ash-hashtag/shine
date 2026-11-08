//! The application shell: menu bar, document tabs, library, inspector and status bar.

use std::path::Path;

use egui::{Color32, Key, Ui};

use super::chips::ChipBrowser;
use super::optics::examples::layout_examples;
use super::optics::input::{Action, SHORTCUTS};
use super::optics::model::{self, Layout};
use super::project::{Catalog, ComponentMeta, Content, DocId, Project};
use super::schematic::examples::schematic_examples;
use super::schematic::model::Schematic;
use super::{Request, ui_kit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExamplesTab {
    Layouts,
    Schematics,
    Chips,
}

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
    pub incoming: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
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
            examples_tab: ExamplesTab::Layouts,
            chips: ChipBrowser::default(),
            dialog: None,
            status: None,
            incoming: Default::default(),
            ctx: None,
            time: 0.0,
        };
        ws.new_layout();
        ws
    }
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
        self.open_tab(id);
        id
    }

    pub fn new_schematic(&mut self) -> DocId {
        let id = self
            .project
            .add_schematic("Schematic", Schematic::default());
        self.open_tab(id);
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
        if let Some(Content::Layout(p)) = self.project.doc_mut(id).map(|d| &mut d.content) {
            p.simulation_active = true;
        }
        self.open_tab(id);
        self.notify(format!("Opened example “{}”", example.name));
    }

    pub fn load_schematic_example(&mut self, index: usize) {
        let Some(example) = schematic_examples().into_iter().nth(index) else {
            return;
        };
        let id = (example.build)(&mut self.project);
        self.open_tab(id);
        self.notify(format!("Opened example “{}”", example.name));
    }

    // ---- files ------------------------------------------------------------

    pub fn save_project_to(&mut self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.project.to_json()).map_err(|e| e.to_string())?;
        self.project.path = Some(path.to_owned());
        Ok(())
    }

    /// Open a project, or import a single saved layout as a new document.
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Layout".into());
        self.open_json(&name, &json)?;
        if Project::is_project_json(&json) {
            self.project.path = Some(path.to_owned());
        }
        Ok(())
    }

    /// Open a project from its JSON, or import a single saved layout named `name`.
    pub fn open_json(&mut self, name: &str, json: &str) -> Result<(), String> {
        if Project::is_project_json(json) {
            self.project = Project::from_json(json)?;
            self.tabs.clear();
            self.active = None;
            if let Some(first) = self.project.documents.first().map(|d| d.id) {
                self.open_tab(first);
            }
        } else {
            let (layout, cell_um) = model::from_json(json)?;
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
        if let Some(i) = schematic_examples()
            .iter()
            .position(|e| slugify(e.name) == slug)
        {
            self.load_schematic_example(i);
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

    /// Files picked in the browser arrive asynchronously; open them here.
    fn receive_files(&mut self) {
        let files: Vec<(String, String)> =
            std::mem::take(&mut *self.incoming.lock().expect("not poisoned"));
        for (name, json) in files {
            match self.open_json(&name, &json) {
                Ok(()) => self.notify(format!("Opened {name}")),
                Err(e) => self.notify(format!("Open failed: {e}")),
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, choose_path: bool) {
        let path = match (&self.project.path, choose_path) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new()
                .add_filter("Shine project", &["json"])
                .set_file_name("project.json")
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
        match crate::web::download("project.json", &self.project.to_json()) {
            Ok(()) => self.notify("Downloaded project.json"),
            Err(e) => self.notify(format!("Download failed: {e}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Shine project or layout", &["json"])
            .pick_file()
        else {
            return;
        };
        match self.open_path(&path) {
            Ok(()) => self.notify(format!("Opened {}", path.display())),
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
                .add_filter("Shine project or layout", &["json"])
                .pick_file()
                .await
            else {
                return;
            };
            let name = file.file_name().trim_end_matches(".json").to_owned();
            let json = String::from_utf8_lossy(&file.read().await).into_owned();
            incoming.lock().expect("not poisoned").push((name, json));
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
        let catalog = self.project.catalog();
        let Some(doc) = self.active.and_then(|id| self.project.doc_mut(id)) else {
            return;
        };
        match &mut doc.content {
            Content::Layout(p) => p.apply(action),
            Content::Schematic(p) => p.apply(&catalog, action),
        }
        self.handle_requests();
    }

    fn handle_requests(&mut self) {
        let Some(id) = self.active else { return };
        let (requests, is_layout) = match self.project.doc_mut(id).map(|d| &mut d.content) {
            Some(Content::Layout(p)) => (std::mem::take(&mut p.requests), true),
            Some(Content::Schematic(p)) => (std::mem::take(&mut p.requests), false),
            None => return,
        };
        for request in requests {
            match request {
                Request::New if is_layout => {
                    self.new_layout();
                }
                Request::New => {
                    self.new_schematic();
                }
                Request::Open => self.open_dialog(),
                Request::Save => self.save(false),
                Request::ToggleLibrary => self.show_library = !self.show_library,
                Request::ToggleInspector => self.show_inspector = !self.show_inspector,
                Request::ShowShortcuts => self.show_shortcuts = !self.show_shortcuts,
                Request::Inspect(doc) => self.open_tab(doc),
            }
        }
    }

    /// Re-simulate the active schematic when it, or anything it uses, changed.
    pub fn refresh_simulation(&mut self) {
        let Some(id) = self.active else { return };
        let active = match self.project.doc(id).map(|d| &d.content) {
            Some(Content::Schematic(p)) => p.simulation_active,
            _ => return,
        };
        if !active {
            return;
        }
        let key = self.project.fingerprint();
        if matches!(self.project.doc(id).map(|d| &d.content), Some(Content::Schematic(p)) if p.sim_key == Some(key))
        {
            return;
        }
        let start = web_time::Instant::now();
        let sim = self.project.simulate_schematic(id);
        tracing::debug!("Simulated schematic {} in {:?}", id.0, start.elapsed());
        if let Some(Content::Schematic(p)) = self.project.doc_mut(id).map(|d| &mut d.content) {
            p.sim = Some(sim);
            p.sim_key = Some(key);
        }
    }

    // ---- UI ---------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.time = ui.input(|i| i.time);
        self.ctx = Some(ui.ctx().clone());
        self.receive_files();
        if self.status.as_ref().is_some_and(|s| s.1 < self.time) {
            self.status = None;
        }
        // Editors refuse to place a document inside itself.
        for doc in &mut self.project.documents {
            if let Content::Schematic(p) = &mut doc.content {
                p.doc_id = Some(doc.id);
            }
        }
        let catalog = self.project.catalog();
        // Shortcuts first, before any widget can take Tab focus.
        match self
            .active
            .and_then(|id| self.project.doc_mut(id))
            .map(|d| &mut d.content)
        {
            Some(Content::Layout(p)) => p.begin_frame(ui),
            Some(Content::Schematic(p)) => p.begin_frame(ui, &catalog),
            None => self.global_keys(ui),
        }
        self.handle_requests();
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
                    let file = self
                        .project
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Unsaved project".into());
                    ui.label(egui::RichText::new(file).small().weak());
                });
            });
        });

        match self.active {
            Some(id) => self.document_ui(ui, id, &catalog),
            None => self.empty_ui(ui),
        }
        self.handle_requests();
        self.refresh_simulation();

        let ctx = ui.ctx().clone();
        self.windows(&ctx);
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
        match self
            .active
            .and_then(|id| self.project.doc(id))
            .map(|d| &d.content)
        {
            Some(Content::Layout(p)) => p.status_text(),
            Some(Content::Schematic(p)) => p.status_text(),
            None => String::new(),
        }
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
                            match &mut doc.content {
                                Content::Layout(p) => p.palette_ui(ui),
                                Content::Schematic(p) => p.palette_ui(ui, catalog),
                            }
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
                            match &mut doc.content {
                                Content::Layout(p) => p.inspector_ui(ui),
                                Content::Schematic(p) => p.inspector_ui(ui, catalog),
                            }
                        }
                    });
                });
        }

        let ctx = ui.ctx().clone();
        if let Some(doc) = self.project.doc_mut(id) {
            match &mut doc.content {
                Content::Layout(p) => {
                    p.canvas_ui(ui);
                    p.overlays(&ctx);
                    p.end_frame(ui);
                }
                Content::Schematic(p) => {
                    p.canvas_ui(ui, catalog);
                    p.overlays(&ctx, catalog);
                    p.end_frame(ui);
                }
            }
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
        let kind = if doc.is_layout() {
            "Free-space layout"
        } else {
            "Schematic"
        };
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
                        "Component · {} in, {} out · used in {} document(s)",
                        interface.inputs.len(),
                        interface.outputs.len(),
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
                    .on_hover_text("Reuse this document as a block in schematics")
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
            let kind = if entry.is_layout {
                "layout"
            } else {
                "schematic"
            };
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
                        .on_hover_text(if doc.is_layout() {
                            "Free-space layout"
                        } else {
                            "Schematic"
                        });
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
                    if ui.button("Free-space layout").clicked() {
                        self.new_layout();
                    }
                    if ui.button("Schematic").clicked() {
                        self.new_schematic();
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
            if ui.button("New free-space layout").clicked() {
                self.new_layout();
            }
            if ui.button("New schematic").clicked() {
                self.new_schematic();
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
            if ui.button("Keyboard & mouse").clicked() {
                self.show_shortcuts = true;
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
        match &mut self.project.doc_mut(self.active?)?.content {
            Content::Layout(p) => Some(&mut p.simulation_active),
            Content::Schematic(p) => Some(&mut p.simulation_active),
        }
    }

    fn empty_ui(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading("No document open");
                ui.add_space(8.0);
                if ui.button("New free-space layout").clicked() {
                    self.new_layout();
                }
                if ui.button("New schematic").clicked() {
                    self.new_schematic();
                }
                if ui.button("Examples…").clicked() {
                    self.show_examples = true;
                }
            });
        });
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
        let mut load_layout = None;
        let mut load_schematic = None;
        egui::Window::new("Examples")
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
                        ExamplesTab::Schematics,
                        "Schematics",
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
                    ExamplesTab::Schematics => list(
                        ui,
                        schematic_examples()
                            .iter()
                            .map(|e| (e.name, e.description))
                            .collect(),
                        &mut load_schematic,
                    ),
                    ExamplesTab::Chips => self.chips.ui(ui),
                }
            });
        self.show_examples = open;
        if let Some(i) = load_layout {
            self.load_layout_example(i);
        }
        if let Some(i) = load_schematic {
            self.load_schematic_example(i);
        }

        self.dialog_ui(ctx);
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.clone() else {
            return;
        };
        let interface = self
            .project
            .doc(d.doc)
            .map(|doc| (doc.interface(), doc.is_layout()));
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
                && let Some((interface, is_layout)) = &interface
            {
                ui.add_space(6.0);
                if interface.inputs.is_empty() && interface.outputs.is_empty() {
                    let hint = if *is_layout {
                        "No ports yet. Add Input port and Output port parts so the component can be connected."
                    } else {
                        "No pins yet. Add Input pin and Output pin blocks so the component can be connected."
                    };
                    ui.colored_label(ui.visuals().warn_fg_color, hint);
                } else {
                    let join = |v: &[String]| if v.is_empty() { "—".to_owned() } else { v.join(", ") };
                    ui.label(
                        egui::RichText::new(format!("Inputs: {}\nOutputs: {}", join(&interface.inputs), join(&interface.outputs))).weak(),
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
