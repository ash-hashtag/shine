pub mod photonics;

#[cfg(feature = "logging")]
pub mod logging;

#[cfg(feature = "gui")]
pub mod panels;

#[cfg(feature = "gui")]
pub use app::ShineApp;

#[cfg(feature = "gui")]
mod app {
    use crate::panels::workspace::Workspace;

    #[derive(Default)]
    pub struct ShineApp {
        pub workspace: Workspace,
    }

    impl eframe::App for ShineApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.workspace.ui(ui));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn starts_with_an_empty_layout() {
            let app = ShineApp::default();
            assert_eq!(app.workspace.tabs.len(), 1);
            assert!(app.workspace.project.documents[0].is_layout());
        }

        #[test]
        fn renders_a_frame() {
            use eframe::App;
            let mut app = ShineApp::default();
            let mut frame = eframe::Frame::_new_kittest();
            let ctx = egui::Context::default();
            ctx.run_ui(Default::default(), |ui| app.ui(ui, &mut frame))
                .textures_delta
                .clear();
        }
    }
}
