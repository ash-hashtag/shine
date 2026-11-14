//! The Assets tab: the project's components and the asset library, in
//! folders, with search. Rows are dragged onto the grid to place them, or
//! onto a library folder to keep them there.

use egui::{RichText, Ui};

use super::Workspace;
use crate::panels::appearance::paint_badge;
use crate::panels::assets::{AssetId, AssetRef, AssetSummary, DEFAULT_FOLDER, EXAMPLES_FOLDER, ports_text};
use crate::panels::optics::model::BlockRef;
use crate::panels::optics::{Placeable, Tool};
use crate::panels::project::{Catalog, CatalogEntry, DocId};
use crate::panels::ui_kit;

/// What is being dragged onto a library folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dragged {
    /// A project component, to keep in the library.
    Project(DocId),
    /// A library asset, to move to another folder.
    Saved(AssetId),
    /// One of Shine's examples: only placed, never moved.
    Example,
}

/// Something chosen in the tab, done after drawing it.
enum Action {
    Open(DocId),
    Edit(DocId),
    AddToLibrary(DocId, String),
    ExportComponent(DocId),
    OpenAsset(AssetRef),
    ExportAsset(AssetRef),
    Remove(AssetId),
    Move(AssetId, String),
    NewComponent,
    NewScripted,
    Import,
    OpenWindow,
    /// Open the library window on one folder (more rows than the tab shows).
    ShowFolder(String),
}

/// Rows a folder shows here; the library window shows the rest.
const ROWS_SHOWN: usize = 40;

fn entry_matches(e: &CatalogEntry, search: &str) -> bool {
    let search = search.trim().to_lowercase();
    if search.is_empty() {
        return true;
    }
    let i = &e.interface;
    std::iter::once(&e.name)
        .chain([&e.description, &e.initials])
        .chain(i.inputs.iter().chain(&i.outputs).chain(&i.signal_inputs).chain(&i.signal_outputs))
        .any(|t| t.to_lowercase().contains(&search))
}

fn tooltip(e: &CatalogEntry, ports: &str, folder: Option<&str>) -> String {
    let mut text = e.name.clone();
    if !e.description.is_empty() {
        text += &format!("\n{}", e.description);
    }
    text += &format!("\n{ports}");
    if let Some(folder) = folder {
        text += &format!("\nIn “{folder}”");
    }
    text + "\n\nDrag onto the grid, or click it and then the grid, to place it."
}

impl Workspace {
    /// Whether `item` is what the active editor is about to place.
    fn placing(&mut self, item: &Placeable) -> bool {
        self.active_panel().is_some_and(|p| matches!(&p.tool, Some(Tool::Place(t)) if t == item))
    }

    /// A row's clicks and drags: place it on the grid like a library part.
    fn place_from_row(&mut self, response: &egui::Response, item: Placeable, dragged: Dragged) {
        let selected = self.placing(&item);
        if response.drag_started() {
            egui::DragAndDrop::set_payload(&response.ctx, dragged);
            if let Some(p) = self.active_panel() {
                p.dragging_new = Some(item.clone());
            }
        }
        if response.clicked()
            && let Some(p) = self.active_panel()
        {
            p.tool = if selected { None } else { Some(Tool::Place(item)) };
        }
    }

