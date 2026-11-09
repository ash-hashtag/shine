//! Headless UI tests: real egui input events drive the panel.

use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Vec2};

use super::*;
use crate::panels::circuit::sim::Outcome;
use model::{cell_center, cell_of};

struct Harness {
    ctx: egui::Context,
    time: f64,
    modifiers: Modifiers,
    sent_modifiers: Modifiers,
}

impl Harness {
    fn new(panel: &mut OpticsPanel) -> Self {
        let mut h = Self {
            ctx: egui::Context::default(),
            time: 0.0,
            modifiers: Modifiers::NONE,
            sent_modifiers: Modifiers::NONE,
        };
        h.frame(panel, Vec::new());
        h
    }

    fn frame(&mut self, panel: &mut OpticsPanel, mut events: Vec<Event>) {
        self.time += 0.05;
        if self.modifiers != self.sent_modifiers {
            events.insert(0, Event::ModifiersChanged(self.modifiers));
            self.sent_modifiers = self.modifiers;
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.ctx
            .run_ui(input, |ui| panel.ui(ui))
            .textures_delta
            .clear();
    }

    fn key_with(&mut self, panel: &mut OpticsPanel, key: Key, modifiers: Modifiers) {
        self.modifiers = modifiers;
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(panel, vec![event(true)]);
        self.frame(panel, vec![event(false)]);
        self.modifiers = Modifiers::NONE;
    }

    fn key(&mut self, panel: &mut OpticsPanel, key: Key) {
        self.key_with(panel, key, Modifiers::NONE);
    }

    fn button(&self, pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: self.modifiers,
        }
    }

    fn move_to(&mut self, pos: Pos2, panel: &mut OpticsPanel) {
        self.frame(panel, vec![Event::PointerMoved(pos)]);
    }

    fn click(&mut self, pos: Pos2, panel: &mut OpticsPanel, button: PointerButton) {
        self.move_to(pos, panel);
        let press = self.button(pos, button, true);
        self.frame(panel, vec![press]);
        let release = self.button(pos, button, false);
        self.frame(panel, vec![release]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2, panel: &mut OpticsPanel, button: PointerButton) {
        self.move_to(from, panel);
        let press = self.button(from, button, true);
        self.frame(panel, vec![press]);
        for k in 1..=6 {
            let p = from + (to - from) * (k as f32 / 6.0);
            self.frame(panel, vec![Event::PointerMoved(p)]);
        }
        let release = self.button(to, button, false);
        self.frame(panel, vec![release]);
    }

    fn wheel(
        &mut self,
        at: Pos2,
        panel: &mut OpticsPanel,
        delta: Vec2,
        unit: MouseWheelUnit,
        source: MouseWheelSource,
    ) {
        let modifiers = self.modifiers;
        self.frame(
            panel,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel {
                    unit,
                    delta,
                    phase: egui::TouchPhase::Move,
                    source,
                    modifiers,
                },
            ],
        );
    }
}

fn scr(panel: &OpticsPanel, world: Pos2) -> Pos2 {
    panel
        .view
        .to_screen(panel.canvas_rect().expect("canvas drawn"), world)
}

fn place(panel: &mut OpticsPanel, i: i32, j: i32, kind: OpticComponentKind) -> usize {
    panel
        .editor
        .place(&mut panel.layout, kind, cell_center(i, j), &editor::no_ports)
        .expect("free cell")
}

fn cells(panel: &OpticsPanel) -> Vec<(i32, i32)> {
    panel
        .layout
        .components
        .iter()
        .map(|c| cell_of(c.pos))
        .collect()
}

const DETECTOR: OpticComponentKind = crate::panels::optics::model::DETECTOR;
const LENS: OpticComponentKind = OpticComponentKind::Lens;

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    }
}

#[test]
fn defaults_and_layout_of_panels() {
    let mut panel = OpticsPanel::default();
    assert!(panel.layout.components.is_empty() && panel.show_toolbar && panel.show_properties);
    assert_eq!(panel.cell_um, 10.0);
    Harness::new(&mut panel);
    let canvas = panel.canvas_rect().unwrap();
    // The canvas sits between the toolbar and the properties panel.
    assert!(
        canvas.left() >= 200.0 && canvas.right() <= 1280.0 - 200.0,
        "{canvas:?}"
    );
    assert!(panel.header_text().contains("100%"));
}

#[test]
fn click_places_selects_and_deselects() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    panel.tool = Some(Tool::Place(Placeable::Part(DETECTOR)));
    let a = scr(&panel, cell_center(2, 2));
    h.click(a, &mut panel, PointerButton::Primary);
    h.click(a, &mut panel, PointerButton::Primary);
    assert_eq!(
        cells(&panel),
        vec![(2, 2)],
        "second click selects, does not stack"
    );
    assert_eq!(panel.editor.selection.active(), Some(0));

    let b = scr(&panel, cell_center(5, 2));
    h.click(b, &mut panel, PointerButton::Primary);
    assert_eq!(cells(&panel), vec![(2, 2), (5, 2)]);

    panel.tool = None;
    h.click(a, &mut panel, PointerButton::Primary);
    h.modifiers = Modifiers::SHIFT;
    h.click(b, &mut panel, PointerButton::Primary);
    h.modifiers = Modifiers::NONE;
    assert_eq!(panel.editor.selection.len(), 2, "shift-click extends");

    h.click(
        scr(&panel, cell_center(9, 9)),
        &mut panel,
        PointerButton::Primary,
    );
    assert!(
        panel.editor.selection.is_empty(),
        "click on empty space deselects"
    );
}

