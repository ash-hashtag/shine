//! Workspace tests: documents, tabs, components and the full UI.

use crate::panels::assets::AssetRef;
use egui::{Event, Modifiers, Pos2, Rect, Vec2};

use super::*;
use crate::panels::optics::OpticComponentKind;
use crate::panels::optics::model::{BlockRef, Builtin, Item, Terminal};
use crate::panels::optics::{Hit, OpticsPanel, Placeable};

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

fn panel(ws: &mut Workspace, id: DocId) -> &mut OpticsPanel {
    ws.project.doc_mut(id).unwrap().panel_mut()
}

fn demo(ws: &mut Workspace, name: &str) -> DocId {
    let i = demos().iter().position(|d| d.name == name).expect("demo exists");
    ws.load_demo(i);
    ws.active.unwrap()
}

const ADDER: &str = "8-bit photonic adder";
const ACCUMULATOR: &str = "Photonic accumulator";
const CASCADE: &str = "Cascaded interferometers";

/// The value held by a document's first register.
fn register_value(ws: &mut Workspace, id: DocId) -> u64 {
    let p = panel(ws, id);
    let o = p.sim.as_ref().expect("simulated").outcome.clone();
    assert!(o.error.is_none(), "{:?}", o.error);
    let inst = p
        .layout
        .instances
        .iter()
        .find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Register { .. })))
        .unwrap();
    crate::panels::optics::output_number(&o, inst.id, 8)
}

/// The value on a document's first display.
fn display_value(ws: &mut Workspace, id: DocId) -> u64 {
    let p = panel(ws, id);
    let o = p.sim.as_ref().expect("simulated").outcome.clone();
    assert!(o.error.is_none(), "{:?}", o.error);
    let inst = p
        .layout
        .instances
        .iter()
        .find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { .. })))
        .unwrap();
    let BlockRef::Builtin(Builtin::Display { bits }) = inst.block else { unreachable!() };
    crate::panels::optics::input_number(&p.layout, &o, inst.id, bits as usize)
}

#[test]
fn starts_with_one_layout_and_manages_tabs() {
    let mut ws = Workspace::default();
    assert_eq!(ws.tabs.len(), 1);
    let first = ws.active.unwrap();
    let second = ws.new_component();
    let third = ws.new_layout();
    assert_eq!(ws.tabs, vec![first, second, third]);
    assert_eq!(ws.active, Some(third));
    assert_eq!(ws.project.doc(third).unwrap().name, "Layout 2");
    assert!(ws.dialog.is_some(), "a new component asks for its details");
    ws.dialog = None;

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
    assert_eq!(ws.project.documents.len(), 3, "closing a tab keeps the document");
    ws.close_tab(DocId(999));

    // With nothing open, the empty state renders and Ctrl+N still works.
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    h.key_with(&mut ws, Key::N, Modifiers::COMMAND);
    assert!(ws.active.is_some());
}

#[test]
fn new_components_have_ports_and_are_placeable() {
    let mut ws = Workspace::default();
    let top = ws.active.unwrap();
    let c = ws.new_component();
    ws.dialog = None;
    let iface = ws.project.doc(c).unwrap().interface();
    assert_eq!(iface.inputs, vec!["in"]);
    assert_eq!(iface.outputs, vec!["out"]);
    assert_eq!(iface.signal_inputs, vec!["s"]);
    assert_eq!(iface.signal_outputs, vec!["q"]);
    assert!(ws.project.catalog().can_place(c, top));
    assert!(!ws.project.catalog().can_place(c, c));
}

#[test]
fn examples_open_and_simulate() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    for i in 0..layout_examples().len() {
        ws.load_layout_example(i);
        h.frame(&mut ws, Vec::new());
        let id = ws.active.unwrap();
        let sim = panel(&mut ws, id).sim.clone().expect("simulated");
        assert!(sim.outcome.error.is_none(), "{:?}", sim.outcome.error);
    }
    for i in 0..demos().len() {
        ws.load_demo(i);
        h.frame(&mut ws, Vec::new());
        let id = ws.active.unwrap();
        let sim = panel(&mut ws, id).sim.clone().expect("simulated");
        assert!(sim.outcome.error.is_none(), "{}: {:?}", demos()[i].name, sim.outcome.error);
    }
    ws.load_layout_example(99);
    ws.load_demo(99);
    assert!(ws.status.as_ref().unwrap().0.contains(CASCADE));
}

