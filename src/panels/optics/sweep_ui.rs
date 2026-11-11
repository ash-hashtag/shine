//! The sweep window: pick what to vary, run, plot every output, export CSV.

use egui::{RichText, Ui};
use egui_plot::{Legend, Line, Plot};

use super::model::{Item, OpticComponentKind, cell_of};
use super::{OpticsPanel, Request};
use crate::panels::circuit::sim::NoDocuments;
use crate::panels::circuit::sweep::{self, SweepKind, SweepResult, SweepSpec};
use crate::panels::units::{self, PowerUnit};

/// The plot's y axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scale {
    /// Power in mW.
    #[default]
    Linear,
    /// Power in dBm.
    Dbm,
    /// Power relative to the input, in dB (transmission, insertion loss).
    Relative,
}

impl Scale {
    fn label(self) -> &'static str {
        match self {
            Scale::Linear => "Power (mW)",
            Scale::Dbm => "Power (dBm)",
            Scale::Relative => "Relative to input (dB)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepMode {
    Wavelength,
    Parameter,
}

/// The sweep window's settings and last result.
#[derive(Clone, Debug)]
pub struct SweepState {
    pub open: bool,
    pub mode: SweepMode,
    /// Laser (part index) tuned by a wavelength sweep.
    pub laser: Option<usize>,
    /// Part or box, and parameter, varied by a parameter sweep.
    pub target: Option<Item>,
    pub param: String,
    pub start: f64,
    pub stop: f64,
    pub points: usize,
    pub scale: Scale,
    /// Show the peak, minimum, insertion loss and extinction table.
    pub show_readouts: bool,
    /// Where the plot was last drawn, for saving it as an image.
    pub plot_rect: Option<egui::Rect>,
    /// Plot boxes' light outputs too (hidden by default when there are detectors).
    pub box_outputs: bool,
    pub result: Option<SweepResult>,
    /// A second parameter to vary: the sweep becomes a map.
    pub vary: Option<sweep::Axis>,
    pub map: Option<sweep::MapResult>,
    /// The output the map shows, by name.
    pub map_output: Option<String>,
}

impl Default for SweepState {
    fn default() -> Self {
        Self {
            open: false,
            mode: SweepMode::Wavelength,
            laser: None,
            target: None,
            param: String::new(),
            start: 1540.0,
            stop: 1560.0,
            points: 201,
            scale: Scale::Linear,
            box_outputs: false,
            show_readouts: true,
            plot_rect: None,
            result: None,
            vary: None,
            map: None,
            map_output: None,
        }
    }
}

impl OpticsPanel {
    /// Lasers of the top layout, by part index.
    fn lasers(&self) -> Vec<usize> {
        (0..self.layout.components.len())
            .filter(|&i| matches!(self.layout.components[i].kind, OpticComponentKind::Laser { .. }))
            .collect()
    }

    pub fn item_name(&self, item: Item) -> String {
        match item {
            Item::Part(i) => self.layout.components.get(i).map_or_else(String::new, |c| {
                let (x, y) = cell_of(c.pos);
                match self.layout.label(i) {
                    Some(label) => format!("{} “{label}”", c.kind.name()),
                    None => format!("{} at {x},{y}", c.kind.name()),
                }
            }),
            Item::Box(id) => self.layout.instance(id).map_or_else(String::new, |inst| {
                format!("{} at {},{}", self.catalog.display(&inst.block).name, inst.cell.0, inst.cell.1)
            }),
        }
    }

    /// Open the sweep window, set up for `item` if given (a laser sweeps its
    /// wavelength; anything else its first parameter).
    pub fn open_sweep(&mut self, item: Option<Item>) {
        // The first parameter of a non-laser item, worked out before borrowing the state.
        let first = item.and_then(|item| self.sweep_params(item).into_iter().next().map(|p| (item, p)));
        let s = &mut self.sweep;
        s.open = true;
        let laser_wavelength = |i: usize| match self.layout.components.get(i).map(|c| c.kind) {
            Some(OpticComponentKind::Laser { wavelength, .. }) => Some(wavelength as f64),
            _ => None,
        };
        match item {
            Some(Item::Part(i)) if laser_wavelength(i).is_some() => {
                s.mode = SweepMode::Wavelength;
                s.laser = Some(i);
            }
            Some(_) if first.is_some() => {
                let (item, first) = first.expect("checked");
                s.mode = SweepMode::Parameter;
                s.target = Some(item);
                s.param = first.name;
                s.start = first.min;
                s.stop = first.max;
                s.points = 101;
                return;
            }
            _ => {}
        }
        if s.mode == SweepMode::Wavelength {
            if s.laser.is_none_or(|i| laser_wavelength(i).is_none()) {
                s.laser = (0..self.layout.components.len()).find(|&i| laser_wavelength(i).is_some());
            }
            if let Some(nm) = s.laser.and_then(laser_wavelength) {
                s.start = nm - 10.0;
                s.stop = nm + 10.0;
            }
        }
    }