#[test]
fn double_click_rotates() {
    let mut panel = OpticsPanel::default();
    let i = place(&mut panel, 1, 1, OpticComponentKind::palette()[2]);
    let mut h = Harness::new(&mut panel);
    let at = scr(&panel, cell_center(1, 1));
    h.click(at, &mut panel, PointerButton::Primary);
    h.click(at, &mut panel, PointerButton::Primary);
    assert_ne!(
        panel.layout.components[i].kind,
        OpticComponentKind::palette()[2]
    );
}

#[test]
fn box_select_by_dragging_empty_space() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 1, 1, LENS);
    place(&mut panel, 2, 1, LENS);
    place(&mut panel, 8, 8, LENS);
    panel.editor.selection.clear();
    let mut h = Harness::new(&mut panel);
    let from = scr(&panel, cell_center(0, 0));
    let to = scr(&panel, cell_center(3, 2));
    h.drag(from, to, &mut panel, PointerButton::Primary);
    assert_eq!(
        panel.editor.selection.iter().collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn dragging_a_component_moves_it() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 1, 1, LENS);
    place(&mut panel, 4, 1, LENS);
    let mut h = Harness::new(&mut panel);
    let from = scr(&panel, cell_center(1, 1));
    h.drag(
        from,
        scr(&panel, cell_center(1, 3)),
        &mut panel,
        PointerButton::Primary,
    );
    assert!(panel.editor.grab.is_none());
    assert_eq!(cells(&panel), vec![(1, 3), (4, 1)]);

    // Dropping onto an occupied cell is refused.
    let from = scr(&panel, cell_center(1, 3));
    h.drag(
        from,
        scr(&panel, cell_center(4, 1)),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(cells(&panel), vec![(1, 3), (4, 1)]);
    assert!(panel.status.as_ref().unwrap().0.contains("occupied"));
}

#[test]
fn grab_with_keyboard_axis_lock_confirm_and_cancel() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 0, LENS);
    let mut h = Harness::new(&mut panel);
    h.move_to(scr(&panel, cell_center(0, 0)), &mut panel);
    h.key(&mut panel, Key::G);
    assert!(panel.editor.grab.is_some());
    h.key(&mut panel, Key::X);
    h.move_to(scr(&panel, cell_center(3, 2)), &mut panel);
    assert!(
        panel.header_text().starts_with("Grab  Δ 3, 0 cells  [X]"),
        "{}",
        panel.header_text()
    );
    h.key(&mut panel, Key::Enter);
    assert!(panel.editor.grab.is_none());
    assert_eq!(cells(&panel), vec![(3, 0)]);

    h.key(&mut panel, Key::G);
    h.move_to(scr(&panel, cell_center(6, 6)), &mut panel);
    h.key(&mut panel, Key::Escape);
    assert_eq!(cells(&panel), vec![(3, 0)], "escape cancels");

    h.key(&mut panel, Key::G);
    h.move_to(scr(&panel, cell_center(5, 5)), &mut panel);
    h.click(
        scr(&panel, cell_center(5, 5)),
        &mut panel,
        PointerButton::Secondary,
    );
    assert_eq!(cells(&panel), vec![(3, 0)], "right click cancels");

    // Grab is relative to where the mouse was when G was pressed.
    h.move_to(scr(&panel, cell_center(3, 0)), &mut panel);
    h.key(&mut panel, Key::G);
    h.move_to(scr(&panel, cell_center(3, 4)), &mut panel);
    h.click(
        scr(&panel, cell_center(3, 4)),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(cells(&panel), vec![(3, 4)], "left click confirms");
}

#[test]
fn selection_and_edit_shortcuts() {
    let mut panel = OpticsPanel::default();
    for i in 0..3 {
        place(&mut panel, i, 0, OpticComponentKind::palette()[0]);
    }
    let mut h = Harness::new(&mut panel);
    h.key(&mut panel, Key::A);
    assert_eq!(panel.editor.selection.len(), 3);
    h.key_with(&mut panel, Key::A, Modifiers::ALT);
    assert!(panel.editor.selection.is_empty());
    panel.editor.selection.select_only(0);
    h.key_with(&mut panel, Key::I, ctrl());
    assert_eq!(
        panel.editor.selection.iter().collect::<Vec<_>>(),
        vec![1, 2]
    );

    let before = panel.layout.components[1].kind;
    h.key(&mut panel, Key::R);
    assert_ne!(panel.layout.components[1].kind, before);
    h.key_with(&mut panel, Key::R, Modifiers::SHIFT);
    assert_eq!(panel.layout.components[1].kind, before);

    h.key(&mut panel, Key::X);
    assert_eq!(cells(&panel), vec![(0, 0)]);
    h.key_with(&mut panel, Key::Z, ctrl());
    assert_eq!(panel.layout.components.len(), 3, "undo delete");
    h.key_with(
        &mut panel,
        Key::Z,
        Modifiers {
            shift: true,
            ..ctrl()
        },
    );
    assert_eq!(panel.layout.components.len(), 1, "redo delete");
    h.key_with(&mut panel, Key::Y, ctrl());
    assert!(panel.status.as_ref().unwrap().0.contains("Nothing to redo"));

    panel.editor.selection.select_only(0);
    h.key(&mut panel, Key::Delete);
    assert!(panel.layout.components.is_empty());
    h.key(&mut panel, Key::G);
    assert!(
        panel
            .status
            .as_ref()
            .unwrap()
            .0
            .contains("Nothing selected")
    );
    h.key_with(&mut panel, Key::D, Modifiers::SHIFT);
    assert!(panel.status.as_ref().unwrap().0.contains("duplicate"));
}