#[test]
fn the_adder_adds_and_bits_toggle_with_a_click() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let id = demo(&mut ws, ADDER);
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    assert_eq!(display_value(&mut ws, id), 155, "100 + 55");

    // Click the least significant bit of A (100 → 101).
    let p = panel(&mut ws, id);
    let a = p.layout.instances[0].cell;
    let world = model::cell_center(a.0 + 7, a.1);
    let canvas = p.canvas_rect().unwrap();
    let at = p.view.to_screen(canvas, world);
    assert_eq!(p.hit(world), Hit::Box(0));
    for pressed in [true, false] {
        h.frame(&mut ws, vec![Event::PointerMoved(at)]);
        h.frame(
            &mut ws,
            vec![Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
        );
    }
    h.frame(&mut ws, Vec::new());
    assert_eq!(panel(&mut ws, id).layout.instances[0].block, BlockRef::Builtin(Builtin::NumberInput { bits: 8, value: 101 }));
    assert_eq!(display_value(&mut ws, id), 156);
}

#[test]
fn the_accumulator_counts_with_the_clock() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let id = demo(&mut ws, ACCUMULATOR);
    h.frame(&mut ws, Vec::new());
    assert_eq!(register_value(&mut ws, id), 0);
    ws.tick();
    h.frame(&mut ws, Vec::new());
    assert_eq!(register_value(&mut ws, id), 3);
    // The panel's Step goes through the workspace.
    panel(&mut ws, id).tick();
    h.frame(&mut ws, Vec::new());
    assert_eq!(panel(&mut ws, id).machine.tick, 2);
    assert_eq!(register_value(&mut ws, id), 6);
    // Running ticks on its own.
    panel(&mut ws, id).running = true;
    for _ in 0..12 {
        h.frame(&mut ws, Vec::new());
    }
    assert!(panel(&mut ws, id).machine.tick >= 3);
    panel(&mut ws, id).reset_clock();
    h.frame(&mut ws, Vec::new());
    assert_eq!(register_value(&mut ws, id), 0);
}

#[test]
fn reused_component_opens_with_tab_and_edits_propagate() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let top = demo(&mut ws, CASCADE);
    h.frame(&mut ws, Vec::new());
    let readings = |ws: &mut Workspace| panel(ws, top).sim.clone().unwrap().outcome.detector_power;
    let before = readings(&mut ws);

    // Select the first component box and press Tab.
    let component = match panel(&mut ws, top).layout.instances[0].block {
        BlockRef::Component(id) => id,
        _ => panic!("expected a component"),
    };
    panel(&mut ws, top).editor.boxes.select_only(0);
    h.key(&mut ws, Key::Tab);
    assert_eq!(ws.active, Some(component), "Tab opened the definition");

    // Change the phase shifter inside the component...
    for c in &mut panel(&mut ws, component).layout.components {
        if let OpticComponentKind::PhaseShifter { phase } = &mut c.kind {
            *phase = std::f32::consts::PI;
        }
    }
    // ...and both instances follow.
    ws.open_tab(top);
    h.frame(&mut ws, Vec::new());
    let after = readings(&mut ws);
    assert_ne!(before, after);
    let total: f64 = after.iter().map(|r| r.1).fold(0.0, |a, b| a + b);
    assert!((total - 1.0).abs() < 1e-9, "{after:?}");
}

