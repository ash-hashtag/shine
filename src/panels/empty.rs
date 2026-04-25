pub struct EmptyPanel;

impl EmptyPanel {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("Empty Panel");
            ui.label("This is an empty panel to verify setup.");
        });
    }
}