#[test]
fn duplicate_follows_the_mouse() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 0, DETECTOR);
    let mut h = Harness::new(&mut panel);
    h.move_to(scr(&panel, cell_center(0, 0)), &mut panel);
    h.key_with(&mut panel, Key::D, Modifiers::SHIFT);
    assert_eq!(panel.layout.components.len(), 2);
    h.move_to(scr(&panel, cell_center(0, 2)), &mut panel);
    h.key(&mut panel, Key::Enter);
    assert_eq!(cells(&panel), vec![(0, 0), (0, 2)]);
    // Undo removes the copy in one step.
    h.key_with(&mut panel, Key::Z, ctrl());
    assert_eq!(cells(&panel), vec![(0, 0)]);
}

#[test]
fn view_toggles_and_framing() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    assert!(panel.simulation_active, "on by default");
    h.key(&mut panel, Key::Space);
    assert!(!panel.simulation_active);
    h.key(&mut panel, Key::T);
    h.key(&mut panel, Key::N);
    assert!(!panel.show_toolbar && !panel.show_properties);
    let wide = panel.canvas_rect().unwrap();
    assert!(
        wide.width() > 1200.0,
        "canvas takes the full width: {wide:?}"
    );
    h.key(&mut panel, Key::F1);
    assert!(panel.show_shortcuts);

    place(&mut panel, -30, -5, LENS);
    place(&mut panel, 30, 5, LENS);
    h.key(&mut panel, Key::Home);
    let canvas = panel.canvas_rect().unwrap();
    assert!(panel.view.zoom < 1.0);
    for c in &panel.layout.components {
        assert!(canvas.contains(scr(&panel, c.pos)));
    }
    panel.editor.selection.select_only(1);
    h.key(&mut panel, Key::Period);
    assert!(
        canvas
            .center()
            .distance(scr(&panel, panel.layout.components[1].pos))
            < 1.0
    );

    // Framing an empty layout resets the view.
    panel.layout = Layout::default();
    h.key(&mut panel, Key::Home);
    assert_eq!(panel.view, View::default());
}

#[test]
fn mouse_navigation() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    let canvas = panel.canvas_rect().unwrap();
    let at = canvas.center();

    let pan = panel.view.pan;
    h.drag(
        at,
        at + Vec2::new(60.0, -30.0),
        &mut panel,
        PointerButton::Middle,
    );
    assert!(
        (panel.view.pan - pan - Vec2::new(60.0, -30.0)).length() < 1.0,
        "middle drag pans"
    );

    h.modifiers = ctrl();
    h.drag(
        at,
        at + Vec2::new(0.0, -50.0),
        &mut panel,
        PointerButton::Middle,
    );
    h.modifiers = Modifiers::NONE;
    assert!(panel.view.zoom > 1.0, "ctrl+middle drag zooms");

    let zoom = panel.view.zoom;
    let world = panel.view.to_world(canvas, at);
    h.wheel(
        at,
        &mut panel,
        Vec2::new(0.0, 1.0),
        MouseWheelUnit::Line,
        MouseWheelSource::Wheel,
    );
    assert!(panel.view.zoom > zoom, "wheel zooms in");
    assert!(
        panel.view.to_world(canvas, at).distance(world) < 1e-2,
        "around the cursor"
    );

    let pan = panel.view.pan;
    h.wheel(
        at,
        &mut panel,
        Vec2::new(5.0, 7.0),
        MouseWheelUnit::Point,
        MouseWheelSource::Trackpad,
    );
    assert!(
        (panel.view.pan - pan - Vec2::new(5.0, 7.0)).length() < 1e-3,
        "trackpad pans"
    );

    let pan = panel.view.pan;
    h.modifiers = ctrl();
    h.wheel(
        at,
        &mut panel,
        Vec2::new(0.0, 1.0),
        MouseWheelUnit::Line,
        MouseWheelSource::Wheel,
    );
    h.modifiers = Modifiers::SHIFT;
    h.wheel(
        at,
        &mut panel,
        Vec2::new(0.0, 1.0),
        MouseWheelUnit::Line,
        MouseWheelSource::Wheel,
    );
    h.modifiers = Modifiers::NONE;
    assert!((panel.view.pan - pan - Vec2::new(40.0, 40.0)).length() < 1e-3);

    let zoom = panel.view.zoom;
    h.frame(&mut panel, vec![Event::PointerMoved(at), Event::Zoom(0.5)]);
    assert!((panel.view.zoom - zoom * 0.5).abs() < 1e-5, "pinch zooms");
    h.wheel(
        at,
        &mut panel,
        Vec2::new(0.0, 0.2),
        MouseWheelUnit::Page,
        MouseWheelSource::Unknown,
    );
}

#[test]
fn context_and_add_menus() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 2, 2, LENS);
    panel.editor.selection.clear();
    let mut h = Harness::new(&mut panel);
    let at = scr(&panel, cell_center(2, 2));
    h.click(at, &mut panel, PointerButton::Secondary);
    let menu = panel.menu.clone().expect("context menu");
    assert_eq!(menu.target, Some(Hit::Part(0)));
    assert_eq!(
        panel.editor.selection.active(),
        Some(0),
        "right click selects"
    );

    // Clicking the canvas elsewhere closes it.
    h.click(
        scr(&panel, cell_center(14, 2)),
        &mut panel,
        PointerButton::Primary,
    );
    assert!(panel.menu.is_none());

    h.move_to(scr(&panel, cell_center(4, 4)), &mut panel);
    h.key_with(&mut panel, Key::A, Modifiers::SHIFT);
    let menu = panel.menu.clone().expect("add menu");
    assert_eq!((menu.target, cell_of(menu.world)), (None, (4, 4)));
    h.key(&mut panel, Key::Escape);
    assert!(panel.menu.is_none());
}

