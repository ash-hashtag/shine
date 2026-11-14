pub mod photonics;

/// Shine as a library: designs, simulation, analysis and files, without the app.
#[cfg(feature = "core")]
pub mod api;

#[cfg(all(feature = "logging", not(target_arch = "wasm32")))]
pub mod logging;

#[cfg(all(feature = "gui", target_arch = "wasm32"))]
pub mod web;

#[cfg(all(feature = "gui", not(target_arch = "wasm32")))]
pub mod crash;

#[cfg(all(feature = "gui", not(target_arch = "wasm32")))]
pub mod cli;

#[cfg(feature = "gui")]
pub mod mcp;

#[cfg(feature = "core")]
pub mod panels;

#[cfg(feature = "gui")]
pub use app::ShineApp;

/// The app's name for the OS: its settings folder and window class.
pub const APP_ID: &str = "Shine";

#[cfg(feature = "gui")]
mod app {
    use crate::panels::workspace::Workspace;

    /// The most the session's project may take in the app's storage: the
    /// browser's is a few MB a site; the desktop's is a file, so no limit.
    fn too_big_to_keep(encoded: usize) -> bool {
        cfg!(target_arch = "wasm32") && encoded > 4_000_000
    }

    #[derive(Default)]
    pub struct ShineApp {
        pub workspace: Workspace,
        /// The fingerprint of the project last written to storage, and the
        /// rest of the session as last written: saving skips what's the same.
        saved_project: Option<u64>,
        saved_session: Option<String>,
        /// The project as last saved was too big for the storage, so no
        /// session is kept until it changes.
        too_big: bool,
    }

    impl ShineApp {
        /// The app with the preferences and the session saved earlier.
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let mut app = Self::default();
            // Known from the start, so work begun before the first frame (a
            // link's download) can wake the app when it's done.
            app.workspace.set_context(cc.egui_ctx.clone());
            if let Some(storage) = cc.storage {
                app.load_from(storage);
            }
            app.load_library(cc.storage);
            app
        }

        /// The preferences and the session in `storage`.
        pub fn load_from(&mut self, storage: &dyn eframe::Storage) {
            use crate::panels::session;
            self.load(
                eframe::get_value(storage, crate::panels::prefs::STORAGE_KEY),
                storage.get_string(session::KEY),
                storage.get_string(session::PROJECT_KEY),
            );
        }

        /// The asset library: a file next to the settings on desktop, the
        /// page's storage in the browser.
        fn load_library(&mut self, storage: Option<&dyn eframe::Storage>) {
            use crate::panels::assets::{Library, STORAGE_KEY};
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(dir) = eframe::storage_dir(crate::APP_ID) {
                self.workspace.library = Library::open_file(dir.join(crate::panels::assets::LIBRARY_FILE));
                return;
            }
            let stored = storage.and_then(|s| s.get_string(STORAGE_KEY));
            if let Some(text) = stored {
                match crate::panels::session::base64_decode(&text).and_then(|b| Library::from_bytes(&b)) {
                    Ok(library) => self.workspace.library = library,
                    Err(e) => tracing::error!("Couldn't read the asset library: {e}"),
                }
            }
        }