#[test]
fn example_components_are_placed_from_the_library() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let id = ws.active.unwrap();
    h.frame(&mut ws, Vec::new());
    panel(&mut ws, id).place(Placeable::Asset(AssetRef::Example(0)), model::cell_center(2, 2));
    h.frame(&mut ws, Vec::new());
    panel(&mut ws, id).place(Placeable::Asset(AssetRef::Example(0)), model::cell_center(2, 8));
    h.frame(&mut ws, Vec::new());
    let fa = ws.project.find("Full adder").expect("imported once");
    assert_eq!(ws.project.documents.len(), 2);
    let p = panel(&mut ws, id);
    assert_eq!(p.layout.instances.len(), 2);
    assert!(p.layout.instances.iter().all(|i| i.block == BlockRef::Component(fa)));
    assert_eq!(p.layout.instances[0].cell, (2, 2));

    // Wire a number input into the adder by dragging between terminals.
    let n = p.layout.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 1, value: 1 }), (2, 0));
    let from = p.terminal_pos(Terminal { item: Item::Box(n), port: 0 }, true).unwrap();
    let to = p.terminal_pos(Terminal { item: Item::Box(p.layout.instances[0].id), port: 0 }, false).unwrap();
    let canvas = p.canvas_rect().unwrap();
    let (from, to) = (p.view.to_screen(canvas, from), p.view.to_screen(canvas, to));
    let press = |pressed, pos| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.frame(&mut ws, vec![Event::PointerMoved(from)]);
    h.frame(&mut ws, vec![press(true, from)]);
    for k in 1..=6 {
        h.frame(&mut ws, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
    }
    h.frame(&mut ws, vec![press(false, to)]);
    h.frame(&mut ws, Vec::new());
    let p = panel(&mut ws, id);
    assert_eq!(p.layout.signals.len(), 1, "connected by dragging");
    let o = p.sim.as_ref().unwrap().outcome.clone();
    assert_eq!(o.signal(Item::Box(p.layout.instances[0].id), 0), 1.0, "sum = 1");

    // A component can't be placed inside itself.
    ws.open_tab(fa);
    h.frame(&mut ws, Vec::new());
    panel(&mut ws, fa).place(Placeable::Block(BlockRef::Component(fa)), model::cell_center(20, 20));
    assert!(panel(&mut ws, fa).layout.instances.is_empty());
    ws.place_example(0, (30, 30));
    assert!(ws.status.as_ref().unwrap().0.contains("itself"));
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
            description: "Routes beams.".into(),
            appearance: Default::default(),
        })
    );
    assert!(ws.status.as_ref().unwrap().0.contains("This project"));
    assert_eq!(ws.project.catalog().components().count(), 1);

    // Renaming to a taken name picks a free one; initials are capped at 4.
    let other = ws.new_layout();
    ws.open_document_dialog(other, false);
    let d = ws.dialog.as_mut().unwrap();
    assert!(!d.component);
    d.name = "Beam router".into();
    ws.apply_document_dialog();
    assert_eq!(ws.project.doc(other).unwrap().name, "Beam router 2");
    assert!(ws.project.doc(other).unwrap().component.is_none(), "rename does not publish");

    ws.open_document_dialog(id, true);
    ws.dialog.as_mut().unwrap().initials = "ABCDEFG".into();
    ws.dialog.as_mut().unwrap().name = "   ".into();
    ws.apply_document_dialog();
    assert_eq!(ws.project.doc(id).unwrap().component.as_ref().unwrap().initials, "ABCD");
    assert_eq!(ws.project.doc(id).unwrap().name, "Untitled");

    ws.apply_document_dialog();
    ws.open_document_dialog(DocId(999), true);
    assert!(ws.dialog.is_none());
}

