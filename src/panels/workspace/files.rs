//! Files coming in: picked in a dialog, dropped on the window, or uploaded
//! in the browser. Each says what it is for; dropped files go to the asset
//! library when they land on it, and open in a tab anywhere else.

use super::Workspace;
use crate::panels::appearance;
use crate::panels::assets::{self, AssetRef, COMPONENT_EXTENSION};
use crate::panels::file_format;
use crate::panels::project::{DocId, Project};

/// What a file was picked or dropped for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePurpose {
    /// A project (or old JSON layout) to open.
    Open,
    /// A picture for the component being edited.
    Picture,
    /// Components to add to the asset library.
    Library,
    /// A matrix, as text, for New layer from a matrix.
    Matrix,
    /// A project a link opened (`?open=`), simulated if it asked.
    Link { simulate: bool },
    /// Dropped on the window, on the asset library or elsewhere.
    Dropped(DropTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Library,
    Workspace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IncomingFile {
    pub purpose: FilePurpose,
    /// The file's name, with its extension.
    pub name: String,
    pub bytes: Vec<u8>,
    /// Where it is, on desktop.
    pub path: Option<std::path::PathBuf>,
}

impl IncomingFile {
    pub fn new(purpose: FilePurpose, name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { purpose, name: name.into(), bytes, path: None }
    }
}

/// The name without its extension, for a new document.
fn stem(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name.to_owned(),
    }
}

impl Workspace {
    /// Files read asynchronously (browser pickers), handled here.
    pub(super) fn receive_files(&mut self) {
        let files: Vec<IncomingFile> = std::mem::take(&mut *self.incoming.lock().expect("not poisoned"));
        for file in files {
            self.receive_file(file);
        }
    }

    pub fn receive_file(&mut self, file: IncomingFile) {
        match file.purpose {
            FilePurpose::Open => self.request_open(stem(&file.name), file.bytes, file.path),
            FilePurpose::Picture => self.set_dialog_picture(&file.name, &file.bytes),
            FilePurpose::Library => self.add_file_to_library(&file.name, &file.bytes),
            FilePurpose::Matrix => self.set_matrix_text(&file.name, &file.bytes),
            FilePurpose::Link { simulate } => {
                let (size, started) = (file.bytes.len(), web_time::Instant::now());
                self.request_open(stem(&file.name), file.bytes, file.path);
                tracing::info!("Opened {} ({:.1} MB) in {:?}", file.name, size as f64 / 1e6, started.elapsed());
                if simulate && self.pending_open.is_none() {
                    self.simulate_active(true);
                }
            }
            FilePurpose::Dropped(target) => self.drop_file(target, file),
        }
    }

    fn drop_file(&mut self, target: DropTarget, file: IncomingFile) {
        if appearance::is_picture(&file.name) {
            if self.dialog.as_ref().is_some_and(|d| d.component) {
                self.set_dialog_picture(&file.name, &file.bytes);
            } else {
                self.notify("Pictures go on components: open Edit component… and drop it there");
            }
            return;
        }
        match target {
            DropTarget::Library => self.add_file_to_library(&file.name, &file.bytes),
            DropTarget::Workspace => self.open_dropped(file),
        }
    }

