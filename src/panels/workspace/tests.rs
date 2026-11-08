//! Workspace tests: documents, tabs, components and the full UI.

use egui::{Event, Modifiers, Pos2, Rect, Vec2};

use super::*;
use crate::panels::optics::OpticComponentKind;
use crate::panels::schematic::model::BlockRef;

struct Harness {
    ctx: egui::Context,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
        }
    }

    fn frame(&mut self, ws: &mut Workspace, events: Vec<Event>) {
        self.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.ctx
            .run_ui(input, |ui| ws.ui(ui))
            .textures_delta
            .clear();
    }

    fn key_with(&mut self, ws: &mut Workspace, key: Key, modifiers: Modifiers) {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(ws, vec![Event::ModifiersChanged(modifiers), event(true)]);
        self.frame(
            ws,
            vec![event(false), Event::ModifiersChanged(Modifiers::NONE)],
        );
    }

    fn key(&mut self, ws: &mut Workspace, key: Key) {
        self.key_with(ws, key, Modifiers::NONE);
    }
}

fn schematic(ws: &mut Workspace, id: DocId) -> &mut crate::panels::schematic::SchematicPanel {
    match &mut ws.project.doc_mut(id).unwrap().content {
        Content::Schematic(p) => p,
        Content::Layout(_) => panic!("not a schematic"),
    }
}

fn layout(ws: &mut Workspace, id: DocId) -> &mut crate::panels::optics::OpticsPanel {
    match &mut ws.project.doc_mut(id).unwrap().content {
        Content::Layout(p) => p,
        Content::Schematic(_) => panic!("not a layout"),
    }
}

#[test]
fn starts_with_one_layout_and_manages_tabs() {
    let mut ws = Workspace::default();
    assert_eq!(ws.tabs.len(), 1);
    let first = ws.active.unwrap();
    let second = ws.new_schematic();
    let third = ws.new_layout();
    assert_eq!(ws.tabs, vec![first, second, third]);
    assert_eq!(ws.active, Some(third));
    assert_eq!(ws.project.doc(third).unwrap().name, "Layout 2");

    ws.open_tab(first);
    assert_eq!(ws.active, Some(first));
    ws.open_tab(first);
    assert_eq!(ws.tabs.len(), 3, "re-opening does not duplicate the tab");
    ws.open_tab(DocId(999));
    assert_eq!(ws.active, Some(first));

    ws.close_tab(first);
    assert_eq!(ws.active, Some(second), "the neighbour becomes active");
    ws.close_tab(third);
    assert_eq!(ws.active, Some(second));
    ws.close_tab(second);
    assert_eq!(ws.active, None);
    assert_eq!(
        ws.project.documents.len(),
        3,
        "closing a tab keeps the document"
    );
    ws.close_tab(DocId(999));

    // With nothing open, the empty state renders and Ctrl+N still works.
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    h.key_with(&mut ws, Key::N, Modifiers::COMMAND);
    assert!(ws.active.is_some());
}

#[test]
fn examples_open_and_simulate() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    for i in 0..layout_examples().len() {
        ws.load_layout_example(i);
        h.frame(&mut ws, Vec::new());
        let id = ws.active.unwrap();
        assert!(layout(&mut ws, id).sim_cache.is_some());
    }
    for i in 0..schematic_examples().len() {
        ws.load_schematic_example(i);
        h.frame(&mut ws, Vec::new());
        let id = ws.active.unwrap();
        let sim = schematic(&mut ws, id).sim.clone().expect("simulated");
        assert!(sim.error.is_none(), "{:?}", sim.error);
        assert!(!sim.readings.is_empty());
    }
    ws.load_layout_example(99);
    ws.load_schematic_example(99);
    assert!(ws.status.as_ref().unwrap().0.contains("Optical DFT"));
}