#[test]
fn unpublishing_and_deleting_respect_users() {
    let mut ws = Workspace::default();
    let top = demo(&mut ws, CASCADE);
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
    demo(&mut ws, ADDER);
    ws.save_project_to(&path).unwrap();
    assert_eq!(ws.project.path.as_deref(), Some(path.as_path()));
    assert_eq!(&std::fs::read(&path).unwrap()[..4], b"SHNE", "binary, not JSON");

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
    assert_eq!(panel(&mut other, id).cell_um, 2.0);

    // Browser uploads arrive as bytes and open on the next frame.
    other.incoming.lock().unwrap().push(IncomingFile::new(FilePurpose::Open, "upload.shine", ws.project.to_bytes()));
    other.incoming.lock().unwrap().push(IncomingFile::new(FilePurpose::Open, "junk", vec![0xff, 0x00]));
    Harness::new().frame(&mut other, Vec::new());
    assert!(other.status.as_ref().unwrap().0.contains("Open failed"));
    // The imported layout is unsaved, so the uploaded project waits for an answer.
    assert!(other.pending_open.is_some());
    other.resolve_unsaved(UnsavedChoice::Discard);
    assert_eq!(other.project.to_bytes(), ws.project.to_bytes());

    assert!(other.open_path(&dir.join("missing.shine")).is_err());
    std::fs::write(dir.join("bad.json"), "{").unwrap();
    assert!(other.open_path(&dir.join("bad.json")).is_err());
    assert!(ws.save_project_to(&dir.join("no/such/dir/p.shine")).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn examples_open_by_slug() {
    let mut ws = Workspace::default();
    assert!(ws.open_example("8-bit-photonic-adder"));
    assert_eq!(ws.project.doc(ws.active.unwrap()).unwrap().name, ADDER);
    assert!(ws.open_example("mach-zehnder-interferometer"));
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

    let before = ws.project.documents.len();
    h.key_with(&mut ws, Key::N, Modifiers::COMMAND);
    assert_eq!(ws.project.documents.len(), before + 1);
}

#[test]
fn edit_menu_actions_reach_the_document() {
    let mut ws = Workspace::default();
    let id = demo(&mut ws, ADDER);
    ws.apply(Action::SelectAll);
    assert_eq!(panel(&mut ws, id).editor.boxes.len(), 11);
    ws.apply(Action::Delete);
    assert!(panel(&mut ws, id).layout.instances.is_empty());
    assert!(panel(&mut ws, id).layout.signals.is_empty(), "wires go with their boxes");
    ws.apply(Action::Undo);
    assert_eq!(panel(&mut ws, id).layout.instances.len(), 11);

    ws.load_layout_example(0);
    let id = ws.active.unwrap();
    ws.apply(Action::SelectAll);
    ws.apply(Action::Delete);
    assert!(panel(&mut ws, id).layout.components.is_empty());

    ws.active = None;
    ws.apply(Action::Undo);
}

#[test]
fn whole_ui_renders_with_windows_and_dialogs() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let adder = demo(&mut ws, ADDER);
    ws.show_examples = true;
    ws.show_shortcuts = true;
    for tab in [ExamplesTab::Demos, ExamplesTab::Layouts, ExamplesTab::Chips] {
        ws.examples_tab = tab;
        h.frame(&mut ws, Vec::new());
    }
    // Inspect every kind of selection.
    let n = panel(&mut ws, adder).layout.instances.len();
    for b in 0..n {
        panel(&mut ws, adder).editor.clear_selection();
        panel(&mut ws, adder).editor.boxes.select_only(b);
        h.frame(&mut ws, Vec::new());
    }
    panel(&mut ws, adder).editor.clear_selection();
    panel(&mut ws, adder).selected_wire = Some(crate::panels::optics::WireRef::Signal(0));
    h.frame(&mut ws, Vec::new());
    let fa = ws.project.find("Full adder").unwrap();
    ws.open_tab(fa);
    let parts = panel(&mut ws, fa).layout.components.len();
    for i in 0..parts {
        panel(&mut ws, fa).editor.clear_selection();
        panel(&mut ws, fa).editor.selection.select_only(i);
        h.frame(&mut ws, Vec::new());
    }

    ws.open_document_dialog(fa, true);
    h.frame(&mut ws, Vec::new());
    assert!(ws.dialog.is_some(), "the dialog stays open until closed");
    // A component without ports warns in the dialog.
    let empty = ws.new_layout();
    ws.open_document_dialog(empty, true);
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
fn simulation_is_cached_by_fingerprint_and_registers() {
    let mut ws = Workspace::default();
    let id = demo(&mut ws, ACCUMULATOR);
    assert!(ws.refresh_simulation());
    assert!(!ws.refresh_simulation(), "unchanged: not recomputed");
    panel(&mut ws, id).sim.as_mut().unwrap().outcome.iterations = 999;
    assert!(!ws.refresh_simulation());
    assert_eq!(panel(&mut ws, id).sim.as_ref().unwrap().outcome.iterations, 999);
    // A component used inside changes the result.
    let fa = ws.project.find("Full adder").unwrap();
    panel(&mut ws, fa).cell_um = 11.0;
    assert!(ws.refresh_simulation());
    // So does the clock.
    ws.tick();
    assert!(ws.refresh_simulation(), "the registers changed");
    panel(&mut ws, id).machine.tick += 1;
    assert!(ws.refresh_simulation());
    panel(&mut ws, id).simulation_active = false;
    panel(&mut ws, id).machine.tick += 1;
    assert!(!ws.refresh_simulation(), "live simulation off");
}

#[test]
fn the_canvas_zoom_is_remembered_and_designs_open_at_it() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    ws.load_demo(0);
    h.frame(&mut ws, Vec::new());
    // Zoom in on the canvas: the preference follows.
    let middle = Pos2::new(700.0, 450.0);
    h.frame(&mut ws, vec![Event::PointerMoved(middle), Event::Zoom(1.5)]);
    let zoom = ws.active_panel().unwrap().view.zoom;
    assert_eq!(ws.prefs.canvas_zoom, Some(zoom));
    // The next design opens at it, not fitted.
    ws.load_demo(1);
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    assert_eq!(ws.active_panel().unwrap().view.zoom, zoom);
    // Unless the preference is to fit.
    ws.prefs.remember_canvas_zoom = false;
    ws.load_demo(2);
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    assert_ne!(ws.active_panel().unwrap().view.zoom, zoom, "fitted");
    // Zooming the canvas leaves the interface size alone.
    assert_eq!(h.ctx.zoom_factor(), 1.0);
}

#[test]
fn the_interface_size_follows_the_preference_and_the_shortcuts() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    ws.prefs.ui_scale = 1.5;
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    assert_eq!(h.ctx.zoom_factor(), 1.5);
    assert_eq!(ws.prefs.ui_scale, 1.5, "kept while the window catches up");
    // Ctrl+= changes the window's size; the preference keeps it.
    h.ctx.set_zoom_factor(1.2);
    h.frame(&mut ws, Vec::new());
    assert!((ws.prefs.ui_scale - 1.2).abs() < 1e-6);
    let canvas_zoom = ws.active_panel().unwrap().view.zoom;
    assert_eq!(ws.prefs.canvas_zoom, None, "the canvas zoom is separate: {canvas_zoom}");
}