    /// A component file opens in a tab of its own (added to this project);
    /// anything else opens as a project or old layout.
    fn open_dropped(&mut self, file: IncomingFile) {
        // An S-parameter file: a component around its data, in a tab.
        if crate::panels::models::is_model_file(&file.name) && !Project::is_project(&file.bytes) {
            match crate::panels::models::ModelSpec::from_file(&file.name, &file.bytes) {
                Ok(model) => {
                    let bundle = crate::panels::models::component(model);
                    if let Some(id) = self.project.import(&bundle) {
                        self.open_tab(id);
                        let name = self.project.doc(id).map(|d| d.name.clone()).unwrap_or_default();
                        self.notify(format!("Opened “{name}” from its S-parameters"));
                    }
                }
                Err(e) => self.notify(format!("Can't read {}: {e}", file.name)),
            }
            return;
        }
        if Project::is_project(&file.bytes)
            && let Ok(project) = Project::from_bytes(&file.bytes)
            && assets::is_component_file(&file.name, &project)
        {
            match self.project.import(&project) {
                Some(id) => {
                    self.open_tab(id);
                    let name = self.project.doc(id).map(|d| d.name.clone()).unwrap_or_default();
                    self.notify(format!("Opened component “{name}” in this project"));
                }
                None => self.notify(format!("{} has no component", file.name)),
            }
            return;
        }
        self.request_open(stem(&file.name), file.bytes, file.path);
    }

    pub fn add_file_to_library(&mut self, name: &str, bytes: &[u8]) {
        self.left_tab = super::LeftTab::Assets;
        match self.library.import_file(name, bytes, "") {
            Ok(0) => self.notify(format!("Everything in {name} is already in your library")),
            Ok(1) => self.notify(format!("Added {name} to your library")),
            Ok(n) => self.notify(format!("Added {n} components from {name} to your library")),
            Err(e) => self.notify(format!("Couldn't add it to the library: {e}")),
        }
    }

    /// Use a picture file for the component in the open dialog.
    pub fn set_dialog_picture(&mut self, name: &str, bytes: &[u8]) {
        let Some(dialog) = self.dialog.as_mut() else { return };
        match appearance::icon_from_file(name, bytes) {
            Ok(icon) => {
                dialog.appearance.icon = Some(icon);
                dialog.picture_error = None;
            }
            Err(e) => dialog.picture_error = Some(e),
        }
    }

