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
    app_stepping(0.25)
}

/// The app with frames `dt` seconds apart (the default, a quarter second, is
/// too slow for double clicks).
fn app_stepping(dt: f32) -> Harness<'static, Workspace> {
    // Simulating live (designs open paused): these tests check what it shows.
    let mut ws = Workspace::default();
    let prefs = shine::panels::prefs::Preferences { simulate_on_open: true, ..ws.prefs.clone() };
    ws.set_preferences(prefs);
    let mut h = Harness::builder()
        .with_size(Vec2::new(1400.0, 900.0))
        .with_max_steps(8)
        .with_step_dt(dt)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    // Results on the frame they're asked for: an unoptimised build is slow
    // enough to send simulations to the background.
    h.state_mut().background_after_ms = f64::INFINITY;
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
        ("Help", &["Keyboard & mouse", "Examples", "About Shine"]),
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
    assert!(h.query_all_by_label("0.500 mW").count() >= 2, "both outputs read 0.500 mW");
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
    let remove = h.get_by_label(&format!("Remove light{}", before.1 + 1)).rect().center();
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
    h.get_by_label("Reset phase").click();
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

#[test]
fn about_shows_the_version_and_whats_new() {
    let mut h = app();
    click_menu(&mut h, "Help", "About Shine");
    h.run_steps(3);
    assert!(h.query_by_label(&format!("Shine {}", env!("CARGO_PKG_VERSION"))).is_some());
    assert!(h.query_by_label_contains("What's new in").is_some());
    assert!(h.query_by_label("Check for updates").is_some());
    assert!(h.query_by_label("Report a problem").is_some());
}

