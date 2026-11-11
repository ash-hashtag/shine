//! Analysis → Monte Carlo…: the active design under fabrication variation.
//! Many varied copies are simulated; each output's spread shows as a
//! histogram with its statistics, and the share meeting a spec as yield.

use egui::RichText;
use egui_plot::{Bar, BarChart, Legend, Plot, VLine};

use super::Workspace;
use crate::panels::circuit::monte_carlo::{self, MonteCarlo, Variation};
use crate::panels::{ui_kit, units};

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloWindow {
    pub open: bool,
    pub variation: Variation,
    pub samples: usize,
    pub seed: u64,
    pub result: Option<MonteCarlo>,
    /// The output shown, by name.
    pub output: Option<String>,
    /// The spec for yield: at least this power (mW), and at most this one if set.
    pub min_mw: f64,
    pub max_mw: Option<f64>,
}

impl Default for MonteCarloWindow {
    fn default() -> Self {
        Self { open: false, variation: Variation::default(), samples: 200, seed: 1, result: None, output: None, min_mw: 0.0, max_mw: None }
    }
}

impl Workspace {
    pub fn open_monte_carlo_window(&mut self) {
        self.monte_carlo_window.open = true;
    }

    /// Run the active design's Monte Carlo with the window's settings.
    pub fn run_monte_carlo(&mut self) {
        let Some(id) = self.active else { return };
        let Some(doc) = self.project.doc(id) else { return };
        let catalog = self.catalog();
        let layout = doc.layout();
        let names = |inst| layout.instance(inst).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        let w = &self.monte_carlo_window;
        let started = web_time::Instant::now();
        let mc = monte_carlo::run(layout, doc.panel().cell_um, &self.project, &doc.panel().machine, Some(id), &w.variation, w.samples, w.seed, &names);
        tracing::debug!("Monte Carlo: {} samples in {:?}", w.samples, started.elapsed());
        let w = &mut self.monte_carlo_window;
        if w.output.as_ref().is_none_or(|o| !mc.series.iter().any(|s| &s.name == o)) {
            w.output = mc.series.iter().find(|s| !s.from_box).or(mc.series.first()).map(|s| s.name.clone());
            // A spec to start from: within 10 % of the nominal power.
            if let Some(s) = mc.series.iter().find(|s| Some(&s.name) == w.output.as_ref()) {
                w.min_mw = s.nominal * 0.9;
                w.max_mw = None;
            }
        }
        w.result = Some(mc);
    }