#[test]
fn menu_choices() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 0, OpticComponentKind::palette()[0]);
    let before = panel.layout.components[0].kind;
    panel.apply_menu_choice(MenuChoice::Rotate(1), cell_center(0, 0));
    assert_ne!(panel.layout.components[0].kind, before);
    panel.apply_menu_choice(MenuChoice::Rotate(-1), cell_center(0, 0));
    assert_eq!(panel.layout.components[0].kind, before);

    panel.apply_menu_choice(MenuChoice::Add(Placeable::Part(DETECTOR)), cell_center(3, 0));
    assert_eq!(cells(&panel), vec![(0, 0), (3, 0)]);
    panel.apply_menu_choice(MenuChoice::Add(Placeable::Part(DETECTOR)), cell_center(3, 0));
    assert!(panel.status.as_ref().unwrap().0.contains("room"));

    panel.apply_menu_choice(MenuChoice::Duplicate, cell_center(3, 0));
    assert!(panel.editor.grab.is_some());
    panel.apply(Action::Cancel);
    assert_eq!(panel.layout.components.len(), 2);
    assert_eq!(
        panel.editor.selection.active(),
        Some(1),
        "original re-selected"
    );

    panel.apply_menu_choice(MenuChoice::Delete, cell_center(3, 0));
    assert_eq!(cells(&panel), vec![(0, 0)]);

    panel.layout.lines = vec![
        Line {
            start: cell_center(5, 5),
            end: cell_center(7, 5),
        },
        Line {
            start: cell_center(1, 1),
            end: cell_center(2, 2),
        },
    ];
    panel.apply_menu_choice(MenuChoice::DeleteLines, cell_center(7, 5));
    assert_eq!(panel.layout.lines.len(), 1);
    assert!(panel.menu.is_none());
}

#[test]
fn rendered_menu_lists_actions() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 2, 2, LENS);
    panel.layout.lines.push(Line {
        start: cell_center(2, 2),
        end: cell_center(4, 2),
    });
    let mut h = Harness::new(&mut panel);
    h.click(
        scr(&panel, cell_center(2, 2)),
        &mut panel,
        PointerButton::Secondary,
    );
    h.frame(&mut panel, Vec::new());
    assert!(panel.menu.is_some(), "menu stays open without input");
}

#[test]
fn palette_drag_and_line_tool() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    panel.dragging_new = Some(Placeable::Part(DETECTOR));
    let target = scr(&panel, cell_center(3, 1));
    h.move_to(target, &mut panel);
    let release = h.button(target, PointerButton::Primary, false);
    h.frame(&mut panel, vec![release]);
    assert_eq!(cells(&panel), vec![(3, 1)]);
    assert!(panel.dragging_new.is_none());

    panel.tool = Some(Tool::DrawLine);
    let from = scr(&panel, cell_center(0, 4));
    h.drag(
        from,
        scr(&panel, cell_center(4, 4)),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(
        panel.layout.lines,
        vec![Line {
            start: cell_center(0, 4),
            end: cell_center(4, 4)
        }]
    );
    // A zero-length line is ignored.
    h.drag(
        from,
        from + Vec2::new(8.0, 0.0),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(panel.layout.lines.len(), 1);
    h.key(&mut panel, Key::Escape);
    assert!(panel.tool.is_none());
}

#[test]
fn undo_waits_for_the_interaction_to_finish() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    let at = scr(&panel, cell_center(9, 9));
    h.move_to(at, &mut panel);
    let press = h.button(at, PointerButton::Primary, true);
    h.frame(&mut panel, vec![press]);
    // Like a slider drag: the layout changes while the button is held.
    place(&mut panel, 0, 0, LENS);
    h.frame(&mut panel, Vec::new());
    assert!(!panel.editor.history.can_undo());
    let release = h.button(at, PointerButton::Primary, false);
    h.frame(&mut panel, vec![release]);
    assert!(panel.editor.history.can_undo());
}

#[test]
fn simulation_readouts_and_examples() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    for i in 0..examples::layout_examples().len() {
        panel.load_example(i);
        h.frame(&mut panel, Vec::new());
        assert!(panel.simulation_active && panel.sim.is_some());
        // Framed so every component is visible.
        let canvas = panel.canvas_rect().unwrap();
        for c in &panel.layout.components {
            assert!(canvas.contains(scr(&panel, c.pos)));
        }
    }
    // Last example: 1→4 fan-out, every detector reads a quarter.
    let detectors: Vec<usize> = (0..panel.layout.components.len())
        .filter(|&i| panel.layout.components[i].kind == DETECTOR)
        .collect();
    for &d in &detectors {
        assert!((panel.detector_reading(d).unwrap() - 0.25).abs() < 1e-6);
    }
    assert_eq!(panel.detector_reading(0), None, "laser is not a detector");
    panel.editor.selection.select_only(detectors[0]);
    h.frame(&mut panel, Vec::new());
    panel.load_example(999);
    assert_eq!(
        panel.layout.components.len(),
        8,
        "unknown example is ignored"
    );

    // The cache is only rebuilt when inputs change.
    panel.sim.as_mut().unwrap().outcome.beams.clear();
    panel.refresh_simulation();
    assert!(panel.sim.as_ref().unwrap().outcome.beams.is_empty());
    panel.cell_um *= 2.0;
    panel.refresh_simulation();
    assert!(!panel.sim.as_ref().unwrap().outcome.beams.is_empty());

    // Hovering a beam shows its tooltip without panicking.
    let laser = panel.layout.components[0].pos;
    h.move_to(scr(&panel, laser + Vec2::new(GRID, 0.0)), &mut panel);
}