#[test]
fn a_component_box_offers_its_exported_parameters_for_sweeping() {
    let mut h = app();
    h.state_mut().place_example(1, (4, 0));
    let id = active(&h).layout.instances[0].id;
    let params = active(&h).sweep_params(Item::Box(id));
    assert_eq!(params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["phase"]);
    // Right-click → Sweep… sets it up on the box's phase.
    let at = screen(&h, cell_center(5, 1));
    h.hover_at(at);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    let entry = h
        .get_all_by_label_contains("Sweep…")
        .min_by(|a, b| a.rect().center().distance(at).total_cmp(&b.rect().center().distance(at)))
        .expect("offered")
        .rect()
        .center();
    h.hover_at(entry);
    h.event(egui::Event::PointerButton { pos: entry, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: entry, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    let s = &active(&h).sweep;
    assert_eq!((s.target, s.param.as_str()), (Some(Item::Box(id)), "phase"));
}

#[test]
fn readings_and_images_are_exported() {
    // Saves go to a folder instead of a dialog.
    let dir = std::env::temp_dir().join(format!("shine-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before anything in this test binary reads it; no other test saves files.
    unsafe { std::env::set_var("SHINE_EXPORT_DIR", &dir) };

    let mut h = app();
    assert!(h.state_mut().open_example("4-channel-wdm-optical-link"));
    h.run_steps(4);
    // The inspector's readouts: copy and save as CSV.
    h.get_by_label("Copy CSV").click();
    h.run_steps(2);
    assert_eq!(active(&h).status.as_ref().map(|s| s.0.as_str()), Some("Readings copied"));
    h.get_by_label("Save CSV…").click();
    h.run_steps(2);
    let csv = std::fs::read_to_string(dir.join("readings.csv")).expect("saved");
    assert!(csv.contains("\"Display 26,9\",11,\"\""), "{csv}");

    // The canvas as an image: the screenshot comes back through egui's
    // callback, which the test harness doesn't render, so hand one over.
    click_menu(&mut h, "View", "Save canvas image…");
    let (name, rect) = h.state().pending_image.expect("waiting for a screenshot");
    assert_eq!(name, "canvas.png");
    let shot = egui::ColorImage::new([1400, 900], vec![egui::Color32::from_rgb(10, 20, 30); 1400 * 900]);
    *h.state().screenshot.lock().unwrap() = Some(std::sync::Arc::new(shot));
    h.run_steps(2);
    assert!(h.state().pending_image.is_none());
    let bytes = std::fs::read(dir.join("canvas.png")).expect("image saved");
    let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let info = reader.info();
    assert_eq!((info.width, info.height), (rect.width().round() as u32, rect.height().round() as u32), "just the canvas");
    std::fs::remove_dir_all(&dir).ok();
}

/// Every control on screen that a screen reader would announce without a
/// name: its role, value and position, to find it.
fn unnamed_controls(h: &Harness<'_, Workspace>, state: &str) -> Vec<String> {
    use egui::accesskit::Role;
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            let control = matches!(
                a.role(),
                Role::Button
                    | Role::CheckBox
                    | Role::RadioButton
                    | Role::Switch
                    | Role::TextInput
                    | Role::MultilineTextInput
                    | Role::SearchInput
                    | Role::SpinButton
                    | Role::Slider
                    | Role::ComboBox
                    | Role::Tab
                    | Role::MenuItem
                    | Role::Link
                    | Role::ListBoxOption
            );
            // A name of symbols only ("×", "▦") says nothing either.
            control && a.label().is_none_or(|l| !l.chars().any(char::is_alphanumeric))
        })
        .map(|n| {
            let a = n.accesskit_node();
            format!("{state}: {:?} {:?} value {:?} at {:?}", a.role(), a.label(), a.value(), n.rect())
        })
        .collect()
}

#[test]
fn every_control_is_named_for_screen_readers() {
    let mut h = app();
    let mut missing = Vec::new();
    missing.extend(unnamed_controls(&h, "start"));

    for menu in ["File", "Edit", "View", "Analysis", "Component", "Help"] {
        h.get_by_label(menu).click();
        h.run_steps(3);
        missing.extend(unnamed_controls(&h, &format!("{menu} menu")));
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }

    // The inspector of every block and part.
    let blocks: Vec<Builtin> = Builtin::palette().into_iter().chain(Builtin::electronic_palette()).collect();
    for (k, b) in blocks.iter().enumerate() {
        active_mut(&mut h).layout.add_instance(BlockRef::Builtin(b.clone()), (0, 6 * k as i32));
    }
    for k in 0..blocks.len() {
        active_mut(&mut h).editor.boxes.select_only(k);
        h.run_steps(2);
        let name = active(&h).catalog.display(&active(&h).layout.instances[k].block).name;
        missing.extend(unnamed_controls(&h, &format!("inspector of {name}")));
        if k == 0 {
            // The laser's phase: the slider and its number box read as "Phase".
            use egui::accesskit::Role;
            let roles: Vec<Role> = h.query_all_by_label("Phase").map(|n| n.accesskit_node().role()).collect();
            assert!(roles.contains(&Role::Slider) && roles.contains(&Role::SpinButton), "{roles:?}");
        }
    }

    // Examples with parts, readings and components.
    for example in ["4-channel-wdm-optical-link"] {
        assert!(h.state_mut().open_example(example));
        h.run_steps(4);
        missing.extend(unnamed_controls(&h, example));
        let parts = active(&h).layout.components.len();
        for k in 0..parts {
            let p = active_mut(&mut h);
            p.editor.boxes.clear();
            p.editor.selection.select_only(k);
            h.run_steps(2);
            missing.extend(unnamed_controls(&h, &format!("{example}, part {k}")));
        }
    }

    // The Assets tab, and the component dialog with its appearance settings.
    h.get_by_label("Assets").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Examples").is_some());
    missing.extend(unnamed_controls(&h, "assets tab"));
    h.get_by_label("Parts").click();
    h.run_steps(2);
    let id = h.state().active.unwrap();
    h.state_mut().open_document_dialog(id, true);
    for _ in 0..10 {
        h.run_steps(1);
    }
    h.get_by_label("Own colour").click();
    h.run_steps(2);
    missing.extend(unnamed_controls(&h, "component dialog"));
    h.get_by_label("Cancel").click();
    h.run_steps(2);

    // The rename dialog.
    click_menu(&mut h, "Component", "Rename…");
    assert!(h.query_by_label("Cancel").is_some() || h.query_by_label("Save").is_some(), "the dialog is open");
    missing.extend(unnamed_controls(&h, "rename dialog"));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // Every window.
    {
        let ws = h.state_mut();
        ws.show_preferences = true;
        ws.show_shortcuts = true;
        ws.show_examples = true;
        ws.prefs.show_performance_graph = true;
        ws.crash_report = Some(shine::crash::report("boom", "src/x.rs:1", "0: main"));
        ws.library_window.open = true;
    }
    h.state_mut().open_export_window(shine::panels::workspace::ExportFormat::Gdsfactory);
    h.state_mut().open_transmission_window();
    h.state_mut().open_monte_carlo_window();
    h.state_mut().open_optimise_window();
    active_mut(&mut h).show_browser = true;
    h.run_steps(4);
    missing.extend(unnamed_controls(&h, "windows"));
    click_menu(&mut h, "Help", "About Shine");
    missing.extend(unnamed_controls(&h, "about"));
    click_menu(&mut h, "Analysis", "Sweep…");
    h.get_by_label("▶  Run sweep").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Insertion loss").is_some(), "the sweep ran");
    missing.extend(unnamed_controls(&h, "sweep"));

    // The same widget in several states is one problem.
    let mut seen = std::collections::HashSet::new();
    missing.retain(|m| seen.insert(m.split_once(": ").map_or(m.clone(), |(_, w)| w.to_owned())));
    assert!(missing.is_empty(), "{} unnamed controls:\n{}", missing.len(), missing.join("\n"));
}

#[test]
fn the_canvas_works_from_the_keyboard() {
    // No pointer on the canvas: select, move, add, zoom and pan with keys.
    use egui::{Key, Modifiers};
    let mut h = app();
    active_mut(&mut h).layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (4, 4));
    h.hover_at(Pos2::new(5.0, 5.0));
    h.run_steps(2);

    // ] selects the first item, and the canvas names it for screen readers.
    h.key_press(Key::CloseBracket);
    h.run_steps(2);
    assert_eq!(active(&h).editor.boxes.active(), Some(0));
    assert!(
        h.query_by_label_contains("Selected: Directional coupler at cell 4, 4").is_some(),
        "the canvas describes the selection"
    );

    // Arrows move it by a cell, Shift+arrows by five; one undo step each.
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(active(&h).layout.instances[0].cell, (5, 4));
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(active(&h).layout.instances[0].cell, (5, 9));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(active(&h).layout.instances[0].cell, (5, 4));

    // G, arrows, Enter: a grab without the mouse.
    h.key_press(Key::G);
    h.run_steps(1);
    h.key_press(Key::ArrowLeft);
    h.key_press(Key::ArrowLeft);
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(active(&h).editor.grab.is_none());
    assert_eq!(active(&h).layout.instances[0].cell, (3, 4));

    // Shift+A opens the add menu next to the selection; type and Enter.
    h.key_press_modifiers(Modifiers::SHIFT, Key::A);
    h.run_steps(3);
    assert!(active(&h).menu.is_some(), "add menu without the pointer on the canvas");
    h.get_by_label("Search parts to add").type_text("phase shifter");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let placed = active(&h).layout.components.len() + active(&h).layout.instances.len();
    assert_eq!(placed, 2, "placed from the keyboard: {:?}", active(&h).status);

    // With nothing selected, arrows pan; + and − zoom.
    h.key_press_modifiers(Modifiers::ALT, Key::A);
    h.run_steps(2);
    let pan = active(&h).view.pan;
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert!(active(&h).view.pan.x < pan.x, "panned");
    let zoom = active(&h).view.zoom;
    h.key_press(Key::Plus);
    h.run_steps(2);
    assert!(active(&h).view.zoom > zoom);
    h.key_press(Key::Minus);
    h.run_steps(2);
    assert!((active(&h).view.zoom - zoom).abs() < 1e-4);
}

#[test]
fn tab_moves_through_the_controls() {
    // Tab moves the keyboard focus (it opens a component only when a
    // component box is selected), and keys go to the focused control.
    use egui::Key;
    let mut h = app();
    active_mut(&mut h).layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (4, 4));
    active_mut(&mut h).editor.boxes.select_only(0);
    h.run_steps(2);
    let focused = |h: &Harness<'_, Workspace>| h.ctx.memory(|m| m.focused());
    assert_eq!(focused(&h), None);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..6 {
        h.key_press(Key::Tab);
        h.run_steps(2);
        let f = focused(&h).expect("Tab focuses a control");
        seen.insert(f);
    }
    assert!(seen.len() >= 5, "Tab moves on each time: {seen:?}");
    assert_eq!(h.state().project.documents.len(), 1, "no component was opened");

    // The coupling's slider takes the arrow keys while it has focus; the
    // box stays put.
    h.get_all_by_label("Coupling").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider).expect("slider").focus();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(active(&h).layout.instances[0].cell, (4, 4), "the box didn't move");
    let BlockRef::Builtin(Builtin::Coupler { coupling }) = active(&h).layout.instances[0].block else { panic!() };
    assert!(coupling > 0.5, "the slider moved: {coupling}");
}