    /// Files dropped on the window this frame; `library` is where the asset
    /// library is on screen.
    pub(super) fn receive_dropped_files(&mut self, ctx: &egui::Context) {
        let (hovering, dropped, pointer) =
            ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone(), i.pointer.latest_pos()));
        self.files_hovering = hovering;
        if dropped.is_empty() {
            return;
        }
        // Where the pointer was when they were let go.
        let on_library = pointer.zip(self.library_rect).is_some_and(|(p, r)| r.contains(p));
        let purpose = FilePurpose::Dropped(if on_library { DropTarget::Library } else { DropTarget::Workspace });
        for f in dropped {
            let name = f.path().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.read_dropped(purpose, name, f);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_dropped(&mut self, purpose: FilePurpose, name: String, file: egui::DroppedFileHandle) {
        // A folder, wherever it lands: its components go into the library.
        if file.path().is_dir() {
            self.import_folder(file.path());
            return;
        }
        match file.bytes() {
            Ok(bytes) => {
                let path = Some(file.path().to_owned()).filter(|p| p.is_absolute());
                self.receive_file(IncomingFile { purpose, name, bytes, path });
            }
            Err(e) => self.notify(format!("Couldn't read {name}: {e}")),
        }
    }

    /// The browser reads files asynchronously: they arrive through the inbox.
    #[cfg(target_arch = "wasm32")]
    fn read_dropped(&mut self, purpose: FilePurpose, name: String, file: egui::DroppedFileHandle) {
        let incoming = self.incoming.clone();
        let ctx = self.ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match file.bytes_async().await {
                Ok(bytes) => incoming.lock().expect("not poisoned").push(IncomingFile::new(purpose, name, bytes)),
                Err(e) => tracing::error!("Couldn't read the dropped file {name}: {e}"),
            }
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// While files are dragged over the window: where they can go.
    pub(super) fn drop_overlay(&self, ctx: &egui::Context) {
        if !self.files_hovering {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("file_drop")));
        let screen = ctx.content_rect();
        let accent = ctx.global_style().visuals.selection.bg_fill;
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(110));
        let font = egui::FontId::proportional(18.0);
        let open_area = match self.library_rect {
            Some(library) => {
                painter.rect(library.shrink(6.0), 8.0, accent.gamma_multiply(0.35), egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                painter.text(library.center(), egui::Align2::CENTER_CENTER, "Drop here to add\nto your asset library", font.clone(), egui::Color32::WHITE);
                egui::Rect::from_min_max(egui::pos2(library.right(), screen.top()), screen.max)
            }
            None => screen,
        };
        painter.text(
            open_area.center(),
            egui::Align2::CENTER_CENTER,
            "Drop to open: a component in a tab of its own,\na project in place of this one, a picture on the component you're editing",
            font,
            egui::Color32::WHITE,
        );
    }

    /// Ask for files (a dialog on desktop, the browser's picker on the web);
    /// they arrive through [`Self::receive_file`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pick_files(&mut self, purpose: FilePurpose, kind: &str, extensions: &[&str], many: bool) {
        let dialog = rfd::FileDialog::new().add_filter(kind, extensions);
        let paths = if many { dialog.pick_files().unwrap_or_default() } else { dialog.pick_file().into_iter().collect() };
        for path in paths {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            match std::fs::read(&path) {
                Ok(bytes) => self.receive_file(IncomingFile { purpose, name, bytes, path: Some(path) }),
                Err(e) => {
                    tracing::error!("Failed to read {}: {e}", path.display());
                    self.notify(format!("Couldn't read {name}: {e}"));
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn pick_files(&mut self, purpose: FilePurpose, kind: &str, extensions: &[&str], many: bool) {
        let incoming = self.incoming.clone();
        let ctx = self.ctx.clone();
        let dialog = rfd::AsyncFileDialog::new().add_filter(kind, extensions);
        wasm_bindgen_futures::spawn_local(async move {
            let files = if many { dialog.pick_files().await.unwrap_or_default() } else { dialog.pick_file().await.into_iter().collect() };
            for file in files {
                let name = file.file_name();
                let bytes = file.read().await;
                incoming.lock().expect("not poisoned").push(IncomingFile::new(purpose, name, bytes));
            }
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Open the project at `path` on this site (a link's `?open=`), and
    /// simulate it if `simulate`. Only paths on the same site: a link can't
    /// make the app fetch from anywhere else.
    pub fn open_link(&mut self, path: &str, simulate: bool) {
        if !path.starts_with('/') || path.starts_with("//") || path.contains("://") || path.contains("..") {
            self.notify(format!("Projects open from this site only: “{path}”"));
            return;
        }
        let name = path.rsplit('/').next().unwrap_or("project").to_owned();
        self.notify(format!("Opening {name}…"));
        let incoming = self.incoming.clone();
        let ctx = self.ctx.clone();
        let purpose = FilePurpose::Link { simulate };
        ehttp::fetch(ehttp::Request::get(path), move |result| {
            match result {
                Ok(r) if r.ok => incoming.lock().expect("not poisoned").push(IncomingFile::new(purpose, name, r.bytes)),
                Ok(r) => tracing::warn!("Couldn't open {}: {} {}", r.url, r.status, r.status_text),
                Err(e) => tracing::warn!("Couldn't open {name}: {e}"),
            }
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Choose a picture for the component in the open dialog.
    pub fn pick_picture(&mut self) {
        self.pick_files(FilePurpose::Picture, "Picture", &appearance::PICTURE_EXTENSIONS, false);
    }

    /// Add every component file in a folder the user picks (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pick_library_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().set_title("Folder of components").pick_folder() {
            self.import_folder(&dir);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn pick_library_folder(&mut self) {
        self.pick_library_files();
    }

    /// Add every `.shinecomp` and `.shine` file in `dir` and its subfolders to
    /// the library. Each folder becomes a library folder: `Optics`,
    /// `Optics / Rings`…
    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_folder(&mut self, dir: &std::path::Path) {
        let base = dir.file_name().map_or_else(|| "Imported".to_owned(), |n| n.to_string_lossy().into_owned());
        let files = assets::component_files_in(dir);
        if files.is_empty() {
            self.notify(format!("No components (.{COMPONENT_EXTENSION}, .{}) or S-parameter files (.sNp, .dat) in {}", file_format::EXTENSION, dir.display()));
            return;
        }
        let count = files.len();
        let read = files.into_iter().filter_map(|(path, sub)| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let folder = std::iter::once(base.clone()).chain(sub).collect::<Vec<_>>().join(" / ");
            match std::fs::read(&path) {
                Ok(bytes) => Some((name, bytes, folder)),
                Err(e) => {
                    tracing::warn!("Couldn't read {}: {e}", path.display());
                    None
                }
            }
        });
        let (added, failed) = self.library.import_files(read.collect::<Vec<_>>());
        for f in &failed {
            tracing::warn!("Not imported: {f}");
        }
        self.left_tab = super::LeftTab::Assets;
        let skipped = if failed.is_empty() { String::new() } else { format!("; {} files couldn't be used (see the log)", failed.len()) };
        self.notify(format!("Added {added} components from {count} files in {base}{skipped}"));
    }

    /// Add component files (or projects' components) to the library.
    pub fn pick_library_files(&mut self) {
        // Shine's files, and S-parameter files (Touchstone up to 16 ports, Lumerical).
        let touchstone: Vec<String> = (1..=16).map(|n| format!("s{n}p")).chain(["ts".into(), "dat".into()]).collect();
        let mut extensions: Vec<&str> = vec![COMPONENT_EXTENSION, file_format::EXTENSION];
        extensions.extend(touchstone.iter().map(String::as_str));
        self.pick_files(FilePurpose::Library, "Shine component or project, or S-parameters", &extensions, true);
    }

    fn save_component_file(&mut self, name: &str, bytes: Vec<u8>) {
        let file = format!("{}.{COMPONENT_EXTENSION}", super::slug(name));
        match crate::panels::export::save(&file, "Shine component", COMPONENT_EXTENSION, &bytes) {
            Ok(Some(saved)) => self.notify(format!("Saved {saved}")),
            Ok(None) => {}
            Err(e) => self.notify(format!("Couldn't save the component: {e}")),
        }
    }

    /// Save a project component (with what it uses) as a `.shinecomp` file.
    pub fn export_component(&mut self, id: DocId) {
        let Some(bundle) = self.project.extract(id) else { return };
        let name = bundle.documents[0].name.clone();
        self.save_component_file(&name, bundle.to_bytes());
    }

    pub fn export_asset(&mut self, asset: AssetRef) {
        let name = self.library.summaries().iter().find(|a| a.asset == asset).map(|a| a.entry.name.clone()).unwrap_or_default();
        if let Some(bytes) = self.library.file_bytes(asset) {
            self.save_component_file(&name, bytes);
        }
    }

    /// Keep a project component (with what it uses) in the asset library.
    pub fn add_to_library(&mut self, id: DocId, folder: &str) {
        let Some(bundle) = self.project.extract(id) else { return };
        let name = bundle.documents[0].name.clone();
        if bundle.documents[0].component.is_none() {
            self.notify(format!("Save “{name}” as a component first"));
            return;
        }
        self.left_tab = super::LeftTab::Assets;
        match self.library.add(&bundle, folder) {
            Ok((_, true)) => self.notify(format!("Added “{name}” to your library")),
            Ok((_, false)) => self.notify(format!("“{name}” is already in your library")),
            Err(e) => self.notify(format!("Couldn't add it: {e}")),
        }
    }

    /// An asset's component copied into this project and opened in a tab.
    pub fn open_asset(&mut self, asset: AssetRef) {
        match self.import_asset(asset) {
            Some(id) => self.open_tab(id),
            None => self.notify("That library component can't be read"),
        }
    }
}
