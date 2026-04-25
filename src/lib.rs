pub mod panels;

use panels::empty::EmptyPanel;

pub struct ShineApp {
    pub name: String,
    pub empty_panel: EmptyPanel,
}

impl Default for ShineApp {
    fn default() -> Self {
        Self {
            name: "Shine".to_owned(),
            empty_panel: EmptyPanel,
        }
    }
}

impl eframe::App for ShineApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(&self.name);
            ui.separator();
            self.empty_panel.ui(ui);
        });
    }
}
