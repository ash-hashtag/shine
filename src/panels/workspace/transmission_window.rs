//! Analysis → Transmission…: how a design passes light between its ports.
//! An S-matrix grid at one wavelength, and for the chosen paths their
//! power, phase and group delay over the wavelengths.

use egui::{Color32, RichText};
use egui_plot::{Legend, Line, Plot};

use super::Workspace;
use crate::panels::circuit::sim::{self, Machine};
use crate::panels::circuit::transfer::{self, Path};
use crate::panels::project::{DocId, layout_interface};
use crate::panels::ui_kit;

/// What the plot shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quantity {
    #[default]
    Power,
    Phase,
    GroupDelay,
}

impl Quantity {
    fn label(self) -> &'static str {
        match self {
            Quantity::Power => "Power (dB)",
            Quantity::Phase => "Phase (rad)",
            Quantity::GroupDelay => "Group delay (ps)",
        }
    }
}

/// A computed result: port names and the field of every path at every wavelength.
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub doc: DocId,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub wavelengths_nm: Vec<f64>,
    /// `fields[o][i][k]`: from input `i` to output `o` at wavelength `k`.
    pub fields: Vec<Vec<Vec<crate::photonics::C64>>>,
}

impl Computed {
    pub fn path(&self, input: usize, output: usize) -> Path {
        transfer::path(&self.wavelengths_nm, &self.fields[output][input])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransmissionWindow {
    pub open: bool,
    pub doc: Option<DocId>,
    pub from_nm: f64,
    pub to_nm: f64,
    pub points: usize,
    pub result: Option<Computed>,
    pub error: Option<String>,
    /// The wavelength the grid shows, as an index into the result.
    pub at: usize,
    /// Plotted paths: (input, output), in the order chosen.
    pub selected: Vec<(usize, usize)>,
    pub quantity: Quantity,
}

impl Default for TransmissionWindow {
    fn default() -> Self {
        Self { open: false, doc: None, from_nm: 1500.0, to_nm: 1600.0, points: 2001, result: None, error: None, at: 0, selected: Vec::new(), quantity: Quantity::Power }
    }
}

/// The eight categorical series colours, in fixed order (light, dark).
const SERIES: [(u32, u32); 8] = [
    (0x2a78d6, 0x3987e5),
    (0xeb6834, 0xd95926),
    (0x1baf7a, 0x199e70),
    (0xeda100, 0xc98500),
    (0xe87ba4, 0xd55181),
    (0x008300, 0x008300),
    (0x6250d6, 0x9085e9),
    (0xe34948, 0xe66767),
];
/// The grid's range: below this much loss a path counts as dark.
const FLOOR_DB: f64 = -40.0;

fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn series(k: usize, dark: bool) -> Color32 {
    let (l, d) = SERIES[k % SERIES.len()];
    hex(if dark { d } else { l })
}

/// A cell's fill for power `p` (0–1): near zero recedes toward the surface,
/// full transmission is the strongest step (the ramp reversed in dark mode).
fn ramp(p: f64, dark: bool) -> Color32 {
    let db = if p > 0.0 { 10.0 * p.log10() } else { FLOOR_DB };
    ui_kit::magnitude_color((db - FLOOR_DB) / -FLOOR_DB, dark)
}

fn db(p: f64) -> f64 {
    if p > 0.0 { (10.0 * p.log10()).max(-200.0) } else { -200.0 }
}

impl Workspace {
    pub fn open_transmission_window(&mut self) {
        let w = &mut self.transmission_window;
        w.open = true;
        if w.doc.is_none() || w.doc.and_then(|d| self.project.doc(d)).is_none() {
            w.doc = self.active;
        }
    }

    /// Compute the transmission with the window's settings.
    pub fn compute_transmission(&mut self) {
        let w = &mut self.transmission_window;
        w.error = None;
        let Some(id) = w.doc else { return };
        let Some(doc) = self.project.doc(id) else { return };
        let wavelengths = crate::panels::interop::wavelengths(w.from_nm.min(w.to_nm), w.from_nm.max(w.to_nm), w.points);
        match sim::transmission(doc.layout(), doc.panel().cell_um, &self.project, &Machine::default(), Some(id), &wavelengths) {
            Ok(t) => {
                let names = layout_interface(doc.layout());
                let (outputs, inputs) = (names.outputs.len(), names.inputs.len());
                let fields = (0..outputs).map(|o| (0..inputs).map(|i| t.matrices.iter().map(|m| m[(o, i)]).collect()).collect()).collect();
                w.at = wavelengths.len() / 2;
                w.selected.retain(|&(i, o)| i < inputs && o < outputs);
                if w.selected.is_empty() {
                    w.selected.push((0, 0));
                }
                w.result = Some(Computed { doc: id, inputs: names.inputs, outputs: names.outputs, wavelengths_nm: t.wavelengths_nm, fields });
            }
            Err(e) => {
                w.result = None;
                w.error = Some(e);
            }
        }
    }

    /// The selected paths as CSV: wavelength, then power, dB, phase and group delay of each.
    pub fn transmission_csv(&self) -> Option<String> {
        let w = &self.transmission_window;
        let r = w.result.as_ref()?;
        let paths: Vec<(String, Path)> = w.selected.iter().map(|&(i, o)| (format!("{} → {}", r.inputs[i], r.outputs[o]), r.path(i, o))).collect();
        let quote = |s: String| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = "\"wavelength (nm)\"".to_owned();
        for (name, _) in &paths {
            for q in ["power", "power (dB)", "phase (rad)", "group delay (ps)"] {
                out += &format!(",{}", quote(format!("{name} {q}")));
            }
        }
        out.push('\n');
        for (k, wl) in r.wavelengths_nm.iter().enumerate() {
            out += &format!("{wl}");
            for (_, p) in &paths {
                out += &format!(",{},{},{},{}", p.power[k], db(p.power[k]), p.phase[k], p.group_delay_ps[k]);
            }
            out.push('\n');
        }
        Some(out)
    }

    pub(super) fn transmission_window_ui(&mut self, ctx: &egui::Context) {
        if !self.transmission_window.open {
            return;
        }
        let mut open = true;
        let (mut compute, mut copy, mut save) = (false, false, false);
        let docs: Vec<(DocId, String)> = self.project.documents.iter().map(|d| (d.id, d.name.clone())).collect();
        ui_kit::window(ctx, "Transmission")
            .id(egui::Id::unique("transmission_window"))
            .open(&mut open)
            .default_size([760.0, 620.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let dark = ui.visuals().dark_mode;
                let w = &mut self.transmission_window;
                ui.label(
                    RichText::new("How the design passes light from each of its inputs to each output: power, phase and group delay over the wavelengths.")
                        .small()
                        .weak(),
                );
                ui.horizontal_wrapped(|ui| {
                    let l = ui.label("Design").id;
                    let current = w.doc.and_then(|id| docs.iter().find(|d| d.0 == id)).map_or("Choose…", |d| d.1.as_str());
                    egui::ComboBox::from_id_salt("transmission_doc")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (id, name) in &docs {
                                ui.selectable_value(&mut w.doc, Some(*id), name);
                            }
                        })
                        .response
                        .labelled_by(l);
                    ui.add(egui::DragValue::new(&mut w.from_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm")).accessible_name("From wavelength");
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut w.to_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm")).accessible_name("To wavelength");
                    let l = ui.label("Points").id;
                    ui.add(egui::DragValue::new(&mut w.points).range(2..=100_000)).labelled_by(l);
                    compute = ui.add_enabled(w.doc.is_some(), egui::Button::new(RichText::new("Compute").strong())).clicked();
                });
                if let Some(e) = &w.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let Some(r) = w.result.clone() else {
                    ui.label(RichText::new("The design needs light Input and Output ports (or pins).").weak());
                    return;
                };
                ui.separator();
                // The S-matrix at one wavelength: a cell per input → output.
                let last = r.wavelengths_nm.len().saturating_sub(1);
                w.at = w.at.min(last);
                ui.horizontal(|ui| {
                    let l = ui.label("At").id;
                    ui.add(egui::Slider::new(&mut w.at, 0..=last).show_value(false)).labelled_by(l);
                    ui.label(RichText::new(format!("{:.3} nm", r.wavelengths_nm[w.at])).monospace());
                });
                ui.label(RichText::new("Click a path to plot it; Ctrl+click to add it.").small().weak());
                egui::ScrollArea::both().id_salt("transmission_grid").max_height(180.0).show(ui, |ui| {
                    egui::Grid::new("transmission_cells").spacing([2.0, 2.0]).show(ui, |ui| {
                        ui.label("");
                        for name in &r.inputs {
                            ui.label(RichText::new(format!("from {name}")).small());
                        }
                        ui.end_row();
                        for (o, out) in r.outputs.iter().enumerate() {
                            ui.label(RichText::new(format!("to {out}")).small());
                            for (i, input) in r.inputs.iter().enumerate() {
                                let p = r.fields[o][i][w.at].norm_sqr();
                                let fill = ramp(p, dark);
                                let chosen = w.selected.iter().position(|&s| s == (i, o));
                                let text = if p > 0.0 { format!("{:.2} dB", db(p)) } else { "–".into() };
                                let ink = crate::panels::appearance::text_on(fill);
                                let mut button = egui::Button::new(RichText::new(&text).monospace().color(ink)).fill(fill).min_size(egui::vec2(84.0, 24.0));
                                if let Some(k) = chosen {
                                    button = button.stroke(egui::Stroke::new(2.5, series(k, dark)));
                                }
                                let response = ui
                                    .add(button)
                                    .accessible_name(format!("{input} → {out}: {text}"))
                                    .on_hover_text(format!("{input} → {out}\n{text} ({:.4} of the power)", p));
                                if response.clicked() {
                                    if ui.input(|i| i.modifiers.command) {
                                        match chosen {
                                            Some(k) => {
                                                w.selected.remove(k);
                                            }
                                            None if w.selected.len() < SERIES.len() => w.selected.push((i, o)),
                                            None => {}
                                        }
                                    } else {
                                        w.selected = vec![(i, o)];
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    for q in [Quantity::Power, Quantity::Phase, Quantity::GroupDelay] {
                        ui.selectable_value(&mut w.quantity, q, q.label());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        save = ui.button("Save CSV…").clicked();
                        copy = ui.button("Copy CSV").clicked();
                    });
                });
                let quantity = w.quantity;
                let paths: Vec<(String, Path)> =
                    w.selected.iter().map(|&(i, o)| (format!("{} → {}", r.inputs[i], r.outputs[o]), r.path(i, o))).collect();
                Plot::new("transmission_plot")
                    .legend(Legend::default())
                    .x_axis_label("Wavelength (nm)")
                    .y_axis_label(quantity.label())
                    .height(ui.available_height().max(180.0))
                    .show(ui, |plot| {
                        for (k, (name, p)) in paths.iter().enumerate() {
                            let ys: Vec<f64> = match quantity {
                                Quantity::Power => p.power.iter().map(|&x| db(x)).collect(),
                                Quantity::Phase => p.phase.clone(),
                                Quantity::GroupDelay => p.group_delay_ps.clone(),
                            };
                            let points: Vec<[f64; 2]> = p.wavelengths_nm.iter().zip(ys).map(|(&x, y)| [x, y]).collect();
                            plot.line(Line::new(name.clone(), points).color(series(k, dark)).width(2.0));
                        }
                    });
            });
        if !open {
            self.transmission_window.open = false;
        }
        if compute {
            self.compute_transmission();
        }
        if copy && let Some(csv) = self.transmission_csv() {
            ctx.copy_text(csv);
            self.notify("Transmission copied as CSV");
        }
        if save && let Some(csv) = self.transmission_csv() {
            match crate::panels::export::save("transmission.csv", "CSV", "csv", csv.as_bytes()) {
                Ok(Some(saved)) => self.notify(format!("Saved {saved}")),
                Ok(None) => {}
                Err(e) => self.notify(format!("Couldn't save: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_goes_light_to_dark_and_reverses_in_dark_mode() {
        assert_eq!(ramp(0.0, false), ui_kit::magnitude_color(0.0, false));
        assert_eq!(ramp(1.0, false), ui_kit::magnitude_color(1.0, false));
        assert_eq!(ramp(1e-9, true), ui_kit::magnitude_color(0.0, true), "near zero recedes into the dark surface");
        assert_ne!(ui_kit::magnitude_color(0.0, true), ui_kit::magnitude_color(0.0, false));
    }
}
