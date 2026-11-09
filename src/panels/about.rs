//! Help → About: the version, what's new in it, where to find help, and (on
//! desktop) a check for a newer release. Nothing goes over the network
//! unless asked.

use std::sync::{Arc, Mutex};

/// The changelog, built in, for "what's new".
const CHANGELOG: &str = include_str!("../../docs/changelog.md");
#[cfg(not(target_arch = "wasm32"))]
const LATEST_RELEASE: &str = "https://api.github.com/repos/ash-hashtag/shine/releases/latest";

/// The section of `changelog` for `version` (`## 0.1.1 …` up to the next
/// `## `), without its heading.
pub fn whats_new<'a>(changelog: &'a str, version: &str) -> Option<&'a str> {
    let heading = format!("## {version}");
    let start = changelog.find(&format!("\n{heading} ")).or_else(|| changelog.find(&format!("\n{heading}\n")))?;
    let body = &changelog[start + 1..];
    let body = &body[body.find('\n')? + 1..];
    let end = body.find("\n## ").unwrap_or(body.len());
    Some(body[..end].trim())
}

/// Whether release `tag` (`v0.2.0`, `0.1.3`) is newer than `current`.
pub fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let v = v.trim().trim_start_matches('v');
        let core = v.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
    };
    matches!((parse(tag), parse(current)), (Some(a), Some(b)) if a > b)
}

/// Where an update check stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateCheck {
    #[default]
    NotChecked,
    Checking,
    UpToDate,
    Available { version: String, url: String },
    Failed(String),
}

/// What a check found, from GitHub's latest-release answer.
pub fn read_release(json: &str, current: &str) -> UpdateCheck {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json);
    match parsed.ok().and_then(|v| Some((v.get("tag_name")?.as_str()?.to_owned(), v.get("html_url")?.as_str()?.to_owned()))) {
        Some((tag, url)) if is_newer(&tag, current) => UpdateCheck::Available { version: tag.trim_start_matches('v').to_owned(), url },
        Some(_) => UpdateCheck::UpToDate,
        None => UpdateCheck::Failed("unexpected answer from GitHub".into()),
    }
}

/// The About window's state.
#[derive(Clone, Debug, Default)]
pub struct About {
    pub open: bool,
    pub update: Arc<Mutex<UpdateCheck>>,
}

impl About {
    /// Ask GitHub for the latest release (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn check_for_updates(&self, ctx: &egui::Context) {
        *self.update.lock().expect("not poisoned") = UpdateCheck::Checking;
        let state = self.update.clone();
        let ctx = ctx.clone();
        let mut request = ehttp::Request::get(LATEST_RELEASE);
        request.headers.insert("User-Agent", format!("shine/{}", env!("CARGO_PKG_VERSION")));
        ehttp::fetch(request, move |result| {
            let found = match result {
                Ok(r) if r.ok => read_release(r.text().unwrap_or_default(), env!("CARGO_PKG_VERSION")),
                Ok(r) => UpdateCheck::Failed(format!("GitHub answered {} {}", r.status, r.status_text)),
                Err(e) => UpdateCheck::Failed(e),
            };
            *state.lock().expect("not poisoned") = found;
            ctx.request_repaint();
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        super::ui_kit::window(ctx, "About Shine").open(&mut open).default_width(460.0).show(ctx, |ui| {
            ui.heading(format!("Shine {}", env!("CARGO_PKG_VERSION")));
            ui.label(env!("CARGO_PKG_DESCRIPTION"));
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} licence",
                    if cfg!(target_arch = "wasm32") {
                        "Browser build".to_owned()
                    } else {
                        format!("Desktop build for {} {}", std::env::consts::OS, std::env::consts::ARCH)
                    },
                    env!("CARGO_PKG_LICENSE")
                ))
                .small()
                .weak(),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("Website", env!("CARGO_PKG_HOMEPAGE"));
                ui.hyperlink_to("Documentation", concat!(env!("CARGO_PKG_HOMEPAGE"), "/docs/"));
                ui.hyperlink_to("Source code", env!("CARGO_PKG_REPOSITORY"));
                ui.hyperlink_to("Report a problem", concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new"));
            });
            ui.add_space(6.0);
            self.update_ui(ui);
            if let Some(notes) = whats_new(CHANGELOG, env!("CARGO_PKG_VERSION")) {
                egui::CollapsingHeader::new(format!("What's new in {}", env!("CARGO_PKG_VERSION"))).default_open(true).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| notes_ui(ui, notes));
                });
            }
        });
        self.open = open;
    }

    fn update_ui(&mut self, ui: &mut egui::Ui) {
        if cfg!(target_arch = "wasm32") {
            ui.label(egui::RichText::new("The browser version is always the latest.").small().weak());
            return;
        }
        let state = self.update.lock().expect("not poisoned").clone();
        ui.horizontal(|ui| {
            #[cfg(not(target_arch = "wasm32"))]
            if ui.add_enabled(state != UpdateCheck::Checking, egui::Button::new("Check for updates")).on_hover_text("Asks GitHub for the latest release").clicked() {
                self.check_for_updates(ui.ctx());
            }
            match state {
                UpdateCheck::NotChecked => {}
                UpdateCheck::Checking => {
                    ui.spinner();
                }
                UpdateCheck::UpToDate => {
                    ui.label("You have the latest version.");
                }
                UpdateCheck::Available { version, url } => {
                    ui.hyperlink_to(format!("Shine {version} is available: download"), url);
                }
                UpdateCheck::Failed(e) => {
                    ui.label(egui::RichText::new(format!("Couldn't check: {e}")).small().color(ui.visuals().warn_fg_color));
                }
            }
        });
    }
}

