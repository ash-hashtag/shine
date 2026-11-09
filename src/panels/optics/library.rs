//! The parts library, the add and context menus, and the inspector.

use egui::{Color32, Id, Pos2, RichText, Ui};

use super::examples::component_examples;
use super::canvas::source_letter;
use super::model::{BlockRef, Builtin, Item, OpticComponentKind, Terminal, cell_of};
use super::{Drag, Hit, MenuChoice, OpticsPanel, Request, Tool, WireRef, input_number, output_number, render};
use crate::panels::circuit::sim;
use crate::panels::prefs::{Palette, WAVELENGTH_PRESETS, is_visible};
use crate::panels::project::{BlockDisplay, Catalog};
use crate::panels::{ui_kit, units};

/// Something the library can put on the grid.
#[derive(Clone, Debug, PartialEq)]
pub enum Placeable {
    /// A free-space primitive, drawn as itself.
    Part(OpticComponentKind),
    /// A built-in block or a project component, drawn as a box.
    Block(BlockRef),
    /// One of [`component_examples`]; the host adds it to the project first.
    Example(usize),
}

impl Placeable {
    pub fn name(&self, catalog: &Catalog) -> String {
        match self {
            Self::Part(OpticComponentKind::Detector { threshold }) if *threshold > 0.0 => "Threshold detector".into(),
            Self::Part(kind) => kind.name().into(),
            Self::Block(block) => catalog.display(block).name,
            Self::Example(i) => component_examples().get(*i).map_or("Example", |e| e.name).into(),
        }
    }
}

pub struct Entry {
    pub section: &'static str,
    pub item: Placeable,
    pub name: String,
    pub description: String,
}

/// Library sections, in order.
pub const SECTIONS: [&str; 7] = [
    "Light & detection",
    "Free-space optics",
    "Electronics & I/O",
    "Example components",
    "My components",
    "Waveguide blocks",
    "Component ports",
];

pub fn part_description(kind: &OpticComponentKind) -> &'static str {
    match kind {
        OpticComponentKind::Laser { .. } => {
            "Coherent source. Its light leaves as a beam the way it faces, or drag from its light output to a box's light input to wire it with a waveguide. Wire a signal to its top to switch it: the amplitude is multiplied by the signal."
        }
        OpticComponentKind::Detector { threshold } if *threshold > 0.0 => {
            "Photodiode with a comparator: outputs 1 when the power reaches the threshold, else 0."
        }
        OpticComponentKind::Detector { .. } => "Photodiode: absorbs light; its output carries the power it receives.",
        OpticComponentKind::BeamSplitter { .. } => "Partially reflecting mirror: splits a beam in two.",
        OpticComponentKind::Mirror { .. } => "Reflects light. Rotate in 45° steps with R.",
        OpticComponentKind::Lens => "Thin lens (passes light straight through in this model).",
        OpticComponentKind::PhaseShifter { .. } => "Delays the light's phase. A signal on top adds π per unit: 1 flips an interferometer.",
        OpticComponentKind::Attenuator { .. } => "Absorbs part of the light. A signal on top scales its transmission.",
        OpticComponentKind::InputPort { .. } => "Where light enters when this layout is used as a component box (left edge, top to bottom).",
        OpticComponentKind::OutputPort => "Where light leaves when this layout is used as a component box (right edge, top to bottom).",
        OpticComponentKind::SignalIn => "Electrical input when this layout is used as a component box (top edge, left to right). Click it to switch it between 0 and 1 and try the layout on its own.",
        OpticComponentKind::SignalOut => "Electrical output when this layout is used as a component box (bottom edge).",
    }
}

fn matches(search: &str, name: &str, description: &str) -> bool {
    let search = search.trim().to_lowercase();
    search.is_empty() || name.to_lowercase().contains(&search) || description.to_lowercase().contains(&search)
}

/// Every library entry matching `search`, grouped by section in [`SECTIONS`] order.
pub fn entries(catalog: &Catalog, with_examples: bool, search: &str) -> Vec<Entry> {
    let palette = OpticComponentKind::palette();
    let mut out = Vec::new();
    let mut part = |section, kind: OpticComponentKind| {
        let item = Placeable::Part(kind);
        out.push(Entry {
            section,
            name: item.name(catalog),
            description: part_description(&kind).into(),
            item,
        });
    };
    part(SECTIONS[0], palette[0]);
    part(SECTIONS[0], palette[6]);
    part(SECTIONS[0], OpticComponentKind::Detector { threshold: 0.5 });
    // The lens is left out: a ray-on-grid model can't focus, so it would
    // only pass light through. Old files that contain one still open.
    for &k in palette[1..=5].iter().filter(|k| **k != OpticComponentKind::Lens) {
        part(SECTIONS[1], k);
    }
    for &k in &palette[7..] {
        part(SECTIONS[6], k);
    }
    let block = |section, b: Builtin| {
        let info = b.info();
        Entry {
            section,
            name: info.name.into(),
            description: info.description.into(),
            item: Placeable::Block(BlockRef::Builtin(b)),
        }
    };
    out.extend(Builtin::electronic_palette().into_iter().map(|b| block(SECTIONS[2], b)));
    let examples = component_examples();
    if with_examples {
        out.extend(examples.iter().enumerate().map(|(i, e)| Entry {
            section: SECTIONS[3],
            item: Placeable::Example(i),
            name: e.name.into(),
            description: e.description.into(),
        }));
    }
    out.extend(
        catalog
            .components()
            .filter(|e| !with_examples || !examples.iter().any(|x| x.name == e.name))
            .map(|e| Entry {
                section: SECTIONS[4],
                item: Placeable::Block(BlockRef::Component(e.id)),
                name: e.name.clone(),
                description: e.description.clone(),
            }),
    );
    out.extend(
        Builtin::palette()
            .into_iter()
            .filter(|b| {
                !matches!(b, Builtin::Laser { .. } | Builtin::Detector | Builtin::InputPin { .. } | Builtin::OutputPin { .. })
            })
            .map(|b| block(SECTIONS[5], b)),
    );
    out.retain(|e| matches(search, &e.name, &e.description));
    out.sort_by_key(|e| SECTIONS.iter().position(|s| *s == e.section));
    out
}

pub fn badge_fill(d: &BlockDisplay) -> Color32 {
    if d.component.is_some() {
        ui_kit::accent(&d.name)
    } else {
        Color32::from_rgb(88, 96, 112)
    }
}

