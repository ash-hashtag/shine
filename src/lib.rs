pub mod photonics;

#[cfg(all(feature = "logging", not(target_arch = "wasm32")))]
pub mod logging;

#[cfg(all(feature = "gui", target_arch = "wasm32"))]
pub mod web;

#[cfg(feature = "gui")]
pub mod mcp;

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

    impl ShineApp {
        /// The app with the preferences saved by an earlier session.
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let mut app = Self::default();
            if let Some(prefs) = cc.storage.and_then(|s| eframe::get_value(s, crate::panels::prefs::STORAGE_KEY)) {
                app.workspace.set_preferences(prefs);
            }
            app
        }
    }

    impl eframe::App for ShineApp {
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            eframe::set_value(storage, crate::panels::prefs::STORAGE_KEY, &self.workspace.prefs);
        }

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
            assert!(app.workspace.project.documents[0].layout().components.is_empty());
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

#[cfg(all(test, feature = "gui"))]
mod docs_coverage;
