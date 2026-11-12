//! The asset library window: folders on the left, a grid of tiles, and the
//! selection's details on the right. Only the rows on screen are drawn and
//! the filtered list is cached, so it stays quick with thousands of assets.

use std::collections::BTreeSet;

use egui::{Rect, RichText, Sense, Ui, Vec2};

use super::Workspace;
use super::assets_ui::Dragged;
use crate::panels::appearance::paint_badge;
use crate::panels::assets::{AssetRef, AssetSummary, DEFAULT_FOLDER, EXAMPLES_FOLDER, ports_text};
use crate::panels::optics::model::BlockRef;
use crate::panels::optics::{Placeable, Tool};
use crate::panels::project::{Catalog, CatalogEntry, DocId};
use crate::panels::ui_kit;

const TILE: Vec2 = Vec2::new(148.0, 112.0);
/// The window's size when it opens, and the smallest it can be made.
const WINDOW_SIZE: [f32; 2] = [1000.0, 640.0];
const MIN_SIZE: [f32; 2] = [640.0, 360.0];
const FOLDERS_WIDTH: f32 = 190.0;
const DETAILS_WIDTH: f32 = 250.0;
const GAP: f32 = 8.0;

/// Which folder the grid shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Folder {
    #[default]
    All,
    Project,
    Named(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    #[default]
    Name,
    /// Most recently added first (Shine's examples last).
    Newest,
}

/// One tile: a project component or a library asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Item {
    Project(DocId),
    Asset(AssetKey),
}

/// [`AssetRef`] with an order, for selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssetKey {
    Example(usize),
    Saved(u64),
}

impl From<AssetRef> for AssetKey {
    fn from(a: AssetRef) -> Self {
        match a {
            AssetRef::Example(i) => AssetKey::Example(i),
            AssetRef::Saved(id) => AssetKey::Saved(id.0),
        }
    }
}

impl From<AssetKey> for AssetRef {
    fn from(a: AssetKey) -> Self {
        match a {
            AssetKey::Example(i) => AssetRef::Example(i),
            AssetKey::Saved(id) => AssetRef::Saved(crate::panels::assets::AssetId(id)),
        }
    }
}

#[derive(Default)]
pub struct LibraryWindow {
    pub open: bool,
    pub search: String,
    pub folder: Folder,
    pub sort: Sort,
    /// Selected tiles, and the one Shift+click extends from.
    pub selected: BTreeSet<Item>,
    anchor: Option<Item>,
    /// A new folder name typed in the details pane.
    new_folder: String,
    /// The library assets shown, as indices into its summaries, for a key of
    /// what decides them.
    cache: Option<(CacheKey, Vec<usize>)>,
}

#[derive(Clone, PartialEq)]
struct CacheKey {
    generation: u64,
    search: String,
    folder: Folder,
    sort: Sort,
}

/// What a tile shows.
struct Card<'a> {
    item: Item,
    entry: &'a CatalogEntry,
    folder: &'a str,
}

enum Action {
    /// Click the grid to place it.
    Place(Item),
    /// Being dragged out: placed where it's let go on the grid.
    Drag(Item),
    Open(Item),
    Export(Item),
    Move(Vec<AssetRef>, String),
    AddToLibrary(Vec<DocId>, String),
    Remove(Vec<AssetRef>),
    Import,
    ImportFolder,
}

impl LibraryWindow {
    /// Indices of the library's summaries shown for the current filter.
    fn shown(&mut self, generation: u64, summaries: &[AssetSummary]) -> &[usize] {
        let key = CacheKey { generation, search: self.search.clone(), folder: self.folder.clone(), sort: self.sort };
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut shown: Vec<usize> = summaries
                .iter()
                .enumerate()
                .filter(|(_, a)| match &self.folder {
                    Folder::All => true,
                    Folder::Project => false,
                    Folder::Named(f) => &a.folder == f,
                })
                .filter(|(_, a)| a.matches(&self.search))
                .map(|(i, _)| i)
                .collect();
            match self.sort {
                Sort::Name => shown.sort_by_cached_key(|&i| summaries[i].entry.name.to_lowercase()),
                Sort::Newest => shown.sort_by_key(|&i| std::cmp::Reverse(AssetKey::from(summaries[i].asset))),
            }
            self.cache = Some((key, shown));
        }
        &self.cache.as_ref().expect("just set").1
    }
}