/// Library icon for an entry.
fn draw_icon(painter: &egui::Painter, rect: egui::Rect, item: &Placeable, catalog: &Catalog) {
    match item {
        Placeable::Part(kind) => render::draw_component(painter, rect.center(), kind, 1.0, 0.6),
        Placeable::Block(block) => {
            let d = catalog.display(block);
            ui_kit::badge(painter, rect.shrink(1.0), &d.initials, badge_fill(&d), Color32::WHITE);
        }
        Placeable::Example(i) => {
            let e = &component_examples()[*i];
            ui_kit::badge(painter, rect.shrink(1.0), e.initials, ui_kit::accent(e.name), Color32::WHITE);
        }
    }
}

fn ports_line(names: &[String]) -> String {
    if names.is_empty() { "—".to_owned() } else { names.join(", ") }
}

/// Tooltip for a box: what it is and how it connects.
pub fn block_tooltip(d: &BlockDisplay) -> String {
    let mut text = format!("{}\n{}", d.name, if d.description.is_empty() { "No description" } else { &d.description });
    if !d.inputs.is_empty() || !d.outputs.is_empty() {
        text += &format!("\n\nLight in: {}\nLight out: {}", ports_line(&d.inputs), ports_line(&d.outputs));
    }
    if !d.signal_inputs.is_empty() || !d.signal_outputs.is_empty() {
        text += &format!(
            "\n\nSignals in: {}\nSignals out: {}",
            ports_line(&d.signal_inputs),
            ports_line(&d.signal_outputs)
        );
    }
    text
}

impl OpticsPanel {
    fn library_row(&mut self, ui: &mut Ui, entry: &Entry) {
        let placeable = match (&entry.item, self.doc_id) {
            (Placeable::Block(BlockRef::Component(id)), Some(me)) => self.catalog.can_place(*id, me),
            _ => true,
        };
        let selected = matches!(&self.tool, Some(Tool::Place(t)) if *t == entry.item);
        let catalog = &self.catalog;
        let response = ui
            .add_enabled_ui(placeable, |ui| {
                ui_kit::item_row_sized(ui, selected, &entry.name, 22.0, |painter, rect| draw_icon(painter, rect, &entry.item, catalog))
            })
            .inner;
        if response.drag_started() {
            self.dragging_new = Some(entry.item.clone());
        }
        if response.clicked() {
            self.tool = if selected { None } else { Some(Tool::Place(entry.item.clone())) };
        }
        let tip = match &entry.item {
            _ if !placeable => format!("{}\n\nCan't be placed here: it uses this document.", entry.name),
            Placeable::Block(b) => block_tooltip(&self.catalog.display(b)),
            Placeable::Example(_) => format!(
                "{}\n{}\n\nPlacing it adds a copy to your project that you can open (Tab) and change.",
                entry.name, entry.description
            ),
            Placeable::Part(_) => format!("{}\n{}", entry.name, entry.description),
        };
        response.on_hover_text(tip);
    }