        /// Preferences and the last session, as kept in the app's storage:
        /// the session, and its project kept apart (older sessions hold it).
        pub fn load(&mut self, prefs: Option<crate::panels::prefs::Preferences>, session: Option<String>, project: Option<String>) {
            use crate::panels::session;
            if let Some(prefs) = prefs {
                self.workspace.set_preferences(prefs);
            }
            if let Some(text) = session {
                let restored = session::Session::decode(&text).and_then(|mut s| {
                    if s.project.is_empty() {
                        s.project = session::base64_decode(project.as_deref().ok_or("its project is missing")?)?;
                    }
                    self.workspace.restore(s)
                });
                match restored {
                    Ok(()) => {
                        // What's in storage now: no need to write it again.
                        self.saved_project = project.is_some().then(|| self.workspace.project.fingerprint());
                        tracing::info!("Restored the last session");
                    }
                    Err(e) => {
                        tracing::warn!("Couldn't restore the last session: {e}");
                        self.workspace.notify(format!("Couldn't restore the last session: {e}"));
                    }
                }
            }
        }
    }

    impl eframe::App for ShineApp {
        /// Every few seconds and on closing. The project is encoded and
        /// written only when it changed (a design of millions of parts is
        /// tens of MB); the rest of the session, a few bytes, when it did.
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            use crate::panels::session;
            eframe::set_value(storage, crate::panels::prefs::STORAGE_KEY, &self.workspace.prefs);
            let fingerprint = self.workspace.project.fingerprint();
            if self.saved_project != Some(fingerprint) {
                let encoded = session::base64_encode(&self.workspace.project.to_bytes());
                self.saved_project = Some(fingerprint);
                // The browser keeps a few MB per site: a project bigger than
                // that (a chip of millions of parts) isn't kept, and neither
                // is the rest of the session, so nothing stale comes back.
                self.too_big = too_big_to_keep(encoded.len());
                if self.too_big {
                    tracing::info!("The project ({:.1} MB) is too big for the browser's storage; it isn't kept between visits", encoded.len() as f64 / 1e6);
                    storage.remove_string(session::KEY);
                    storage.remove_string(session::PROJECT_KEY);
                    self.saved_session = None;
                    return;
                }
                storage.set_string(session::PROJECT_KEY, encoded);
            } else if self.too_big {
                return;
            }
            let text = self.workspace.session_without_project().encode();
            if self.saved_session.as_deref() != Some(text.as_str()) {
                storage.set_string(session::KEY, text.clone());
                self.saved_session = Some(text);
            }
            // Kept in a file of its own on desktop; here only when it changed.
            if self.workspace.library.path.is_none()
                && let Some(bytes) = self.workspace.library.take_changes()
            {
                storage.set_string(crate::panels::assets::STORAGE_KEY, crate::panels::session::base64_encode(&bytes));
            }
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
            after.load(None, Some(text), None);
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
            calm.load(None, Some(before.workspace.session().encode()), None);
            assert!(calm.workspace.status.is_none());

            // A damaged session starts fresh and says why.
            let mut broken = ShineApp::default();
            broken.load(Some(Default::default()), Some("shine-session-1:AAAA".into()), None);
            assert_eq!(broken.workspace.project.documents.len(), 1);
            assert!(broken.workspace.status.as_ref().unwrap().0.contains("Couldn't restore"));
        }

        /// eframe's storage, in memory, counting writes.
        #[derive(Default)]
        struct Memory {
            kv: std::collections::HashMap<String, String>,
            writes: std::collections::HashMap<String, usize>,
        }

        impl eframe::Storage for Memory {
            fn get_string(&self, key: &str) -> Option<String> {
                self.kv.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                *self.writes.entry(key.to_owned()).or_default() += 1;
                self.kv.insert(key.to_owned(), value);
            }
            fn remove_string(&mut self, key: &str) {
                self.kv.remove(key);
            }
            fn flush(&mut self) {}
        }

        #[test]
        fn saving_writes_the_project_only_when_it_changes() {
            use crate::panels::session::{KEY, PROJECT_KEY};
            use eframe::App;
            let mut app = ShineApp::default();
            assert!(app.workspace.open_example("4-channel-wdm-optical-link"));
            let mut storage = Memory::default();
            app.save(&mut storage);
            app.save(&mut storage);
            assert_eq!((storage.writes[PROJECT_KEY], storage.writes[KEY]), (1, 1), "nothing changed: nothing written again");
            // Panning changes the session, not the project.
            let id = app.workspace.active.unwrap();
            app.workspace.project.doc_mut(id).unwrap().panel_mut_untracked().view.pan.x += 40.0;
            app.save(&mut storage);
            assert_eq!((storage.writes[PROJECT_KEY], storage.writes[KEY]), (1, 2));
            // An edit writes the project.
            app.workspace.project.doc_mut(id).unwrap().layout_mut().components.clear();
            app.save(&mut storage);
            assert_eq!(storage.writes[PROJECT_KEY], 2);

            // It comes back, and isn't written again until it changes.
            let mut after = ShineApp::default();
            after.load_from(&storage);
            assert_eq!(after.workspace.project.fingerprint(), app.workspace.project.fingerprint());
            assert_eq!(after.workspace.project.doc(id).unwrap().panel().view.pan.x, app.workspace.project.doc(id).unwrap().panel().view.pan.x);
            after.save(&mut storage);
            assert_eq!(storage.writes[PROJECT_KEY], 2, "restored as stored");
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