#[test]
fn slow_designs_simulate_in_the_background() {
    let mut ws = Workspace::default();
    let id = demo(&mut ws, "16×16 photonic matrix processor");
    assert!(ws.refresh_simulation(), "the first run is on the spot");
    let power = |ws: &mut Workspace| panel(ws, id).sim.as_ref().unwrap().outcome.port_power.values().sum::<f64>();
    let before = power(&mut ws);
    // Pretend it's slow: the next edit's simulation runs on another thread.
    ws.background_after_ms = 0.0;
    ws.stats.last_simulation_ms = 1.0;
    // Switch an input off.
    let bit = panel(&mut ws, id)
        .layout
        .instances
        .iter()
        .position(|i| matches!(i.block, BlockRef::Builtin(Builtin::Value { value, .. }) if value == 1.0))
        .unwrap();
    if let BlockRef::Builtin(Builtin::Value { value, .. }) = &mut panel(&mut ws, id).layout.instances[bit].block {
        *value = 0.0;
    }
    ws.refresh_simulation();
    if !cfg!(target_arch = "wasm32") {
        assert!(ws.simulating_in_background());
        assert_eq!(power(&mut ws), before, "the last result stays on screen meanwhile");
    }
    ws.finish_background();
    assert!(!ws.simulating_in_background());
    assert!(power(&mut ws) < before, "one input dark: less light out");
    assert!(!ws.refresh_simulation(), "and it's current");
}