#[test]
fn properties_for_every_kind_and_errors() {
    let mut panel = OpticsPanel::default();
    for (k, kind) in OpticComponentKind::palette().into_iter().enumerate() {
        place(&mut panel, k as i32, 0, kind);
    }
    panel.simulation_active = true;
    let mut h = Harness::new(&mut panel);
    for i in 0..panel.layout.components.len() {
        panel.editor.selection.select_only(i);
        h.frame(&mut panel, Vec::new());
    }
    panel.editor.selection.select_all(11);
    h.frame(&mut panel, Vec::new());

    let key = panel.sim.as_ref().unwrap().key;
    panel.sim = Some(SimCache {
        key,
        outcome: Outcome::failed("boom"),
    });
    h.frame(&mut panel, Vec::new());
    assert_eq!(
        panel.sim.as_ref().unwrap().outcome.error.as_deref(),
        Some("boom")
    );
}

#[test]
fn windows_render() {
    let mut panel = OpticsPanel {
        show_examples: true,
        show_shortcuts: true,
        ..Default::default()
    };
    let mut h = Harness::new(&mut panel);
    h.frame(&mut panel, Vec::new());
    panel.examples_tab = ExamplesTab::Chips;
    h.frame(&mut panel, Vec::new());
    h.frame(&mut panel, Vec::new());
    assert!(panel.show_examples && panel.show_shortcuts);
}

#[test]
fn save_load_and_new() {
    let dir = std::env::temp_dir().join(format!("shine-panel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("layout.json");

    let mut panel = OpticsPanel::default();
    panel.load_example(0);
    panel.cell_um = 3.25;
    panel.save_to(&path).unwrap();

    let mut other = OpticsPanel::default();
    other.load_from(&path).unwrap();
    assert_eq!(other.layout, panel.layout);
    assert_eq!(other.cell_um, 3.25);
    assert!(other.load_from(&dir.join("missing.json")).is_err());
    std::fs::write(dir.join("bad.json"), "{").unwrap();
    assert!(other.load_from(&dir.join("bad.json")).is_err());
    assert_eq!(
        other.layout, panel.layout,
        "failed loads leave the layout alone"
    );
    assert!(panel.save_to(&dir.join("no/such/dir/x.json")).is_err());

    other.apply(Action::New);
    assert!(other.layout.components.is_empty());
    assert!(other.status.as_ref().unwrap().0.contains("New layout"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_expires() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    panel.apply(Action::Undo);
    assert!(panel.status.is_some());
    for _ in 0..70 {
        h.frame(&mut panel, Vec::new());
    }
    assert!(panel.status.is_none());
}

#[test]
fn header_describes_tool_and_cell() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    panel.tool = Some(Tool::Place(Placeable::Part(DETECTOR)));
    // Pan so a negative cell is on screen.
    panel.view.pan_by(Vec2::new(200.0, 200.0));
    h.move_to(scr(&panel, cell_center(-3, -2)), &mut panel);
    let text = panel.header_text();
    assert!(
        text.contains("cell -3, -2") && text.contains("placing Detector"),
        "{text}"
    );
    panel.tool = Some(Tool::DrawLine);
    assert!(panel.header_text().contains("line tool"));
}

#[test]
fn shortcuts_are_ignored_while_typing() {
    let ctx = egui::Context::default();
    let mut panel = OpticsPanel {
        simulation_active: false,
        ..Default::default()
    };
    let mut text = String::new();
    let field = Id::unique("test text field");
    let mut run = |events: Vec<Event>, focus: bool, panel: &mut OpticsPanel| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut text).id(field));
            if focus {
                response.request_focus();
            }
            panel.ui(ui);
        })
        .textures_delta
        .clear();
    };
    let space = |pressed| Event::Key {
        key: Key::Space,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    run(Vec::new(), true, &mut panel);
    run(vec![space(true), space(false)], false, &mut panel);
    assert!(!panel.simulation_active, "space went to the text field");

    ctx.memory_mut(|m| m.surrender_focus(field));
    run(Vec::new(), false, &mut panel);
    run(vec![space(true), space(false)], false, &mut panel);
    assert!(panel.simulation_active, "without focus the shortcut works");
}

#[test]
fn shortcuts_survive_clicking_library_rows() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 0, LENS);
    place(&mut panel, 1, 0, LENS);
    let mut h = Harness::new(&mut panel);
    // A part row in the left toolbar.
    h.click(Pos2::new(100.0, 64.0), &mut panel, PointerButton::Primary);
    assert!(panel.tool.is_some(), "the click hit a row");
    h.key(&mut panel, Key::Escape);
    h.key(&mut panel, Key::A);
    assert_eq!(panel.editor.selection.len(), 2);
}

#[test]
fn box_select_released_outside_the_canvas_finishes() {
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 0, LENS);
    place(&mut panel, 2, 1, LENS);
    let mut h = Harness::new(&mut panel);
    let from = scr(&panel, cell_center(-1, -1));
    // Released over the properties panel on the right.
    h.drag(
        from,
        Pos2::new(1270.0, 790.0),
        &mut panel,
        PointerButton::Primary,
    );
    assert_eq!(panel.editor.selection.len(), 2);
    // The undo history keeps working afterwards.
    h.key(&mut panel, Key::X);
    h.key_with(&mut panel, Key::Z, ctrl());
    assert_eq!(panel.layout.components.len(), 2);
}

// ---- boxes, wires and electronics ------------------------------------------------

use model::{BlockRef, Builtin, Item, PortRef, Terminal};

fn add_box(panel: &mut OpticsPanel, block: Builtin, cell: (i32, i32)) -> model::InstanceId {
    panel.layout.add_instance(BlockRef::Builtin(block), cell)
}

