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
        /// The app with the preferences and the session saved earlier.
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let mut app = Self::default();
            if let Some(storage) = cc.storage {
                app.load(
                    eframe::get_value(storage, crate::panels::prefs::STORAGE_KEY),
                    storage.get_string(crate::panels::session::KEY),
                );
            }
            app
        }

        /// Preferences and the last session, as kept in the app's storage.
        pub fn load(&mut self, prefs: Option<crate::panels::prefs::Preferences>, session: Option<String>) {
            use crate::panels::session;
            if let Some(prefs) = prefs {
                self.workspace.set_preferences(prefs);
            }
            if let Some(text) = session {
                match session::Session::decode(&text).and_then(|s| self.workspace.restore(s)) {
                    Ok(()) => tracing::info!("Restored the last session"),
                    Err(e) => {
                        tracing::warn!("Couldn't restore the last session: {e}");
                        self.workspace.notify(format!("Couldn't restore the last session: {e}"));
                    }
                }
            }
        }
    }

    impl eframe::App for ShineApp {
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            eframe::set_value(storage, crate::panels::prefs::STORAGE_KEY, &self.workspace.prefs);
            storage.set_string(crate::panels::session::KEY, self.workspace.session().encode());
        }

        /// Save the session often, so a crash or a closed tab loses little.
        fn auto_save_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(5)
        }

        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            // CPU time of the previous frame, drawing included.
            self.workspace.stats.frame_cpu_ms = frame.info().cpu_usage.map(|s| s as f64 * 1e3);
            if ui.input(|i| i.viewport().close_requested()) {
                // The save that follows marks the session as closed normally.
                self.workspace.closing = true;
            }
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
        fn the_last_session_comes_back_with_its_unsaved_changes() {
            use crate::panels::optics::model::{DETECTOR, PlacedComponent};
            let mut before = ShineApp::default();
            let ws = &mut before.workspace;
            assert!(ws.open_example("4-channel-wdm-optical-link"));
            let id = ws.active.unwrap();
            ws.project.doc_mut(id).unwrap().panel_mut().layout.components.push(PlacedComponent { kind: DETECTOR, pos: egui::Pos2::new(5.0, 5.0) });
            ws.project.path = Some("/somewhere/links.shine".into());
            assert!(ws.has_unsaved_changes());

            // A crash: the last periodic save had no clean exit.
            let text = ws.session().encode();
            let mut after = ShineApp::default();
            after.load(None, Some(text));
            let ws2 = &after.workspace;
            assert_eq!(ws2.project.fingerprint(), before.workspace.project.fingerprint(), "same project");
            assert_eq!((ws2.tabs.clone(), ws2.active), (before.workspace.tabs.clone(), before.workspace.active));
            assert_eq!(ws2.file_name(), "links.shine");
            let view = |ws: &crate::panels::workspace::Workspace| ws.project.doc(ws.active.unwrap()).unwrap().panel().view;
            assert_eq!(view(ws2), view(&before.workspace), "the same view");
            assert!(ws2.has_unsaved_changes(), "still marked unsaved");
            assert!(ws2.status.as_ref().unwrap().0.contains("didn't close properly"));

            // A normal close says nothing.
            before.workspace.closing = true;
            let mut calm = ShineApp::default();
            calm.load(None, Some(before.workspace.session().encode()));
            assert!(calm.workspace.status.is_none());

            // A damaged session starts fresh and says why.
            let mut broken = ShineApp::default();
            broken.load(Some(Default::default()), Some("shine-session-1:AAAA".into()));
            assert_eq!(broken.workspace.project.documents.len(), 1);
            assert!(broken.workspace.status.as_ref().unwrap().0.contains("Couldn't restore"));
        }

        #[test]
        fn new_project_starts_over_after_asking() {
            let mut app = ShineApp::default();
            let ws = &mut app.workspace;
            assert!(ws.open_example("8-bit-photonic-adder"));
            assert!(ws.has_unsaved_changes());
            ws.new_project();
            assert!(ws.pending_open.is_some(), "asks first");
            ws.resolve_unsaved(crate::panels::workspace::UnsavedChoice::Discard);
            assert_eq!(ws.project.documents.len(), 1);
            assert!(ws.project.documents[0].layout().components.is_empty());
            assert!(!ws.has_unsaved_changes());
            assert_eq!(ws.file_name(), "Unsaved project");
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