    /// Parts library: search, folders (collapsible), and tools.
    pub fn palette_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let browse = ui
                .add(egui::Button::new("▦").min_size(egui::vec2(24.0, 0.0)))
                .on_hover_text("Browse every part and component by category");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search parts")
                    .desired_width(f32::INFINITY),
            );
            if browse.clicked() {
                self.show_browser = !self.show_browser;
            }
        });
        ui.add_space(2.0);
        let search = self.search.clone();
        let items = entries(&self.catalog, self.embedded, &search);
        if !search.trim().is_empty() {
            for entry in &items {
                self.library_row(ui, entry);
            }
            if items.is_empty() {
                ui.label(RichText::new("Nothing matches").weak());
            }
        } else {
            for (k, section) in SECTIONS.into_iter().enumerate() {
                let rows: Vec<&Entry> = items.iter().filter(|e| e.section == section).collect();
                let my_components = section == SECTIONS[4];
                if rows.is_empty() && !(my_components && self.embedded) {
                    continue;
                }
                // Folders: the essentials start open, the rest stay out of the way.
                egui::CollapsingHeader::new(RichText::new(format!("{section}  ·  {}", rows.len())).strong())
                    .id_salt(("library_folder", section))
                    .default_open(k == 0 || my_components)
                    .show(ui, |ui| {
                        for entry in rows {
                            self.library_row(ui, entry);
                        }
                        if my_components
                            && self.embedded
                            && ui
                                .small_button("+ New component")
                                .on_hover_text("Start an empty component with ports, ready to fill and reuse")
                                .clicked()
                        {
                            self.requests.push(Request::NewComponent);
                        }
                    });
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.selectable_label(self.tool.is_none(), "Select").clicked() {
                self.tool = None;
            }
            if ui.selectable_label(self.tool == Some(Tool::DrawLine), "Line").clicked() {
                self.tool = Some(Tool::DrawLine);
            }
        });
        ui.label(RichText::new("Drag onto the grid · drag ports to wire").small().weak());
    }

    /// The categorised browser: folders on the left, every part as a tile.
    pub(super) fn browser_ui(&mut self, ctx: &egui::Context) {
        if !self.show_browser {
            return;
        }
        let mut open = true;
        let mut picked: Option<(Placeable, bool)> = None;
        ui_kit::window(ctx, "Parts & components")
            .id(Id::unique("optics_browser"))
            .open(&mut open)
            .collapsible(false)
            .default_size([720.0, 440.0])
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.browser_search)
                        .hint_text("Search everything")
                        .desired_width(f32::INFINITY),
                );
                ui.label(RichText::new("Click a tile, then click the grid to place it, or drag it onto the grid.").small().weak());
                ui.add_space(4.0);
                let all = entries(&self.catalog, self.embedded, &self.browser_search);
                let searching = !self.browser_search.trim().is_empty();
                ui.horizontal_top(|ui| {
                    // Folders.
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        for (k, section) in SECTIONS.into_iter().enumerate() {
                            let n = all.iter().filter(|e| e.section == section).count();
                            if n == 0 && !(k == 4 && self.embedded) {
                                continue;
                            }
                            let label = format!("🗀  {section}  ({n})");
                            if ui.selectable_label(!searching && self.browser_section == k, label).clicked() {
                                self.browser_section = k;
                                self.browser_search.clear();
                            }
                        }
                        if self.embedded && ui.button("+ New component").clicked() {
                            self.requests.push(Request::NewComponent);
                            self.show_browser = false;
                        }
                    });
                    ui.separator();
                    // Tiles of the chosen folder, or every match while searching.
                    egui::ScrollArea::vertical().id_salt("browser_tiles").show(ui, |ui| {
                        let shown: Vec<&Entry> = all
                            .iter()
                            .filter(|e| searching || e.section == SECTIONS[self.browser_section.min(SECTIONS.len() - 1)])
                            .collect();
                        if shown.is_empty() {
                            ui.label(RichText::new(if searching { "Nothing matches" } else { "Nothing here yet" }).weak());
                        }
                        ui.horizontal_wrapped(|ui| {
                            for entry in shown {
                                let r = self.tile(ui, entry);
                                if r.drag_started() {
                                    picked = Some((entry.item.clone(), true));
                                } else if r.clicked() {
                                    picked = Some((entry.item.clone(), false));
                                }
                            }
                        });
                    });
                });
            });
        if !open {
            self.show_browser = false;
        }
        if let Some((item, dragged)) = picked {
            self.notify(format!("Placing {}: click the grid", item.name(&self.catalog)));
            if dragged {
                self.dragging_new = Some(item.clone());
            }
            self.tool = Some(Tool::Place(item));
            self.show_browser = false;
        }
    }

    /// One tile in the browser: icon, name and the start of the description.
    fn tile(&self, ui: &mut Ui, entry: &Entry) -> egui::Response {
        let size = egui::vec2(160.0, 82.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            ui.painter().rect(
                rect,
                6.0,
                if response.hovered() { visuals.weak_bg_fill } else { ui.visuals().faint_bg_color },
                egui::Stroke::new(1.0, visuals.bg_stroke.color),
                egui::StrokeKind::Inside,
            );
            let icon = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(22.0, 22.0));
            draw_icon(ui.painter(), icon, &entry.item, &self.catalog);
            let text = ui.visuals().text_color();
            let name = ui.painter().layout(entry.name.clone(), egui::FontId::proportional(13.0), text, rect.width() - 46.0);
            let name_bottom = icon.top() + 2.0 + name.size().y;
            ui.painter().galley(icon.right_top() + egui::vec2(8.0, 2.0), name, text);
            let mut description = entry.description.clone();
            if description.chars().count() > 70 {
                description = description.chars().take(68).collect::<String>() + "…";
            }
            let weak = ui.visuals().weak_text_color();
            let galley = ui.painter().layout(description, egui::FontId::proportional(10.5), weak, rect.width() - 16.0);
            let top = name_bottom.max(icon.bottom()) + 4.0;
            ui.painter().with_clip_rect(rect.shrink(2.0)).galley(egui::pos2(rect.left() + 8.0, top), galley, weak);
        }
        response.on_hover_text(format!("{}\n{}", entry.name, entry.description))
    }

    // ---- menus ------------------------------------------------------------

    pub(super) fn menu_ui(&mut self, ctx: &egui::Context) {
        let Some(mut menu) = self.menu.clone() else { return };
        let has_lines = {
            let cell = cell_of(menu.world);
            self.layout.lines.iter().any(|l| cell_of(l.start) == cell || cell_of(l.end) == cell)
        };
        let just_opened = menu.opened_at >= self.time;
        let shown = egui::Area::new(Id::unique("optics_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| {
                        let mut choice = None;
                        ui.set_min_width(220.0);
                        ui.set_max_width(260.0);
                        match menu.target {
                            Some(Hit::Part(i)) if i < self.layout.components.len() => {
                                ui.label(RichText::new(self.layout.components[i].kind.name()).strong());
                                if ui.button("⟳  Rotate                R").clicked() {
                                    choice = Some(MenuChoice::Rotate(1));
                                }
                                if ui.button("⟲  Rotate back      Shift+R").clicked() {
                                    choice = Some(MenuChoice::Rotate(-1));
                                }
                                if ui.button("⎘  Duplicate          Shift+D").clicked() {
                                    choice = Some(MenuChoice::Duplicate);
                                }
                                if !self.sweep_params(Item::Part(i)).is_empty()
                                    && ui.button("〰  Sweep…").clicked()
                                {
                                    choice = Some(MenuChoice::Sweep);
                                }
                                if ui.button("🗑  Delete                 X").clicked() {
                                    choice = Some(MenuChoice::Delete);
                                }
                                ui.separator();
                            }
                            Some(Hit::Box(b)) if b < self.layout.instances.len() => {
                                let d = self.catalog.display(&self.layout.instances[b].block);
                                ui.label(RichText::new(&d.name).strong());
                                if d.component.is_some() && ui.button("↗  Open definition   Tab").clicked() {
                                    choice = Some(MenuChoice::Inspect);
                                }
                                if ui.button("⎘  Duplicate          Shift+D").clicked() {
                                    choice = Some(MenuChoice::Duplicate);
                                }
                                let id = self.layout.instances[b].id;
                                if !self.sweep_params(Item::Box(id)).is_empty()
                                    && ui.button("〰  Sweep…").clicked()
                                {
                                    choice = Some(MenuChoice::Sweep);
                                }
                                if ui.button("🗑  Delete                 X").clicked() {
                                    choice = Some(MenuChoice::Delete);
                                }
                                ui.separator();
                            }
                            Some(Hit::Waveguide(w)) => {
                                if ui.button("🗑  Delete waveguide").clicked() {
                                    choice = Some(MenuChoice::DeleteWire(WireRef::Waveguide(w)));
                                }
                                ui.separator();
                            }
                            Some(Hit::Signal(w)) => {
                                if ui.button("🗑  Delete signal wire").clicked() {
                                    choice = Some(MenuChoice::DeleteWire(WireRef::Signal(w)));
                                }
                                ui.separator();
                            }
                            _ => {}
                        }
                        if has_lines && ui.button("Delete lines here").clicked() {
                            choice = Some(MenuChoice::DeleteLines);
                        }
                        ui.label(RichText::new("Add here").strong());
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut menu.search)
                                .hint_text("Type to search…")
                                .desired_width(f32::INFINITY),
                        );
                        if just_opened {
                            search.request_focus();
                        }
                        let items = entries(&self.catalog, self.embedded, &menu.search);
                        let enter = search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter && let Some(first) = items.first() {
                            choice = Some(MenuChoice::Add(first.item.clone()));
                        }
                        let catalog = &self.catalog;
                        let row = |ui: &mut Ui, entry: &Entry| {
                            ui_kit::item_row_sized(ui, false, &entry.name, 22.0, |p, r| draw_icon(p, r, &entry.item, catalog))
                                .on_hover_text(&entry.description)
                                .clicked()
                        };
                        if menu.search.trim().is_empty() {
                            // Like Blender's Shift+A: hover a category to open its list.
                            for (k, section) in SECTIONS.into_iter().enumerate() {
                                if !items.iter().any(|e| e.section == section) {
                                    continue;
                                }
                                let open = menu.open_section.is_some_and(|(s, _)| s == k);
                                let response = ui.add(
                                    egui::Button::selectable(open, format!("{section}  ›"))
                                        .min_size(egui::vec2(ui.available_width(), 22.0)),
                                );
                                if response.hovered() || response.clicked() {
                                    menu.open_section = Some((k, response.rect.top()));
                                }
                            }
                        } else {
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                for entry in &items {
                                    if row(ui, entry) {
                                        choice = Some(MenuChoice::Add(entry.item.clone()));
                                    }
                                }
                                if items.is_empty() {
                                    ui.label(RichText::new("Nothing matches").weak());
                                }
                            });
                        }
                        choice
                    })
                    .inner
            });
        // The open category's entries, beside its row.
        let mut choice = shown.inner;
        if menu.search.trim().is_empty()
            && let Some((k, y)) = menu.open_section
        {
            let section = SECTIONS[k];
            let items = entries(&self.catalog, self.embedded, "");
            let catalog = &self.catalog;
            egui::Area::new(Id::unique("optics_submenu"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(shown.response.rect.right() + 2.0, y - 6.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(230.0);
                        for entry in items.iter().filter(|e| e.section == section) {
                            let clicked = ui_kit::item_row_sized(ui, false, &entry.name, 22.0, |p, r| draw_icon(p, r, &entry.item, catalog))
                                .on_hover_text(&entry.description)
                                .clicked();
                            if clicked {
                                choice = Some(MenuChoice::Add(entry.item.clone()));
                            }
                        }
                    });
                });
        }
        if let Some(choice) = choice {
            self.apply_menu_choice(choice, menu.world);
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.menu = None;
            return;
        }
        let on_canvas = ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background));
        if !just_opened && ctx.input(|i| i.pointer.any_pressed()) && !shown.response.contains_pointer() && on_canvas {
            self.menu = None;
            return;
        }
        // Keep the typed search.
        if let Some(m) = &mut self.menu {
            m.search = menu.search;
            m.open_section = menu.open_section;
        }
    }

    // ---- inspector --------------------------------------------------------

    /// Selection properties, simulation settings and readouts.
    pub fn inspector_ui(&mut self, ui: &mut Ui) {
        ui_kit::section(ui, "Selection");
        if let Some(w) = self.selected_wire {
            self.wire_inspector(ui, w);
        } else if let Some(b) = self.editor.boxes.active().filter(|&b| b < self.layout.instances.len()) {
            self.box_inspector(ui, b);
        } else if let Some(i) = self.editor.selection.active().filter(|&i| i < self.layout.components.len()) {
            self.part_inspector(ui, i);
        } else {
            ui.label(RichText::new("Nothing selected").weak());
            ui.label(RichText::new("Click a part or box, or drag a rectangle around several.").small().weak());
        }
        if self.editor.len() > 1 {
            ui.label(RichText::new(format!("{} selected · showing the last clicked", self.editor.len())).small().weak());
        }
        self.simulation_ui(ui);
    }

    fn wire_inspector(&mut self, ui: &mut Ui, w: WireRef) {
        match w {
            WireRef::Waveguide(i) => {
                ui.strong("Waveguide");
                if let Some(o) = self.outcome() {
                    for (field, wl) in o.wire_fields.get(i).into_iter().flatten().zip(&o.wavelengths) {
                        ui.monospace(format!(
                            "{wl:.1} nm  {}  phase {:.3}π",
                            units::format_both(field.norm_sqr(), self.prefs.power_unit),
                            field.arg().rem_euclid(std::f64::consts::TAU) / std::f64::consts::PI
                        ));
                    }
                }
                let length = self.layout.wires.get(i).and_then(|w| w.length_um).filter(|l| *l > 0.0);
                let about = match length {
                    Some(l) => format!("{l} µm of waveguide (n_eff 2.4), set by the MCP server."),
                    None => "An ideal connection: no length, loss or phase. Use a delay line block for a length of waveguide.".to_owned(),
                };
                ui.label(RichText::new(about).small().weak());
            }
            WireRef::Signal(i) => {
                ui.strong("Signal wire");
                if let (Some(o), Some(wire)) = (self.outcome(), self.layout.signals.get(i)) {
                    ui.monospace(format!("value {}", fmt_value(o.signal(wire.from.item, wire.from.port))));
                }
            }
        }
        if ui.small_button("Delete").on_hover_text("X").clicked() {
            self.delete_wire(w);
        }
    }

    fn box_inspector(&mut self, ui: &mut Ui, b: usize) {
        let id = self.layout.instances[b].id;
        let d = self.catalog.display(&self.layout.instances[b].block);
        let (ci, cj) = self.layout.instances[b].cell;
        ui.horizontal(|ui| {
            ui.strong(&d.name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("cell {ci}, {cj}")).weak());
            });
        });
        if !d.description.is_empty() {
            ui.label(RichText::new(&d.description).small().weak());
        }
        let mut changed = false;
        match &mut self.layout.instances[b].block {
            BlockRef::Builtin(block) => changed = builtin_params_ui(ui, block, self.prefs.power_unit),
            BlockRef::Component(doc) => {
                let doc = *doc;
                self.component_params_ui(ui, b, doc);
                if ui.button("Open definition").on_hover_text("Tab, or double-click the box").clicked() {
                    self.apply(super::input::Action::Inspect);
                }
            }
        }
        if changed {
            self.prune_connections();
        }
        if matches!(self.layout.instances[b].block, BlockRef::Builtin(_)) {
            self.exports_ui(ui, Item::Box(id));
        }
        let outcome = self.outcome().cloned();
        if let Some(o) = &outcome {
            let block = &self.layout.instances[b].block;
            match block {
                BlockRef::Builtin(Builtin::Display { bits }) => {
                    let n = input_number(&self.layout, o, id, *bits as usize);
                    ui.horizontal(|ui| {
                        ui.label("Shows");
                        ui.monospace(RichText::new(format!("{n}  ({n:0w$b})", w = *bits as usize)).strong());
                    });
                }
                BlockRef::Builtin(Builtin::Register { bits }) => {
                    let n = output_number(o, id, *bits as usize);
                    ui.horizontal(|ui| {
                        ui.label("Holds");
                        ui.monospace(RichText::new(format!("{n}  ({n:0w$b})", w = *bits as usize)).strong());
                    });
                }
                _ => {}
            }
            let signals: Vec<(String, f64)> = d
                .signal_outputs
                .iter()
                .enumerate()
                .map(|(k, name)| (name.clone(), o.signal(Item::Box(id), k)))
                .collect();
            let light: Vec<(String, f64)> = d
                .outputs
                .iter()
                .enumerate()
                .filter_map(|(k, name)| o.port_power.get(&(id, k)).map(|p| (name.clone(), *p)))
                .collect();
            if !signals.is_empty() || !light.is_empty() {
                egui::CollapsingHeader::new("Outputs").default_open(signals.len() + light.len() <= 6).show(ui, |ui| {
                    egui::Grid::new("box_outputs").num_columns(2).show(ui, |ui| {
                        for (name, v) in &signals {
                            ui.label(name);
                            ui.monospace(fmt_value(*v));
                            ui.end_row();
                        }
                        for (name, p) in &light {
                            ui.label(format!("{name} (light)"));
                            ui.monospace(units::format_power(*p, self.prefs.power_unit));
                            ui.end_row();
                        }
                    });
                });
            }
        }
        ui.horizontal(|ui| {
            if ui.small_button("Duplicate").on_hover_text("Shift+D").clicked() {
                self.apply(super::input::Action::Duplicate);
            }
            if ui.small_button("Delete").on_hover_text("X").clicked() {
                self.apply(super::input::Action::Delete);
            }
        });
    }

    fn part_inspector(&mut self, ui: &mut Ui, i: usize) {
        let reading = self.detector_reading(i);
        let unit = self.prefs.power_unit;
        let iface = sim::interface(&self.layout);
        let driver = self.layout.signal_source(Terminal { item: Item::Part(i), port: 0 });
        let output = self.outcome().map(|o| o.signal(Item::Part(i), 0));
        let mut label = self.layout.label(i).unwrap_or_default().to_owned();
        let component = &mut self.layout.components[i];
        let (ci, cj) = cell_of(component.pos);
        ui.horizontal(|ui| {
            ui.strong(component.kind.name());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("cell {ci}, {cj}")).weak());
            });
        });
        let mut label_changed = false;
        egui::Grid::new("part_params").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let tau = std::f32::consts::TAU;
            match &mut component.kind {
                OpticComponentKind::Laser { amplitude, phase, wavelength, source, color, .. } => {
                    ui.label("Power");
                    let mut a = *amplitude as f64;
                    if units::power_editor(ui, &mut a, unit).changed() {
                        *amplitude = a as f32;
                    }
                    ui.end_row();
                    ui.label("Phase");
                    ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad"))
                        .on_hover_text("Only matters relative to lasers on the same source");
                    ui.end_row();
                    ui.label("Wavelength");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(wavelength).range(200.0..=2000.0).speed(0.5).suffix(" nm"));
                        egui::ComboBox::from_id_salt("laser_wavelength")
                            .selected_text("Presets")
                            .show_ui(ui, |ui| {
                                for &(nm, what) in WAVELENGTH_PRESETS {
                                    ui.selectable_value(wavelength, nm, format!("{nm:.0} nm  {what}"));
                                }
                            });
                    });
                    ui.end_row();
                    ui.label("");
                    let band = if is_visible(*wavelength as f64) { "visible" } else if *wavelength > 780.0 { "infrared" } else { "ultraviolet" };
                    ui.label(RichText::new(band).small().weak());
                    ui.end_row();
                    ui.label("Source");
                    egui::ComboBox::from_id_salt("laser_source")
                        .selected_text(if *source == 0 { "Own (independent)".to_owned() } else { format!("Shared {}", source_letter(*source)) })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(source, 0, "Own (independent)");
                            for s in 1..=8u8 {
                                ui.selectable_value(source, s, format!("Shared {}", source_letter(s)));
                            }
                        })
                        .response
                        .on_hover_text(
                            "Lasers on the same shared source are phase-locked, like light split from one laser, and interfere. \
                             Independent lasers never interfere with each other: their powers add, as with real separate lasers.",
                        );
                    ui.end_row();
                    ui.label("Colour");
                    ui.horizontal(|ui| {
                        let mut own = color.is_some();
                        if ui.checkbox(&mut own, "Own colour").on_hover_text("Draw this laser's light in a colour you choose").changed() {
                            *color = own.then_some(color.unwrap_or([220, 60, 60]));
                        }
                        if let Some(c) = color {
                            ui.color_edit_button_srgb(c);
                        }
                    });
                    ui.end_row();
                }
                OpticComponentKind::BeamSplitter { reflectivity, transparency, .. } => {
                    ui.label("Reflectivity");
                    if ui.add(egui::Slider::new(reflectivity, 0.0..=1.0)).changed() {
                        *transparency = 1.0 - *reflectivity;
                    }
                    ui.end_row();
                }
                OpticComponentKind::PhaseShifter { phase } => {
                    ui.label("Phase");
                    ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad"));
                    ui.end_row();
                }
                OpticComponentKind::Attenuator { transmission } => {
                    ui.label("Transmission");
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(transmission, 0.0..=1.0));
                        ui.label(RichText::new(format!("{:.2} dB loss", units::loss_db(*transmission as f64))).small().weak());
                    });
                    ui.end_row();
                }
                OpticComponentKind::Detector { threshold } => {
                    ui.label("Output");
                    let mut digital = *threshold > 0.0;
                    if ui
                        .checkbox(&mut digital, "bit (comparator)")
                        .on_hover_text("Output 1 when the power reaches the threshold, else 0")
                        .changed()
                    {
                        *threshold = if digital { 0.5 } else { 0.0 };
                    }
                    ui.end_row();
                    if digital {
                        ui.label("Threshold");
                        ui.add(egui::DragValue::new(threshold).range(0.001..=100.0).speed(0.01).suffix(" mW"))
                            .on_hover_text("Light at or above this power reads 1");
                        ui.end_row();
                    }
                    ui.label("Power");
                    match reading {
                        Some(p) => ui.monospace(units::format_both(p, unit)),
                        None => ui.label(RichText::new("simulation off").weak()),
                    };
                    ui.end_row();
                }
                OpticComponentKind::OutputPort => {
                    ui.label("Power");
                    match reading {
                        Some(p) => ui.monospace(units::format_both(p, unit)),
                        None => ui.label(RichText::new("simulation off").weak()),
                    };
                    ui.end_row();
                }
                OpticComponentKind::InputPort { .. }
                | OpticComponentKind::SignalIn
                | OpticComponentKind::SignalOut
                | OpticComponentKind::Mirror { .. }
                | OpticComponentKind::Lens => {}
            }
            let kind = component.kind;
            let port_lists = [
                (&iface.optical_in, "light input"),
                (&iface.optical_out, "light output"),
            ];
            for (list, what) in port_lists {
                if let Some(k) = list.iter().position(|&p| p == Item::Part(i)) {
                    ui.label("Port");
                    ui.label(format!("{what} {} of {}", k + 1, list.len()));
                    ui.end_row();
                }
            }
            for (list, what) in [(&iface.signal_in, "signal input"), (&iface.signal_out, "signal output")] {
                if let Some(k) = list.iter().position(|&p| p == i) {
                    ui.label("Port");
                    ui.label(format!("{what} {} of {}", k + 1, list.len()));
                    ui.end_row();
                }
            }
            if matches!(
                kind,
                OpticComponentKind::InputPort { .. }
                    | OpticComponentKind::OutputPort
                    | OpticComponentKind::SignalIn
                    | OpticComponentKind::SignalOut
            ) {
                ui.label("Name");
                label_changed = ui
                    .add(egui::TextEdit::singleline(&mut label).hint_text("shown on the box").desired_width(120.0))
                    .changed();
                ui.end_row();
            }
            if let Some(&input) = kind.signal_inputs().first() {
                ui.label("Driven by");
                ui.label(match driver {
                    Some(_) => RichText::new("a signal wire"),
                    None => RichText::new(format!("nothing ({input} fixed)")).weak(),
                })
                .on_hover_text("Drag from a signal output (square terminal) to the terminal above this part");
                ui.end_row();
            }
            if kind == OpticComponentKind::SignalIn {
                ui.label("Test value");
                ui.label(RichText::new("click the pin to toggle").weak())
                    .on_hover_text("Used when this layout is simulated on its own; inside a box the pin follows its wire");
                ui.end_row();
            }
            if let (Some(v), false) = (output, kind.signal_outputs().is_empty()) {
                ui.label("Signal");
                ui.monospace(fmt_value(v));
                ui.end_row();
            }
        });
        if label_changed {
            self.layout.set_label(i, &label);
        }
        self.exports_ui(ui, Item::Part(i));
        ui.horizontal(|ui| {
            if ui.small_button("Rotate").on_hover_text("R").clicked() {
                self.apply(super::input::Action::RotateCw);
            }
            if ui.small_button("Delete").on_hover_text("X").clicked() {
                self.apply(super::input::Action::Delete);
            }
        });
    }

    /// The values a component box sets for its component's exported
    /// parameters; unset ones show (and use) the value inside.
    fn component_params_ui(&mut self, ui: &mut Ui, b: usize, doc: crate::panels::project::DocId) {
        let params = self.catalog.get(doc).map(|e| e.parameters.clone()).unwrap_or_default();
        if params.is_empty() {
            return;
        }
        let inst = &mut self.layout.instances[b];
        egui::Grid::new("component_params").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            for p in &params {
                let label = ui.label(&p.name);
                let set = inst.param(&p.name);
                let mut v = set.unwrap_or(p.default);
                let speed = ((p.max - p.min) / 300.0).max(1e-4);
                let unit = if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) };
                let range = p.min.min(v)..=p.max.max(v);
                let r = ui.add(egui::DragValue::new(&mut v).range(range).speed(speed).max_decimals(4).suffix(unit)).labelled_by(label.id);
                if r.changed() {
                    inst.set_param(&p.name, Some(v));
                }
                if set.is_some() {
                    if ui.small_button("↺").on_hover_text(format!("Back to the component's value ({})", fmt_value(p.default))).clicked() {
                        inst.set_param(&p.name, None);
                    }
                } else {
                    ui.label(RichText::new("default").small().weak());
                }
                ui.end_row();
            }
        });
    }

    /// Inside a component: export a part's or block's parameters, so each
    /// box using the component can set them from its inspector.
    fn exports_ui(&mut self, ui: &mut Ui, item: Item) {
        let is_component = self.doc_id.and_then(|id| self.catalog.get(id)).is_some_and(|e| e.is_component);
        let params = crate::panels::circuit::sweep::params(&self.layout, item);
        if !is_component || params.is_empty() {
            return;
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Export to boxes using this component").small().weak())
            .on_hover_text("An exported parameter can be set on each box from its inspector, without opening the component");
        ui.horizontal_wrapped(|ui| {
            for p in &params {
                let at = self.layout.exports.iter().position(|e| e.item == item && e.param == p.name);
                if ui.selectable_label(at.is_some(), &p.name).clicked() {
                    match at {
                        Some(k) => {
                            self.layout.exports.remove(k);
                        }
                        None => {
                            let taken = |n: &str| self.layout.exports.iter().any(|e| e.name == n);
                            let name = (1..).map(|k| if k == 1 { p.name.clone() } else { format!("{} {k}", p.name) }).find(|n| !taken(n)).unwrap();
                            self.layout.exports.push(super::model::Export { name, item, param: p.name.clone() });
                        }
                    }
                }
            }
        });
        for e in self.layout.exports.iter_mut().filter(|e| e.item == item) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} as", e.param)).small());
                ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(110.0))
                    .on_hover_text("The name boxes show for it");
            });
        }
    }

    fn simulation_ui(&mut self, ui: &mut Ui) {
        ui_kit::section(ui, "Simulation");
        ui.checkbox(&mut self.simulation_active, "Live simulation").on_hover_text("Space");
        ui.horizontal(|ui| {
            ui.label("Cell size");
            ui.add(egui::DragValue::new(&mut self.cell_um).range(0.01..=10_000.0).speed(0.01).suffix(" µm"));
        });
        ui.horizontal(|ui| {
            ui.label(format!("Clock  tick {}", self.machine.tick));
            if ui.small_button("Step").on_hover_text("Registers store their inputs").clicked() {
                self.tick();
            }
            let run = if self.running { "Pause" } else { "Run" };
            if ui.small_button(run).on_hover_text(format!("Tick every {}s (Preferences)", self.prefs.tick_seconds)).clicked() {
                self.running = !self.running;
            }
            if ui.small_button("Reset").on_hover_text("Clear every register").clicked() {
                self.reset_clock();
            }
        });
        if ui
            .button("〰  Sweep…")
            .on_hover_text("Plot every output over a range of wavelengths or of one parameter")
            .clicked()
        {
            let selected = self
                .editor
                .boxes
                .active()
                .and_then(|b| self.layout.instances.get(b).map(|i| Item::Box(i.id)))
                .or(self.editor.selection.active().map(Item::Part));
            self.open_sweep(selected);
        }
        let Some(o) = self.outcome().cloned() else { return };
        if let Some(e) = &o.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        let summary = match o.wavelengths.as_slice() {
            [] => "No light".to_owned(),
            wls => format!(
                "{} beams at {} nm",
                o.beams.len(),
                wls.iter().map(|w| format!("{w:.0}")).collect::<Vec<_>>().join(", ")
            ),
        };
        let rounds = if o.iterations > 1 { format!(" · settled in {} rounds", o.iterations) } else { String::new() };
        ui.label(RichText::new(summary + &rounds).small().weak());

        // Which colour is which light.
        if !o.wavelengths.is_empty() {
            let palette = Palette::new(&self.prefs, o.wavelengths.iter().copied());
            let mut channels: Vec<(f64, Option<[u8; 3]>)> =
                o.wavelengths.iter().copied().zip(o.channel_colors.iter().copied()).collect();
            channels.sort_by(|a, b| a.0.total_cmp(&b.0));
            channels.dedup();
            ui.horizontal_wrapped(|ui| {
                for (nm, chosen) in channels {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, palette.color(nm, chosen));
                    ui.label(RichText::new(format!("{nm:.0} nm")).small());
                }
            });
        }
        egui::CollapsingHeader::new(RichText::new("What this simulation assumes").small()).show(ui, |ui| {
            for line in [
                "Light is a classical wave: interference and resonance are exact, but there are no photons, noise or quantum effects.",
                "Beams are thin rays on the grid: no diffraction, spreading or polarisation.",
                "Only lasers on the same shared source interfere; independent lasers add power.",
                "Waveguides have one effective index (2.4), no dispersion or loss.",
                "Steady state only: no time, bandwidth or data rate.",
                "Parts are ideal unless you add loss; powers are relative units.",
            ] {
                ui.label(RichText::new(format!("• {line}")).small().weak());
            }
        });

        let displays: Vec<(usize, String, u64)> = self
            .layout
            .instances
            .iter()
            .enumerate()
            .filter_map(|(b, inst)| match inst.block {
                BlockRef::Builtin(Builtin::Display { bits }) => {
                    Some((b, format!("Display {},{}", inst.cell.0, inst.cell.1), input_number(&self.layout, &o, inst.id, bits as usize)))
                }
                _ => None,
            })
            .collect();
        if !displays.is_empty() {
            ui_kit::section(ui, "Displays");
            for (b, name, n) in displays {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.editor.boxes.contains(b), name).clicked() {
                        self.editor.clear_selection();
                        self.selected_wire = None;
                        self.editor.boxes.select_only(b);
                    }
                    ui.monospace(RichText::new(n.to_string()).strong());
                });
            }
        }

        if o.detector_power.is_empty() {
            return;
        }
        ui_kit::section(ui, "Readouts");
        let scale = o.detector_power.iter().map(|r| r.1).fold(1e-12, f64::max);
        egui::Grid::new("readouts").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            for &(i, power) in &o.detector_power {
                let Some(component) = self.layout.components.get(i) else { continue };
                let (ci, cj) = cell_of(component.pos);
                let name = self.layout.label(i).map(str::to_owned).unwrap_or_else(|| {
                    format!("{} {ci},{cj}", if component.kind == OpticComponentKind::OutputPort { "Out" } else { "Det" })
                });
                if ui.selectable_label(self.editor.selection.contains(i), name).clicked() {
                    self.editor.clear_selection();
                    self.selected_wire = None;
                    self.editor.selection.select_only(i);
                }
                ui.add(egui::ProgressBar::new((power / scale) as f32).desired_width(70.0).desired_height(8.0));
                ui.monospace(units::format_power(power, self.prefs.power_unit))
                    .on_hover_text(units::format_both(power, self.prefs.power_unit));
                ui.end_row();
            }
        });
    }

    /// Whether a connection is being drawn (for tests and the status line).
    /// Whether the mouse is moving something: a grab, a drag, a new part.
    pub fn interacting(&self) -> bool {
        self.editor.grab.is_some() || self.drag != Drag::None || self.dragging_new.is_some()
    }

    pub fn connecting(&self) -> bool {
        matches!(self.drag, Drag::Connect(_))
    }

    /// Screen position of the menu, if open.
    pub fn menu_position(&self) -> Option<Pos2> {
        self.menu.as_ref().map(|m| m.screen)
    }
}