#[test]
fn wires_are_drawn_from_the_keyboard() {
    // W on the start, ] to the end, Enter: the first free matching ports.
    use egui::Key;
    let mut h = app();
    let p = active_mut(&mut h);
    p.layout.components.push(shine::panels::optics::model::PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    });
    let mzi = p.layout.add_instance(BlockRef::Builtin(Builtin::Mzi { theta: std::f64::consts::FRAC_PI_2, phi: 0.0 }), (4, 0));
    let number = p.layout.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 2, value: 3 }), (0, 10));
    let display = p.layout.add_instance(BlockRef::Builtin(Builtin::Display { bits: 2 }), (0, 16));
    h.hover_at(Pos2::new(5.0, 5.0));
    h.run_steps(2);

    // Laser (the first item) → the MZI (the next).
    h.key_press(Key::CloseBracket);
    h.run_steps(1);
    h.key_press(Key::W);
    h.run_steps(1);
    assert!(h.query_all_by_label_contains("Wiring from Laser").count() >= 2, "the canvas and status bar say a wire is being drawn");
    h.key_press(Key::CloseBracket);
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(active(&h).layout.feeds.len(), 1, "{:?}", active(&h).status);
    assert_eq!(active(&h).layout.feeds[0].to.instance, mzi);
    let o = active(&h).outcome().expect("simulated");
    assert!((o.port_power[&(mzi, 0)] - 0.5).abs() < 1e-9, "light reaches the MZI");

    // Number input → display: signals, one wire per bit pressed.
    for _ in 0..2 {
        active_mut(&mut h).editor.boxes.select_only(1);
        h.run_steps(1);
        h.key_press(Key::W);
        h.key_press(Key::CloseBracket);
        h.run_steps(1);
        h.key_press(Key::W);
        h.run_steps(2);
    }
    let signals = &active(&h).layout.signals;
    assert_eq!(signals.len(), 2, "{:?}", active(&h).status);
    assert!(signals.iter().all(|w| w.from.item == Item::Box(number) && w.to.item == Item::Box(display)));
    assert_ne!(signals[0].to.port, signals[1].to.port, "each wire takes the next free input");

    // Esc abandons a wire.
    h.key_press(Key::W);
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(1);
    h.key_press(Key::CloseBracket);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(active(&h).layout.signals.len(), 2);
}

/// A file dropped on the window, as the desktop app gets one.
#[derive(Debug)]
struct Dropped {
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
}

fn drop_file(h: &mut Harness<'_, Workspace>, at: Pos2, name: &str, bytes: Vec<u8>) {
    h.hover_at(at);
    h.run_steps(1);
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped { path: format!("/tmp/{name}").into(), bytes }));
    h.run_steps(3);
}

