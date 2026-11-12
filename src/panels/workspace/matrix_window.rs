//! Component → New layer from a matrix…: any matrix as a component of MZI
//! meshes (see `optics::layers`), typed, pasted or read from a file.

use egui::RichText;

use super::Workspace;
use super::files::FilePurpose;
use crate::panels::optics::layers;
use crate::panels::ui_kit;

/// Above this many MZIs, the window says it's a big design before making it.
const MANY_MZIS: usize = 20_000;

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixWindow {
    pub open: bool,
    /// The matrix as text: a row per line.
    pub text: String,
    pub name: String,
    /// One mesh for a unitary, rather than a weight layer.
    pub unitary: bool,
}

impl Default for MatrixWindow {
    fn default() -> Self {
        Self { open: false, text: "0.5 -1 2\n0.25 0 1\n".into(), name: "Weights".into(), unitary: false }
    }
}

/// What the window's matrix would make, or why it can't.
fn preview(w: &MatrixWindow) -> Result<String, String> {
    let m = layers::parse_matrix(&w.text)?;
    let (rows, cols) = (m.nrows(), m.ncols());
    if w.unitary {
        if rows != cols {
            return Err(format!("a unitary is square; this is {rows}×{cols}"));
        }
        return Ok(format!("A {rows}×{rows} mesh of {} MZIs.", rows * rows.saturating_sub(1) / 2));
    }
    let scale = layers::svd(&m)?.scale;
    let mzis = layers::layer_mzis(rows, cols);
    let mut said = format!("A {rows}×{cols} weight layer of {mzis} MZIs, computing W x / {scale:.6} (W's largest singular value).");
    if mzis > MANY_MZIS {
        said += " A big design: placing it is quick, opening its meshes is slower.";
    }
    Ok(said)
}

impl Workspace {
    pub fn open_matrix_window(&mut self) {
        self.matrix_window.open = true;
    }

    /// The matrix read from a file, into the window.
    pub(super) fn set_matrix_text(&mut self, name: &str, bytes: &[u8]) {
        match String::from_utf8(bytes.to_vec()) {
            Ok(text) => {
                self.matrix_window.text = text;
                self.matrix_window.open = true;
            }
            Err(_) => self.notify(format!("“{name}” isn't text: write the matrix a row per line")),
        }
    }

    /// Make the window's layer: a component in the project, opened.
    pub fn create_matrix_layer(&mut self) -> Result<(), String> {
        let w = self.matrix_window.clone();
        let m = layers::parse_matrix(&w.text)?;
        let name = if w.name.trim().is_empty() { "Weights" } else { w.name.trim() };
        let id = if w.unitary {
            layers::unitary_mesh(&mut self.project, name, &m)?
        } else {
            layers::weight_layer(&mut self.project, name, &m)?.id
        };
        self.matrix_window.open = false;
        self.open_tab_asking(id);
        let made = self.project.doc(id).map_or_else(String::new, |d| d.name.clone());
        self.notify(format!("“{made}” is a component now: place it from Assets → This project"));
        Ok(())
    }

    pub(super) fn matrix_window_ui(&mut self, ctx: &egui::Context) {
        if !self.matrix_window.open {
            return;
        }
        let mut open = true;
        let (mut create, mut load) = (false, false);
        ui_kit::window(ctx, "New layer from a matrix")
            .id(egui::Id::unique("matrix_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                let w = &mut self.matrix_window;
                ui.label(
                    RichText::new(
                        "Any matrix W as a component that multiplies light by it: two meshes of MZIs (W = U Σ V†) with an attenuator per mode between them. Light can't be amplified, so it computes W x divided by W's largest singular value. Inputs in1…, outputs out1….",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                egui::Grid::new("matrix_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label("Name").id;
                    ui.text_edit_singleline(&mut w.name).labelled_by(l);
                    ui.end_row();
                    ui.label("Kind");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut w.unitary, false, "Weight layer");
                        ui.selectable_value(&mut w.unitary, true, "Unitary: one mesh");
                    });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label("The matrix, a row per line (spaces or commas between entries; complex as 0.5-0.2i; # for comments):");
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut w.text).code_editor().desired_rows(8).desired_width(f32::INFINITY));
                });
                ui.add_space(4.0);
                let said = preview(w);
                match &said {
                    Ok(s) => ui.label(RichText::new(s).small()),
                    Err(e) => ui.label(RichText::new(e).small().color(ui.visuals().error_fg_color)),
                };
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(said.is_ok(), egui::Button::new("Create")).clicked() {
                        create = true;
                    }
                    if ui.button("Read from a file…").clicked() {
                        load = true;
                    }
                });
            });
        if !open {
            self.matrix_window.open = false;
        }
        if load {
            self.pick_files(FilePurpose::Matrix, "Matrix", &["txt", "csv", "tsv", "dat"], false);
        }
        if create && let Err(e) = self.create_matrix_layer() {
            self.notify(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_says_what_it_will_make_and_makes_it() {
        let mut ws = Workspace::default();
        assert!(preview(&ws.matrix_window).unwrap().starts_with("A 2×3 weight layer of 4 MZIs"));
        ws.matrix_window.unitary = true;
        assert!(preview(&ws.matrix_window).unwrap_err().contains("square"));
        ws.matrix_window.text = "0 1\n1 0".into();
        ws.matrix_window.name = "Swap".into();
        assert_eq!(preview(&ws.matrix_window).unwrap(), "A 2×2 mesh of 1 MZIs.");
        ws.open_matrix_window();
        ws.create_matrix_layer().unwrap();
        assert_eq!(ws.project.doc(ws.active.unwrap()).unwrap().name, "Swap", "made and opened");
        assert!(!ws.matrix_window.open);
        ws.set_matrix_text("w.txt", b"1 2 3\n4 5 6\n");
        ws.matrix_window.unitary = false;
        assert!(preview(&ws.matrix_window).unwrap().starts_with("A 2×3 weight layer"));
    }
}