#[test]
fn library_lists_sections_and_searches() {
    let catalog = crate::panels::project::Catalog::default();
    let all = library::entries(&catalog, true, "");
    for section in library::SECTIONS {
        if section != library::SECTIONS[4] {
            assert!(all.iter().any(|e| e.section == section), "{section}");
        }
    }
    // Primitives are not repeated as waveguide boxes.
    assert!(!all.iter().any(|e| e.section == library::SECTIONS[5] && e.name == "Laser"));
    let found = library::entries(&catalog, true, "regist");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].item, Placeable::Block(BlockRef::Builtin(Builtin::Register { bits: 8 })));
    assert!(library::entries(&catalog, false, "full adder").is_empty(), "examples need a project");
    assert_eq!(Placeable::Part(OpticComponentKind::Detector { threshold: 0.5 }).name(&catalog), "Threshold detector");
    assert_eq!(Placeable::Example(0).name(&catalog), "Full adder");
}

#[test]
fn signal_wires_are_drawn_by_dragging_between_terminals() {
    let mut panel = OpticsPanel::default();
    let n = add_box(&mut panel, Builtin::NumberInput { bits: 2, value: 3 }, (0, 0));
    let d = add_box(&mut panel, Builtin::Display { bits: 2 }, (0, 4));
    let mut h = Harness::new(&mut panel);
    let terminal = |p: &OpticsPanel, item, port, out| scr(p, p.terminal_pos(Terminal { item, port }, out).unwrap());
    // Output to input, and input to output: both connect.
    let (a, b) = (terminal(&panel, Item::Box(n), 0, true), terminal(&panel, Item::Box(d), 0, false));
    h.drag(a, b, &mut panel, PointerButton::Primary);
    let (a, b) = (terminal(&panel, Item::Box(d), 1, false), terminal(&panel, Item::Box(n), 1, true));
    h.drag(a, b, &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.signals.len(), 2);
    h.frame(&mut panel, Vec::new());
    let o = panel.outcome().unwrap();
    assert_eq!(input_number(&panel.layout, o, d, 2), 3);
    // Output to output is refused with a hint.
    let (a, b) = (terminal(&panel, Item::Box(n), 0, true), terminal(&panel, Item::Box(n), 1, true));
    h.drag(a, b, &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.signals.len(), 2);
    assert!(panel.status.as_ref().unwrap().0.contains("input"));

    // Clicking a wire selects it; X deletes it.
    let path = panel.signal_points(0).unwrap();
    let mid = scr(&panel, path[1] + (path[2] - path[1]) * 0.5);
    h.click(mid, &mut panel, PointerButton::Primary);
    assert_eq!(panel.selected_wire, Some(WireRef::Signal(0)));
    h.frame(&mut panel, Vec::new());
    h.key(&mut panel, Key::X);
    assert_eq!(panel.layout.signals.len(), 1);
    assert!(panel.selected_wire.is_none());
}

#[test]
fn waveguides_and_signal_light_mismatches() {
    let mut panel = OpticsPanel::default();
    let a = add_box(&mut panel, Builtin::YSplitter, (0, 0));
    let b = add_box(&mut panel, Builtin::Attenuator { loss_db: 0.0 }, (6, 3));
    let f = add_box(&mut panel, Builtin::FunctionCell {
        name: "f".into(),
        inputs: 1,
        light_inputs: 0,
        outputs: vec!["in1".into()],
        light_outputs: Vec::new(),
        wavelength_nm: 1550.0,
    }, (6, 7));
    let mut h = Harness::new(&mut panel);
    let out = scr(&panel, panel.light_port_pos(PortRef { instance: a, port: 1 }, true).unwrap());
    let into = scr(&panel, panel.light_port_pos(PortRef { instance: b, port: 0 }, false).unwrap());
    assert_eq!(panel.hit(panel.view.to_world(panel.canvas_rect().unwrap(), out)), Hit::LightOut(PortRef { instance: a, port: 1 }));
    h.drag(out, into, &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.wires.len(), 1);
    // Hovering the waveguide describes it.
    let path = panel.waveguide_points(0).unwrap();
    assert!(panel.hover_text(panel.hit(path[12])).unwrap().starts_with("Waveguide"));

    // Light can't go into a signal terminal: the editor offers a detector.
    let signal_in = scr(&panel, panel.terminal_pos(Terminal { item: Item::Box(f), port: 0 }, false).unwrap());
    h.drag(out, signal_in, &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.wires.len(), 1);
    let offer = panel.bridge.expect("converter offered");
    assert!(matches!(offer.bridge, bridge::Bridge::Detect { .. }));
    h.frame(&mut panel, Vec::new());
    h.key(&mut panel, Key::Escape);
    assert!(panel.bridge.is_none(), "Escape dismisses the offer");
    // Light into a light output, or signals into each other's kind of
    // output, only get a hint.
    assert!(bridge::mismatch_hint(Hit::LightOut(PortRef { instance: a, port: 0 }), Hit::SignalOut(Terminal { item: Item::Box(f), port: 0 })).is_some());
    assert!(bridge::mismatch_hint(Hit::LightOut(PortRef { instance: a, port: 0 }), Hit::LightIn(PortRef { instance: b, port: 0 })).is_none());

    // Shrinking a block drops wires to ports it no longer has.
    panel.layout.signals.push(model::SignalWire {
        from: Terminal { item: Item::Box(f), port: 0 },
        to: Terminal { item: Item::Box(f), port: 0 },
    });
    if let BlockRef::Builtin(Builtin::FunctionCell { inputs, .. }) = &mut panel.layout.instances[2].block {
        *inputs = 0;
    }
    panel.layout.instances[0].block = BlockRef::Builtin(Builtin::Attenuator { loss_db: 0.0 });
    panel.prune_connections();
    assert!(panel.layout.signals.is_empty() && panel.layout.wires.is_empty());
}