/// A project with a published component that uses the example Full adder.
fn component_file() -> Vec<u8> {
    use shine::panels::project::Project;
    let mut p = Project::new();
    let fa = shine::panels::optics::examples::import_component(&mut p, 0).unwrap();
    let mut layout = shine::panels::optics::model::Layout::default();
    layout.add_instance(BlockRef::Component(fa), (0, 0));
    let id = p.add_component("Adder pair", "AP", "Two adders", layout, 10.0);
    p.extract(id).unwrap().to_bytes()
}

#[test]
fn the_assets_tab_lists_folders_searches_and_places() {
    let mut h = app();
    h.get_by_label("Assets").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("This project").is_some());
    assert!(h.query_by_label_contains("Examples").is_some(), "Shine's components are in the library");
    assert!(h.query_by_label("Full adder").is_some());

    // Search by a port name.
    h.get_by_label("Search assets").focus();
    h.run_steps(1);
    h.get_by_label("Search assets").type_text("carry");
    h.run_steps(2);
    assert!(h.query_by_label("Full adder").is_some());
    assert!(h.query_by_label("1→4 splitter").is_none(), "filtered out: search {:?}", h.state().asset_search);

    // Drag it onto the grid: the project gets its own copy.
    let from = h.get_by_label("Full adder").rect().center();
    let to = screen(&h, cell_center(6, 4));
    drag(&mut h, from, to);
    let fa = h.state().project.find("Full adder").expect("copied into the project");
    assert_eq!(active(&h).layout.instances.len(), 1);
    assert_eq!(active(&h).layout.instances[0].block, BlockRef::Component(fa));
    // Now it is one of the project's components too.
    h.get_by_label("Search assets").type_text("x");
    h.run_steps(1);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert!(h.query_by_label_contains("This project  ·  1").is_some());
}

#[test]
fn files_dropped_on_the_library_are_kept_and_elsewhere_open() {
    let mut h = app();
    h.get_by_label("Assets").click();
    h.run_steps(2);
    let library = h.state().library_rect.expect("the left panel is on screen");

    // On the library: kept for every project, in "My components".
    drop_file(&mut h, library.center(), "pair.shinecomp", component_file());
    assert_eq!(h.state().library.len(), 1, "{:?}", h.state().status);
    assert!(h.query_by_label_contains("My components  ·  1").is_some());
    assert!(h.query_by_label("Adder pair").is_some());
    assert_eq!(h.state().project.documents.len(), 1, "the project is untouched");

    // Anywhere else: opened in a tab of its own, in this project.
    let canvas = active(&h).canvas_rect().unwrap().center();
    drop_file(&mut h, canvas, "pair.shinecomp", component_file());
    let ws = h.state();
    assert_eq!(ws.tabs.len(), 2);
    let opened = ws.project.doc(ws.active.unwrap()).unwrap();
    assert_eq!(opened.name, "Adder pair");
    assert!(ws.project.find("Full adder").is_some(), "with the component it uses");

    // A library component placed in a layout copies it once.
    let first = h.state().tabs[0];
    h.state_mut().open_tab(first);
    let asset = h.state().library.summaries().last().unwrap().asset;
    h.state_mut().place_asset(asset, (2, 2));
    h.run_steps(2);
    assert_eq!(h.state().project.documents.len(), 3, "reuses the copies already here");

    // Not a Shine file: says so.
    drop_file(&mut h, library.center(), "notes.txt", b"hello".to_vec());
    assert!(h.state().status.as_ref().unwrap().0.contains("isn't a Shine"), "{:?}", h.state().status);
}

