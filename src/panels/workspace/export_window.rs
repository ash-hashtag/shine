//! File → Export for other tools…: S-parameters for circuit simulators, a
//! netlist for SAX, and a layout description for gdsfactory (towards a mask).

use egui::RichText;

use super::Workspace;
use crate::panels::interop::{self, gdsfactory, sax};
use crate::panels::project::DocId;
use crate::panels::ui_kit;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Touchstone,
    Gdsfactory,
    Sax,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportWindow {
    pub open: bool,
    pub format: ExportFormat,
    /// The document to export; the active one when opened.
    pub doc: Option<DocId>,
    pub from_nm: f64,
    pub to_nm: f64,
    pub points: usize,
    pub spacing_um: f64,
    /// What the last export said: the summary and what didn't carry over.
    pub result: Option<(String, Vec<String>)>,
}

impl Default for ExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            format: ExportFormat::Touchstone,
            doc: None,
            from_nm: 1500.0,
            to_nm: 1600.0,
            points: 401,
            spacing_um: gdsfactory::DEFAULT_SPACING_UM,
            result: None,
        }
    }
}

impl Workspace {
    pub fn open_export_window(&mut self, format: ExportFormat) {
        let w = &mut self.export_window;
        w.open = true;
        w.format = format;
        w.doc = self.active;
        w.result = None;
    }

    /// Export with the window's settings: the file is saved (or downloaded).
    pub fn run_export(&mut self) {
        let w = self.export_window.clone();
        let Some(id) = w.doc.or(self.active) else { return };
        let Some(doc) = self.project.doc(id) else { return };
        let name = doc.name.clone();
        let stem = super::slug(&name);
        let source = self.file_name();
        let outcome = match w.format {
            ExportFormat::Touchstone => {
                let wavelengths = interop::wavelengths(w.from_nm, w.to_nm, w.points);
                interop::touchstone(&self.project, id, &wavelengths, &source).map(|(text, ports)| {
                    let ext = format!("s{ports}p");
                    (format!("{stem}.{ext}"), "Touchstone S-parameters", ext, text, format!("{ports} ports, {} wavelengths", w.points), Vec::new())
                })
            }
            ExportFormat::Sax => {
                let wavelengths = interop::wavelengths(w.from_nm, w.to_nm, w.points);
                sax::export(&self.project, id, &wavelengths, &source).map(|e| {
                    let summary = format!("{} components, {} connections, {} ports", e.instances, e.connections, e.ports);
                    (format!("{}.py", stem.replace('-', "_")), "SAX netlist", "py".to_owned(), e.python, summary, Vec::new())
                })
            }
            ExportFormat::Gdsfactory => gdsfactory::export(&self.project, id, w.spacing_um, &source).map(|e| {
                let summary = format!("{} components, {} waveguides, {} ports", e.instances, e.routes, e.ports);
                (format!("{stem}.pic.yml"), "gdsfactory layout", "yml".to_owned(), e.yaml, summary, e.notes)
            }),
        };
        match outcome {
            Ok((file, kind, ext, text, summary, notes)) => match crate::panels::export::save(&file, kind, &ext, text.as_bytes()) {
                Ok(Some(saved)) => {
                    for n in &notes {
                        tracing::warn!("Export of “{name}”: {n}");
                    }
                    self.notify(format!("Saved {saved}"));
                    self.export_window.result = Some((format!("Saved {saved}: {summary}"), notes));
                }
                Ok(None) => {}
                Err(e) => self.export_window.result = Some((format!("Couldn't save: {e}"), Vec::new())),
            },
            Err(e) => self.export_window.result = Some((format!("Can't export “{name}”: {e}"), Vec::new())),
        }
    }

    pub(super) fn export_window_ui(&mut self, ctx: &egui::Context) {
        if !self.export_window.open {
            return;
        }
        let mut open = true;
        let mut export = false;
        let docs: Vec<(DocId, String)> = self.project.documents.iter().map(|d| (d.id, d.name.clone())).collect();
        ui_kit::window(ctx, "Export for other tools")
            .id(egui::Id::unique("export_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(440.0)
            .show(ctx, |ui| {
                let w = &mut self.export_window;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut w.format, ExportFormat::Touchstone, "S-parameters (Touchstone)");
                    ui.selectable_value(&mut w.format, ExportFormat::Sax, "Netlist for SAX");
                    ui.selectable_value(&mut w.format, ExportFormat::Gdsfactory, "Layout for gdsfactory");
                });
                ui.label(
                    RichText::new(match w.format {
                        ExportFormat::Touchstone => {
                            "How light passes between the design's Input and Output ports at each wavelength, as a .sNp file for circuit simulators (Lumerical, OptiSPICE, SAX…), scikit-rf and measurement tools."
                        }
                        ExportFormat::Sax => {
                            "The circuit as a SAX netlist (Python), with models that compute what Shine does: run it, swap in your foundry kit's models, or use it inside a larger SAX circuit. The wavelengths are what it prints when run, and where S-parameter components are sampled."
                        }
                        ExportFormat::Gdsfactory => {
                            "The design's waveguide blocks as gdsfactory components with their places and connections. gdsfactory draws the mask layout (GDS or OASIS) from it with your foundry's process kit: the way to a chip."
                        }
                    })
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                egui::Grid::new("export_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label("Design").id;
                    let current = w.doc.and_then(|id| docs.iter().find(|d| d.0 == id)).map_or("Choose…", |d| d.1.as_str());
                    egui::ComboBox::from_id_salt("export_doc")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (id, name) in &docs {
                                ui.selectable_value(&mut w.doc, Some(*id), name);
                            }
                        })
                        .response
                        .labelled_by(l);
                    ui.end_row();
                    match w.format {
                        ExportFormat::Touchstone | ExportFormat::Sax => {
                            ui.label("Wavelengths");
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut w.from_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm")).accessible_name("From wavelength");
                                ui.label("to");
                                ui.add(egui::DragValue::new(&mut w.to_nm).range(200.0..=2000.0).speed(0.5).suffix(" nm")).accessible_name("To wavelength");
                            });
                            ui.end_row();
                            let l = ui.label("Points").id;
                            ui.add(egui::DragValue::new(&mut w.points).range(2..=20001)).labelled_by(l);
                            ui.end_row();
                        }
                        ExportFormat::Gdsfactory => {
                            let l = ui.label("Spacing").id;
                            ui.add(egui::DragValue::new(&mut w.spacing_um).range(0.0..=5000.0).speed(1.0).suffix(" µm"))
                                .labelled_by(l)
                                .on_hover_text("Room between neighbouring components, for the waveguides gdsfactory routes");
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.add_enabled(w.doc.is_some(), egui::Button::new(RichText::new("Export…").strong())).clicked() {
                    export = true;
                }
                if let Some((summary, notes)) = &w.result {
                    ui.add_space(4.0);
                    ui.label(summary);
                    if !notes.is_empty() {
                        ui.label(RichText::new("Not carried over:").small().strong());
                        for n in notes {
                            ui.label(RichText::new(format!("• {n}")).small().weak());
                        }
                    }
                }
            });
        if !open {
            self.export_window.open = false;
        }
        if export {
            self.run_export();
        }
    }
}