#[test]
fn reused_component_opens_with_tab_and_edits_propagate() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    ws.load_schematic_example(2);
    let top = ws.active.unwrap();
    h.frame(&mut ws, Vec::new());
    let before = schematic(&mut ws, top).sim.clone().unwrap().readings;

    // Select the first component instance and press Tab.
    let component = match schematic(&mut ws, top).schematic.instances[1].block {
        BlockRef::Component(id) => id,
        _ => panic!("expected a component"),
    };
    schematic(&mut ws, top).selection.select_only(1);
    h.key(&mut ws, Key::Tab);
    assert_eq!(ws.active, Some(component), "Tab opened the definition");
    assert!(ws.project.doc(component).unwrap().is_layout());

    // Change the phase shifter inside the component...
    for c in &mut layout(&mut ws, component).layout.components {
        if let OpticComponentKind::PhaseShifter { phase } = &mut c.kind {
            *phase = std::f32::consts::PI;
        }
    }
    // ...and both instances in the schematic follow.
    ws.open_tab(top);
    h.frame(&mut ws, Vec::new());
    let after = schematic(&mut ws, top).sim.clone().unwrap().readings;
    assert_ne!(before, after);
    let total: f64 = after.iter().map(|r| r.1).sum();
    assert!((total - 1.0).abs() < 1e-9);
}

#[test]
fn saving_a_document_as_a_component() {
    let mut ws = Workspace::default();
    let id = ws.active.unwrap();
    ws.open_document_dialog(id, true);
    let d = ws.dialog.as_mut().unwrap();
    assert!(d.component && !d.initials_edited);
    d.name = "  Beam router ".into();
    d.initials = ui_kit::initials("Beam router");
    d.description = " Routes beams. ".into();
    ws.apply_document_dialog();
    let doc = ws.project.doc(id).unwrap();
    assert_eq!(doc.name, "Beam router");
    assert_eq!(
        doc.component,
        Some(ComponentMeta {
            initials: "BR".into(),
            description: "Routes beams.".into()
        })
    );
    assert!(ws.status.as_ref().unwrap().0.contains("My components"));
    assert_eq!(ws.project.catalog().components().count(), 1);

    // Renaming to a taken name picks a free one; initials are capped at 4.
    let other = ws.new_schematic();
    ws.open_document_dialog(other, false);
    let d = ws.dialog.as_mut().unwrap();
    assert!(!d.component);
    d.name = "Beam router".into();
    ws.apply_document_dialog();
    assert_eq!(ws.project.doc(other).unwrap().name, "Beam router 2");
    assert!(
        ws.project.doc(other).unwrap().component.is_none(),
        "rename does not publish"
    );

    ws.open_document_dialog(id, true);
    ws.dialog.as_mut().unwrap().initials = "ABCDEFG".into();
    ws.dialog.as_mut().unwrap().name = "   ".into();
    ws.apply_document_dialog();
    assert_eq!(
        ws.project
            .doc(id)
            .unwrap()
            .component
            .as_ref()
            .unwrap()
            .initials,
        "ABCD"
    );
    assert_eq!(ws.project.doc(id).unwrap().name, "Untitled");

    ws.apply_document_dialog();
    ws.open_document_dialog(DocId(999), true);
    assert!(ws.dialog.is_none());
}

#[test]
fn unpublishing_and_deleting_respect_users() {
    let mut ws = Workspace::default();
    ws.load_schematic_example(2);
    let top = ws.active.unwrap();
    let component = ws.project.catalog().components().next().unwrap().id;

    ws.unpublish(component);
    assert!(ws.project.doc(component).unwrap().component.is_some());
    assert!(ws.status.as_ref().unwrap().0.contains("still used"));
    ws.delete_document(component);
    assert!(ws.project.doc(component).is_some());
    assert!(ws.status.as_ref().unwrap().0.contains("used in"));

    ws.delete_document(top);
    assert!(ws.project.doc(top).is_none() && !ws.tabs.contains(&top));
    ws.unpublish(component);
    assert!(ws.project.doc(component).unwrap().component.is_none());
    ws.delete_document(component);
    assert!(ws.project.doc(component).is_none());
}