#[test]
fn a_component_gets_a_colour_shape_and_picture() {
    use shine::panels::appearance::{BoxShape, IconFormat};
    let mut h = app();
    let id = h.state().active.unwrap();
    h.state_mut().open_document_dialog(id, true);
    // The dialog fades in, ignoring clicks meanwhile.
    for _ in 0..10 {
        h.run_steps(1);
    }
    assert!(h.query_by_label("Own colour").is_some(), "the dialog has appearance settings");
    h.get_by_label("Own colour").click();
    h.run_steps(2);
    h.get_by_label("Cut corners").click();
    h.run_steps(2);

    // A big photo dropped on the dialog: shrunk and stored as WebP.
    let photo = {
        let img = image::RgbaImage::from_fn(900, 600, |x, y| image::Rgba([(x / 4) as u8, (y / 3) as u8, 90, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    drop_file(&mut h, Pos2::new(700.0, 450.0), "photo.png", photo.clone());
    let icon = h.state().dialog.as_ref().unwrap().appearance.icon.clone().expect("the picture is used");
    assert_eq!(icon.format, IconFormat::WebP);
    assert!(icon.bytes.len() < photo.len() / 10, "{} bytes from {}", icon.bytes.len(), photo.len());
    assert!(h.query_by_label("Remove picture").is_some());
    h.get_by_label("Save").click();
    h.run_steps(3);

    let ws = h.state();
    let meta = ws.project.doc(ws.active.unwrap()).unwrap().component.clone().expect("published");
    assert_eq!(meta.appearance.shape, BoxShape::Chamfered);
    assert!(meta.appearance.color.is_some());
    assert_eq!(meta.appearance.icon, Some(icon));

    // An SVG is kept as it is.
    let id = ws.active.unwrap();
    h.state_mut().open_document_dialog(id, true);
    h.run_steps(2);
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#;
    drop_file(&mut h, Pos2::new(700.0, 450.0), "logo.svg", svg.to_vec());
    let webp = h.state().project.doc(id).unwrap().component.clone().unwrap().appearance.icon.unwrap();
    let icon = h.state().dialog.as_ref().unwrap().appearance.icon.clone().unwrap();
    assert_eq!((icon.format, &*icon.bytes), (IconFormat::Svg, &svg[..]));

    // The component's boxes and rows draw it (several frames: pictures decode
    // in the background).
    h.get_by_label("Save").click();
    let component = h.state().active.unwrap();
    let layout = h.state_mut().new_layout();
    h.state_mut().project.doc_mut(layout).unwrap().panel_mut().layout.add_instance(BlockRef::Component(component), (2, 2));
    h.run_steps(8);
    let hint = egui::SizeHint::Size { width: 64, height: 64, maintain_aspect_ratio: true };
    for icon in [webp, icon] {
        let ready = (0..200).any(|_| {
            h.run_steps(1);
            matches!(h.ctx.try_load_texture(&icon.uri(), Default::default(), hint), Ok(egui::load::TexturePoll::Ready { .. }))
        });
        assert!(ready, "{:?} picture decodes", icon.format);
    }
}

fn double_click(h: &mut Harness<'_, Workspace>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    // A double click is two clicks within 0.3 s: use `app_stepping` with short frames.
    for _ in 0..2 {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        h.run_steps(1);
    }
    h.run_steps(2);
}

#[test]
fn double_clicking_a_component_box_opens_it_in_a_tab() {
    let mut h = app_stepping(1.0 / 60.0);
    h.state_mut().place_example(1, (4, 0));
    h.run_steps(2);
    let mz = h.state().project.find("Mach–Zehnder interferometer").unwrap();
    let at = screen(&h, cell_center(5, 1));
    double_click(&mut h, at);
    assert_eq!(h.state().active, Some(mz), "opened: tabs {:?}, status {:?}", h.state().tabs, h.state().status);
}

/// The tile (or row) named `name`: a button, not a heading or label.
fn tile<'a>(h: &'a Harness<'_, Workspace>, name: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label(name).find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("a tile")
}

/// A library with `n` distinct components in `folders` folders.
fn big_library(n: usize, folders: usize) -> shine::panels::assets::Library {
    use shine::panels::project::Project;
    let mut library = shine::panels::assets::Library::new();
    for k in 0..n {
        let mut p = Project::new();
        let layout = shine::panels::optics::model::Layout::with_components(vec![shine::panels::optics::model::PlacedComponent {
            kind: OpticComponentKind::OutputPort,
            pos: cell_center(0, 0),
        }]);
        let id = p.add_component(&format!("Part {k:04}"), "P", &format!("Number {k}"), layout, 10.0);
        library.add(&p.extract(id).unwrap(), &format!("Folder {}", k % folders)).unwrap();
    }
    library
}

#[test]
fn the_library_window_browses_thousands_in_a_grid() {
    let mut h = app_stepping(1.0 / 60.0);
    h.state_mut().library = big_library(3000, 12);
    click_menu(&mut h, "View", "Asset library…");
    h.run_steps(2);
    assert!(h.query_by_label("3004 components").is_some(), "every asset, and the 4 examples");

    // Only the visible rows are drawn, so a frame stays quick.
    let start = std::time::Instant::now();
    h.run_steps(10);
    let per_frame = start.elapsed().as_secs_f64() * 1e3 / 10.0;
    let tiles = h.query_all_by_label_contains("Part ").count();
    assert!(tiles < 100, "{tiles} tiles drawn of 3000");
    assert!(per_frame < 50.0, "{per_frame:.1} ms per frame");

    // A folder, then a search inside it.
    h.get_by_label_contains("Folder 3  (250)").click();
    h.run_steps(2);
    assert!(h.query_by_label("250 components").is_some());
    h.get_by_label("Search the asset library").focus();
    h.run_steps(1);
    h.get_by_label("Search the asset library").type_text("Part 0003");
    h.run_steps(2);
    assert!(h.query_by_label("1 component").is_some());

    // Select it: its details; file it in a new folder.
    tile(&h, "Part 0003").click();
    h.run_steps(2);
    assert!(h.query_by_label("Number 3").is_some(), "the description in the details");
    h.get_by_label("New folder name").focus();
    h.run_steps(1);
    h.get_by_label("New folder name").type_text("Favourites");
    h.run_steps(1);
    h.get_by_label("Make").click();
    h.run_steps(2);
    let summaries = h.state().library.summaries();
    let moved = summaries.iter().find(|a| a.entry.name == "Part 0003").unwrap();
    assert_eq!(moved.folder, "Favourites");

    // Double-click opens it in a tab (copied into the project). After a
    // pause: clicks close together count as a triple click.
    h.run_steps(30);
    let at = tile(&h, "Part 0003").rect().center();
    double_click(&mut h, at);
    let ws = h.state();
    assert_eq!(ws.project.doc(ws.active.unwrap()).unwrap().name, "Part 0003");
}

#[test]
fn the_assets_tab_shows_a_few_per_folder_and_opens_by_double_click() {
    let mut h = app_stepping(1.0 / 60.0);
    h.state_mut().library = big_library(100, 1);
    h.get_by_label("Assets").click();
    h.run_steps(2);
    assert!(h.query_by_label("All 100 in the library…").is_some(), "the rest are in the window");
    assert!(h.query_all_by_label_contains("Part ").count() <= 40);
    // Below the visible rows: a screen reader's click.
    h.get_by_label("All 100 in the library…").click_accesskit();
    h.run_steps(2);
    assert!(h.state().library_window.open);
    assert!(h.query_by_label("100 components").is_some());

    // Double-click a row: opened in a tab.
    h.state_mut().library_window.open = false;
    h.run_steps(2);
    h.run_steps(30);
    let at = tile(&h, "Part 0000").rect().center();
    double_click(&mut h, at);
    let ws = h.state();
    assert_eq!(ws.project.doc(ws.active.unwrap()).unwrap().name, "Part 0000");
}

#[test]
fn the_library_window_opens_at_its_full_size() {
    // Its size from the first frame, not growing frame by frame.
    let mut h = app();
    click_menu(&mut h, "View", "Asset library…");
    let size = |h: &Harness<'_, Workspace>| h.get_all_by_label("Asset library").map(|n| n.rect()).max_by(|a, b| a.area().total_cmp(&b.area())).unwrap().size();
    let first = size(&h);
    assert!(first.x >= 900.0 && first.y >= 560.0, "opens at its size: {first:?}");
    for _ in 0..10 {
        h.run_steps(1);
        assert_eq!(size(&h), first, "keeps it");
    }
    // With more to show, still the same size.
    h.state_mut().library = big_library(500, 30);
    h.run_steps(5);
    assert_eq!(size(&h), first);
}

#[test]
fn a_folder_of_components_is_imported_with_its_subfolders() {
    let dir = std::env::temp_dir().join(format!("shine-components-{}", std::process::id()));
    let rings = dir.join("Rings");
    std::fs::create_dir_all(&rings).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join("pair.shinecomp"), component_file()).unwrap();
    std::fs::write(rings.join("again.shinecomp"), component_file()).unwrap();
    std::fs::write(dir.join(".hidden").join("skip.shinecomp"), component_file()).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a component").unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();

    let mut h = app();
    // Dropped on the window, like a file.
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped { path: dir.clone(), bytes: Vec::new() }));
    h.run_steps(3);
    let library = &h.state().library;
    assert_eq!(library.len(), 1, "the same component twice is kept once: {:?}", h.state().status);
    assert_eq!(library.folders(), vec![name.clone()]);
    assert!(h.state().status.as_ref().unwrap().0.contains("from 2 files"), "{:?}", h.state().status);

    // A different component in the subfolder goes into its own library folder.
    let mut other = shine::panels::project::Project::new();
    let id = other.add_component("Ring filter", "RF", "", shine::panels::optics::model::Layout::default(), 10.0);
    std::fs::write(rings.join("ring.shinecomp"), other.extract(id).unwrap().to_bytes()).unwrap();
    h.state_mut().import_folder(&dir);
    assert_eq!(h.state().library.folders(), vec![name.clone(), format!("{name} / Rings")]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_a_selects_everything_to_move_or_delete() {
    use egui::{Key, Modifiers};
    let mut h = app();
    let p = active_mut(&mut h);
    p.layout.components.push(shine::panels::optics::model::PlacedComponent { kind: OpticComponentKind::palette()[0], pos: cell_center(0, 0) });
    p.layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (4, 0));
    p.layout.add_instance(BlockRef::Builtin(Builtin::YSplitter), (4, 6));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    assert_eq!(active(&h).editor.len(), 3, "every part and box");

    // Move them all, then delete them all.
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(active(&h).layout.instances.iter().map(|i| i.cell).collect::<Vec<_>>(), vec![(5, 0), (5, 6)]);
    assert_eq!(shine::panels::optics::model::cell_of(active(&h).layout.components[0].pos), (1, 0));
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(active(&h).layout.components.is_empty() && active(&h).layout.instances.is_empty());
}

#[test]
fn waveguide_attenuators_and_phase_shifters_take_a_signal() {
    use shine::panels::optics::model::{Layout, Terminal};
    use shine::panels::circuit::sim::{Machine, NoDocuments, Options, simulate};
    let run = |block: Builtin, signal: Option<&str>| {
        let mut l = Layout::default();
        let laser = l.add_instance(BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }), (0, 0));
        let b = l.add_instance(BlockRef::Builtin(block), (4, 0));
        l.connect_optical(PortRef { instance: laser, port: 0 }, PortRef { instance: b, port: 0 });
        if let Some(expr) = signal {
            // A function cell with no inputs: a constant signal.
            let cell = l.add_instance(
                BlockRef::Builtin(Builtin::FunctionCell { name: "k".into(), inputs: 0, light_inputs: 0, outputs: vec![expr.into()], light_outputs: Vec::new(), wavelength_nm: 1550.0 }),
                (4, -5),
            );
            l.connect_signal(Terminal { item: Item::Box(cell), port: 0 }, Terminal { item: Item::Box(b), port: 0 });
        }
        let o = simulate(&l, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        (o.port_power[&(b, 0)], o)
    };
    let att = Builtin::Attenuator { loss_db: 3.0103 };
    assert!((run(att.clone(), None).0 - 0.5).abs() < 1e-4, "3 dB passes half");
    assert!((run(att.clone(), Some("1")).0 - 0.5).abs() < 1e-4, "1 leaves it as set");
    assert!((run(att.clone(), Some("0.5")).0 - 0.25).abs() < 1e-4, "0.5 halves it again");
    assert_eq!(run(att.clone(), Some("0")).0, 0.0, "0 blocks it");
    // The phase shifter's signal adds π: in one arm of an interferometer it
    // turns the bright output dark.
    let interferometer = |signal: Option<&str>| {
        let mut l = Layout::default();
        let laser = l.add_instance(BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }), (0, 0));
        let split = l.add_instance(BlockRef::Builtin(Builtin::YSplitter), (4, 0));
        let ps = l.add_instance(BlockRef::Builtin(Builtin::PhaseShifter { phase: 0.0 }), (8, 0));
        let join = l.add_instance(BlockRef::Builtin(Builtin::YCombiner), (12, 0));
        let port = |instance, port| PortRef { instance, port };
        l.connect_optical(port(laser, 0), port(split, 0));
        l.connect_optical(port(split, 0), port(ps, 0));
        l.connect_optical(port(ps, 0), port(join, 0));
        l.connect_optical(port(split, 1), port(join, 1));
        if let Some(expr) = signal {
            let cell = l.add_instance(
                BlockRef::Builtin(Builtin::FunctionCell { name: "k".into(), inputs: 0, light_inputs: 0, outputs: vec![expr.into()], light_outputs: Vec::new(), wavelength_nm: 1550.0 }),
                (8, -5),
            );
            l.connect_signal(Terminal { item: Item::Box(cell), port: 0 }, Terminal { item: Item::Box(ps), port: 0 });
        }
        let o = simulate(&l, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        o.port_power[&(join, 0)]
    };
    assert!((interferometer(None) - 1.0).abs() < 1e-9, "in phase: all the light");
    assert!(interferometer(Some("1")) < 1e-9, "π apart: dark");

    // The inspector says what the signal does.
    let mut h = app();
    let p = active_mut(&mut h);
    let b = p.layout.add_instance(BlockRef::Builtin(att), (4, 0));
    let cell = p.layout.add_instance(
        BlockRef::Builtin(Builtin::FunctionCell { name: "k".into(), inputs: 0, light_inputs: 0, outputs: vec!["0.5".into()], light_outputs: Vec::new(), wavelength_nm: 1550.0 }),
        (4, -5),
    );
    p.layout.connect_signal(Terminal { item: Item::Box(cell), port: 0 }, Terminal { item: Item::Box(b), port: 0 });
    p.editor.boxes.select_only(0);
    h.run_steps(3);
    assert!(h.query_all_by_label_contains("Waveguide attenuator").count() >= 1, "its own name");
    assert!(h.query_by_label_contains("passes 25%").is_some(), "Driven by a signal: 0.5 × 50% → passes 25%");
}