fn entry_matches(e: &CatalogEntry, search: &str) -> bool {
    let search = search.trim().to_lowercase();
    search.is_empty()
        || [&e.name, &e.description, &e.initials].iter().any(|t| t.to_lowercase().contains(&search))
        || e.interface.inputs.iter().chain(&e.interface.outputs).chain(&e.interface.signal_inputs).chain(&e.interface.signal_outputs).any(|t| t.to_lowercase().contains(&search))
}

impl Workspace {
    pub fn open_library_window(&mut self) {
        self.library_window.open = true;
    }

    pub(super) fn library_window_ui(&mut self, ctx: &egui::Context) {
        if !self.library_window.open {
            return;
        }
        let mut open = true;
        let mut action = None;
        if ctx.embed_viewports() {
            // No separate windows here (the browser, tests): a window inside
            // the app, opened at its full size and resizable.
            ui_kit::window(ctx, "Asset library")
                .id(egui::Id::unique("asset_library_window"))
                .open(&mut open)
                .default_size(WINDOW_SIZE)
                .min_size(MIN_SIZE)
                .resizable(true)
                .collapsible(false)
                .scroll(false)
                .show(ctx, |ui| self.library_body(ui, &mut action));
        } else {
            // Its own window, like a file picker, beside the editor.
            let builder = egui::ViewportBuilder::default()
                .with_title("Asset library — Shine")
                .with_inner_size(WINDOW_SIZE)
                .with_min_inner_size(MIN_SIZE);
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("shine_asset_library"), builder, |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| self.library_body(ui, &mut action));
                if ui.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            });
        }
        if !open {
            self.library_window.open = false;
        }
        self.apply_library_action(action);
    }

    /// The window's contents, laid out in fixed areas of the space it has,
    /// so the window keeps the size it was given (or dragged to).
    fn library_body(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        let catalog = self.catalog();
        let summaries = self.library.summaries();
        let generation = self.library.generation();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.library_window.search).hint_text("Search name, description, folder, port, parameter").desired_width(320.0))
                .accessible_name("Search the asset library");
            egui::ComboBox::from_id_salt("library_sort")
                .selected_text(match self.library_window.sort {
                    Sort::Name => "Name A–Z",
                    Sort::Newest => "Newest first",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.library_window.sort, Sort::Name, "Name A–Z");
                    ui.selectable_value(&mut self.library_window.sort, Sort::Newest, "Newest first");
                })
                .response
                .accessible_name("Sort by");
            if ui.button("Import files…").on_hover_text("Add .shinecomp files, or every component of a .shine project").clicked() {
                *action = Some(Action::Import);
            }
            if cfg!(not(target_arch = "wasm32"))
                && ui
                    .button("Import folder…")
                    .on_hover_text("Add every component file in a folder and its subfolders; each folder becomes a library folder")
                    .clicked()
            {
                *action = Some(Action::ImportFolder);
            }
        });
        ui.separator();
        let area = ui.available_rect_before_wrap();
        let folders = Rect::from_min_size(area.min, egui::vec2(FOLDERS_WIDTH, area.height()));
        let details = Rect::from_min_max(egui::pos2(area.max.x - DETAILS_WIDTH, area.min.y), area.max);
        let grid = Rect::from_min_max(egui::pos2(folders.max.x + 10.0, area.min.y), egui::pos2(details.min.x - 10.0, area.max.y));
        let line = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().vline(folders.max.x + 5.0, area.y_range(), line);
        ui.painter().vline(details.min.x - 5.0, area.y_range(), line);
        let inside = |rect: Rect, ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                add(ui);
            });
        };
        inside(folders, ui, &mut |ui| self.library_folders_ui(ui, &catalog, action));
        inside(grid, ui, &mut |ui| self.library_grid_ui(ui, &catalog, &summaries, generation, action));
        inside(details, ui, &mut |ui| {
            egui::ScrollArea::vertical().id_salt("library_details").auto_shrink([false, false]).show(ui, |ui| {
                self.library_details_ui(ui, &catalog, &summaries, action);
            });
        });
        // The whole area is the window's: it neither grows nor shrinks with what's shown.
        ui.allocate_rect(area, Sense::hover());
    }

    fn library_folders_ui(&mut self, ui: &mut Ui, catalog: &Catalog, action: &mut Option<Action>) {
        let w = &mut self.library_window;
        let total = self.library.summaries().len() + catalog.components().count();
        let mut row = |ui: &mut Ui, folder: Folder, label: String| {
            let selected = w.folder == folder;
            if ui.add(egui::Button::selectable(selected, label).min_size(egui::vec2(ui.available_width(), 22.0))).clicked() {
                w.folder = folder;
                w.selected.clear();
            }
        };
        row(ui, Folder::All, format!("All  ({total})"));
        let project = catalog.components().count();
        // Library components dropped on "This project" are only placed: nothing to file.
        row(ui, Folder::Project, format!("This project  ({project})"));
        ui.add_space(4.0);
        ui.label(RichText::new("LIBRARY").small().strong().weak());
        let examples = self.library.summaries().iter().filter(|a| a.folder == EXAMPLES_FOLDER).count();
        row(ui, Folder::Named(EXAMPLES_FOLDER.into()), format!("{EXAMPLES_FOLDER}  ({examples})"));
        let mut folders = self.library.folder_counts();
        if !folders.iter().any(|(f, _)| f == DEFAULT_FOLDER) {
            folders.insert(0, (DEFAULT_FOLDER.to_owned(), 0));
        }
        egui::ScrollArea::vertical().id_salt("library_folders").show(ui, |ui| {
            for (folder, n) in folders {
                // Drop tiles here to file them (or keep project components).
                let (_, dropped) = ui.dnd_drop_zone::<Dragged, ()>(egui::Frame::NONE, |ui| {
                    row(ui, Folder::Named(folder.clone()), format!("🗀  {folder}  ({n})"));
                });
                match dropped.as_deref() {
                    Some(Dragged::Project(id)) => *action = Some(Action::AddToLibrary(vec![*id], folder.clone())),
                    Some(Dragged::Saved(id)) => *action = Some(Action::Move(vec![AssetRef::Saved(*id)], folder.clone())),
                    Some(Dragged::Example) | None => {}
                }
            }
        });
    }

    fn library_grid_ui(&mut self, ui: &mut Ui, catalog: &Catalog, summaries: &[AssetSummary], generation: u64, action: &mut Option<Action>) {
        let w = &mut self.library_window;
        let search = w.search.clone();
        let mut cards: Vec<Card<'_>> = Vec::new();
        if matches!(w.folder, Folder::All | Folder::Project) {
            let mut mine: Vec<&CatalogEntry> = catalog.components().filter(|e| entry_matches(e, &search)).collect();
            mine.sort_by_cached_key(|e| e.name.to_lowercase());
            cards.extend(mine.into_iter().map(|e| Card { item: Item::Project(e.id), entry: e, folder: "This project" }));
        }
        let shown = w.shown(generation, summaries).to_vec();
        cards.extend(shown.iter().map(|&i| {
            let a = &summaries[i];
            Card { item: Item::Asset(a.asset.into()), entry: &a.entry, folder: a.folder.as_str() }
        }));

        ui.label(RichText::new(match cards.len() {
            0 => "Nothing here".to_owned(),
            1 => "1 component".to_owned(),
            n => format!("{n} components"),
        }).small().weak());
        if cards.is_empty() {
            ui.label(RichText::new(if search.trim().is_empty() { "Drop .shinecomp files on a folder, or drag a project component there." } else { "Nothing matches the search." }).weak());
            return;
        }
        let columns = (((ui.available_width() + GAP) / (TILE.x + GAP)).floor() as usize).max(1);
        let rows = cards.len().div_ceil(columns);
        let show_folder = w.folder == Folder::All;
        let mut clicked: Option<(Item, egui::Modifiers)> = None;
        let mut double: Option<Item> = None;
        let mut dragged: Option<Item> = None;
        egui::ScrollArea::vertical().id_salt("library_grid").auto_shrink([false, false]).show_rows(ui, TILE.y + GAP, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for card in cards.iter().skip(r * columns).take(columns) {
                        let response = tile(ui, card, w.selected.contains(&card.item), show_folder);
                        if response.double_clicked() {
                            double = Some(card.item);
                        } else if response.clicked() {
                            clicked = Some((card.item, ui.input(|i| i.modifiers)));
                        }
                        if response.drag_started() {
                            dragged = Some(card.item);
                        }
                    }
                });
                ui.add_space(GAP - ui.spacing().item_spacing.y);
            }
        });
        let order: Vec<Item> = cards.iter().map(|c| c.item).collect();
        if let Some((item, m)) = clicked {
            if m.command {
                if !w.selected.remove(&item) {
                    w.selected.insert(item);
                }
                w.anchor = Some(item);
            } else if m.shift
                && let Some(a) = w.anchor.and_then(|a| order.iter().position(|x| *x == a))
                && let Some(b) = order.iter().position(|x| *x == item)
            {
                w.selected = order[a.min(b)..=a.max(b)].iter().copied().collect();
            } else {
                w.selected = [item].into();
                w.anchor = Some(item);
            }
        }
        if let Some(item) = double {
            *action = Some(Action::Open(item));
        }
        if let Some(item) = dragged {
            *action = Some(Action::Drag(item));
            egui::DragAndDrop::set_payload(
                ui.ctx(),
                match item {
                    Item::Project(id) => Dragged::Project(id),
                    Item::Asset(AssetKey::Saved(id)) => Dragged::Saved(crate::panels::assets::AssetId(id)),
                    Item::Asset(AssetKey::Example(_)) => Dragged::Example,
                },
            );
        }
    }

    fn library_details_ui(&mut self, ui: &mut Ui, catalog: &Catalog, summaries: &[AssetSummary], action: &mut Option<Action>) {
        let w = &mut self.library_window;
        let selected: Vec<Item> = w.selected.iter().copied().collect();
        let assets: Vec<AssetRef> = selected
            .iter()
            .filter_map(|i| match i {
                Item::Asset(k @ AssetKey::Saved(_)) => Some(AssetRef::from(*k)),
                _ => None,
            })
            .collect();
        let project: Vec<DocId> = selected.iter().filter_map(|i| if let Item::Project(id) = i { Some(*id) } else { None }).collect();
        let entry_of = |item: Item| -> Option<(&CatalogEntry, String)> {
            match item {
                Item::Project(id) => catalog.get(id).map(|e| (e, "This project".to_owned())),
                Item::Asset(k) => summaries.iter().find(|a| a.asset == AssetRef::from(k)).map(|a| (&a.entry, a.folder.clone())),
            }
        };
        match selected.as_slice() {
            [] => {
                ui.label(RichText::new("Select a component to see it here.").weak());
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Double-click opens it in a tab. Drag it onto the grid to place it, or onto a folder to file it. Ctrl+click and Shift+click select several.")
                        .small()
                        .weak(),
                );
                return;
            }
            [one] => {
                let Some((e, folder)) = entry_of(*one) else { return };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 56.0), Sense::hover());
                paint_badge(ui.painter(), rect, &e.appearance, &e.name, &e.initials);
                ui.heading(&e.name);
                ui.label(RichText::new(format!("in {folder}")).small().weak());
                if !e.description.is_empty() {
                    ui.label(&e.description);
                }
                ui.add_space(4.0);
                ui.label(RichText::new(ports_text(&e.interface)).small());
                let i = &e.interface;
                for (what, names) in [("Light in", &i.inputs), ("Light out", &i.outputs), ("Signals in", &i.signal_inputs), ("Signals out", &i.signal_outputs)] {
                    if !names.is_empty() {
                        ui.label(RichText::new(format!("{what}: {}", names.join(", "))).small().weak());
                    }
                }
                if !e.parameters.is_empty() {
                    let names: Vec<&str> = e.parameters.iter().map(|p| p.name.as_str()).collect();
                    ui.label(RichText::new(format!("Parameters: {}", names.join(", "))).small().weak());
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Place").on_hover_text("Then click the grid").clicked() {
                        *action = Some(Action::Place(*one));
                    }
                    if ui.button("Open in a tab").clicked() {
                        *action = Some(Action::Open(*one));
                    }
                    if ui.button("Export…").on_hover_text("Save it as a .shinecomp file to share").clicked() {
                        *action = Some(Action::Export(*one));
                    }
                });
            }
            many => {
                ui.heading(format!("{} selected", many.len()));
            }
        }
        ui.add_space(8.0);
        // Filing: library assets move, project components are added.
        if !assets.is_empty() || !project.is_empty() {
            let verb = if assets.is_empty() { "Add to folder" } else { "Move to folder" };
            ui.label(RichText::new(verb).small().strong());
            let folders: Vec<String> = self.library.folders();
            let mut target = None;
            ui.horizontal_wrapped(|ui| {
                for f in folders.iter().map(String::as_str).chain((!folders.iter().any(|f| f == DEFAULT_FOLDER)).then_some(DEFAULT_FOLDER)) {
                    if ui.small_button(f).clicked() {
                        target = Some(f.to_owned());
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut w.new_folder).hint_text("New folder").desired_width(140.0)).accessible_name("New folder name");
                if ui.add_enabled(!w.new_folder.trim().is_empty(), egui::Button::new("Make")).clicked() {
                    target = Some(w.new_folder.trim().to_owned());
                    w.new_folder.clear();
                }
            });
            if let Some(folder) = target {
                *action = Some(if assets.is_empty() { Action::AddToLibrary(project.clone(), folder) } else { Action::Move(assets.clone(), folder) });
            }
        }
        if !assets.is_empty() {
            ui.add_space(8.0);
            let label = if assets.len() == 1 { "Remove from library".to_owned() } else { format!("Remove {} from library", assets.len()) };
            if ui.button(label).on_hover_text("Projects using them keep their copies").clicked() {
                *action = Some(Action::Remove(assets));
            }
        }
    }

    fn apply_library_action(&mut self, action: Option<Action>) {
        let placeable = |item: Item| match item {
            Item::Project(id) => Placeable::Block(BlockRef::Component(id)),
            Item::Asset(k) => Placeable::Asset(k.into()),
        };
        match action {
            Some(Action::Place(item)) => {
                if let Some(p) = self.active_panel() {
                    p.tool = Some(Tool::Place(placeable(item)));
                }
                self.notify("Click the grid to place it (Esc stops)");
            }
            Some(Action::Drag(item)) => {
                if let Some(p) = self.active_panel() {
                    p.dragging_new = Some(placeable(item));
                }
            }
            Some(Action::Open(Item::Project(id))) => self.open_tab_asking(id),
            Some(Action::Open(Item::Asset(k))) => self.open_asset(k.into()),
            Some(Action::Export(Item::Project(id))) => self.export_component(id),
            Some(Action::Export(Item::Asset(k))) => self.export_asset(k.into()),
            Some(Action::Move(assets, folder)) => {
                for a in assets {
                    if let AssetRef::Saved(id) = a {
                        self.library.set_folder(id, &folder);
                    }
                }
                self.library_window.folder = Folder::Named(folder);
            }
            Some(Action::AddToLibrary(ids, folder)) => {
                for id in ids {
                    self.add_to_library(id, &folder);
                }
            }
            Some(Action::Remove(assets)) => {
                let n = assets.len();
                for a in assets {
                    if let AssetRef::Saved(id) = a {
                        self.library.remove(id);
                    }
                    self.library_window.selected.remove(&Item::Asset(a.into()));
                }
                self.notify(if n == 1 { "Removed from your library".to_owned() } else { format!("Removed {n} components from your library") });
            }
            Some(Action::Import) => self.pick_library_files(),
            Some(Action::ImportFolder) => self.pick_library_folder(),
            None => {}
        }
    }
}

