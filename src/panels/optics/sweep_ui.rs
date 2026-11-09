//! The sweep window: pick what to vary, run, plot every output, export CSV.

use egui::{RichText, Ui};
use egui_plot::{Legend, Line, Plot};

use super::model::{Item, OpticComponentKind, cell_of};
use super::{OpticsPanel, Request};
use crate::panels::circuit::sim::NoDocuments;
use crate::panels::circuit::sweep::{self, SweepKind, SweepResult, SweepSpec};

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
    pub param: &'static str,
    pub start: f64,
    pub stop: f64,
    pub points: usize,
    /// Plot in dB rather than linear power.
    pub db: bool,
    /// Plot boxes' light outputs too (hidden by default when there are detectors).
    pub box_outputs: bool,
    pub result: Option<SweepResult>,
}

impl Default for SweepState {
    fn default() -> Self {
        Self {
            open: false,
            mode: SweepMode::Wavelength,
            laser: None,
            target: None,
            param: "",
            start: 1540.0,
            stop: 1560.0,
            points: 201,
            db: false,
            box_outputs: false,
            result: None,
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

    fn item_name(&self, item: Item) -> String {
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
            Some(item) if !sweep::params(&self.layout, item).is_empty() => {
                let first = sweep::params(&self.layout, item)[0];
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

    /// What the window would run, if it is complete.
    pub fn sweep_spec(&self) -> Option<SweepSpec> {
        let s = &self.sweep;
        let kind = match s.mode {
            SweepMode::Wavelength => SweepKind::Wavelength { laser: s.laser? },
            SweepMode::Parameter => SweepKind::Parameter { item: s.target?, param: s.param },
        };
        Some(SweepSpec { kind, start: s.start, stop: s.stop, points: s.points.clamp(2, 5001) })
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
        let result = sweep::run(layout, self.cell_um, &NoDocuments, &self.machine, None, &spec, &names);
        self.sweep.result = Some(result);
    }

    pub(super) fn sweep_ui(&mut self, ctx: &egui::Context) {
        if !self.sweep.open {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new("Sweep")
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
        let params = self.sweep.target.map(|t| sweep::params(&self.layout, t)).unwrap_or_default();
        let mut run = false;
        let s = &mut self.sweep;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.mode, SweepMode::Wavelength, "Wavelength");
            ui.selectable_value(&mut s.mode, SweepMode::Parameter, "Parameter");
        });
        egui::Grid::new("sweep_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            match s.mode {
                SweepMode::Wavelength => {
                    ui.label("Tune");
                    if names.is_empty() {
                        ui.label(RichText::new("Add a laser to sweep its wavelength").weak());
                    } else {
                        let current = s.laser.and_then(|i| names.iter().find(|n| n.0 == i)).map_or("Choose a laser", |n| n.1.as_str());
                        egui::ComboBox::from_id_salt("sweep_laser").selected_text(current).show_ui(ui, |ui| {
                            for (i, name) in &names {
                                ui.selectable_value(&mut s.laser, Some(*i), name);
                            }
                        });
                    }
                    ui.end_row();
                    ui.label("From");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.start).speed(0.1).suffix(" nm"));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut s.stop).speed(0.1).suffix(" nm"));
                    });
                    ui.end_row();
                }
                SweepMode::Parameter => {
                    ui.label("Part");
                    ui.horizontal(|ui| {
                        ui.label(target_name.as_deref().unwrap_or("none"));
                        let usable = selection.filter(|&item| !sweep::params(&self.layout, item).is_empty());
                        if ui
                            .add_enabled(usable.is_some(), egui::Button::new("Use selection"))
                            .on_disabled_hover_text("Select a part or block with numeric parameters")
                            .clicked()
                            && let Some(item) = usable
                        {
                            let first = sweep::params(&self.layout, item)[0];
                            s.target = Some(item);
                            s.param = first.name;
                            s.start = first.min;
                            s.stop = first.max;
                        }
                    });
                    ui.end_row();
                    ui.label("Parameter");
                    egui::ComboBox::from_id_salt("sweep_param").selected_text(s.param).show_ui(ui, |ui| {
                        for p in &params {
                            if ui.selectable_label(s.param == p.name, p.name).clicked() {
                                s.param = p.name;
                                s.start = p.min;
                                s.stop = p.max;
                            }
                        }
                    });
                    ui.end_row();
                    let unit = params.iter().find(|p| p.name == s.param).map_or("", |p| p.unit);
                    ui.label("From");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.start).speed(0.01).suffix(format!(" {unit}")));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut s.stop).speed(0.01).suffix(format!(" {unit}")));
                    });
                    ui.end_row();
                }
            }
            ui.label("Points");
            ui.add(egui::DragValue::new(&mut s.points).range(2..=5001));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            let ready = match s.mode {
                SweepMode::Wavelength => s.laser.is_some(),
                SweepMode::Parameter => s.target.is_some() && !s.param.is_empty(),
            };
            if ui.add_enabled(ready, egui::Button::new(RichText::new("▶  Run sweep").strong())).clicked() {
                run = true;
            }
            ui.checkbox(&mut s.db, "dB").on_hover_text("10·log10 of the power, relative to a unit laser");
            ui.checkbox(&mut s.box_outputs, "Box outputs").on_hover_text("Also plot the light leaving every block and component");
        });
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
        let db = s.db;
        // Box outputs are many; show them when asked, or when there's nothing else.
        let only_boxes = result.series.iter().all(|s| s.from_box);
        let shown: Vec<&sweep::Series> = result.series.iter().filter(|x| s.box_outputs || only_boxes || !x.from_box).collect();
        Plot::new("sweep_plot")
            .height((ui.available_height() - 36.0).max(200.0))
            .legend(Legend::default())
            .x_axis_label(result.x_label.clone())
            .y_axis_label(if db { "Power (dB)" } else { "Power" })
            .show(ui, |p| {
                for series in &shown {
                    let points: Vec<[f64; 2]> = result
                        .xs
                        .iter()
                        .zip(&series.power)
                        .map(|(&x, &y)| [x, if db { sweep::db(y) } else { y }])
                        .collect();
                    p.line(Line::new(series.name.clone(), points));
                }
            });
        ui.horizontal(|ui| {
            if ui.button("Copy CSV").on_hover_text("Paste into a spreadsheet").clicked() {
                ui.ctx().copy_text(result.to_csv());
            }
            if ui.button("Save CSV…").clicked() {
                save_csv(&result.to_csv());
            }
            ui.label(RichText::new(format!("{} points × {} outputs", result.xs.len(), result.series.len())).small().weak());
        });
        run
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_csv(csv: &str) {
    if let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("sweep.csv").save_file()
        && let Err(e) = std::fs::write(&path, csv)
    {
        tracing::error!("Failed to save {}: {e}", path.display());
    }
}

#[cfg(target_arch = "wasm32")]
fn save_csv(csv: &str) {
    if let Err(e) = crate::web::download("sweep.csv", csv.as_bytes()) {
        tracing::error!("Download failed: {e}");
    }
}