#[test]
fn a_scripted_component_is_written_and_applied_in_the_inspector() {
    let mut h = app();
    click_menu(&mut h, "Component", "New scripted component");
    h.run_steps(3);
    let id = h.state().active.unwrap();
    assert!(h.state().project.doc(id).unwrap().component.is_some(), "a component, open in its tab");
    assert!(h.query_by_label("JavaScript of this component").is_some(), "its script in the inspector");

    // Add a light output and apply: the component gets a third pin.
    let code = shine::panels::script::TEMPLATE.replace(r#"outputs: ["out1", "out2"]"#, r#"outputs: ["out1", "out2", "lost"]"#).replace(
        "return [[Math.sqrt(1 - k)], [Math.sqrt(k)]];",
        "return [[Math.sqrt(1 - k)], [Math.sqrt(k)], [0]];",
    );
    if let Some(edit) = &mut active_mut(&mut h).script_edit {
        edit.1 = code;
    }
    h.run_steps(2);
    h.get_by_label("Apply").click_accesskit();
    h.run_steps(3);
    let interface = h.state().project.doc(id).unwrap().interface();
    assert_eq!(interface.outputs, vec!["out1", "out2", "lost"], "{:?}", active(&h).status);

    // A mistake: explained, nothing changed.
    if let Some(edit) = &mut active_mut(&mut h).script_edit {
        edit.1 = "const component = ;".into();
    }
    h.run_steps(2);
    h.get_by_label("Apply").click_accesskit();
    h.run_steps(3);
    assert!(h.query_by_label_contains("line 1").is_some(), "the error says where");
    assert_eq!(h.state().project.doc(id).unwrap().interface().outputs.len(), 3);
}