/// One tile: picture or badge, name, ports, and the folder when showing all.
fn tile(ui: &mut Ui, card: &Card<'_>, selected: bool, show_folder: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TILE, Sense::click_and_drag());
    let e = card.entry;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &e.name));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let fill = if selected { ui.visuals().selection.bg_fill.gamma_multiply(0.35) } else if response.hovered() { visuals.weak_bg_fill } else { ui.visuals().faint_bg_color };
        let stroke = if selected { ui.visuals().selection.stroke } else { egui::Stroke::new(1.0, visuals.bg_stroke.color) };
        ui.painter().rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
        let badge = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 30.0), egui::vec2(56.0, 40.0));
        paint_badge(ui.painter(), badge, &e.appearance, &e.name, &e.initials);
        let text = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let painter = ui.painter().with_clip_rect(rect.shrink(4.0));
        let name = painter.layout(e.name.clone(), egui::FontId::proportional(12.5), text, rect.width() - 14.0);
        let name_top = badge.bottom() + 6.0;
        let name_height = name.size().y.min(32.0);
        painter.galley(egui::pos2(rect.center().x - name.size().x / 2.0, name_top), name, text);
        let below = if show_folder { card.folder.to_owned() } else { ports_text(&e.interface) };
        painter.text(
            egui::pos2(rect.center().x, (name_top + name_height + 2.0).min(rect.bottom() - 14.0)),
            egui::Align2::CENTER_TOP,
            below,
            egui::FontId::proportional(10.0),
            weak,
        );
    }
    let mut tip = e.name.clone();
    if !e.description.is_empty() {
        tip += &format!("\n{}", e.description);
    }
    tip += &format!("\n{}\nIn “{}”\n\nDouble-click to open it in a tab; drag it onto the grid to place it.", ports_text(&e.interface), card.folder);
    response.on_hover_text(tip)
}
