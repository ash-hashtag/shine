//! Analysis → Optimise…: let Shine find the parameter values that bring an
//! output to a goal (desktop app: the optimiser runs natively).

use egui::RichText;

use super::Workspace;
use crate::panels::circuit::sweep;
use crate::panels::optics::model::Item;
use crate::panels::{ui_kit, units};

/// What the window asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum GoalKind {
    #[default]
    Maximise,
    Minimise,
    Target,
}

/// A parameter to adjust, as listed in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct VariableRow {
    pub item: Item,
    pub param: String,
    pub min: f64,
    pub max: f64,
}

/// What an optimisation found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The values, one per variable.
    pub values: Vec<f64>,
    /// The output's power before and after, mW.
    pub before: f64,
    pub after: f64,
    pub simulations: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptimiseWindow {
    pub open: bool,
    pub variables: Vec<VariableRow>,
    pub output: Option<String>,
    pub goal: GoalKind,
    pub target_mw: f64,
    pub steps: u64,
    /// The last run, or why it failed.
    pub result: Option<Result<Found, String>>,
}

impl Default for OptimiseWindow {
    fn default() -> Self {
        Self { open: false, variables: Vec::new(), output: None, goal: GoalKind::Maximise, target_mw: 0.5, steps: 300, result: None }
    }
}

impl Workspace {
    pub fn open_optimise_window(&mut self) {
        self.optimise_window.open = true;
    }

    /// The active design's outputs, as sweeps and Monte Carlo name them.
    fn output_names(&self) -> Vec<String> {
        let Some(doc) = self.active.and_then(|id| self.project.doc(id)) else { return Vec::new() };
        let p = doc.panel();
        let Some(o) = p.outcome() else { return Vec::new() };
        let catalog = self.catalog();
        let layout = &p.layout;
        let names = |inst| layout.instance(inst).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        sweep::outputs(layout, o, &names).into_iter().map(|o| o.0).collect()
    }