/// Changelog Markdown, simply: `###` headings, bullets (with continuation
/// lines joined), `**bold**` and backticks dropped.
fn notes_ui(ui: &mut egui::Ui, notes: &str) {
    let plain = |s: &str| s.replace("**", "").replace('`', "");
    let mut item = String::new();
    let flush = |ui: &mut egui::Ui, item: &mut String| {
        if !item.is_empty() {
            ui.label(format!("• {}", plain(item)));
            item.clear();
        }
    };
    for line in notes.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("### ") {
            flush(ui, &mut item);
            ui.add_space(4.0);
            ui.strong(h);
        } else if let Some(rest) = t.strip_prefix("- ") {
            flush(ui, &mut item);
            item.push_str(rest);
        } else if t.is_empty() {
            flush(ui, &mut item);
        } else if !item.is_empty() {
            item.push(' ');
            item.push_str(t);
        } else {
            ui.label(plain(t));
        }
    }
    flush(ui, &mut item);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whats_new_is_this_versions_changelog_section() {
        let log = "# Changelog\n\n## 0.1.1 (unreleased)\n\n### Added\n\n- Sweeps\n\n## 0.1.0 (2026-11-09)\n\n- First\n";
        assert_eq!(whats_new(log, "0.1.1"), Some("### Added\n\n- Sweeps"));
        assert_eq!(whats_new(log, "0.1.0"), Some("- First"));
        assert_eq!(whats_new(log, "0.1"), None, "whole versions only");
        assert_eq!(whats_new(log, "9.9.9"), None);
        // The built-in changelog has a section for this version.
        assert!(whats_new(CHANGELOG, env!("CARGO_PKG_VERSION")).is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(!is_newer("v0.1.1", "0.1.1"));
        assert!(!is_newer("v0.1.0", "0.1.1"));
        assert!(!is_newer("v0.2.0-rc.1", "0.2.0"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }

    #[test]
    fn release_answers_are_read() {
        let newer = r#"{"tag_name": "v9.0.0", "html_url": "https://github.com/ash-hashtag/shine/releases/tag/v9.0.0"}"#;
        assert_eq!(
            read_release(newer, "0.1.1"),
            UpdateCheck::Available { version: "9.0.0".into(), url: "https://github.com/ash-hashtag/shine/releases/tag/v9.0.0".into() }
        );
        assert_eq!(read_release(r#"{"tag_name": "v0.1.0", "html_url": "x"}"#, "0.1.1"), UpdateCheck::UpToDate);
        assert!(matches!(read_release("<html>", "0.1.1"), UpdateCheck::Failed(_)));
    }

    #[test]
    fn the_window_draws_every_state() {
        let ctx = egui::Context::default();
        let mut about = About { open: true, ..Default::default() };
        for state in [
            UpdateCheck::NotChecked,
            UpdateCheck::Checking,
            UpdateCheck::UpToDate,
            UpdateCheck::Available { version: "9.0.0".into(), url: "https://example.com".into() },
            UpdateCheck::Failed("offline".into()),
        ] {
            *about.update.lock().unwrap() = state;
            ctx.run_ui(Default::default(), |ui| about.ui(ui.ctx())).textures_delta.clear();
        }
        assert!(about.open);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod network {
    use super::*;

    /// Asks the real GitHub: run with `cargo test -- --ignored update_check`.
    #[test]
    #[ignore = "needs the network"]
    fn update_check_against_github() {
        let about = About::default();
        about.check_for_updates(&egui::Context::default());
        for _ in 0..100 {
            let state = about.update.lock().unwrap().clone();
            if !matches!(state, UpdateCheck::Checking) {
                assert!(matches!(state, UpdateCheck::UpToDate | UpdateCheck::Available { .. }), "{state:?}");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("no answer in 10 s");
    }
}