#[test]
fn simulate_toggle_is_on_by_default_and_flips() {
    let mut ws = Workspace::default();
    assert_eq!(ws.simulation_flag().copied(), Some(true), "new layouts simulate");
    *ws.simulation_flag().unwrap() = false;
    let id = ws.active.unwrap();
    assert!(!panel(&mut ws, id).simulation_active);
    ws.active = None;
    assert!(ws.simulation_flag().is_none());

    // The toggle sits in the menu bar; clicking it flips the active document.
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    h.frame(&mut ws, Vec::new());
    let button = egui::Pos2::new(1330.0, 11.0);
    h.frame(&mut ws, vec![Event::PointerMoved(button)]);
    for pressed in [true, false] {
        h.frame(
            &mut ws,
            vec![Event::PointerButton { pos: button, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
        );
    }
    assert_eq!(ws.simulation_flag().copied(), Some(false), "clicking “Stop simulation” turned it off");
}

#[test]
fn url_queries_open_examples_tabs_and_windows() {
    let mut ws = Workspace::default();
    ws.apply_url_query("?example=cascaded-interferometers&tab=mach-zehnder-interferometer");
    let active = ws.project.doc(ws.active.unwrap()).unwrap();
    assert_eq!(active.name, "Mach–Zehnder interferometer", "the component's definition tab is active");
    assert_eq!(ws.tabs.len(), 3);

    ws.apply_url_query("examples=chips&chip=wdm-demultiplexer");
    assert!(ws.show_examples);
    assert_eq!(ws.examples_tab, ExamplesTab::Chips);
    assert_eq!(ws.chips.examples[ws.chips.selected].name(), "WDM demultiplexer");
    ws.apply_url_query("examples=layouts");
    assert_eq!(ws.examples_tab, ExamplesTab::Layouts);
    ws.apply_url_query("examples");
    assert_eq!(ws.examples_tab, ExamplesTab::Demos);

    // Unknown values are ignored.
    let before = ws.tabs.len();
    ws.apply_url_query("example=nope&tab=nope&chip=nope&other=1&");
    assert_eq!(ws.tabs.len(), before);
}

#[test]
fn a_components_pins_toggle_to_try_it_on_its_own() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    demo(&mut ws, ADDER);
    let fa = ws.project.find("Full adder").unwrap();
    ws.open_tab(fa);
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    let pins = crate::panels::circuit::sim::interface(&panel(&mut ws, fa).layout).signal_in;
    let outs = crate::panels::circuit::sim::interface(&panel(&mut ws, fa).layout).signal_out;
    let read = |ws: &mut Workspace| {
        let p = panel(ws, fa);
        let o = p.sim.as_ref().unwrap().outcome.clone();
        assert!(o.error.is_none(), "{:?}", o.error);
        let v = |k: usize| o.input(&p.layout, Terminal { item: Item::Part(outs[k]), port: 0 });
        (v(0), v(1))
    };
    assert_eq!(read(&mut ws), (0.0, 0.0));
    // Click pin a, then b, with the mouse: 1 + 1 = 10.
    for (k, expected) in [(0, (1.0, 0.0)), (1, (0.0, 1.0)), (2, (1.0, 1.0))] {
        let p = panel(&mut ws, fa);
        let at = p.view.to_screen(p.canvas_rect().unwrap(), p.layout.components[pins[k]].pos);
        for pressed in [true, false] {
            h.frame(&mut ws, vec![Event::PointerMoved(at)]);
            h.frame(
                &mut ws,
                vec![Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
            );
        }
        h.frame(&mut ws, Vec::new());
        assert_eq!(read(&mut ws), expected, "after toggling pin {k}");
    }
    assert_eq!(panel(&mut ws, fa).pin_value(pins[2]), Some(1.0));
    // Resetting the clock keeps the pins; the adder using it is unaffected.
    panel(&mut ws, fa).reset_clock();
    assert_eq!(panel(&mut ws, fa).machine.inputs, vec![1.0, 1.0, 1.0]);
    assert!(!panel(&mut ws, fa).toggle_pin(9999));
}

#[test]
fn preferences_reach_the_editor_and_render() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let id = ws.active.unwrap();
    ws.show_preferences = true;
    h.frame(&mut ws, Vec::new());
    ws.prefs.show_grid = false;
    ws.prefs.default_wavelength_nm = 1310.0;
    ws.prefs.theme = crate::panels::prefs::Theme::Dark;
    h.frame(&mut ws, Vec::new());
    assert!(!panel(&mut ws, id).prefs.show_grid, "the editor follows the preferences");
    assert_eq!(h.ctx.options(|o| o.theme_preference), egui::ThemePreference::Dark, "theme applied");

    // New lasers take the preferred wavelength.
    let laser = OpticComponentKind::palette()[0];
    panel(&mut ws, id).place(Placeable::Part(laser), model::cell_center(1, 1));
    assert!(matches!(panel(&mut ws, id).layout.components[0].kind, OpticComponentKind::Laser { wavelength: 1310.0, .. }));

    // The laser inspector with a shared source and its own colour.
    if let OpticComponentKind::Laser { source, color, .. } = &mut panel(&mut ws, id).layout.components[0].kind {
        *source = 2;
        *color = Some([1, 2, 3]);
    }
    panel(&mut ws, id).editor.selection.select_only(0);
    h.frame(&mut ws, Vec::new());
    h.frame(&mut ws, Vec::new());
    let o = panel(&mut ws, id).sim.as_ref().unwrap().outcome.clone();
    assert_eq!(o.channel_colors, vec![Some([1, 2, 3])]);

    // Loading saved preferences applies them; "live simulation" covers documents.
    ws.set_preferences(Preferences { live_simulation: false, ..Preferences::default() });
    assert!(!panel(&mut ws, id).simulation_active);
    let new = ws.new_layout();
    assert!(!panel(&mut ws, new).simulation_active);
    ws.show_preferences = false;
    h.frame(&mut ws, Vec::new());
}

#[test]
fn sweeps_run_through_the_workspace_with_components() {
    let mut ws = Workspace::default();
    let mut h = Harness::new();
    let id = demo(&mut ws, "4-channel WDM optical link");
    h.frame(&mut ws, Vec::new());
    let laser = panel(&mut ws, id).layout.components.iter().position(|c| matches!(c.kind, OpticComponentKind::Laser { .. })).unwrap();
    panel(&mut ws, id).open_sweep(Some(Item::Part(laser)));
    panel(&mut ws, id).sweep.points = 21;
    panel(&mut ws, id).run_sweep();
    h.frame(&mut ws, Vec::new());
    let r = panel(&mut ws, id).sweep.result.clone().expect("the workspace ran it");
    // A link can do the same: ?example=…&sweep
    let mut linked = Workspace::default();
    linked.apply_url_query("example=4-channel-wdm-optical-link&sweep");
    Harness::new().frame(&mut linked, Vec::new());
    let doc = linked.active.unwrap();
    assert!(panel(&mut linked, doc).sweep.open && panel(&mut linked, doc).sweep.result.is_some());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(r.xs.len(), 21);
    assert!(r.series.iter().any(|s| s.name.contains("Add–drop ring")), "boxes are named in the legend: {:?}", r.series.iter().map(|s| &s.name).collect::<Vec<_>>());
    h.frame(&mut ws, Vec::new());
}

#[test]
fn slow_designs_resimulate_less_often_while_dragging() {
    // As in the browser, which has no threads to simulate in the background.
    let mut ws = Workspace { background_after_ms: f64::INFINITY, ..Workspace::default() };
    let id = ws.active.unwrap();
    let laser = OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None };
    panel(&mut ws, id).layout.components.push(crate::panels::optics::model::PlacedComponent { kind: laser, pos: Pos2::new(20.0, 20.0) });
    assert!(ws.refresh_simulation(), "first run");
    let nudge = |ws: &mut Workspace, k: f32| panel(ws, id).layout.components[0].pos.y = 20.0 + 40.0 * k;

    // A fast design follows every move, dragging or not.
    panel(&mut ws, id).dragging_new = Some(Placeable::Part(OpticComponentKind::Lens));
    nudge(&mut ws, 1.0);
    assert!(ws.refresh_simulation());

    // A slow one, while dragging, waits a tenth of a second between runs…
    ws.stats.last_simulation_ms = 50.0;
    nudge(&mut ws, 2.0);
    assert!(!ws.refresh_simulation(), "deferred");
    ws.time += 0.2;
    assert!(ws.refresh_simulation(), "caught up");
    ws.stats.last_simulation_ms = 50.0;
    nudge(&mut ws, 3.0);
    assert!(!ws.refresh_simulation(), "deferred again");
    // …and runs at once when the drag ends.
    panel(&mut ws, id).dragging_new = None;
    assert!(ws.refresh_simulation(), "released");
    assert!(!ws.refresh_simulation(), "and then up to date");
}

