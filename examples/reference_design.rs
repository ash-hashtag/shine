//! Measures the reference design (a programmable photonic matrix processor,
//! `shine::panels::optics::reference`) at its shipped size and larger: how
//! long it takes to build, save, open, simulate, sweep and export, and how
//! long editor frames take while looking at it and editing it.
//!
//! `cargo run --release --example reference_design [-- --write] [-- --sizes 16,32,64] [-- --editor-only]`
//!
//! `--write` saves the shipped design to `reference/photonic-matrix-processor.shine`.

use std::time::{Duration, Instant};

use egui::{Event, Pos2, Rect, Vec2};
use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::circuit::sweep::{self, SweepKind, SweepSpec};
use shine::panels::interop::{self, gdsfactory, sax};
use shine::panels::optics::model::{BlockRef, Builtin, InstanceId};
use shine::panels::optics::reference;
use shine::panels::project::Project;
use shine::panels::workspace::Workspace;

const SAVED: &str = "reference/photonic-matrix-processor.shine";

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let s = Instant::now();
    let v = f();
    (v, ms(s.elapsed()))
}

fn frame(ctx: &egui::Context, ws: &mut Workspace, t: &mut f64, events: Vec<Event>) -> f64 {
    *t += 1.0 / 60.0;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1000.0))),
        time: Some(*t),
        events,
        ..Default::default()
    };
    let start = Instant::now();
    ctx.run_ui(input, |ui| ws.ui(ui)).textures_delta.clear();
    ms(start.elapsed())
}

fn stats(xs: &[f64]) -> String {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    format!("median {:>8.2} ms  worst {:>8.2} ms", v[v.len() / 2], v[v.len() - 1])
}

fn measure(n: usize, editor_only: bool) {
    let (project, build) = timed(|| reference::sized_project(n));
    let leaf = |p: &Project| {
        // Boxes once every component is opened up.
        fn count(p: &Project, id: shine::panels::project::DocId) -> usize {
            let l = p.doc(id).unwrap().layout();
            l.instances.iter().map(|i| match i.block {
                BlockRef::Component(d) => count(p, d),
                BlockRef::Builtin(_) => 1,
            }).sum()
        }
        count(p, p.documents[0].id)
    };
    let mesh_doc = project.documents.iter().find(|d| d.name.contains("mesh")).unwrap();
    tracing::info!(
        "\n{n}×{n} processor: {} MZI cells and {n} modulators, {} parts when opened up; the mesh document has {} boxes and {} waveguides",
        n * (n - 1) / 2,
        leaf(&project),
        mesh_doc.layout().instances.len(),
        mesh_doc.layout().wires.len()
    );
    let (bytes, save) = timed(|| project.to_bytes());
    let (_, open) = timed(|| Project::from_bytes(&bytes).unwrap());
    tracing::info!("  build {build:>8.1} ms   save {save:>6.1} ms ({} kB)   open {open:>6.1} ms", bytes.len() / 1024);

    if !editor_only {
        compute(&project, n);
    }
    editor(&project, &bytes);
}

/// Simulation, sweeps and exports.
fn compute(project: &Project, _n: usize) {
    let top = project.documents[0].id;
    let doc = project.doc(top).unwrap();
    let mesh_doc = project.documents.iter().find(|d| d.name.contains("mesh")).unwrap();
    let (o, simulate) = timed(|| sim::simulate(doc.layout(), 10.0, project, &Machine::default(), Some(top), &Options::default()));
    assert!(o.error.is_none(), "{:?}", o.error);
    let laser = doc.layout().instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(shine::panels::optics::model::Builtin::Laser { .. }))).unwrap().id;
    let spec = SweepSpec { kind: SweepKind::Parameter { item: shine::panels::optics::model::Item::Box(laser), param: "wavelength".into() }, start: 1540.0, stop: 1560.0, points: 201 };
    let names = |id: InstanceId| format!("box {}", id.0);
    let (r, sweep_ms) = timed(|| sweep::run(doc.layout(), 10.0, project, &Machine::default(), Some(top), &spec, &names));
    assert!(r.error.is_none(), "{:?}", r.error);
    tracing::info!("  simulate {simulate:>8.1} ms   wavelength sweep, 201 points {sweep_ms:>8.1} ms");

    let wl = interop::wavelengths(1540.0, 1560.0, 201);
    let (ts, touchstone) = timed(|| interop::touchstone(project, mesh_doc.id, &wl, "").unwrap());
    let (s, sax_ms) = timed(|| sax::export(project, mesh_doc.id, &wl, "").unwrap());
    let (g, gds) = timed(|| gdsfactory::export(project, top, gdsfactory::DEFAULT_SPACING_UM, "").unwrap());
    tracing::info!(
        "  mesh S-parameters ({} ports, 201 points) {touchstone:>7.1} ms   SAX netlist ({} instances) {sax_ms:>6.1} ms   gdsfactory layout ({} instances) {gds:>6.1} ms",
        ts.1,
        s.instances,
        g.instances
    );

}