    /// Parameters a sweep can vary on `item`: a part's or block's own, or
    /// the parameters a component box's component exports.
    pub fn sweep_params(&self, item: Item) -> Vec<sweep::Param> {
        let component = match item {
            Item::Box(id) => match self.layout.instance(id).map(|i| &i.block) {
                Some(super::model::BlockRef::Component(doc)) => Some(*doc),
                _ => None,
            },
            Item::Part(_) => None,
        };
        match component {
            Some(doc) => self
                .catalog
                .get(doc)
                .map(|e| {
                    e.parameters
                        .iter()
                        .map(|p| sweep::Param { name: p.name.clone(), unit: p.unit, min: p.min, max: p.max })
                        .collect()
                })
                .unwrap_or_default(),
            None => sweep::params(&self.layout, item),
        }
    }

    /// What the window would run, if it is complete.
    pub fn sweep_spec(&self) -> Option<SweepSpec> {
        let s = &self.sweep;
        let kind = match s.mode {
            SweepMode::Wavelength => SweepKind::Wavelength { laser: s.laser? },
            SweepMode::Parameter => SweepKind::Parameter { item: s.target?, param: s.param.clone() },
        };
        Some(SweepSpec { kind, start: s.start, stop: s.stop, points: s.points.clamp(2, 5001) })
    }

    /// The second parameter, when the sweep is a map.
    pub fn sweep_axis(&self) -> Option<sweep::Axis> {
        self.sweep.vary.clone().filter(|a| !a.param.is_empty())
    }

    /// Run the sweep: here for a standalone layout, by the host otherwise
    /// (components need the project).
    pub fn run_sweep(&mut self) {
        if self.embedded {
            self.requests.push(Request::RunSweep);
            return;
        }
        let Some(spec) = self.sweep_spec() else { return };
        let catalog = &self.catalog;
        let layout = &self.layout;
        let names = |id| layout.instance(id).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        if let Some(y) = self.sweep_axis() {
            let map = sweep::run_map(layout, self.cell_um, &NoDocuments, &self.machine, None, &spec, &y, &names);
            self.sweep.map = Some(map);
            return;
        }
        let result = sweep::run(layout, self.cell_um, &NoDocuments, &self.machine, None, &spec, &names);
        self.sweep.result = Some(result);
    }

    pub(super) fn sweep_ui(&mut self, ctx: &egui::Context) {
        if !self.sweep.open {
            return;
        }
        let mut open = true;
        let mut run = false;
        crate::panels::ui_kit::window(ctx, "Sweep")
            .id(egui::Id::unique("optics_sweep"))
            .open(&mut open)
            .default_size([640.0, 520.0])
            .show(ctx, |ui| run = self.sweep_contents(ui));
        self.sweep.open = open;
        if run {
            self.run_sweep();
        }
    }