    pub(super) fn monte_carlo_window_ui(&mut self, ctx: &egui::Context) {
        if !self.monte_carlo_window.open {
            return;
        }
        let unit = self.prefs.power_unit;
        let mut open = true;
        let (mut run, mut copy, mut save) = (false, false, false);
        ui_kit::window(ctx, "Monte Carlo")
            .id(egui::Id::unique("monte_carlo_window"))
            .open(&mut open)
            .default_size([640.0, 560.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let w = &mut self.monte_carlo_window;
                ui.label(
                    RichText::new("How the design holds up to fabrication variation: every block's and part's parameters are drawn around their values, and each sample is simulated in full.")
                        .small()
                        .weak(),
                );
                egui::Grid::new("mc_settings").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                    let v = &mut w.variation;
                    let l = ui.label("Effective index σ").id;
                    ui.add(egui::DragValue::new(&mut v.n_eff).range(0.0..=0.1).speed(0.0001).max_decimals(4))
                        .labelled_by(l)
                        .on_hover_text("Absolute: waveguides and rings (width and thickness variation)");
                    let l = ui.label("Coupling σ").id;
                    ui.add(egui::DragValue::new(&mut v.coupling).range(0.0..=1.0).speed(0.001).max_decimals(3))
                        .labelled_by(l)
                        .on_hover_text("Relative: 0.03 is 3 % of each coupling and split");
                    ui.end_row();
                    let l = ui.label("Phase σ").id;
                    ui.add(egui::DragValue::new(&mut v.phase).range(0.0..=3.2).speed(0.005).suffix(" rad")).labelled_by(l);
                    let l = ui.label("Loss σ").id;
                    ui.add(egui::DragValue::new(&mut v.loss).range(0.0..=1.0).speed(0.005).max_decimals(3)).labelled_by(l).on_hover_text("Relative");
                    ui.end_row();
                    let l = ui.label("Samples").id;
                    ui.add(egui::DragValue::new(&mut w.samples).range(10..=100_000)).labelled_by(l);
                    let l = ui.label("Seed").id;
                    ui.add(egui::DragValue::new(&mut w.seed)).labelled_by(l).on_hover_text("The same seed gives the same samples");
                    ui.end_row();
                });
                run = ui.button(RichText::new("▶  Run").strong()).clicked();
                let Some(mc) = w.result.clone() else { return };
                if let Some(e) = &mc.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    return;
                }
                ui.separator();
                let Some(series) = mc.series.iter().find(|s| Some(&s.name) == w.output.as_ref()).or(mc.series.first()) else {
                    ui.label(RichText::new("The design has no outputs to measure.").weak());
                    return;
                };
                ui.horizontal(|ui| {
                    let l = ui.label("Output").id;
                    egui::ComboBox::from_id_salt("mc_output")
                        .selected_text(series.name.as_str())
                        .show_ui(ui, |ui| {
                            for s in &mc.series {
                                if ui.selectable_label(s.name == series.name, &s.name).clicked() {
                                    w.output = Some(s.name.clone());
                                }
                            }
                        })
                        .response
                        .labelled_by(l);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        save = ui.button("Save CSV…").clicked();
                        copy = ui.button("Copy CSV").clicked();
                    });
                });
                let st = series.stats();
                ui.horizontal_wrapped(|ui| {
                    let fmt = |p: f64| units::format_power(p, unit);
                    for (label, value) in [
                        ("Nominal", fmt(series.nominal)),
                        ("Mean", fmt(st.mean)),
                        ("σ", fmt(st.std)),
                        ("5–95 %", format!("{} – {}", fmt(st.p5), fmt(st.p95))),
                        ("Range", format!("{} – {}", fmt(st.min), fmt(st.max))),
                    ] {
                        ui.label(RichText::new(label).small().weak());
                        ui.label(RichText::new(value).monospace());
                        ui.add_space(8.0);
                    }
                });
                ui.horizontal(|ui| {
                    let l = ui.label("Spec: at least").id;
                    ui.add(egui::DragValue::new(&mut w.min_mw).range(0.0..=1e6).speed(0.001).max_decimals(6).suffix(" mW")).labelled_by(l);
                    let mut has_max = w.max_mw.is_some();
                    if ui.checkbox(&mut has_max, "and at most").changed() {
                        w.max_mw = has_max.then_some(series.nominal * 1.1);
                    }
                    if let Some(max) = &mut w.max_mw {
                        ui.add(egui::DragValue::new(max).range(0.0..=1e6).speed(0.001).max_decimals(6).suffix(" mW")).accessible_name("Spec: at most");
                    }
                    let y = series.yield_of(w.min_mw, w.max_mw);
                    ui.label(RichText::new(format!("Yield {:.1} %", y * 100.0)).strong())
                        .on_hover_text(format!("{} of {} samples meet the spec", (y * series.power.len() as f64).round(), series.power.len()));
                });
                // A histogram of the samples, the nominal value marked.
                let bins = ((series.power.len() as f64).sqrt().round() as usize).clamp(5, 60);
                let (lo, hi) = (st.min, st.max.max(st.min + 1e-12));
                let width = (hi - lo) / bins as f64;
                let mut counts = vec![0usize; bins];
                for &p in &series.power {
                    counts[(((p - lo) / width) as usize).min(bins - 1)] += 1;
                }
                let dark = ui.visuals().dark_mode;
                let fill = if dark { egui::Color32::from_rgb(0x39, 0x87, 0xe5) } else { egui::Color32::from_rgb(0x2a, 0x78, 0xd6) };
                let bars: Vec<Bar> = counts.iter().enumerate().map(|(k, &n)| Bar::new(lo + (k as f64 + 0.5) * width, n as f64).width(width * 0.9)).collect();
                let ink = ui.visuals().text_color();
                Plot::new("mc_histogram")
                    .legend(Legend::default())
                    .x_axis_label("Power (mW)")
                    .y_axis_label("Samples")
                    .height(ui.available_height().max(160.0))
                    .show(ui, |plot| {
                        plot.bar_chart(BarChart::new("Samples", bars).color(fill));
                        plot.vline(VLine::new("Nominal", series.nominal).color(ink).width(1.5));
                    });
            });
        if !open {
            self.monte_carlo_window.open = false;
        }
        if run {
            self.run_monte_carlo();
        }
        let csv = self.monte_carlo_window.result.as_ref().map(MonteCarlo::to_csv);
        if copy && let Some(csv) = &csv {
            ctx.copy_text(csv.clone());
            self.notify("Monte Carlo samples copied as CSV");
        }
        if save && let Some(csv) = csv {
            match crate::panels::export::save("monte-carlo.csv", "CSV", "csv", csv.as_bytes()) {
                Ok(Some(saved)) => self.notify(format!("Saved {saved}")),
                Ok(None) => {}
                Err(e) => self.notify(format!("Couldn't save: {e}")),
            }
        }
    }
}
