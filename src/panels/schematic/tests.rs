//! Headless UI tests for the schematic editor.

use egui::{Event, Key, Modifiers, PointerButton};

use super::*;
use crate::panels::project::{ComponentMeta, Project};

/// Drives a schematic panel the way the workspace does, with a catalog.
struct Harness {
    ctx: egui::Context,
    time: f64,
    catalog: Catalog,
}

impl Harness {
    fn new(panel: &mut SchematicPanel, catalog: Catalog) -> Self {
        let mut h = Self {
            ctx: egui::Context::default(),
            time: 0.0,
            catalog,
        };
        h.frame(panel, Vec::new());
        h
    }

    fn frame(&mut self, panel: &mut SchematicPanel, events: Vec<Event>) {
        self.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let catalog = &self.catalog;
        let ctx = self.ctx.clone();
        self.ctx
            .run_ui(input, |ui| {
                panel.begin_frame(ui, catalog);
                egui::Panel::left("lib")
                    .exact_size(200.0)
                    .show(ui, |ui| panel.palette_ui(ui, catalog));
                egui::Panel::right("insp")
                    .exact_size(220.0)
                    .show(ui, |ui| panel.inspector_ui(ui, catalog));
                panel.canvas_ui(ui, catalog);
                panel.overlays(&ctx, catalog);
                panel.end_frame(ui);
            })
            .textures_delta
            .clear();
    }

    fn key_with(&mut self, panel: &mut SchematicPanel, key: Key, modifiers: Modifiers) {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(panel, vec![Event::ModifiersChanged(modifiers), event(true)]);
        self.frame(
            panel,
            vec![event(false), Event::ModifiersChanged(Modifiers::NONE)],
        );
    }

    fn key(&mut self, panel: &mut SchematicPanel, key: Key) {
        self.key_with(panel, key, Modifiers::NONE);
    }

    fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn move_to(&mut self, pos: Pos2, panel: &mut SchematicPanel) {
        self.frame(panel, vec![Event::PointerMoved(pos)]);
    }

    fn click(&mut self, pos: Pos2, panel: &mut SchematicPanel, button: PointerButton) {
        self.move_to(pos, panel);
        self.frame(panel, vec![Self::button(pos, button, true)]);
        self.frame(panel, vec![Self::button(pos, button, false)]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2, panel: &mut SchematicPanel) {
        self.move_to(from, panel);
        self.frame(
            panel,
            vec![Self::button(from, PointerButton::Primary, true)],
        );
        for k in 1..=6 {
            self.frame(
                panel,
                vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))],
            );
        }
        self.frame(panel, vec![Self::button(to, PointerButton::Primary, false)]);
    }
}

fn scr(panel: &SchematicPanel, world: Pos2) -> Pos2 {
    panel
        .view
        .to_screen(panel.canvas_rect().expect("drawn"), world)
}

fn b(block: Builtin) -> BlockRef {
    BlockRef::Builtin(block)
}

/// Screen positions of an instance's centre, output ports and input ports.
fn ports(panel: &SchematicPanel, catalog: &Catalog, index: usize) -> (Pos2, Vec<Pos2>, Vec<Pos2>) {
    let shape = &panel.shapes(catalog)[index];
    (
        scr(panel, shape.rect.center()),
        shape.outputs.iter().map(|p| scr(panel, *p)).collect(),
        shape.inputs.iter().map(|p| scr(panel, *p)).collect(),
    )
}

/// A laser and a detector side by side.
fn two_blocks() -> SchematicPanel {
    let mut s = Schematic::default();
    s.add(
        b(Builtin::Laser {
            amplitude: 1.0,
            phase: 0.0,
            wavelength_nm: 1550.0,
        }),
        Pos2::new(0.0, 0.0),
    );
    s.add(b(Builtin::Detector), Pos2::new(200.0, 0.0));
    SchematicPanel::with_schematic(s)
}