/// Editor frames, on the processor and inside the mesh.
fn editor(project: &Project, bytes: &[u8]) {
    // What every frame does to the whole project, whatever is on screen.
    let reps = 20;
    let (_, catalog) = timed(|| (0..reps).for_each(|_| { std::hint::black_box(project.catalog()); }));
    let (_, fingerprint) = timed(|| (0..reps).for_each(|_| { std::hint::black_box(project.fingerprint()); }));
    tracing::info!("  project.catalog() {:>7.2} ms   project.fingerprint() {:>7.2} ms", catalog / reps as f64, fingerprint / reps as f64);

    // The editor, on the processor and then inside the mesh.
    let mut ws = Workspace::default();
    let (_, open_ws) = timed(|| ws.open_bytes("reference.shine", bytes).unwrap());
    let ctx = egui::Context::default();
    let mut t = 0.0;
    let first = frame(&ctx, &mut ws, &mut t, Vec::new());
    for _ in 0..3 {
        frame(&ctx, &mut ws, &mut t, Vec::new());
    }
    let idle: Vec<f64> = (0..20).map(|_| frame(&ctx, &mut ws, &mut t, Vec::new())).collect();
    let centre = Pos2::new(800.0, 500.0);
    let zoom: Vec<f64> = (0..20).map(|k| frame(&ctx, &mut ws, &mut t, vec![Event::PointerMoved(centre), Event::Zoom(if k % 2 == 0 { 1.1 } else { 1.0 / 1.1 })])).collect();
    tracing::info!("  editor, processor: open {open_ws:>6.1} ms   first frame {first:>8.1} ms   idle {}   zooming {}", stats(&idle), stats(&zoom));
    // Edit: set an input. The frame shouldn't wait for the
    // simulation; the result arrives when it's ready.
    let mut edits = Vec::new();
    let mut results = Vec::new();
    for k in 0..6 {
        let id = ws.active.unwrap();
        let p = ws.project.doc_mut(id).unwrap().panel_mut();
        let input = p.layout.instances.iter_mut().filter(|i| matches!(i.block, BlockRef::Builtin(Builtin::Value { .. }))).nth(k).unwrap();
        if let BlockRef::Builtin(Builtin::Value { value, .. }) = &mut input.block {
            *value = 0.5;
        }
        let start = Instant::now();
        edits.push(frame(&ctx, &mut ws, &mut t, Vec::new()));
        // Frames go on (as they would, at 60 a second) until the result is in.
        loop {
            let fresh = {
                let p = ws.project.doc(id).unwrap().panel();
                !p.is_stale(p.simulation_key(ws.project.fingerprint()))
            };
            if fresh {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            edits.push(frame(&ctx, &mut ws, &mut t, Vec::new()));
        }
        results.push(ms(start.elapsed()));
    }
    tracing::info!("  setting an input: frames {}   result shown after {}", stats(&edits), stats(&results));
    let mesh_id = ws.project.documents.iter().find(|d| d.name.contains("mesh")).unwrap().id;
    ws.open_tab(mesh_id);
    let first = frame(&ctx, &mut ws, &mut t, Vec::new());
    let idle: Vec<f64> = (0..20).map(|_| frame(&ctx, &mut ws, &mut t, Vec::new())).collect();
    let zoom: Vec<f64> = (0..20).map(|k| frame(&ctx, &mut ws, &mut t, vec![Event::PointerMoved(centre), Event::Zoom(if k % 2 == 0 { 1.1 } else { 1.0 / 1.1 })])).collect();
    tracing::info!("  editor, mesh: first frame {first:>8.1} ms   idle {}   zooming {}", stats(&idle), stats(&zoom));
}

fn main() {
    let _log = shine::logging::init(&shine::logging::LogConfig::from_env()).expect("logging");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--write") {
        let bytes = reference::reference_project().to_bytes();
        std::fs::create_dir_all("reference").expect("reference/");
        std::fs::write(SAVED, &bytes).expect("write the reference design");
        tracing::info!("Wrote {SAVED} ({} kB)", bytes.len() / 1024);
        return;
    }
    let sizes: Vec<usize> = args
        .iter()
        .position(|a| a == "--sizes")
        .and_then(|i| args.get(i + 1))
        .map_or_else(|| vec![reference::SIZE], |s| s.split(',').filter_map(|x| x.parse().ok()).collect());
    let editor_only = args.iter().any(|a| a == "--editor-only");
    for n in sizes {
        measure(n, editor_only);
    }
}
