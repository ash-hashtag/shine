use shine::ShineApp;

fn main() -> eframe::Result {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Shine",
        options,
        Box::new(|_cc| Ok(Box::<ShineApp>::default())),
    )
}