#[test]
fn the_transmission_window_shows_the_s_matrix_and_paths() {
    // Pins around a 30 % coupler: bar 70 % (−1.55 dB), cross 30 % (−5.23 dB).
    let mut h = app();
    let p = active_mut(&mut h);
    let pr = |instance, port| PortRef { instance, port };
    let a = p.layout.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "a".into() }), (0, 0));
    let b = p.layout.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "b".into() }), (0, 2));
    let dc = p.layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.3 }), (4, 0));
    let bar = p.layout.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "bar".into() }), (8, 0));
    let cross = p.layout.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "cross".into() }), (8, 2));
    p.layout.connect_optical(pr(a, 0), pr(dc, 0));
    p.layout.connect_optical(pr(b, 0), pr(dc, 1));
    p.layout.connect_optical(pr(dc, 0), pr(bar, 0));
    p.layout.connect_optical(pr(dc, 1), pr(cross, 0));
    h.run_steps(2);
    click_menu(&mut h, "Analysis", "Transmission…");
    h.get_by_label("Compute").click();
    h.run_steps(3);
    assert!(h.query_by_label("a → bar: -1.55 dB").is_some(), "the grid names each cell");
    assert!(h.query_by_label("a → cross: -5.23 dB").is_some());

    // Ctrl+click adds a path to the plot; the CSV has both.
    h.get_by_label("a → cross: -5.23 dB").click_modifiers(egui::Modifiers::COMMAND);
    h.run_steps(2);
    assert_eq!(h.state().transmission_window.selected, vec![(0, 0), (0, 1)]);
    h.get_by_label("Group delay (ps)").click();
    h.run_steps(2);
    let csv = h.state().transmission_csv().unwrap();
    let header = csv.lines().next().unwrap();
    assert!(header.contains("a → bar group delay (ps)") && header.contains("a → cross power (dB)"), "{header}");
    assert_eq!(csv.lines().count(), 2002, "a row per wavelength");
}