    /// The window's contents; returns whether Run was pressed.
    fn sweep_contents(&mut self, ui: &mut Ui) -> bool {
        let lasers = self.lasers();
        let names: Vec<(usize, String)> = lasers.iter().map(|&i| (i, self.item_name(Item::Part(i)))).collect();
        let target_name = self.sweep.target.map(|t| self.item_name(t));
        let selection = self
            .editor
            .boxes
            .active()
            .and_then(|b| self.layout.instances.get(b).map(|i| Item::Box(i.id)))
            .or(self.editor.selection.active().map(Item::Part));
        let params = self.sweep.target.map(|t| self.sweep_params(t)).unwrap_or_default();
        let vary_name = self.sweep.vary.as_ref().map(|a| self.item_name(a.item));
        let vary_params = self.sweep.vary.as_ref().map(|a| self.sweep_params(a.item)).unwrap_or_default();
        let usable = selection.filter(|&item| !self.sweep_params(item).is_empty());
        let first_of_usable = usable.map(|item| self.sweep_params(item).swap_remove(0));
        let mut run = false;
        let s = &mut self.sweep;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.mode, SweepMode::Wavelength, "Wavelength");
            ui.selectable_value(&mut s.mode, SweepMode::Parameter, "Parameter");
        });
        egui::Grid::new("sweep_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            match s.mode {
                SweepMode::Wavelength => {
                    let tune = ui.label("Tune").id;
                    if names.is_empty() {
                        ui.label(RichText::new("Add a laser to sweep its wavelength").weak());
                    } else {
                        let current = s.laser.and_then(|i| names.iter().find(|n| n.0 == i)).map_or("Choose a laser", |n| n.1.as_str());
                        egui::ComboBox::from_id_salt("sweep_laser")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (i, name) in &names {
                                    ui.selectable_value(&mut s.laser, Some(*i), name);
                                }
                            })
                            .response
                            .labelled_by(tune);
                    }
                    ui.end_row();
                    ui.label("From");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.start).speed(0.1).suffix(" nm")).accessible_name("From wavelength");
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut s.stop).speed(0.1).suffix(" nm")).accessible_name("To wavelength");
                    });
                    ui.end_row();
                }
                SweepMode::Parameter => {
                    ui.label("Part");
                    ui.horizontal(|ui| {
                        ui.label(target_name.as_deref().unwrap_or("none"));
                        if ui
                            .add_enabled(usable.is_some(), egui::Button::new("Use selection"))
                            .on_disabled_hover_text("Select a part or block with numeric parameters")
                            .clicked()
                            && let Some(item) = usable
                        {
                            let first = first_of_usable.clone().expect("usable");
                            s.target = Some(item);
                            s.param = first.name;
                            s.start = first.min;
                            s.stop = first.max;
                        }
                    });
                    ui.end_row();
                    let label = ui.label("Parameter").id;
                    egui::ComboBox::from_id_salt("sweep_param")
                        .selected_text(s.param.as_str())
                        .show_ui(ui, |ui| {
                            for p in &params {
                                if ui.selectable_label(s.param == p.name, &p.name).clicked() {
                                    s.param = p.name.clone();
                                    s.start = p.min;
                                    s.stop = p.max;
                                }
                            }
                        })
                        .response
                        .labelled_by(label);
                    ui.end_row();
                    let unit = params.iter().find(|p| p.name == s.param).map_or("", |p| p.unit);
                    ui.label("From");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.start).speed(0.01).suffix(format!(" {unit}")))
                            .accessible_name(format!("From {}", s.param));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut s.stop).speed(0.01).suffix(format!(" {unit}")))
                            .accessible_name(format!("To {}", s.param));
                    });
                    ui.end_row();
                }
            }
            let label = ui.label("Points").id;
            ui.add(egui::DragValue::new(&mut s.points).range(2..=5001)).labelled_by(label);
            ui.end_row();
            // A second parameter turns the sweep into a map.
            ui.label("Also vary");
            let mut on = s.vary.is_some();
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, "a second parameter (a map)").changed() {
                    s.vary = on.then(|| sweep::Axis { item: s.target.unwrap_or(Item::Part(0)), param: String::new(), start: 0.0, stop: 1.0, points: 21 });
                    if !on {
                        s.map = None;
                    }
                }
            });
            ui.end_row();
            if let Some(axis) = &mut s.vary {
                ui.label("Then");
                ui.horizontal(|ui| {
                    ui.label(vary_name.as_deref().unwrap_or("none"));
                    if ui
                        .add_enabled(usable.is_some(), egui::Button::new("Use selection"))
                        .on_disabled_hover_text("Select a part or block with numeric parameters")
                        .clicked()
                        && let (Some(item), Some(first)) = (usable, first_of_usable.clone())
                    {
                        *axis = sweep::Axis { item, param: first.name, start: first.min, stop: first.max, points: axis.points };
                    }
                });
                ui.end_row();
                let label = ui.label("Its parameter").id;
                egui::ComboBox::from_id_salt("sweep_vary_param")
                    .selected_text(if axis.param.is_empty() { "Choose…" } else { axis.param.as_str() })
                    .show_ui(ui, |ui| {
                        for p in &vary_params {
                            if ui.selectable_label(axis.param == p.name, &p.name).clicked() {
                                axis.param = p.name.clone();
                                axis.start = p.min;
                                axis.stop = p.max;
                            }
                        }
                    })
                    .response
                    .labelled_by(label);
                ui.end_row();
                ui.label("From");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut axis.start).speed(0.01)).accessible_name(format!("From {}", axis.param));
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut axis.stop).speed(0.01)).accessible_name(format!("To {}", axis.param));
                });
                ui.end_row();
                let label = ui.label("Its points").id;
                ui.add(egui::DragValue::new(&mut axis.points).range(2..=201)).labelled_by(label);
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let ready = match s.mode {
                SweepMode::Wavelength => s.laser.is_some(),
                SweepMode::Parameter => s.target.is_some() && !s.param.is_empty(),
            } && s.vary.as_ref().is_none_or(|a| !a.param.is_empty());
            if ui.add_enabled(ready, egui::Button::new(RichText::new("▶  Run sweep").strong())).clicked() {
                run = true;
            }
            egui::ComboBox::from_label("Plot").selected_text(s.scale.label()).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.scale, Scale::Linear, Scale::Linear.label());
                ui.selectable_value(&mut s.scale, Scale::Dbm, Scale::Dbm.label());
                ui.selectable_value(&mut s.scale, Scale::Relative, Scale::Relative.label()).on_hover_text(
                    "Output over input power: the tuned laser for a wavelength sweep, all lasers for a parameter sweep",
                );
            });
            ui.checkbox(&mut s.box_outputs, "Box outputs").on_hover_text("Also plot the light leaving every block and component");
        });
        if s.vary.is_some() {
            map_ui(ui, s, self.prefs.power_unit);
            return run;
        }
        let Some(result) = &s.result else {
            ui.label(RichText::new("Each point is a full simulation, light and electronics.").small().weak());
            return run;
        };
        if let Some(e) = &result.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if result.xs.is_empty() {
            return run;
        }
        let scale = s.scale;
        // Box outputs are many; show them when asked, or when there's nothing else.
        let only_boxes = result.series.iter().all(|s| s.from_box);
        let shown: Vec<&sweep::Series> = result.series.iter().filter(|x| s.box_outputs || only_boxes || !x.from_box).collect();
        let readouts: Vec<sweep::Readout> =
            result.readouts().into_iter().filter(|r| shown.iter().any(|s| s.name == r.name)).collect();
        let table = if s.show_readouts { (readouts.len() as f32 * 20.0 + 28.0).min(150.0) } else { 0.0 };
        let plot = Plot::new("sweep_plot")
            .height((ui.available_height() - 64.0 - table).max(180.0))
            .legend(Legend::default())
            .x_axis_label(result.x_label.clone())
            .y_axis_label(scale.label())
            .show(ui, |p| {
                for series in &shown {
                    let ys = match scale {
                        Scale::Linear => series.power.clone(),
                        Scale::Dbm => series.power.iter().map(|&y| sweep::db(y)).collect(),
                        Scale::Relative => result.relative_db(series),
                    };
                    let points: Vec<[f64; 2]> = result.xs.iter().zip(ys).map(|(&x, y)| [x, y]).collect();
                    p.line(Line::new(series.name.clone(), points));
                }
            });
        s.plot_rect = Some(plot.response.rect);
        let unit = self.prefs.power_unit;
        let header = egui::CollapsingHeader::new("Readouts").id_salt("sweep_readouts").open(Some(s.show_readouts)).show(ui, |ui| {
            readouts_table(ui, &readouts, unit);
        });
        if header.header_response.clicked() {
            s.show_readouts = !s.show_readouts;
        }
        let (mut save_csv, mut save_image) = (false, None);
        ui.horizontal(|ui| {
            if ui.button("Copy CSV").on_hover_text("Paste into a spreadsheet").clicked() {
                ui.ctx().copy_text(result.to_csv());
            }
            if ui.button("Save CSV…").clicked() {
                save_csv = true;
            }
            if ui.button("Save image…").on_hover_text("The plot as a PNG image").clicked() {
                save_image = s.plot_rect;
            }
            ui.label(RichText::new(format!("{} points × {} outputs", result.xs.len(), result.series.len())).small().weak());
        });
        if save_csv {
            let csv = result.to_csv();
            self.saved(crate::panels::export::save("sweep.csv", "CSV", "csv", csv.as_bytes()));
        }
        if let Some(rect) = save_image {
            self.requests.push(Request::SaveImage { name: "sweep.png", rect });
        }
        run
    }

    /// Say where a file went, or why it didn't.
    pub(super) fn saved(&mut self, result: Result<Option<String>, String>) {
        match result {
            Ok(Some(to)) => self.notify(format!("Saved {to}")),
            Ok(None) => {}
            Err(e) => {
                tracing::error!("{e}");
                self.notify(format!("Not saved: {e}"));
            }
        }
    }
}