/// A signal value: bits as 0/1, anything else with decimals.
pub fn fmt_value(v: f64) -> String {
    if v == 0.0 || v == 1.0 { format!("{v:.0}") } else { format!("{v:.4}") }
}

/// Parameter widgets for a built-in block; returns whether anything changed.
pub fn builtin_params_ui(ui: &mut Ui, block: &mut Builtin, unit: units::PowerUnit) -> bool {
    let mut changed = false;
    let tau = std::f64::consts::TAU;
    if let Builtin::FunctionCell { .. } = block {
        return function_cell_ui(ui, block);
    }
    egui::Grid::new("block_params").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let mut row = |ui: &mut Ui, label: &str, widget: &mut dyn FnMut(&mut Ui) -> egui::Response| {
            ui.label(label);
            changed |= widget(ui).changed();
            ui.end_row();
        };
        match block {
            Builtin::Laser { amplitude, phase, wavelength_nm } => {
                row(ui, "Power", &mut |ui| units::power_editor(ui, amplitude, unit));
                row(ui, "Phase", &mut |ui| ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad")));
                row(ui, "Wavelength", &mut |ui| {
                    ui.add(egui::DragValue::new(wavelength_nm).range(200.0..=2000.0).speed(0.1).suffix(" nm"))
                });
            }
            Builtin::InputPin { name } | Builtin::OutputPin { name } => {
                row(ui, "Name", &mut |ui| ui.text_edit_singleline(name));
            }
            Builtin::Coupler { coupling } => {
                row(ui, "Coupling", &mut |ui| ui.add(egui::Slider::new(coupling, 0.0..=1.0)));
            }
            Builtin::PhaseShifter { phase } => {
                row(ui, "Phase", &mut |ui| ui.add(egui::Slider::new(phase, 0.0..=tau).suffix(" rad")));
            }
            Builtin::Attenuator { loss_db } => {
                row(ui, "Loss", &mut |ui| {
                    let t = units::from_db(-*loss_db);
                    ui.add(egui::DragValue::new(loss_db).range(0.0..=100.0).speed(0.1).suffix(" dB"))
                        .on_hover_text(format!("Passes {t:.3} of the power"))
                });
            }
            Builtin::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm } => {
                row(ui, "Length", &mut |ui| {
                    ui.add(egui::DragValue::new(length_um).range(0.0..=1e6).speed(0.05).suffix(" µm"))
                });
                row(ui, "n_eff", &mut |ui| {
                    ui.add(egui::DragValue::new(n_eff).range(1.0..=4.0).speed(0.01))
                        .on_hover_text("Effective index at the reference wavelength")
                });
                row(ui, "n_g", &mut |ui| {
                    ui.add(egui::DragValue::new(n_g).range(1.0..=6.0).speed(0.01)).on_hover_text(
                        "Group index: sets how the phase changes with wavelength, so resonance spacing and delay. \
                         Silicon strip ≈ 4.2; equal to n_eff for no dispersion",
                    )
                });
                row(ui, "at", &mut |ui| {
                    ui.add(egui::DragValue::new(reference_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm"))
                        .on_hover_text("Wavelength the indices are given at")
                });
                row(ui, "Loss", &mut |ui| {
                    ui.add(egui::DragValue::new(loss_db_per_cm).range(0.0..=1000.0).speed(0.1).suffix(" dB/cm"))
                });
                let delay = crate::photonics::Waveguide::new(*length_um, *n_eff).with_group_index(*n_g, *reference_nm).group_delay_ps();
                ui.label("");
                ui.label(RichText::new(format!("delay {delay:.3} ps")).small().weak());
                ui.end_row();
            }
            Builtin::Mzi { theta, phi } => {
                row(ui, "θ (internal)", &mut |ui| {
                    ui.add(egui::Slider::new(theta, 0.0..=tau).suffix(" rad"))
                        .on_hover_text("π: in1 → out1 (bar). 0: in1 → out2 (cross). π/2: half to each.")
                });
                row(ui, "φ (input)", &mut |ui| ui.add(egui::Slider::new(phi, 0.0..=tau).suffix(" rad")));
                // Where light from in1 goes, so a switch at π doesn't look broken.
                let s = crate::photonics::Component::s_matrix(&crate::photonics::Mzi::new(*theta, *phi), crate::photonics::REFERENCE_NM);
                let (bar, cross) = (s[(0, 0)].norm_sqr() * 100.0, s[(1, 0)].norm_sqr() * 100.0);
                ui.label("");
                ui.label(RichText::new(format!("in1 → out1 {bar:.0}%, out2 {cross:.0}%")).small().weak());
                ui.end_row();
            }
            Builtin::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm }
            | Builtin::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => {
                row(ui, "Coupling", &mut |ui| ui.add(egui::Slider::new(coupling, 0.0..=1.0)));
                row(ui, "Length", &mut |ui| {
                    ui.add(egui::DragValue::new(length_um).range(0.1..=1e5).speed(0.01).suffix(" µm"))
                });
                row(ui, "n_eff", &mut |ui| {
                    ui.add(egui::DragValue::new(n_eff).range(1.0..=4.0).speed(0.01))
                        .on_hover_text("Effective index at the reference wavelength")
                });
                row(ui, "n_g", &mut |ui| {
                    ui.add(egui::DragValue::new(n_g).range(1.0..=6.0).speed(0.01)).on_hover_text(
                        "Group index: sets how the phase changes with wavelength, so resonance spacing and delay. \
                         Silicon strip ≈ 4.2; equal to n_eff for no dispersion",
                    )
                });
                row(ui, "at", &mut |ui| {
                    ui.add(egui::DragValue::new(reference_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm"))
                        .on_hover_text("Wavelength the indices are given at")
                });
                row(ui, "Loss", &mut |ui| {
                    ui.add(egui::DragValue::new(loss_db_per_cm).range(0.0..=1000.0).speed(0.1).suffix(" dB/cm"))
                });
                let fsr = *reference_nm * *reference_nm / (*n_g * *length_um * 1000.0);
                ui.label("");
                ui.label(RichText::new(format!("resonances {fsr:.2} nm apart (FSR)")).small().weak());
                ui.end_row();
            }
            Builtin::SplitterTree { levels } => {
                row(ui, "Levels", &mut |ui| ui.add(egui::Slider::new(levels, 0..=5)));
            }
            Builtin::SwitchTree { levels, address } => {
                row(ui, "Levels", &mut |ui| ui.add(egui::Slider::new(levels, 1..=5)));
                *address = (*address).min((1 << *levels) - 1);
                let max = (1 << *levels) - 1;
                row(ui, "Address", &mut |ui| ui.add(egui::Slider::new(address, 0..=max)));
            }
            Builtin::Dft { n } => {
                row(ui, "Modes", &mut |ui| ui.add(egui::Slider::new(n, 2..=16)));
            }
            Builtin::NumberInput { bits, value } => {
                row(ui, "Bits", &mut |ui| ui.add(egui::Slider::new(bits, 1..=16)));
                let max = (1u64 << *bits) - 1;
                *value = (*value).min(max);
                row(ui, "Value", &mut |ui| ui.add(egui::DragValue::new(value).range(0..=max)));
            }
            Builtin::Display { bits } | Builtin::Register { bits } => {
                row(ui, "Bits", &mut |ui| ui.add(egui::Slider::new(bits, 1..=16)));
            }
            Builtin::Detector | Builtin::YSplitter | Builtin::YCombiner | Builtin::Crossing | Builtin::FunctionCell { .. } => {}
        }
    });
    changed
}

/// Editor for a function cell's inputs and expressions.
fn function_cell_ui(ui: &mut Ui, block: &mut Builtin) -> bool {
    let Builtin::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm } = block else {
        return false;
    };
    let mut changed = false;
    egui::Grid::new("function_params").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(name).changed();
        ui.end_row();
        ui.label("Signal inputs");
        changed |= ui.add(egui::DragValue::new(inputs).range(0..=16)).on_hover_text("in1, in2, …").changed();
        ui.end_row();
        ui.label("Light inputs");
        changed |= ui
            .add(egui::DragValue::new(light_inputs).range(0..=8))
            .on_hover_text("p1, p2, …: power received on each left-edge port")
            .changed();
        ui.end_row();
    });
    let mut list = |ui: &mut Ui, title: &str, prefix: &str, exprs: &mut Vec<String>, hint: &str| {
        ui.label(RichText::new(title).strong());
        let mut remove = None;
        for (k, expr) in exprs.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.monospace(format!("{prefix}{} =", k + 1));
                let error = crate::panels::circuit::expr::parse(expr).err();
                let edit = ui.add(egui::TextEdit::singleline(expr).code_editor().desired_width(130.0));
                changed |= edit.changed();
                if let Some(e) = error {
                    edit.on_hover_text(e.to_string());
                    ui.colored_label(ui.visuals().error_fg_color, "!");
                }
                if ui.small_button("×").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            exprs.remove(k);
            changed = true;
        }
        if ui.small_button(format!("+ {hint}")).clicked() {
            exprs.push("0".into());
            changed = true;
        }
    };
    list(ui, "Signal outputs", "out", outputs, "signal output");
    list(ui, "Light outputs", "light", light_outputs, "light output");
    if !light_outputs.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Wavelength");
            changed |= ui.add(egui::DragValue::new(wavelength_nm).range(200.0..=2000.0).speed(0.1).suffix(" nm")).changed();
        });
    }
    ui.label(
        RichText::new(
            "Variables: in1… (signals), p1… (light power), t (clock ticks). Operators: + - * / % ** << >> & | ^ ! ~, comparisons and && || give 1 or 0, c ? a : b. Functions: abs sqrt floor ceil round exp ln sin cos pow bit clamp min max sum argmax. Light outputs are field amplitudes (power = amplitude²).",
        )
        .small()
        .weak(),
    );
    changed
}