#[test]
fn registers_tick_in_a_standalone_panel() {
    let mut panel = OpticsPanel::default();
    let n = add_box(&mut panel, Builtin::NumberInput { bits: 1, value: 1 }, (0, 0));
    let r = add_box(&mut panel, Builtin::Register { bits: 1 }, (0, 3));
    panel.layout.connect_signal(Terminal { item: Item::Box(n), port: 0 }, Terminal { item: Item::Box(r), port: 0 });
    let mut h = Harness::new(&mut panel);
    assert_eq!(panel.outcome().unwrap().signal(Item::Box(r), 0), 0.0);
    panel.tick();
    h.frame(&mut panel, Vec::new());
    assert_eq!(panel.outcome().unwrap().signal(Item::Box(r), 0), 1.0);
    assert!(panel.header_text().contains("tick 1"));
    panel.running = true;
    for _ in 0..25 {
        h.frame(&mut panel, Vec::new());
    }
    assert!(panel.machine.tick >= 2, "running ticks every 0.5 s");
    panel.reset_clock();
    assert_eq!(panel.machine.tick, 0);
    assert!(!panel.running);
    // Embedded panels ask their host.
    panel.embedded = true;
    panel.tick();
    assert_eq!(panel.requests, vec![Request::Tick]);
}

#[test]
fn boxes_select_move_and_inspect() {
    let mut panel = OpticsPanel::default();
    add_box(&mut panel, Builtin::NumberInput { bits: 4, value: 0 }, (0, 0));
    add_box(&mut panel, Builtin::Coupler { coupling: 0.5 }, (0, 3));
    place(&mut panel, 5, 5, LENS);
    let mut h = Harness::new(&mut panel);
    // Clicking a number input's bit toggles it (most significant first).
    h.click(scr(&panel, cell_center(1, 0)), &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.instances[0].block, BlockRef::Builtin(Builtin::NumberInput { bits: 4, value: 4 }));
    assert_eq!(panel.editor.boxes.active(), Some(0));
    // Dragging a box moves it; it can't land on another box.
    h.drag(scr(&panel, cell_center(0, 3)), scr(&panel, cell_center(0, 6)), &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.instances[1].cell, (0, 6));
    h.drag(scr(&panel, cell_center(0, 6)), scr(&panel, cell_center(0, 0)), &mut panel, PointerButton::Primary);
    assert_eq!(panel.layout.instances[1].cell, (0, 6));
    // Every selection renders in the inspector; hover texts exist.
    for b in 0..2 {
        panel.editor.clear_selection();
        panel.editor.boxes.select_only(b);
        h.frame(&mut panel, Vec::new());
        assert!(panel.hover_text(Hit::Box(b)).is_some());
    }
    assert!(panel.hover_text(Hit::Box(0)).unwrap().contains("toggle"));
    assert!(panel.hover_text(Hit::Part(0)).unwrap().contains("Lens"));
    assert!(panel.hover_text(Hit::Empty).is_none());
    // Tab on a built-in box has nothing to open.
    panel.apply(Action::Inspect);
    assert!(panel.status.as_ref().unwrap().0.contains("component box"));
    // Box select catches boxes and parts.
    h.drag(scr(&panel, cell_center(-1, -1)), scr(&panel, cell_center(6, 7)), &mut panel, PointerButton::Primary);
    assert_eq!((panel.editor.boxes.len(), panel.editor.selection.len()), (2, 1));
    assert!(panel.bounds(true).unwrap().contains(cell_center(0, 6)));
}

#[test]
fn add_menu_search_places_the_first_match() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    h.move_to(scr(&panel, cell_center(2, 2)), &mut panel);
    h.key_with(&mut panel, Key::A, Modifiers::SHIFT);
    assert!(panel.menu.is_some());
    h.frame(&mut panel, vec![Event::Text("display".into())]);
    assert_eq!(panel.menu.as_ref().unwrap().search, "display");
    h.key(&mut panel, Key::Enter);
    assert!(panel.menu.is_none());
    assert_eq!(panel.layout.instances.len(), 1);
    assert_eq!(panel.layout.instances[0].cell, (2, 2));
    // Example components need a host.
    panel.place(Placeable::Example(0), cell_center(9, 9));
    assert!(panel.status.as_ref().unwrap().0.contains("project"));
    panel.embedded = true;
    panel.place(Placeable::Example(0), cell_center(9, 9));
    assert_eq!(panel.requests, vec![Request::PlaceExample { example: 0, cell: (9, 9) }]);
}

