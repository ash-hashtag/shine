//! Integration tests of the whole editor UI: the app is driven like a user
//! would, by clicking widgets found by their labels and dragging with the
//! mouse, through the crate's public API only.

use egui::{Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use shine::panels::optics::OpticsPanel;
use shine::panels::optics::model::{BlockRef, Builtin, Item, OpticComponentKind, PortRef, cell_center, cell_of};
use shine::panels::workspace::Workspace;

fn app() -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(Vec2::new(1400.0, 900.0))
        .with_max_steps(8)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), Workspace::default());
    h.run_steps(3);
    h
}

fn active<'a>(h: &'a Harness<'_, Workspace>) -> &'a OpticsPanel {
    let ws = h.state();
    ws.project.doc(ws.active.unwrap()).unwrap().panel()
}

fn active_mut<'a>(h: &'a mut Harness<'_, Workspace>) -> &'a mut OpticsPanel {
    let ws = h.state_mut();
    let id = ws.active.unwrap();
    ws.project.doc_mut(id).unwrap().panel_mut()
}

/// Screen position of a world point on the active canvas.
fn screen(h: &Harness<'_, Workspace>, world: Pos2) -> Pos2 {
    let p = active(h);
    p.view.to_screen(p.canvas_rect().expect("canvas drawn"), world)
}