#[test]
fn unsaved_changes_are_tracked_and_saves_are_atomic() {
    let dir = std::env::temp_dir().join(format!("shine-unsaved-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("work.shine");
    let mut ws = Workspace::default();
    assert!(!ws.has_unsaved_changes(), "a fresh workspace has nothing to lose");
    assert_eq!(ws.file_name(), "Unsaved project");
    let id = ws.active.unwrap();
    panel(&mut ws, id).layout.components.push(crate::panels::optics::model::PlacedComponent { kind: crate::panels::optics::model::DETECTOR, pos: Pos2::new(20.0, 20.0) });
    assert!(ws.has_unsaved_changes());

    // Saving writes the file whole (no temporary left behind) and is clean.
    std::fs::write(&path, b"old contents").unwrap();
    ws.save_project_to(&path).unwrap();
    assert!(!ws.has_unsaved_changes());
    assert_eq!(&std::fs::read(&path).unwrap()[..4], b"SHNE");
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    assert_eq!(ws.file_name(), "work.shine");
    // A save that can't be written leaves the old state and says so.
    assert!(ws.save_project_to(&dir.join("missing").join("x.shine")).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_project_asks_about_unsaved_changes() {
    let mut other = Workspace::default();
    demo(&mut other, ADDER);
    let bytes = other.project.to_bytes();

    // Nothing to lose: opens at once, and counts as saved.
    let mut ws = Workspace::default();
    ws.request_open("adder".into(), bytes.clone(), None);
    assert!(ws.pending_open.is_none());
    assert!(!ws.has_unsaved_changes());
    let opened = ws.project.fingerprint();

    // With changes it waits; Cancel keeps everything.
    let id = ws.active.unwrap();
    panel(&mut ws, id).layout.components.push(crate::panels::optics::model::PlacedComponent { kind: crate::panels::optics::model::DETECTOR, pos: Pos2::new(900.0, 900.0) });
    let changed = ws.project.fingerprint();
    ws.request_open("again".into(), bytes.clone(), None);
    assert!(ws.pending_open.is_some());
    ws.resolve_unsaved(UnsavedChoice::Cancel);
    assert!(ws.pending_open.is_none());
    assert_eq!(ws.project.fingerprint(), changed, "cancel keeps the changes");

    // Open without saving replaces the project.
    ws.request_open("again".into(), bytes.clone(), None);
    ws.resolve_unsaved(UnsavedChoice::Discard);
    assert_eq!(ws.project.fingerprint(), opened);
    assert!(!ws.has_unsaved_changes());

    // Importing an old JSON layout adds a document, so it never asks.
    let id = ws.active.unwrap();
    panel(&mut ws, id).layout.components.clear();
    let docs = ws.project.documents.len();
    ws.request_open("old".into(), br#"{"version": 1, "cell_um": 2.5, "components": [], "lines": []}"#.to_vec(), None);
    assert!(ws.pending_open.is_none());
    assert_eq!(ws.project.documents.len(), docs + 1);

    // The dialog draws, and its buttons answer it.
    let mut h = Harness::new();
    ws.request_open("again".into(), bytes, None);
    assert!(ws.pending_open.is_some());
    h.frame(&mut ws, Vec::new());
    h.key(&mut ws, Key::Escape);
    assert!(ws.pending_open.is_none(), "Escape cancels");
}
