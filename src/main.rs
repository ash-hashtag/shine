use shine::ShineApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Shine",
        options,
        Box::new(|_cc| Ok(Box::<ShineApp>::default())),
    )
}