/// Press, move in steps, release: a real mouse drag.
fn drag(h: &mut Harness<'_, Workspace>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=10 {
        h.hover_at(from + (to - from) * (k as f32 / 10.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(2);
}

fn click_menu(h: &mut Harness<'_, Workspace>, menu: &str, item: &str) {
    h.get_by_label(menu).click();
    h.run_steps(3);
    if h.query_by_label(item).is_none() {
        let labels: Vec<String> = h.root().children_recursive().filter_map(|n| n.accesskit_node().label()).collect();
        panic!("{menu} → {item} not found; on screen: {labels:?}");
    }
    h.get_by_label(item).click();
    h.run_steps(3);
}

#[test]
fn every_menu_opens_and_lists_its_actions() {
    let mut h = app();
    let menus: [(&str, &[&str]); 6] = [
        ("File", &["New layout", "New component", "Open…", "Save", "Save as…", "Examples…"]),
        ("Edit", &["Undo", "Redo", "Duplicate", "Delete", "Preferences…"]),
        ("View", &["Library", "Inspector", "Live simulation", "Frame all", "Frame selected", "Performance graph", "Keyboard & mouse"]),
        ("Analysis", &["Sweep…"]),
        ("Component", &["Rename…", "Stop sharing as component"]),
        ("Help", &["Keyboard & mouse", "Examples"]),
    ];
    for (menu, items) in menus {
        h.get_by_label(menu).click();
        h.run_steps(2);
        for item in items {
            assert!(h.query_all_by_label(item).count() > 0, "{menu} → {item} missing");
        }
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
}

#[test]
fn menu_actions_do_what_they_say() {
    let mut h = app();
    click_menu(&mut h, "File", "New layout");
    assert_eq!(h.state().tabs.len(), 2);
    click_menu(&mut h, "File", "New component");
    assert_eq!(h.state().tabs.len(), 3);
    assert!(h.state().project.doc(h.state().active.unwrap()).unwrap().component.is_some());
    // It asks for the component's details first; cancel keeps the defaults.
    assert!(h.state().dialog.is_some());
    // Wait out the modal's sizing pass: it moves after its first frame.
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());

    click_menu(&mut h, "Edit", "Preferences…");
    assert!(h.state().show_preferences);
    assert!(h.query_by_label("Power in").is_some(), "the preferences window is drawn");

    click_menu(&mut h, "Help", "Keyboard & mouse");
    assert!(h.state().show_shortcuts);
    click_menu(&mut h, "File", "Examples…");
    assert!(h.state().show_examples);

    // View toggles hide and show the side panels.
    click_menu(&mut h, "View", "Library");
    assert!(!h.state().show_library);
    click_menu(&mut h, "View", "Inspector");
    assert!(!h.state().show_inspector);

    // Analysis → Sweep… opens the sweep window on the active document.
    click_menu(&mut h, "Analysis", "Sweep…");
    assert!(active(&h).sweep.open);
}

#[test]
fn a_laser_dragged_from_the_library_and_wired_to_an_mzi_lights_both_outputs() {
    // The whole flow with the mouse: a laser from the library onto the grid,
    // an MZI from the waveguide blocks, then a waveguide from the laser's
    // light output to the MZI's first input.
    let mut h = app();
    let laser_row = h.get_by_label("Laser").rect().center();
    let laser_at = screen(&h, cell_center(4, 5));
    drag(&mut h, laser_row, laser_at);
    assert_eq!(active(&h).layout.components.len(), 1, "laser placed");
    let laser = active(&h).layout.components[0];
    assert!(matches!(laser.kind, OpticComponentKind::Laser { .. }));
    assert!(active(&h).outcome().unwrap().beams.iter().any(|b| b.field.norm_sqr() > 0.99), "it shines");

    h.get_by_label_contains("Waveguide blocks").click();
    h.run_steps(2);
    let mzi_row = h.get_by_label("MZI switch").rect().center();
    let mzi_at = screen(&h, cell_center(10, 9));
    drag(&mut h, mzi_row, mzi_at);
    let mzi = active(&h).layout.instances.last().expect("MZI placed").id;
    assert!(matches!(active(&h).layout.instances[0].block, BlockRef::Builtin(Builtin::Mzi { .. })));

    let OpticComponentKind::Laser { dir, .. } = laser.kind else { unreachable!() };
    let from = screen(&h, shine::panels::optics::model::laser_light_out(laser.pos, dir));
    let input = PortRef { instance: mzi, port: 0 };
    let to = screen(&h, active(&h).light_port_pos(input, false).unwrap());
    drag(&mut h, from, to);

    let p = active(&h);
    assert_eq!(p.layout.feed_of(0), Some(input), "wired with a waveguide");
    assert_eq!(cell_of(p.layout.components[0].pos), cell_of(laser.pos), "the laser didn't move");
    let o = p.outcome().expect("simulated");
    for k in 0..2 {
        assert!((o.port_power[&(mzi, k)] - 0.5).abs() < 1e-9, "out{} gets half: {:?}", k + 1, o.port_power);
    }
    // The inspector says where the light goes, and shows the outputs with units.
    assert!(h.query_by_label("in1 → out1 50%, out2 50%").is_some(), "split readout");
    assert!(h.query_all_by_label("500 µW").count() >= 2, "both outputs read 500 µW");
}

#[test]
fn every_block_has_an_inspector() {
    // Select one of each built-in block and draw its inspector.
    let mut h = app();
    let blocks: Vec<Builtin> = Builtin::palette().into_iter().chain(Builtin::electronic_palette()).collect();
    for (k, b) in blocks.iter().enumerate() {
        active_mut(&mut h).layout.add_instance(BlockRef::Builtin(b.clone()), (0, 6 * k as i32));
    }
    for k in 0..blocks.len() {
        active_mut(&mut h).editor.boxes.select_only(k);
        h.run_steps(2);
        let name = active(&h).catalog.display(&active(&h).layout.instances[k].block).name;
        assert!(h.query_all_by_label(&name).count() > 0, "inspector for {name}");
    }
}

#[test]
fn right_click_menus_on_boxes_and_wires() {
    let mut h = app();
    let p = active_mut(&mut h);
    let a = p.layout.add_instance(BlockRef::Builtin(Builtin::YSplitter), (2, 2));
    let b = p.layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (8, 2));
    p.layout.connect_optical(PortRef { instance: a, port: 0 }, PortRef { instance: b, port: 0 });
    h.run_steps(2);

    // On a box: its actions, including a sweep of its parameter.
    let at = screen(&h, cell_center(8, 2));
    h.hover_at(at);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    // The menu's entry is the one next to the click (the inspector has one too).
    let entry = h
        .get_all_by_label_contains("Sweep…")
        .min_by(|a, b| a.rect().center().distance(at).total_cmp(&b.rect().center().distance(at)))
        .expect("box menu offers a sweep")
        .rect()
        .center();
    h.hover_at(entry);
    h.event(egui::Event::PointerButton { pos: entry, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: entry, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert_eq!(active(&h).sweep.target, Some(Item::Box(b)));

    // On a waveguide: delete it.
    active_mut(&mut h).sweep.open = false;
    h.run_steps(2);
    let path = active(&h).waveguide_points(0).unwrap();
    let mid = screen(&h, path[12]);
    h.hover_at(mid);
    h.event(egui::Event::PointerButton { pos: mid, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: mid, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    h.get_by_label_contains("Delete waveguide").click();
    h.run_steps(2);
    assert!(active(&h).layout.wires.is_empty());
}

#[test]
fn projects_round_trip_through_files_with_laser_waveguides() {
    // Save a project with a wired laser, open it in a fresh app, and get the
    // same simulation.
    let dir = std::env::temp_dir().join(format!("shine-ui-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wired.shine");
    let mut ws = Workspace::default();
    let id = ws.active.unwrap();
    let p = ws.project.doc_mut(id).unwrap().panel_mut();
    p.layout.components.push(shine::panels::optics::model::PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 2.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    });
    let mzi = p.layout.add_instance(BlockRef::Builtin(Builtin::Mzi { theta: std::f64::consts::FRAC_PI_2, phi: 0.0 }), (4, 3));
    p.layout.connect_feed(0, PortRef { instance: mzi, port: 0 });
    ws.save_project_to(&path).unwrap();

    let mut opened = Workspace::default();
    opened.open_path(&path).unwrap();
    let doc = opened.project.documents.iter().find(|d| !d.layout().feeds.is_empty()).expect("the wired layout");
    let o = opened.project.simulate(doc.id, &Default::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    assert!((o.port_power[&(mzi, 0)] - 2.0).abs() < 1e-9, "4 mW split in two: {:?}", o.port_power);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn windows_open_below_the_menus() {
    // Every floating window opens clear of the menu bar, so the menus keep
    // working while it's open.
    let mut h = app();
    {
        let ws = h.state_mut();
        ws.show_preferences = true;
        ws.show_shortcuts = true;
        ws.show_examples = true;
    }
    active_mut(&mut h).open_sweep(None);
    active_mut(&mut h).show_browser = true;
    h.run_steps(4);
    for title in ["Preferences", "Keyboard & mouse", "Examples", "Sweep", "Parts & components"] {
        let top = h.get_all_by_label(title).map(|n| n.rect().top()).fold(f32::INFINITY, f32::min);
        assert!(top >= shine::panels::ui_kit::TOP_BARS_HEIGHT, "{title} window at y = {top}, over the menus");
    }
    for menu in ["File", "Edit", "View", "Analysis", "Component", "Help"] {
        h.get_by_label(menu).click();
        h.run_steps(3);
        let item = match menu {
            "File" => "New layout",
            "Edit" => "Preferences…",
            "View" => "Frame all",
            "Analysis" => "Sweep…",
            "Component" => "Rename…",
            _ => "Examples",
        };
        assert!(h.query_all_by_label(item).count() > 0, "{menu} didn't open with the windows up");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
}

#[test]
fn the_sweep_window_is_driven_by_its_controls() {
    // Select a coupler, open the sweep, switch to a parameter sweep of the
    // selection, run it, and change the plot's scale: all with clicks.
    let mut h = app();
    let p = active_mut(&mut h);
    p.layout.components.push(shine::panels::optics::model::PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    });
    let coupler = p.layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (4, 0));
    p.layout.connect_feed(0, PortRef { instance: coupler, port: 0 });
    p.editor.boxes.select_only(0);
    h.run_steps(2);
    click_menu(&mut h, "Analysis", "Sweep…");
    h.run_steps(3);
    // Opened on the selected coupler, it's already a parameter sweep; switch
    // to wavelength and back with the tabs (the first "Parameter" is the tab).
    assert_eq!(active(&h).sweep.mode, shine::panels::optics::sweep_ui::SweepMode::Parameter);
    h.get_by_label("Wavelength").click();
    h.run_steps(2);
    assert_eq!(active(&h).sweep.mode, shine::panels::optics::sweep_ui::SweepMode::Wavelength);
    h.get_all_by_label("Parameter").next().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Use selection").click();
    h.run_steps(2);
    assert_eq!(active(&h).sweep.target, Some(Item::Box(coupler)));
    assert_eq!(active(&h).sweep.param, "coupling");
    h.get_by_label("▶  Run sweep").click();
    h.run_steps(3);
    let r = active(&h).sweep.result.clone().expect("ran");
    assert!(r.error.is_none(), "{:?}", r.error);
    // Coupling 0 → all in the bar port; 1 → all crossed; total stays 1 mW.
    let (bar, cross) = (&r.series[0].power, &r.series[1].power);
    assert!((bar[0] - 1.0).abs() < 1e-9 && cross[0].abs() < 1e-9, "{:?}", (bar[0], cross[0]));
    for k in 0..r.xs.len() {
        assert!((bar[k] + cross[k] - 1.0).abs() < 1e-9);
    }
    assert!(h.query_by_label_contains("Insertion loss").is_some(), "readouts shown");
    h.get_by_label("Plot").click();
    h.run_steps(2);
    h.get_by_label("Relative to input (dB)").click();
    h.run_steps(2);
    assert_eq!(active(&h).sweep.scale, shine::panels::optics::sweep_ui::Scale::Relative);
}

#[test]
fn function_cells_are_edited_in_their_inspector() {
    let mut h = app();
    active_mut(&mut h).layout.add_instance(BlockRef::Builtin(Builtin::electronic_palette().pop().unwrap()), (2, 2));
    active_mut(&mut h).editor.boxes.select_only(0);
    h.run_steps(2);
    let outputs = |h: &Harness<'_, Workspace>| match &active(h).layout.instances[0].block {
        BlockRef::Builtin(Builtin::FunctionCell { outputs, light_outputs, .. }) => (outputs.len(), light_outputs.len()),
        _ => unreachable!(),
    };
    let before = outputs(&h);
    h.get_by_label("+ signal output").click();
    h.run_steps(2);
    h.get_by_label("+ light output").click();
    h.run_steps(2);
    assert_eq!(outputs(&h), (before.0 + 1, before.1 + 1));
    assert!(h.query_by_label("Wavelength").is_some(), "light outputs have a wavelength");
    // Remove the light output again with its ×.
    let remove = h.get_all_by_label("×").last().unwrap().rect().center();
    h.hover_at(remove);
    h.event(egui::Event::PointerButton { pos: remove, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: remove, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert_eq!(outputs(&h), (before.0 + 1, before.1));
}

#[test]
fn an_mzi_at_pi_says_it_sends_everything_to_out1() {
    // θ = π is the bar state: all of in1 leaves out1. The inspector says so,
    // so a dark out2 doesn't look like a fault.
    let mut h = app();
    active_mut(&mut h).layout.add_instance(BlockRef::Builtin(Builtin::Mzi { theta: std::f64::consts::PI, phi: 0.0 }), (2, 2));
    active_mut(&mut h).editor.boxes.select_only(0);
    h.run_steps(2);
    assert!(h.query_by_label("in1 → out1 100%, out2 0%").is_some());
}

#[test]
fn a_components_exported_phase_is_set_on_its_box() {
    // The MZ example component exports its phase shifter's phase: toggle the
    // export inside it, go back, and set the phase on the box from its
    // inspector. The light moves to the other output.
    let mut h = app();
    let top = h.state().active.unwrap();
    h.state_mut().place_example(1, (4, 0));
    let mz = active(&h).layout.instances[0].id;
    let BlockRef::Component(mz_doc) = active(&h).layout.instances[0].block else { panic!("a component box") };
    active_mut(&mut h).layout.components.push(shine::panels::optics::model::PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    });
    active_mut(&mut h).layout.connect_feed(0, PortRef { instance: mz, port: 0 });
    h.run_steps(3);
    let before = active(&h).outcome().unwrap().port_power.clone();

    // Inside the component: select the phase shifter and export its phase.
    h.state_mut().open_tab(mz_doc);
    h.run_steps(3);
    let shifter = active(&h).layout.components.iter().position(|c| matches!(c.kind, OpticComponentKind::PhaseShifter { .. })).unwrap();
    active_mut(&mut h).editor.selection.select_only(shifter);
    h.run_steps(3);
    // The example exports it already; the toggle removes and restores it.
    assert_eq!(active(&h).layout.exports.len(), 1, "exported by the example");
    h.get_by_label("phase").click();
    h.run_steps(3);
    assert!(active(&h).layout.exports.is_empty(), "un-exported");
    h.get_by_label("phase").click();
    h.run_steps(3);
    assert_eq!(active(&h).layout.exports.len(), 1, "exported again");
    assert_eq!(active(&h).layout.exports[0].name, "phase");

    // Back on the top layout, the box offers the phase.
    h.state_mut().open_tab(top);
    active_mut(&mut h).editor.boxes.select_only(0);
    h.run_steps(3);
    assert!(h.query_by_label("phase").is_some(), "the box's inspector lists the exported phase");
    assert!(h.query_by_label("default").is_some(), "unset: the component's own value");
    h.get_by_value("0.00 rad").set_value_accesskit(std::f64::consts::PI);
    h.run_steps(3);
    assert_eq!(active(&h).layout.instances[0].param("phase"), Some(std::f64::consts::PI));
    let after = active(&h).outcome().unwrap().port_power.clone();
    let (b0, b1) = (before[&(mz, 0)], before[&(mz, 1)]);
    let (a0, a1) = (after[&(mz, 0)], after[&(mz, 1)]);
    assert!((b0 - a1).abs() < 1e-9 && (b1 - a0).abs() < 1e-9 && (b0 - b1).abs() > 0.9, "before {b0},{b1} after {a0},{a1}");

    // ↺ goes back to the component's value.
    h.get_by_label("↺").click();
    h.run_steps(3);
    assert_eq!(active(&h).layout.instances[0].param("phase"), None);
}

#[test]
fn fps_and_the_performance_graph() {
    let mut h = app();
    click_menu(&mut h, "View", "Performance graph");
    assert!(h.state().prefs.show_performance_graph);
    h.run_steps(3);
    assert!(h.query_all_by_label("Performance").count() > 0, "the graph's window");
    h.state_mut().prefs.show_frame_time = true;
    h.run_steps(3);
    let text = |n: egui_kittest::Node<'_>| {
        let a = n.accesskit_node();
        a.label().or_else(|| a.value()).unwrap_or_default()
    };
    let readouts: Vec<String> = h.query_all_by_label_contains("simulation").map(text).filter(|l| l.contains(" ms")).collect();
    assert_eq!(readouts.len(), 2, "status bar and graph: {readouts:?}");
    let readout = &readouts[0];
    assert!(readout.contains("fps") || readout.starts_with("idle"), "{readout}");
    assert!(!h.state().stats.frames.is_empty(), "frames are recorded");
}

#[test]
fn a_crash_in_the_last_run_is_shown_once() {
    let mut h = app();
    h.state_mut().crash_report = Some(shine::crash::report("boom", "src/x.rs:1", "0: main"));
    h.run_steps(3);
    assert!(h.query_all_by_label("Shine crashed last time").count() > 0);
    assert!(h.query_by_label("Report on GitHub").is_some() && h.query_by_label("Copy details").is_some());
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.state().crash_report.is_none());
    assert_eq!(h.query_all_by_label("Shine crashed last time").count(), 0);
}