    /// Run the optimiser on the active design.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_optimise(&mut self) {
        use crate::panels::circuit::optimise::{Goal, Objective, Variable, optimise};
        let Some(id) = self.active else { return };
        let Some(doc) = self.project.doc(id) else { return };
        let w = &self.optimise_window;
        let Some(output) = w.output.clone() else {
            self.optimise_window.result = Some(Err("Choose the output to optimise".into()));
            return;
        };
        let vars: Vec<Variable> = w.variables.iter().map(|v| Variable { item: v.item, param: v.param.clone(), min: v.min, max: v.max }).collect();
        let goal = match w.goal {
            GoalKind::Maximise => Goal::Maximise,
            GoalKind::Minimise => Goal::Minimise,
            GoalKind::Target => Goal::Target(w.target_mw),
        };
        let catalog = self.catalog();
        let layout = doc.layout();
        let names = |inst| layout.instance(inst).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        let started = web_time::Instant::now();
        let r = optimise(layout, doc.panel().cell_um, &self.project, &doc.panel().machine, Some(id), &vars, &Objective { output, goal }, w.steps, &names);
        tracing::debug!("Optimised in {} simulations, {:?}", r.simulations, started.elapsed());
        self.optimise_window.result = Some(match r.error {
            Some(e) => Err(e),
            None => Ok(Found { values: r.values, before: r.before, after: r.after, simulations: r.simulations }),
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn run_optimise(&mut self) {
        self.optimise_window.result = Some(Err("Optimisation runs in the desktop app".into()));
    }

    /// Put the values found into the design (undoable).
    pub fn apply_optimised(&mut self) {
        let Some(Ok(found)) = self.optimise_window.result.clone() else { return };
        let rows = self.optimise_window.variables.clone();
        if let Some(p) = self.active_panel() {
            for (row, v) in rows.iter().zip(found.values) {
                sweep::set(&mut p.layout, row.item, &row.param, v);
            }
        }
        self.notify("Applied the optimised values");
    }

    pub(super) fn optimise_window_ui(&mut self, ctx: &egui::Context) {
        if !self.optimise_window.open {
            return;
        }
        let unit = self.prefs.power_unit;
        let outputs = self.output_names();
        // The selected part or box, and what it can adjust.
        let selection = self.active.and_then(|id| self.project.doc(id)).and_then(|d| {
            let p = d.panel();
            let item = p
                .editor
                .boxes
                .active()
                .and_then(|b| p.layout.instances.get(b).map(|i| Item::Box(i.id)))
                .or(p.editor.selection.active().map(Item::Part))?;
            Some((item, p.item_name(item), p.sweep_params(item)))
        });
        let names: Vec<String> = self
            .optimise_window
            .variables
            .iter()
            .map(|v| self.active.and_then(|id| self.project.doc(id)).map_or_else(String::new, |d| d.panel().item_name(v.item)))
            .collect();
        let mut open = true;
        let (mut run, mut apply) = (false, false);
        ui_kit::window(ctx, "Optimise")
            .id(egui::Id::unique("optimise_window"))
            .open(&mut open)
            .default_width(520.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let w = &mut self.optimise_window;
                ui.label(
                    RichText::new("Shine adjusts the parameters below, within their bounds, until the output meets the goal. Each step is a full simulation; it finds the best values near the current ones.")
                        .small()
                        .weak(),
                );
                ui_kit::section(ui, "Adjust");
                let mut remove = None;
                egui::Grid::new("optimise_vars").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (k, v) in w.variables.iter_mut().enumerate() {
                        ui.label(format!("{} · {}", names.get(k).map_or("", String::as_str), v.param));
                        ui.add(egui::DragValue::new(&mut v.min).speed(0.01)).accessible_name(format!("{} from", v.param));
                        ui.add(egui::DragValue::new(&mut v.max).speed(0.01)).accessible_name(format!("{} to", v.param));
                        if ui.small_button("×").accessible_name(format!("Stop adjusting {}", v.param)).clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    w.variables.remove(k);
                }
                match &selection {
                    Some((item, name, params)) if !params.is_empty() => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("Add from {name}:")).small());
                            for p in params {
                                let taken = w.variables.iter().any(|v| v.item == *item && v.param == p.name);
                                if ui.add_enabled(!taken, egui::Button::new(p.name.as_str()).small()).clicked() {
                                    w.variables.push(VariableRow { item: *item, param: p.name.clone(), min: p.min, max: p.max });
                                }
                            }
                        });
                    }
                    _ => {
                        ui.label(RichText::new("Select a part or block to add its parameters.").small().weak());
                    }
                }
                ui_kit::section(ui, "Goal");
                ui.horizontal_wrapped(|ui| {
                    let l = ui.label("Output").id;
                    egui::ComboBox::from_id_salt("optimise_output")
                        .selected_text(w.output.as_deref().unwrap_or("Choose…"))
                        .show_ui(ui, |ui| {
                            for n in &outputs {
                                ui.selectable_value(&mut w.output, Some(n.clone()), n);
                            }
                        })
                        .response
                        .labelled_by(l);
                    ui.selectable_value(&mut w.goal, GoalKind::Maximise, "Maximise");
                    ui.selectable_value(&mut w.goal, GoalKind::Minimise, "Minimise");
                    ui.selectable_value(&mut w.goal, GoalKind::Target, "Reach");
                    if w.goal == GoalKind::Target {
                        ui.add(egui::DragValue::new(&mut w.target_mw).range(0.0..=1e6).speed(0.001).max_decimals(6).suffix(" mW"))
                            .accessible_name("Target power");
                    }
                });
                ui.horizontal(|ui| {
                    let l = ui.label("At most").id;
                    ui.add(egui::DragValue::new(&mut w.steps).range(10..=5000)).labelled_by(l);
                    ui.label(RichText::new("steps").small().weak());
                    let ready = !w.variables.is_empty() && w.output.is_some() && cfg!(not(target_arch = "wasm32"));
                    run = ui
                        .add_enabled(ready, egui::Button::new(RichText::new("▶  Optimise").strong()))
                        .on_disabled_hover_text(if cfg!(target_arch = "wasm32") { "Optimisation runs in the desktop app" } else { "Add a parameter and choose an output" })
                        .clicked();
                });
                match &w.result {
                    Some(Ok(found)) => {
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "{} → {}  ({} simulations)",
                                units::format_power(found.before, unit),
                                units::format_power(found.after, unit),
                                found.simulations
                            ))
                            .strong(),
                        );
                        for (k, (v, x)) in w.variables.iter().zip(&found.values).enumerate() {
                            ui.label(RichText::new(format!("{} · {} = {x:.6}", names.get(k).map_or("", String::as_str), v.param)).monospace());
                        }
                        apply = ui.button("Apply to the design").on_hover_text("Undo with Ctrl+Z").clicked();
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    None => {}
                }
            });
        if !open {
            self.optimise_window.open = false;
        }
        if run {
            self.run_optimise();
        }
        if apply {
            self.apply_optimised();
        }
    }
}