#[test]
fn project_files_round_trip_and_layouts_import() {
    let dir = std::env::temp_dir().join(format!("shine-workspace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("project.shine");

    let mut ws = Workspace::default();
    ws.load_schematic_example(2);
    ws.save_project_to(&path).unwrap();
    assert_eq!(ws.project.path.as_deref(), Some(path.as_path()));
    assert_eq!(
        &std::fs::read(&path).unwrap()[..4],
        b"SHNE",
        "binary, not JSON"
    );

    let mut other = Workspace::default();
    other.open_path(&path).unwrap();
    assert_eq!(other.project.documents.len(), ws.project.documents.len());
    assert_eq!(other.tabs.len(), 1);
    assert_eq!(other.project.to_bytes(), ws.project.to_bytes());
    assert_eq!(other.project.path.as_deref(), Some(path.as_path()));

    // A JSON layout saved by an earlier version is imported as a new document.
    let layout_path = dir.join("my layout.json");
    std::fs::write(
        &layout_path,
        r#"{"version": 1, "cell_um": 2.0, "components": [{"kind": "Detector", "pos": {"x": 20.0, "y": 20.0}}], "lines": []}"#,
    )
    .unwrap();
    let count = other.project.documents.len();
    other.open_path(&layout_path).unwrap();
    assert_eq!(other.project.documents.len(), count + 1);
    let id = other.active.unwrap();
    assert_eq!(other.project.doc(id).unwrap().name, "my layout");
    assert_eq!(layout(&mut other, id).cell_um, 2.0);

    // Browser uploads arrive as bytes and open on the next frame.
    other
        .incoming
        .lock()
        .unwrap()
        .push(("upload".into(), ws.project.to_bytes()));
    other
        .incoming
        .lock()
        .unwrap()
        .push(("junk".into(), vec![0xff, 0x00]));
    Harness::new().frame(&mut other, Vec::new());
    assert_eq!(other.project.to_bytes(), ws.project.to_bytes());
    assert!(other.status.as_ref().unwrap().0.contains("Open failed"));

    assert!(other.open_path(&dir.join("missing.shine")).is_err());
    std::fs::write(dir.join("bad.json"), "{").unwrap();
    assert!(other.open_path(&dir.join("bad.json")).is_err());
    assert!(
        ws.save_project_to(&dir.join("no/such/dir/p.shine"))
            .is_err()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn examples_open_by_slug() {
    let mut ws = Workspace::default();
    assert!(ws.open_example("optical-dft"));
    assert_eq!(
        ws.project.doc(ws.active.unwrap()).unwrap().name,
        "Optical DFT"
    );
    assert!(ws.open_example("mach-zehnder-interferometer"));
    assert!(ws.project.doc(ws.active.unwrap()).unwrap().is_layout());
    assert!(!ws.open_example("no-such-example"));
}

#[test]
fn keyboard_routes_through_the_active_editor() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    h.key(&mut ws, Key::T);
    h.key(&mut ws, Key::N);
    assert!(!ws.show_library && !ws.show_inspector);
    h.key(&mut ws, Key::F1);
    assert!(ws.show_shortcuts);
    h.frame(&mut ws, Vec::new());

    // Ctrl+N makes a new document of the same kind.
    let schematic_id = ws.new_schematic();
    h.key_with(&mut ws, Key::N, Modifiers::COMMAND);
    let new = ws.active.unwrap();
    assert_ne!(new, schematic_id);
    assert!(!ws.project.doc(new).unwrap().is_layout());
    ws.open_tab(ws.tabs[0]);
    h.key_with(&mut ws, Key::N, Modifiers::COMMAND);
    assert!(ws.project.doc(ws.active.unwrap()).unwrap().is_layout());
}

#[test]
fn edit_menu_actions_reach_the_document() {
    let mut ws = Workspace::default();
    ws.load_schematic_example(0);
    let id = ws.active.unwrap();
    ws.apply(Action::SelectAll);
    assert_eq!(schematic(&mut ws, id).selection.len(), 6);
    ws.apply(Action::Delete);
    assert!(schematic(&mut ws, id).schematic.instances.is_empty());
    ws.apply(Action::Undo);
    assert_eq!(schematic(&mut ws, id).schematic.instances.len(), 6);

    ws.load_layout_example(0);
    let id = ws.active.unwrap();
    ws.apply(Action::SelectAll);
    ws.apply(Action::Delete);
    assert!(layout(&mut ws, id).layout.components.is_empty());

    ws.active = None;
    ws.apply(Action::Undo);
}

#[test]
fn whole_ui_renders_with_windows_and_dialogs() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    ws.load_schematic_example(2);
    ws.show_examples = true;
    ws.show_shortcuts = true;
    for tab in [
        ExamplesTab::Layouts,
        ExamplesTab::Schematics,
        ExamplesTab::Chips,
    ] {
        ws.examples_tab = tab;
        h.frame(&mut ws, Vec::new());
    }
    let component = ws.project.catalog().components().next().unwrap().id;
    ws.open_document_dialog(component, true);
    h.frame(&mut ws, Vec::new());
    assert!(ws.dialog.is_some(), "the dialog stays open until closed");
    // A component without ports warns in the dialog.
    let empty = ws.new_schematic();
    ws.open_document_dialog(empty, true);
    h.frame(&mut ws, Vec::new());
    ws.open_document_dialog(ws.tabs[0], true);
    h.frame(&mut ws, Vec::new());
    // Escape closes the modal.
    h.key(&mut ws, Key::Escape);
    h.frame(&mut ws, Vec::new());
    assert!(ws.dialog.is_none());
    ws.notify("hello");
    h.frame(&mut ws, Vec::new());
    for _ in 0..100 {
        h.frame(&mut ws, Vec::new());
    }
    assert!(ws.status.is_none(), "messages expire");
}

#[test]
fn simulation_is_cached_by_fingerprint() {
    let mut ws = Workspace::default();
    ws.load_schematic_example(0);
    let id = ws.active.unwrap();
    ws.refresh_simulation();
    let key = schematic(&mut ws, id).sim_key;
    assert!(key.is_some());
    schematic(&mut ws, id).sim = None;
    ws.refresh_simulation();
    assert!(
        schematic(&mut ws, id).sim.is_none(),
        "unchanged project: not recomputed"
    );
    schematic(&mut ws, id).schematic.wire_settings.n_eff = 3.0;
    ws.refresh_simulation();
    assert!(schematic(&mut ws, id).sim.is_some());
    schematic(&mut ws, id).simulation_active = false;
    schematic(&mut ws, id).sim = None;
    schematic(&mut ws, id).schematic.wire_settings.n_eff = 2.0;
    ws.refresh_simulation();
    assert!(schematic(&mut ws, id).sim.is_none(), "live simulation off");
}

#[test]
fn simulate_toggle_is_on_by_default_and_flips() {
    let mut ws = Workspace::default();
    assert_eq!(
        ws.simulation_flag().copied(),
        Some(true),
        "new layouts simulate"
    );
    *ws.simulation_flag().unwrap() = false;
    let id = ws.active.unwrap();
    assert!(!layout(&mut ws, id).simulation_active);
    ws.new_schematic();
    assert_eq!(
        ws.simulation_flag().copied(),
        Some(true),
        "new schematics simulate"
    );
    ws.active = None;
    assert!(ws.simulation_flag().is_none());

    // The toggle sits in the menu bar; clicking it flips the active document.
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    let button = egui::Pos2::new(1330.0, 11.0);
    h.frame(&mut ws, vec![Event::PointerMoved(button)]);
    h.frame(
        &mut ws,
        vec![Event::PointerButton {
            pos: button,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    h.frame(
        &mut ws,
        vec![Event::PointerButton {
            pos: button,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        ws.simulation_flag().copied(),
        Some(false),
        "clicking “Stop simulation” turned it off"
    );
}

#[test]
fn url_queries_open_examples_tabs_and_windows() {
    let mut ws = Workspace::default();
    ws.apply_url_query("?example=reusable-free-space-component&tab=free-space-mzi");
    let active = ws.project.doc(ws.active.unwrap()).unwrap();
    assert_eq!(
        active.name, "Free-space MZI",
        "the component's definition tab is active"
    );
    assert_eq!(ws.tabs.len(), 3);

    ws.apply_url_query("examples=chips&chip=wdm-demultiplexer");
    assert!(ws.show_examples);
    assert_eq!(ws.examples_tab, ExamplesTab::Chips);
    assert_eq!(
        ws.chips.examples[ws.chips.selected].name(),
        "WDM demultiplexer"
    );
    ws.apply_url_query("examples=schematics");
    assert_eq!(ws.examples_tab, ExamplesTab::Schematics);
    ws.apply_url_query("examples");
    assert_eq!(ws.examples_tab, ExamplesTab::Layouts);

    // Unknown values are ignored.
    let before = ws.tabs.len();
    ws.apply_url_query("example=nope&tab=nope&chip=nope&other=1&");
    assert_eq!(ws.tabs.len(), before);
}