/// One row per curve: peak, minimum, insertion loss, extinction ratio.
/// A map: one output's power over the two swept values, as a heatmap.
fn map_ui(ui: &mut Ui, s: &mut SweepState, unit: PowerUnit) {
    let Some(map) = &s.map else {
        ui.label(RichText::new("Each point is a full simulation, light and electronics.").small().weak());
        return;
    };
    if let Some(e) = &map.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    if map.xs.is_empty() || map.ys.is_empty() || map.series.is_empty() {
        return;
    }
    let only_boxes = map.series.iter().all(|m| m.from_box);
    let shown: Vec<&sweep::MapSeries> = map.series.iter().filter(|m| s.box_outputs || only_boxes || !m.from_box).collect();
    let chosen = s.map_output.as_ref().and_then(|n| shown.iter().position(|m| &m.name == n)).unwrap_or(0);
    let Some(series) = shown.get(chosen) else { return };
    let mut csv = false;
    let mut save = false;
    ui.horizontal(|ui| {
        let l = ui.label("Show").id;
        egui::ComboBox::from_id_salt("sweep_map_output")
            .selected_text(series.name.as_str())
            .show_ui(ui, |ui| {
                for m in &shown {
                    if ui.selectable_label(m.name == series.name, &m.name).clicked() {
                        s.map_output = Some(m.name.clone());
                    }
                }
            })
            .response
            .labelled_by(l);
        csv = ui.button("Copy CSV").clicked();
        save = ui.button("Save CSV…").clicked();
    });
    // Shade by power relative to this output's peak, in dB (40 dB of range).
    let peak = series.power.iter().flatten().fold(0.0f64, |a, &b| a.max(b));
    let shade = |p: f64| if peak > 0.0 && p > 0.0 { (1.0 + 10.0 * (p / peak).log10() / 40.0).clamp(0.0, 1.0) } else { 0.0 };
    let dark = ui.visuals().dark_mode;
    let (x0, x1) = (map.xs[0], *map.xs.last().expect("non-empty"));
    let (y0, y1) = (map.ys[0], *map.ys.last().expect("non-empty"));
    let height = (ui.available_height() - 40.0).max(160.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(56.0, 4.0), rect.max - egui::vec2(8.0, 30.0));
    let (columns, rows) = (map.xs.len(), map.ys.len());
    let cell = egui::vec2(plot.width() / columns as f32, plot.height() / rows as f32);
    let painter = ui.painter_at(rect);
    for (r, row) in series.power.iter().enumerate() {
        for (c, &p) in row.iter().enumerate() {
            // y grows upwards.
            let min = egui::pos2(plot.left() + c as f32 * cell.x, plot.bottom() - (r + 1) as f32 * cell.y);
            painter.rect_filled(egui::Rect::from_min_size(min, cell + egui::vec2(0.5, 0.5)), 0.0, crate::panels::ui_kit::magnitude_color(shade(p), dark));
        }
    }
    let ink = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let small = egui::FontId::proportional(11.0);
    painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0, weak), egui::StrokeKind::Outside);
    painter.text(egui::pos2(plot.left(), plot.bottom() + 4.0), egui::Align2::LEFT_TOP, fmt_x(x0), small.clone(), weak);
    painter.text(egui::pos2(plot.right(), plot.bottom() + 4.0), egui::Align2::RIGHT_TOP, fmt_x(x1), small.clone(), weak);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 16.0), egui::Align2::CENTER_TOP, &map.x_label, small.clone(), ink);
    painter.text(egui::pos2(plot.left() - 4.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, fmt_x(y0), small.clone(), weak);
    painter.text(egui::pos2(plot.left() - 4.0, plot.top()), egui::Align2::RIGHT_TOP, fmt_x(y1), small.clone(), weak);
    painter.text(egui::pos2(rect.left(), plot.center().y), egui::Align2::LEFT_CENTER, &map.y_label, small, ink);
    let description = format!(
        "Map of {} over {} ({} to {}) and {} ({} to {}); peak {}",
        series.name,
        map.x_label,
        fmt_x(x0),
        fmt_x(x1),
        map.y_label,
        fmt_x(y0),
        fmt_x(y1),
        units::format_power(peak, unit)
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &description));
    // The value under the pointer.
    if let Some(at) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let c = (((at.x - plot.left()) / cell.x) as usize).min(columns - 1);
        let r = (((plot.bottom() - at.y) / cell.y) as usize).min(rows - 1);
        let p = series.power[r][c];
        response.on_hover_text_at_pointer(format!(
            "{}: {}\n{}: {}\n{}",
            map.x_label,
            fmt_x(map.xs[c]),
            map.y_label,
            fmt_x(map.ys[r]),
            units::format_both(p, unit)
        ));
    }
    if csv {
        ui.ctx().copy_text(map.to_csv());
    }
    if save {
        let _ = crate::panels::export::save("sweep-map.csv", "CSV", "csv", map.to_csv().as_bytes());
    }
}