#[test]
fn converters_bridge_light_and_signals() {
    // A laser beam into a coupler box; its upper output should drive a display.
    let mut panel = OpticsPanel::default();
    place(&mut panel, 0, 1, OpticComponentKind::palette()[0]);
    let dc = add_box(&mut panel, Builtin::Coupler { coupling: 0.0 }, (2, 1));
    let d = add_box(&mut panel, Builtin::Display { bits: 1 }, (8, 6));
    let light = PortRef { instance: dc, port: 0 };
    let display_in = Terminal { item: Item::Box(d), port: 0 };
    let mut h = Harness::new(&mut panel);
    let from = scr(&panel, panel.light_port_pos(light, true).unwrap());
    let to = scr(&panel, panel.terminal_pos(display_in, false).unwrap());
    // Hovering the wrong kind of port while dragging shows the warning.
    h.move_to(from, &mut panel);
    let press = h.button(from, PointerButton::Primary, true);
    h.frame(&mut panel, vec![press]);
    for k in 1..=6 {
        h.frame(&mut panel, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
    }
    assert!(panel.connecting());
    let release = h.button(to, PointerButton::Primary, false);
    h.frame(&mut panel, vec![release]);
    let offer = panel.bridge.expect("offered");
    assert_eq!(offer.bridge, bridge::Bridge::Detect { light, signal: display_in });
    h.frame(&mut panel, Vec::new());
    panel.insert_bridge(offer.bridge).unwrap();
    let detector = panel.layout.components.len() - 1;
    assert_eq!(cell_of(panel.layout.components[detector].pos), (4, 1), "right of the port");
    assert_eq!(panel.layout.components[detector].kind, OpticComponentKind::Detector { threshold: 0.5 }, "a display wants bits");
    h.frame(&mut panel, Vec::new());
    assert_eq!(input_number(&panel.layout, panel.outcome().unwrap(), d, 1), 1, "the light reaches the display");
    assert!(panel.insert_bridge(offer.bridge).unwrap_err().contains("no room"));

    // A bit switching light into a box input gets a laser in front of it.
    let n = add_box(&mut panel, Builtin::NumberInput { bits: 1, value: 1 }, (10, 0));
    let att = add_box(&mut panel, Builtin::Attenuator { loss_db: 0.0 }, (12, 4));
    let bit = Terminal { item: Item::Box(n), port: 0 };
    let into = PortRef { instance: att, port: 0 };
    assert_eq!(bridge::needed(Hit::LightIn(into), Hit::SignalOut(bit)), Some(bridge::Bridge::Drive { signal: bit, light: into }));
    panel.insert_bridge(bridge::Bridge::Drive { signal: bit, light: into }).unwrap();
    let laser = panel.layout.components.len() - 1;
    assert_eq!(cell_of(panel.layout.components[laser].pos), (11, 4));
    h.frame(&mut panel, Vec::new());
    assert!((panel.outcome().unwrap().port_power[&(att, 0)] - 1.0).abs() < 1e-9, "the bit turns the laser on");
    // The offer's buttons render and Cancel closes it.
    panel.bridge = Some(offer);
    h.frame(&mut panel, Vec::new());
    h.frame(&mut panel, Vec::new());
    assert!(panel.bridge.is_some());
    panel.apply_bridge_choice(false);
    assert!(panel.bridge.is_none());
    assert!(panel.insert_bridge(bridge::Bridge::Drive { signal: bit, light: PortRef { instance: model::InstanceId(99), port: 0 } }).is_err());
}

#[test]
fn wheel_over_a_menu_does_not_move_the_grid() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    let at = scr(&panel, cell_center(3, 3));
    h.move_to(at, &mut panel);
    h.key_with(&mut panel, Key::A, Modifiers::SHIFT);
    h.frame(&mut panel, Vec::new());
    assert!(panel.menu.is_some());
    let (zoom, pan) = (panel.view.zoom, panel.view.pan);
    // Over the menu: the wheel belongs to it.
    let over_menu = panel.menu_position().unwrap() + Vec2::new(60.0, 60.0);
    h.wheel(over_menu, &mut panel, Vec2::new(0.0, -3.0), MouseWheelUnit::Line, MouseWheelSource::Wheel);
    h.frame(&mut panel, Vec::new());
    assert_eq!((panel.view.zoom, panel.view.pan), (zoom, pan), "the grid stayed put");
    assert!(panel.menu.is_some());
    // Over bare canvas it zooms as usual.
    let canvas = panel.canvas_rect().unwrap();
    h.wheel(canvas.right_bottom() - Vec2::new(30.0, 30.0), &mut panel, Vec2::new(0.0, -3.0), MouseWheelUnit::Line, MouseWheelSource::Wheel);
    assert_ne!(panel.view.zoom, zoom);
}

#[test]
fn add_menu_has_category_submenus() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    h.move_to(scr(&panel, cell_center(3, 3)), &mut panel);
    h.key_with(&mut panel, Key::A, Modifiers::SHIFT);
    h.frame(&mut panel, Vec::new());
    // Hover down the menu until a category opens its list beside it.
    let menu = panel.menu_position().unwrap();
    let mut open = None;
    for dy in (60..420).step_by(6) {
        h.move_to(menu + Vec2::new(40.0, dy as f32), &mut panel);
        if let Some(section) = panel.menu.as_ref().unwrap().open_section {
            open = Some(section);
            break;
        }
    }
    let (section, y) = open.expect("hovering a category opens it");
    assert_eq!(library::SECTIONS[section], "Light & detection", "the first category");
    h.frame(&mut panel, Vec::new());
    // Its first entry (Laser) sits to the right of the row; clicking adds it.
    let menu_right = h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
            .find(|(l, _)| l.order == egui::Order::Foreground && l.id == egui::Id::unique("optics_menu"))
            .map(|(_, r)| r.right())
    });
    let x = menu_right.expect("menu drawn") + 60.0;
    h.click(Pos2::new(x, y + 6.0), &mut panel, PointerButton::Primary);
    assert!(panel.menu.is_none(), "choosing closes the menu");
    assert_eq!(panel.layout.components.len(), 1);
    assert!(matches!(panel.layout.components[0].kind, OpticComponentKind::Laser { .. }));
    assert_eq!(cell_of(panel.layout.components[0].pos), (3, 3), "placed where the menu opened");
}

#[test]
fn browser_lists_folders_and_picks_a_part() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    panel.show_browser = true;
    for section in 0..library::SECTIONS.len() {
        panel.browser_section = section;
        h.frame(&mut panel, Vec::new());
    }
    panel.browser_search = "threshold".into();
    h.frame(&mut panel, Vec::new());
    assert!(panel.show_browser);
    // Closing the window hides it.
    panel.show_browser = false;
    h.frame(&mut panel, Vec::new());
    // The ▦ button next to the search box opens it.
    h.click(Pos2::new(18.0, 18.0), &mut panel, PointerButton::Primary);
    assert!(panel.show_browser, "the browse button opens the browser");
}

#[test]
fn library_folders_collapse_and_search_flattens() {
    let mut panel = OpticsPanel::default();
    let mut h = Harness::new(&mut panel);
    h.frame(&mut panel, Vec::new());
    panel.search = "mirror".into();
    h.frame(&mut panel, Vec::new());
    panel.search.clear();
    h.frame(&mut panel, Vec::new());
}
