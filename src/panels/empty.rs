pub struct EmptyPanel;

impl EmptyPanel {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("Empty Panel");
            ui.label("This is an empty panel to verify setup.");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| EmptyPanel.ui(ui))
            .textures_delta
            .clear();
    }
}