fn readouts_table(ui: &mut Ui, readouts: &[sweep::Readout], unit: PowerUnit) {
    // Resonance columns only when some curve has resonances.
    let spectral = readouts.iter().any(|r| r.spectrum.is_some());
    let dash = || "–".to_owned();
    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
        egui::Grid::new("sweep_readouts_grid").striped(true).num_columns(if spectral { 8 } else { 5 }).spacing([14.0, 2.0]).show(ui, |ui| {
            ui.label(RichText::new("Output").strong());
            ui.label(RichText::new("Peak").strong()).on_hover_text("Highest power, and where");
            ui.label(RichText::new("Minimum").strong()).on_hover_text("Lowest power, and where");
            ui.label(RichText::new("Insertion loss").strong())
                .on_hover_text("Input power over the peak power, in dB: how much is lost on the best path");
            ui.label(RichText::new("Extinction").strong()).on_hover_text("Peak over minimum, in dB");
            if spectral {
                ui.label(RichText::new("FSR").strong())
                    .on_hover_text("Free spectral range: the spacing of neighbouring resonances (dips or peaks)");
                ui.label(RichText::new("Width").strong()).on_hover_text(
                    "Full width of the strongest resonance at half its depth (or height); '–' when the sweep has too few points across it",
                );
                ui.label(RichText::new("Q").strong()).on_hover_text("Quality factor: the resonance's wavelength over its width");
            }
            ui.end_row();
            for r in readouts {
                ui.label(&r.name);
                ui.label(RichText::new(format!("{} at {}", units::format_power(r.peak, unit), fmt_x(r.peak_at))).monospace());
                ui.label(RichText::new(format!("{} at {}", units::format_power(r.min, unit), fmt_x(r.min_at))).monospace());
                ui.label(RichText::new(r.insertion_loss_db.map_or_else(|| "–".into(), |d| format!("{d:.2} dB"))).monospace());
                ui.label(RichText::new(format!("{:.1} dB", r.extinction_db)).monospace());
                if spectral {
                    let s = r.spectrum.as_ref();
                    let fsr = s.and_then(|s| s.fsr_nm).map_or_else(dash, |v| format!("{v:.3} nm"));
                    let width = s.and_then(|s| s.fwhm_nm).map_or_else(dash, |v| if v < 0.01 { format!("{:.1} pm", v * 1e3) } else { format!("{v:.3} nm") });
                    let q = s.and_then(|s| s.q).map_or_else(dash, |q| format!("{q:.0}"));
                    let kind = s.map_or("", |s| if s.dips { " (dips)" } else { " (peaks)" });
                    ui.label(RichText::new(fsr).monospace()).on_hover_text(format!(
                        "{} resonance(s){kind}",
                        s.map_or(0, |s| s.resonances.len())
                    ));
                    ui.label(RichText::new(width).monospace());
                    ui.label(RichText::new(q).monospace());
                }
                ui.end_row();
            }
        });
    });
}

/// A swept value, short.
fn fmt_x(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
