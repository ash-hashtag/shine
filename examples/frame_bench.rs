//! How long editor frames take while dragging a component, and what they
//! spend it on.
//!
//! `cargo run --release --example frame_bench [example component index]`

use std::time::{Duration, Instant};

use egui::{Event, Pos2, Rect, Vec2};
use shine::panels::optics::model::{OpticComponentKind, PlacedComponent, PortRef, cell_center};
use shine::panels::workspace::Workspace;

fn frame(ctx: &egui::Context, ws: &mut Workspace, t: &mut f64, events: Vec<Event>) -> Duration {
    *t += 1.0 / 60.0;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0))),
        time: Some(*t),
        events,
        ..Default::default()
    };
    let start = Instant::now();
    ctx.run_ui(input, |ui| ws.ui(ui)).textures_delta.clear();
    start.elapsed()
}

fn report(name: &str, times: &[Duration]) {
    let mut ms: Vec<f64> = times.iter().map(|d| d.as_secs_f64() * 1e3).collect();
    ms.sort_by(f64::total_cmp);
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    println!("{name:<40} mean {mean:>7.2} ms   median {:>7.2} ms   worst {:>7.2} ms", ms[ms.len() / 2], ms[ms.len() - 1]);
}

fn main() {
    let which: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(1);
    println!("example component {which} (0 full adder, 1 Mach–Zehnder, 2 Fabry–Pérot, 3 splitter)");
    for components in [1, 3, 6] {
        let mut ws = Workspace::default();
        // Lasers wired into Mach–Zehnder components, one per row.
        for k in 0..components {
            let row = 6 * k as i32;
            ws.place_example(which, (6, row));
            let id = ws.active.unwrap();
            let p = ws.project.doc_mut(id).unwrap().panel_mut();
            p.layout.components.push(PlacedComponent {
                kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
                pos: cell_center(0, row),
            });
            let laser = p.layout.components.len() - 1;
            let mz = p.layout.instances[k].id;
            if p.catalog.get(match p.layout.instances[k].block { shine::panels::optics::model::BlockRef::Component(d) => d, _ => unreachable!() }).is_some_and(|e| e.interface.counts().optical_in > 0) {
                p.layout.connect_feed(laser, PortRef { instance: mz, port: 0 });
            }
        }
        let ctx = egui::Context::default();
        let mut t = 0.0;
        for _ in 0..5 {
            frame(&ctx, &mut ws, &mut t, Vec::new());
        }
        // Idle frames: nothing changes, and nothing should ask for more.
        let idle: Vec<Duration> = (0..30).map(|_| frame(&ctx, &mut ws, &mut t, Vec::new())).collect();
        println!("  idle frames keep asking to repaint: {}", ctx.has_requested_repaint());

        // Drag the first box across the grid with the mouse.
        let id = ws.active.unwrap();
        let p = ws.project.doc(id).unwrap().panel();
        let rect = p.canvas_rect().unwrap();
        // The middle of the first box.
        let inst = &p.layout.instances[0];
        let cells = shine::panels::optics::model::box_geometry(inst.cell, p.counts(&inst.block)).cells;
        let (sx, sy) = cells.iter().fold((0.0, 0.0), |(x, y), &(i, j)| (x + i as f32, y + j as f32));
        let middle = Pos2::new(sx / cells.len() as f32 + 0.5, sy / cells.len() as f32 + 0.5) * shine::panels::optics::model::GRID;
        let start = p.view.to_screen(rect, middle);
        let first = inst.cell;
        let press = |pressed| Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, &mut ws, &mut t, vec![Event::PointerMoved(start), press(true)]);
        let drag: Vec<Duration> = (1..=60)
            .map(|k| frame(&ctx, &mut ws, &mut t, vec![Event::PointerMoved(start + Vec2::new(k as f32 * 4.0, k as f32 * 2.0))]))
            .collect();
        let end = start + Vec2::new(240.0, 120.0);
        frame(&ctx, &mut ws, &mut t, vec![Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        let moved = ws.project.doc(id).unwrap().layout().instances[0].cell;
        assert_ne!(moved, first, "the drag moved the box");

        println!("\n{components} component(s), {} documents", ws.project.documents.len());
        report("idle frame", &idle);
        report("frame while dragging", &drag);

        // The pieces a frame does, on their own.
        let n = 200;
        let time = |f: &mut dyn FnMut()| {
            let s = Instant::now();
            for _ in 0..n {
                f();
            }
            s.elapsed() / n
        };
        println!("  project.catalog()      {:>9.3} ms", time(&mut || { std::hint::black_box(ws.project.catalog()); }).as_secs_f64() * 1e3);
        println!("  project.fingerprint()  {:>9.3} ms", time(&mut || { std::hint::black_box(ws.project.fingerprint()); }).as_secs_f64() * 1e3);
        let machine = Default::default();
        println!("  project.simulate()     {:>9.3} ms", time(&mut || { std::hint::black_box(ws.project.simulate(id, &machine)); }).as_secs_f64() * 1e3);
    }
}