    pub(super) fn assets_ui(&mut self, ui: &mut Ui, catalog: &Catalog) {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button("⊞ Library").on_hover_text("Browse every component in a grid (View → Asset library…)").clicked() {
                action = Some(Action::OpenWindow);
            }
            if ui
                .button("Import…")
                .on_hover_text("Add component files (.shinecomp), or every component of a project (.shine), to your library")
                .clicked()
            {
                action = Some(Action::Import);
            }
            // Too narrow beside the buttons: on a line of its own.
            if ui.available_width() < 140.0 {
                ui.end_row();
            }
            ui.add(egui::TextEdit::singleline(&mut self.asset_search).hint_text("Search assets").desired_width(f32::INFINITY))
                .accessible_name("Search assets")
                .on_hover_text("By name, description, folder, port or parameter");
        });
        ui.label(RichText::new("Drag onto the grid to place · drop files or project components on a folder to keep them").small().weak());
        ui.add_space(4.0);
        let search = self.asset_search.clone();
        let searching = !search.trim().is_empty();
        let mut found = 0;

        // The project's own components.
        let mine: Vec<&CatalogEntry> = catalog.components().filter(|e| entry_matches(e, &search)).collect();
        found += mine.len();
        if !mine.is_empty() || !searching {
            egui::CollapsingHeader::new(RichText::new(format!("This project  ·  {}", mine.len())).strong())
                .id_salt("assets_project")
                .default_open(true)
                .open(searching.then_some(true))
                .show(ui, |ui| {
                    for e in &mine {
                        let item = Placeable::Block(BlockRef::Component(e.id));
                        let selected = self.placing(&item);
                        let placeable = self.active.is_none_or(|me| catalog.can_place(e.id, me));
                        let response = ui
                            .add_enabled_ui(placeable, |ui| {
                                ui_kit::item_row_sized(ui, selected, &e.name, 24.0, |p, r| paint_badge(p, r, &e.appearance, &e.name, &e.initials))
                            })
                            .inner;
                        self.place_from_row(&response, item, Dragged::Project(e.id));
                        if response.double_clicked() {
                            action = Some(Action::Open(e.id));
                        }
                        let ports = ports_text(&e.interface);
                        let folders = self.library.folders();
                        response.on_hover_text(tooltip(e, &ports, None)).context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                action = Some(Action::Open(e.id));
                            }
                            if ui.button("Edit component…").clicked() {
                                action = Some(Action::Edit(e.id));
                            }
                            ui.menu_button("Add to library", |ui| {
                                for f in folders.iter().map(String::as_str).chain((!folders.iter().any(|f| f == DEFAULT_FOLDER)).then_some(DEFAULT_FOLDER)) {
                                    if ui.button(f).clicked() {
                                        action = Some(Action::AddToLibrary(e.id, f.to_owned()));
                                    }
                                }
                            });
                            if ui.button("Export…").on_hover_text("Save it, with what it uses, as a .shinecomp file to share").clicked() {
                                action = Some(Action::ExportComponent(e.id));
                            }
                        });
                    }
                    if !searching
                        && ui.small_button("+ New component").on_hover_text("Start an empty component with ports, ready to fill and reuse").clicked()
                    {
                        action = Some(Action::NewComponent);
                    }
                    if !searching
                        && ui
                            .small_button("+ New scripted component")
                            .on_hover_text("Its light and signals computed by a JavaScript program, in a sandbox")
                            .clicked()
                    {
                        action = Some(Action::NewScripted);
                    }
                });
        }

        // The library: Shine's examples, then the user's folders.
        let summaries = self.library.summaries();
        let mut folders = vec![EXAMPLES_FOLDER.to_owned()];
        folders.extend(self.library.folders());
        if self.library.is_empty() && !folders.iter().any(|f| f == DEFAULT_FOLDER) {
            // Somewhere to drop the first one.
            folders.push(DEFAULT_FOLDER.to_owned());
        }
        let user_folders: Vec<String> = folders.iter().filter(|f| *f != EXAMPLES_FOLDER).cloned().collect();
        for folder in &folders {
            let rows: Vec<&AssetSummary> = summaries.iter().filter(|a| &a.folder == folder && a.matches(&search)).collect();
            found += rows.len();
            if rows.is_empty() && searching {
                continue;
            }
            let examples = folder == EXAMPLES_FOLDER;
            // With many folders, they start closed: the library window is for browsing.
            let start_open = examples || user_folders.len() <= 6;
            let mut draw = |ui: &mut Ui| {
                egui::CollapsingHeader::new(RichText::new(format!("{folder}  ·  {}", rows.len())).strong())
                    .id_salt(("assets_folder", folder.as_str()))
                    .default_open(start_open)
                    .open(searching.then_some(true))
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.label(RichText::new("Drop components here to keep them for every project").small().weak());
                        }
                        for a in rows.iter().take(ROWS_SHOWN) {
                            self.asset_row(ui, a, &user_folders, &mut action);
                        }
                        if rows.len() > ROWS_SHOWN
                            && ui.link(format!("All {} in the library…", rows.len())).clicked()
                        {
                            action = Some(Action::ShowFolder(folder.clone()));
                        }
                    });
            };
            if examples {
                draw(ui);
                continue;
            }
            let (_, dropped) = ui.dnd_drop_zone::<Dragged, ()>(egui::Frame::NONE, draw);
            match dropped.as_deref() {
                Some(Dragged::Project(id)) => action = Some(Action::AddToLibrary(*id, folder.clone())),
                Some(Dragged::Saved(id)) => action = Some(Action::Move(*id, folder.clone())),
                Some(Dragged::Example) | None => {}
            }
        }
        if searching && found == 0 {
            ui.label(RichText::new("Nothing matches").weak());
        }

        match action {
            Some(Action::Open(id)) => self.open_tab_asking(id),
            Some(Action::Edit(id)) => self.open_document_dialog(id, true),
            Some(Action::AddToLibrary(id, folder)) => self.add_to_library(id, &folder),
            Some(Action::ExportComponent(id)) => self.export_component(id),
            Some(Action::OpenAsset(asset)) => self.open_asset(asset),
            Some(Action::ExportAsset(asset)) => self.export_asset(asset),
            Some(Action::Remove(id)) => {
                self.library.remove(id);
                self.notify("Removed from your library (projects using it keep their copy)");
            }
            Some(Action::Move(id, folder)) => {
                self.library.set_folder(id, &folder);
                self.moving_asset = None;
            }
            Some(Action::NewComponent) => {
                self.new_component();
            }
            Some(Action::NewScripted) => {
                self.new_scripted_component();
            }
            Some(Action::Import) => self.pick_library_files(),
            Some(Action::OpenWindow) => self.open_library_window(),
            Some(Action::ShowFolder(folder)) => {
                self.library_window.folder = super::library_window::Folder::Named(folder);
                self.library_window.search = search.clone();
                self.open_library_window();
            }
            None => {}
        }
    }

    fn asset_row(&mut self, ui: &mut Ui, a: &AssetSummary, folders: &[String], action: &mut Option<Action>) {
        let e = &a.entry;
        let item = Placeable::Asset(a.asset);
        let selected = self.placing(&item);
        let response = ui_kit::item_row_sized(ui, selected, &e.name, 24.0, |p, r| paint_badge(p, r, &e.appearance, &e.name, &e.initials));
        let dragged = match a.asset {
            AssetRef::Saved(id) => Dragged::Saved(id),
            AssetRef::Example(_) => Dragged::Example,
        };
        self.place_from_row(&response, item, dragged);
        if response.double_clicked() {
            *action = Some(Action::OpenAsset(a.asset));
        }
        let moving = &mut self.moving_asset;
        response.on_hover_text(tooltip(e, &a.ports(), Some(&a.folder))).context_menu(|ui| {
            if ui.button("Open in a tab").on_hover_text("Copy it into this project and open it").clicked() {
                *action = Some(Action::OpenAsset(a.asset));
            }
            if ui.button("Export…").on_hover_text("Save it as a .shinecomp file to share").clicked() {
                *action = Some(Action::ExportAsset(a.asset));
            }
            if let AssetRef::Saved(id) = a.asset {
                ui.menu_button("Move to folder", |ui| {
                    for f in folders.iter().filter(|f| **f != a.folder) {
                        if ui.button(f).clicked() {
                            *action = Some(Action::Move(id, f.clone()));
                        }
                    }
                    ui.separator();
                    let (_, text) = moving.get_or_insert_with(|| (id, String::new()));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(text).hint_text("New folder").desired_width(120.0)).accessible_name("New folder name");
                        if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Move")).clicked() {
                            *action = Some(Action::Move(id, text.trim().to_owned()));
                        }
                    });
                });
                ui.separator();
                if ui.button("Remove from library").clicked() {
                    *action = Some(Action::Remove(id));
                }
            }
        });
    }
}