#[test]
fn drag_from_port_to_port_draws_a_waveguide() {
    let mut panel = two_blocks();
    let mut h = Harness::new(&mut panel, Catalog::default());
    let (_, laser_out, _) = ports(&panel, &h.catalog, 0);
    let (_, _, detector_in) = ports(&panel, &h.catalog, 1);
    h.drag(laser_out[0], detector_in[0], &mut panel);
    assert_eq!(panel.schematic.wires.len(), 1);
    assert_eq!(
        panel.selected_wire,
        Some(0),
        "the new waveguide is selected"
    );

    // Dragging backwards, from the input, works too (after removing the wire).
    panel.schematic.wires.clear();
    h.drag(detector_in[0], laser_out[0], &mut panel);
    assert_eq!(panel.schematic.wires.len(), 1);

    // Dropping on nothing does not connect.
    panel.schematic.wires.clear();
    h.drag(
        laser_out[0],
        laser_out[0] + Vec2::new(60.0, 150.0),
        &mut panel,
    );
    assert!(panel.schematic.wires.is_empty());
}

#[test]
fn picking_up_a_connected_input_moves_the_waveguide() {
    let mut panel = two_blocks();
    panel
        .schematic
        .add(b(Builtin::Detector), Pos2::new(200.0, 120.0));
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let mut h = Harness::new(&mut panel, Catalog::default());
    let (_, _, first_in) = ports(&panel, &h.catalog, 1);
    let (_, _, second_in) = ports(&panel, &h.catalog, 2);
    h.drag(first_in[0], second_in[0], &mut panel);
    assert_eq!(panel.schematic.wires.len(), 1);
    assert_eq!(panel.schematic.wires[0].to.instance, ids[2]);

    // Picking it up and letting go on empty space removes it.
    h.drag(
        second_in[0],
        second_in[0] + Vec2::new(100.0, 100.0),
        &mut panel,
    );
    assert!(panel.schematic.wires.is_empty());
}

#[test]
fn click_selects_blocks_and_waveguides() {
    let mut panel = two_blocks();
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let mut h = Harness::new(&mut panel, Catalog::default());
    let (laser, out, _) = ports(&panel, &h.catalog, 0);
    let (_, _, input) = ports(&panel, &h.catalog, 1);

    h.click(laser, &mut panel, PointerButton::Primary);
    assert_eq!(panel.selection.active(), Some(0));
    let middle = out[0].lerp(input[0], 0.5);
    h.click(middle, &mut panel, PointerButton::Primary);
    assert_eq!((panel.selected_wire, panel.selection.len()), (Some(0), 0));
    h.key(&mut panel, Key::X);
    assert!(
        panel.schematic.wires.is_empty(),
        "delete removes the selected waveguide"
    );
    assert_eq!(panel.schematic.instances.len(), 2);

    h.click(
        scr(&panel, Pos2::new(100.0, 300.0)),
        &mut panel,
        PointerButton::Primary,
    );
    assert!(panel.selection.is_empty());
}

#[test]
fn moving_blocks_snaps_and_keeps_wires() {
    let mut panel = two_blocks();
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let mut h = Harness::new(&mut panel, Catalog::default());
    let (centre, ..) = ports(&panel, &h.catalog, 1);
    let zoom = panel.view.zoom;
    h.drag(centre, centre + Vec2::new(0.0, 83.0 * zoom), &mut panel);
    let pos = panel.schematic.instances[1].pos;
    assert_eq!(pos, Pos2::new(200.0, 80.0), "snapped to the 10-unit grid");
    assert_eq!(panel.schematic.wires.len(), 1);

    // Keyboard grab with an axis lock, then cancel.
    h.move_to(centre, &mut panel);
    h.key(&mut panel, Key::G);
    h.key(&mut panel, Key::Y);
    h.move_to(centre + Vec2::new(50.0, 50.0), &mut panel);
    assert_eq!(panel.schematic.instances[1].pos.x, 200.0, "locked to Y");
    h.key(&mut panel, Key::Escape);
    assert_eq!(panel.schematic.instances[1].pos, Pos2::new(200.0, 80.0));
}