#[test]
fn a_sweep_with_a_second_parameter_draws_a_map() {
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
    h.get_by_label("Use selection").click();
    h.run_steps(2);
    h.get_by_label("a second parameter (a map)").click();
    h.run_steps(2);
    // The laser's power along y.
    active_mut(&mut h).sweep.vary = Some(shine::panels::circuit::sweep::Axis {
        item: Item::Part(0),
        param: "power".into(),
        start: 1.0,
        stop: 2.0,
        points: 5,
    });
    h.run_steps(2);
    h.get_by_label("▶  Run sweep").click();
    h.run_steps(3);
    let map = active(&h).sweep.map.clone().expect("a map");
    assert!(map.error.is_none(), "{:?}", map.error);
    assert_eq!((map.xs.len(), map.ys.len()), (101, 5));
    assert!(h.query_by_label_contains("Map of").is_some(), "the heatmap describes itself");
}

#[test]
fn monte_carlo_shows_the_spread_and_yield() {
    let mut h = app();
    assert!(h.state_mut().open_example("4-channel-wdm-optical-link"));
    h.run_steps(3);
    click_menu(&mut h, "Analysis", "Monte Carlo…");
    h.state_mut().monte_carlo_window.samples = 40;
    h.get_by_label("▶  Run").click();
    h.run_steps(3);
    let mc = h.state().monte_carlo_window.result.clone().expect("ran");
    assert!(mc.error.is_none(), "{:?}", mc.error);
    assert!(mc.series.iter().all(|s| s.power.len() == 40));
    assert!(h.query_by_label_contains("Yield").is_some(), "the yield against the spec");
    assert!(h.query_by_label("Mean").is_some() && h.query_by_label("σ").is_some());
}

#[test]
fn the_optimiser_finds_a_coupling_and_applies_it() {
    use egui::{Key, Modifiers};
    let mut h = app();
    let p = active_mut(&mut h);
    p.layout.components.push(shine::panels::optics::model::PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    });
    let dc = p.layout.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (4, 0));
    p.layout.connect_feed(0, PortRef { instance: dc, port: 0 });
    p.editor.boxes.select_only(0);
    h.run_steps(3);
    click_menu(&mut h, "Analysis", "Optimise…");
    h.get_by_label("coupling").click();
    h.run_steps(2);
    assert_eq!(h.state().optimise_window.variables.len(), 1, "the selection's coupling, to adjust");
    // The coupler's bar output, as sweeps and the optimiser name it.
    let bar = {
        let ws = h.state();
        let p = ws.project.doc(ws.active.unwrap()).unwrap().panel();
        let catalog = ws.catalog();
        let names = |inst: shine::panels::optics::model::InstanceId| p.layout.instance(inst).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        shine::panels::circuit::sweep::outputs(&p.layout, p.outcome().unwrap(), &names).into_iter().map(|o| o.0).find(|n| n.ends_with(" out1"))
    };
    {
        let t = &mut h.state_mut().optimise_window.targets[0];
        t.output = bar;
        t.goal = shine::panels::workspace::GoalKind::Target;
        t.target_mw = 0.25;
    }
    h.run_steps(2);
    h.get_by_label("▶  Optimise").click();
    h.run_steps(3);
    let Some(Ok(found)) = h.state().optimise_window.result.clone() else { panic!("{:?}", h.state().optimise_window.result) };
    assert!((found.values[0] - 0.75).abs() < 1e-3 && (found.after[0] - 0.25).abs() < 1e-5, "{found:?}");

    // Apply it to the design, and undo.
    h.get_by_label("Apply to the design").click();
    h.run_steps(3);
    let coupling = |h: &Harness<'_, Workspace>| match active(h).layout.instances[0].block {
        BlockRef::Builtin(Builtin::Coupler { coupling }) => coupling,
        _ => unreachable!(),
    };
    assert!((coupling(&h) - 0.75).abs() < 1e-3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(coupling(&h), 0.5, "undone");
}