#[test]
fn box_select_duplicate_delete_undo() {
    let mut panel = two_blocks();
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let mut h = Harness::new(&mut panel, Catalog::default());
    h.drag(
        scr(&panel, Pos2::new(-20.0, -20.0)),
        scr(&panel, Pos2::new(320.0, 120.0)),
        &mut panel,
    );
    assert_eq!(panel.selection.len(), 2);

    h.move_to(scr(&panel, Pos2::new(100.0, 20.0)), &mut panel);
    h.key_with(&mut panel, Key::D, Modifiers::SHIFT);
    h.move_to(scr(&panel, Pos2::new(100.0, 220.0)), &mut panel);
    h.key(&mut panel, Key::Enter);
    assert_eq!(panel.schematic.instances.len(), 4);
    assert_eq!(
        panel.schematic.wires.len(),
        2,
        "the copy keeps its internal waveguide"
    );
    assert_eq!(panel.schematic.instances[2].pos, Pos2::new(0.0, 200.0));

    h.key_with(&mut panel, Key::Z, Modifiers::COMMAND);
    assert_eq!(panel.schematic.instances.len(), 2);
    h.key_with(&mut panel, Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(panel.schematic.instances.len(), 4);

    h.key(&mut panel, Key::A);
    h.key(&mut panel, Key::Delete);
    assert!(panel.schematic.instances.is_empty() && panel.schematic.wires.is_empty());
    h.key_with(&mut panel, Key::Z, Modifiers::COMMAND);
    assert_eq!(panel.schematic.instances.len(), 4);
}

#[test]
fn duplicate_cancel_restores_selection() {
    let mut panel = two_blocks();
    let mut h = Harness::new(&mut panel, Catalog::default());
    panel.selection.select_only(1);
    h.key_with(&mut panel, Key::D, Modifiers::SHIFT);
    assert_eq!(panel.schematic.instances.len(), 3);
    h.key(&mut panel, Key::Escape);
    assert_eq!(panel.schematic.instances.len(), 2);
    assert_eq!(panel.selection.active(), Some(1));
}

#[test]
fn placing_with_tools_menus_and_drops() {
    let mut panel = SchematicPanel::default();
    let mut h = Harness::new(&mut panel, Catalog::default());
    panel.tool = Some(b(Builtin::Mzi {
        theta: 0.0,
        phi: 0.0,
    }));
    h.click(
        scr(&panel, Pos2::new(100.0, 100.0)),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(panel.schematic.instances.len(), 1);
    let placed = panel.shapes(&h.catalog)[0].rect;
    assert!(
        placed.contains(Pos2::new(100.0, 100.0)),
        "centred on the click"
    );

    panel.tool = None;
    panel.dragging_new = Some(b(Builtin::Detector));
    let at = scr(&panel, Pos2::new(400.0, 100.0));
    h.move_to(at, &mut panel);
    h.frame(
        &mut panel,
        vec![Harness::button(at, PointerButton::Primary, false)],
    );
    assert_eq!(panel.schematic.instances.len(), 2);

    // Shift+A opens the add menu at the cursor; choosing adds there.
    h.move_to(scr(&panel, Pos2::new(250.0, 300.0)), &mut panel);
    h.key_with(&mut panel, Key::A, Modifiers::SHIFT);
    let menu = panel.menu.clone().expect("add menu");
    assert_eq!(menu.target, None);
    h.frame(&mut panel, Vec::new());
    panel.apply_menu_choice(
        &h.catalog.clone(),
        MenuChoice::Add(b(Builtin::YSplitter)),
        menu.world,
    );
    assert_eq!(panel.schematic.instances.len(), 3);
    assert!(panel.menu.is_none());

    // Right click on a block opens its context menu; Escape closes it.
    let (centre, ..) = ports(&panel, &h.catalog, 0);
    h.click(centre, &mut panel, PointerButton::Secondary);
    assert_eq!(panel.menu.as_ref().and_then(|m| m.target), Some(0));
    h.frame(&mut panel, Vec::new());
    h.key(&mut panel, Key::Escape);
    assert!(panel.menu.is_none());
}

#[test]
fn menu_choices() {
    let mut panel = two_blocks();
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let catalog = Catalog::default();
    panel.apply_menu_choice(&catalog, MenuChoice::Disconnect(1), Pos2::ZERO);
    assert!(panel.schematic.wires.is_empty());
    panel.selection.select_only(0);
    panel.apply_menu_choice(&catalog, MenuChoice::Delete, Pos2::ZERO);
    assert_eq!(panel.schematic.instances.len(), 1);
    panel.apply_menu_choice(&catalog, MenuChoice::Inspect(DocId(9)), Pos2::ZERO);
    assert_eq!(panel.requests, vec![Request::Inspect(DocId(9))]);
    panel.selection.select_only(0);
    panel.apply_menu_choice(&catalog, MenuChoice::Duplicate, Pos2::ZERO);
    assert!(panel.grab.is_some());
}

/// A project with one published component, and a schematic that uses it.
fn project_with_component() -> (Project, DocId, DocId) {
    let mut p = Project::new();
    let mut inner = Schematic::default();
    let i = inner.add(b(Builtin::InputPin { name: "in".into() }), Pos2::ZERO);
    let o = inner.add(
        b(Builtin::OutputPin { name: "out".into() }),
        Pos2::new(200.0, 0.0),
    );
    inner.connect(
        PortRef {
            instance: i,
            port: 0,
        },
        PortRef {
            instance: o,
            port: 0,
        },
    );
    let component = p.add_schematic("Wire through", inner);
    p.doc_mut(component).unwrap().component = Some(ComponentMeta {
        initials: "WT".into(),
        description: "Passes light straight through.".into(),
    });
    let mut top = Schematic::default();
    top.add(BlockRef::Component(component), Pos2::ZERO);
    let top_id = p.add_schematic("Top", top);
    (p, component, top_id)
}

#[test]
fn components_open_with_tab_or_double_click() {
    let (p, component, top) = project_with_component();
    let mut panel = SchematicPanel::with_schematic(match &p.doc(top).unwrap().content {
        crate::panels::project::Content::Schematic(s) => s.schematic.clone(),
        _ => unreachable!(),
    });
    panel.doc_id = Some(top);
    let mut h = Harness::new(&mut panel, p.catalog());
    let (centre, ..) = ports(&panel, &h.catalog, 0);
    h.click(centre, &mut panel, PointerButton::Primary);
    h.click(centre, &mut panel, PointerButton::Primary);
    assert_eq!(panel.requests, vec![Request::Inspect(component)]);
    panel.requests.clear();
    h.key(&mut panel, Key::Tab);
    assert_eq!(panel.requests, vec![Request::Inspect(component)]);

    // Tab on a built-in block explains instead.
    panel.requests.clear();
    panel
        .schematic
        .add(b(Builtin::Detector), Pos2::new(300.0, 0.0));
    panel.selection.select_only(1);
    h.key(&mut panel, Key::Tab);
    assert!(panel.requests.is_empty());
    assert!(
        panel
            .status
            .as_ref()
            .unwrap()
            .0
            .contains("Select a component")
    );

    // Hovering shows the saved description.
    let text = panel.hover_text(&h.catalog, Hit::Instance(0)).unwrap();
    assert!(
        text.contains("Passes light straight through.") && text.contains("Tab"),
        "{text}"
    );
}

#[test]
fn a_component_cannot_be_placed_inside_itself() {
    let (p, component, top) = project_with_component();
    let catalog = p.catalog();
    let mut panel = SchematicPanel {
        doc_id: Some(component),
        ..Default::default()
    };
    assert_eq!(
        panel.place(&catalog, BlockRef::Component(component), Pos2::ZERO),
        None
    );
    assert!(panel.status.as_ref().unwrap().0.contains("itself"));
    assert_eq!(
        panel.place(&catalog, BlockRef::Component(top), Pos2::ZERO),
        None,
        "top uses the component"
    );
    panel.doc_id = Some(top);
    assert!(
        panel
            .place(&catalog, BlockRef::Component(component), Pos2::ZERO)
            .is_some()
    );
}

#[test]
fn library_search() {
    let (p, ..) = project_with_component();
    let catalog = p.catalog();
    let all = SchematicPanel::library_items(&catalog, "");
    assert_eq!(all.len(), Builtin::palette().len() + 1);
    assert!(matches!(all.last(), Some((None, BlockRef::Component(_)))));
    let rings = SchematicPanel::library_items(&catalog, "RING");
    assert_eq!(rings.len(), 2);
    let by_description = SchematicPanel::library_items(&catalog, "straight through");
    assert_eq!(by_description.len(), 1);
    assert!(SchematicPanel::library_items(&catalog, "zzz").is_empty());
}

#[test]
fn hover_texts_describe_ports_and_waveguides() {
    let mut panel = two_blocks();
    let ids: Vec<_> = panel.schematic.instances.iter().map(|i| i.id).collect();
    panel.schematic.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    let catalog = Catalog::default();
    let mut project = Project::new();
    let id = project.add_schematic("S", panel.schematic.clone());
    panel.sim = Some(project.simulate_schematic(id));

    let port = panel
        .hover_text(
            &catalog,
            Hit::Output(PortRef {
                instance: ids[0],
                port: 0,
            }),
        )
        .unwrap();
    assert!(
        port.contains("LS output") && port.contains("Power 1.0000"),
        "{port}"
    );
    let input = panel
        .hover_text(
            &catalog,
            Hit::Input(PortRef {
                instance: ids[1],
                port: 0,
            }),
        )
        .unwrap();
    assert!(input.contains("PD input"));
    let wire = panel.hover_text(&catalog, Hit::Wire(0)).unwrap();
    assert!(
        wire.contains("ideal") && wire.contains("1550.0 nm: power 1.0000"),
        "{wire}"
    );
    panel.schematic.wires[0].length_um = Some(3.0);
    assert!(
        panel
            .hover_text(&catalog, Hit::Wire(0))
            .unwrap()
            .contains("3.00 µm (fixed)")
    );
    panel.schematic.wire_settings.model = WireModel::Geometric { um_per_unit: 1.0 };
    panel.schematic.wires[0].length_um = None;
    assert!(
        panel
            .hover_text(&catalog, Hit::Wire(0))
            .unwrap()
            .contains("from drawing")
    );
    assert_eq!(panel.hover_text(&catalog, Hit::Empty), None);
    assert_eq!(panel.hover_text(&catalog, Hit::Wire(9)), None);
}

#[test]
fn inspector_and_library_render_every_block() {
    let (p, component, _) = project_with_component();
    let mut s = Schematic::default();
    for (k, block) in Builtin::palette().into_iter().enumerate() {
        s.add(
            b(block),
            Pos2::new((k % 5) as f32 * 120.0, (k / 5) as f32 * 120.0),
        );
    }
    s.add(BlockRef::Component(component), Pos2::new(0.0, 600.0));
    s.add(BlockRef::Component(DocId(404)), Pos2::new(200.0, 600.0));
    let ids: Vec<_> = s.instances.iter().map(|i| i.id).collect();
    s.connect(
        PortRef {
            instance: ids[0],
            port: 0,
        },
        PortRef {
            instance: ids[1],
            port: 0,
        },
    );
    // A wire whose port no longer exists is drawn as broken, not a crash.
    s.wires.push(model::Wire {
        from: PortRef {
            instance: ids[0],
            port: 5,
        },
        to: PortRef {
            instance: ids[4],
            port: 0,
        },
        length_um: None,
    });
    let mut panel = SchematicPanel::with_schematic(s.clone());
    let mut project = p;
    let id = project.add_schematic("All", s);
    panel.sim = Some(project.simulate_schematic(id));
    let mut h = Harness::new(&mut panel, project.catalog());
    for i in 0..panel.schematic.instances.len() {
        panel.selection.select_only(i);
        h.frame(&mut panel, Vec::new());
    }
    panel.selection.clear();
    panel.selected_wire = Some(0);
    h.frame(&mut panel, Vec::new());
    panel.schematic.wire_settings.model = WireModel::Geometric { um_per_unit: 1.0 };
    h.frame(&mut panel, Vec::new());
    panel.search = "dft".into();
    h.frame(&mut panel, Vec::new());
    assert!(
        panel.sim.as_ref().unwrap().error.is_some(),
        "the broken wire is reported"
    );
}

#[test]
fn status_text_follows_the_mode() {
    let mut panel = two_blocks();
    assert!(panel.status_text().contains("drag from a port"));
    panel.selection.select_only(0);
    assert!(panel.status_text().contains("1 selected"));
    panel.selected_wire = Some(0);
    assert!(panel.status_text().contains("waveguide selected"));
    panel.apply(&Catalog::default(), Action::Grab);
    assert!(panel.status_text().starts_with("Move"));
    panel.apply(&Catalog::default(), Action::Cancel);
    panel.apply(&Catalog::default(), Action::RotateCw);
    assert!(panel.status_text().contains("inputs on the left"));
}

#[test]
fn host_requests() {
    let mut panel = SchematicPanel::default();
    let catalog = Catalog::default();
    for action in [
        Action::ToggleToolbar,
        Action::ToggleProperties,
        Action::ShowShortcuts,
        Action::Save,
        Action::Open,
        Action::New,
    ] {
        panel.apply(&catalog, action);
    }
    assert_eq!(
        panel.requests,
        vec![
            Request::ToggleLibrary,
            Request::ToggleInspector,
            Request::ShowShortcuts,
            Request::Save,
            Request::Open,
            Request::New
        ]
    );
    panel.apply(&catalog, Action::ToggleSimulation);
    assert!(!panel.simulation_active);
    panel.apply(&catalog, Action::Undo);
    assert!(panel.status.as_ref().unwrap().0.contains("Nothing to undo"));
    panel.apply(&catalog, Action::Redo);
    assert!(panel.status.as_ref().unwrap().0.contains("Nothing to redo"));
    panel.apply(&catalog, Action::Duplicate);
    assert!(panel.status.as_ref().unwrap().0.contains("duplicate"));
}

#[test]
fn frame_all_fits_blocks() {
    let mut panel = two_blocks();
    panel
        .schematic
        .add(b(Builtin::Detector), Pos2::new(2000.0, 900.0));
    let mut h = Harness::new(&mut panel, Catalog::default());
    h.key(&mut panel, Key::Home);
    let canvas = panel.canvas_rect().unwrap();
    for shape in panel.shapes(&h.catalog) {
        assert!(canvas.contains(scr(&panel, shape.rect.center())));
    }
    panel.selection.select_only(0);
    h.key(&mut panel, Key::Period);
    assert!(scr(&panel, panel.shapes(&h.catalog)[0].rect.center()).distance(canvas.center()) < 1.0);
    panel.schematic = Schematic::default();
    h.key(&mut panel, Key::Home);
    assert_eq!(panel.view, View::default());
}

#[test]
fn builtin_params_cover_every_block() {
    let ctx = egui::Context::default();
    ctx.run_ui(Default::default(), |ui| {
        for mut block in Builtin::palette() {
            builtin_params_ui(ui, &mut block);
        }
        let mut tree = Builtin::SwitchTree {
            levels: 1,
            address: 7,
        };
        builtin_params_ui(ui, &mut tree);
        assert_eq!(
            tree,
            Builtin::SwitchTree {
                levels: 1,
                address: 1
            },
            "address clamped to the outputs"
        );
    })
    .textures_delta
    .clear();
}

#[test]
fn shortcuts_survive_clicking_library_rows_and_tab() {
    let mut panel = two_blocks();
    let mut h = Harness::new(&mut panel, Catalog::default());
    // Click a library row (a focusable widget) inside the left panel.
    h.click(Pos2::new(100.0, 60.0), &mut panel, PointerButton::Primary);
    h.key(&mut panel, Key::A);
    assert_eq!(
        panel.selection.len(),
        2,
        "A still selects all after clicking the library"
    );
    // Tab twice must not park focus in the search field.
    h.key(&mut panel, Key::Tab);
    h.key(&mut panel, Key::Tab);
    assert!(!h.ctx.text_edit_focused());
    h.key_with(&mut panel, Key::A, Modifiers::ALT);
    assert!(panel.selection.is_empty());
}

#[test]
fn typing_in_the_search_field_does_not_trigger_shortcuts() {
    let mut panel = two_blocks();
    let mut h = Harness::new(&mut panel, Catalog::default());
    // The search field is the first widget of the library.
    h.click(Pos2::new(100.0, 12.0), &mut panel, PointerButton::Primary);
    assert!(h.ctx.text_edit_focused());
    h.frame(&mut panel, vec![Event::Text("x".into())]);
    h.key(&mut panel, Key::X);
    h.key(&mut panel, Key::A);
    assert_eq!(panel.schematic.instances.len(), 2);
    assert!(panel.selection.is_empty());
    assert_eq!(panel.search, "x");
}

#[test]
fn drags_released_outside_the_canvas_finish() {
    let mut panel = two_blocks();
    let mut h = Harness::new(&mut panel, Catalog::default());
    // Release over the inspector panel: the box still selects, nothing gets stuck.
    let from = scr(&panel, Pos2::new(-20.0, -20.0));
    h.drag(from, Pos2::new(990.0, 690.0), &mut panel);
    assert_eq!(panel.selection.len(), 2);
    assert!(panel.status_text().contains("selected"));
    // A waveguide dropped outside the canvas is not connected.
    let (_, out, _) = ports(&panel, &h.catalog, 0);
    h.drag(out[0], Pos2::new(990.0, 300.0), &mut panel);
    assert!(panel.schematic.wires.is_empty());
    assert!(!panel.status_text().starts_with("Drop on a port"));
}
