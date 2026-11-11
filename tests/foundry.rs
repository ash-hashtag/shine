//! The bundled SiEPIC EBeam compact models (library/siepic-ebeam, MIT): each
//! opens with its ports the right way round, behaves at 1550 nm as the kit
//! documents, and is in the asset library ready to place.

use shine::panels::assets::{AssetRef, KIT_FOLDER, Library};
use shine::panels::models::ModelSpec;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/library/siepic-ebeam");

fn load(name: &str) -> ModelSpec {
    let path = format!("{DIR}/{name}");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    ModelSpec::from_file(name, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// |S|² from each input (columns) to each output (rows) at 1550 nm.
fn powers(m: &ModelSpec) -> Vec<Vec<f64>> {
    let s = m.table().at(1550.0);
    (0..s.nrows()).map(|o| (0..s.ncols()).map(|i| s[(o, i)].norm_sqr()).collect()).collect()
}

fn ports(m: &ModelSpec) -> (usize, usize) {
    (m.inputs.len(), m.outputs.len())
}

#[test]
fn the_y_branch_splits_one_input_in_two() {
    let y = load("Y-branch.sparam");
    assert_eq!(ports(&y), (1, 2), "a 1×2 splitter: {:?} → {:?}", y.inputs, y.outputs);
    let p = powers(&y);
    // About 0.15 dB of excess loss: just under half each way.
    for o in 0..2 {
        assert!((0.47..0.5).contains(&p[o][0]), "{p:?}");
    }
}

#[test]
fn the_broadband_coupler_splits_evenly() {
    let m = load("Broadband 50-50 coupler.sparam");
    assert_eq!(ports(&m), (2, 2));
    for row in powers(&m) {
        for p in row {
            assert!((0.47..0.53).contains(&p), "50:50 within 3%: {p}");
        }
    }
}

#[test]
fn directional_couplers_cross_more_as_they_get_longer() {
    // Gap 200 nm: from barely coupled at 0 µm to mostly crossed at 30 µm.
    let mut last = 0.0;
    for length in [0, 5, 10, 15, 20, 25, 30] {
        let m = load(&format!("Directional coupler, gap 200 nm, length {length} µm.sparam"));
        assert_eq!(ports(&m), (2, 2));
        let p = powers(&m);
        let (bar, cross) = (p[0][0], p[1][0]);
        assert!(cross > last, "{length} µm: κ {cross} after {last}");
        assert!(bar + cross > 0.95 && bar + cross <= 1.0 + 1e-6, "{length} µm: nearly lossless, {}", bar + cross);
        last = cross;
    }
    assert!(last > 0.85);
}

#[test]
fn ring_couplers_couple_a_few_percent() {
    for name in ["Ring coupler, gap 100 nm, radius 5 µm.dat", "Ring coupler, gap 100 nm, radius 10 µm.dat", "Ring coupler, gap 150 nm, radius 10 µm.dat"] {
        let m = load(name);
        assert_eq!(ports(&m), (2, 2), "{name}");
        let p = powers(&m);
        let cross = p[1][0].max(p[0][0].min(p[1][0]));
        assert!((0.01..0.12).contains(&cross), "{name}: {p:?}");
    }
    // A wider gap couples less.
    let k = |name: &str| powers(&load(name))[1][0].min(powers(&load(name))[0][0]);
    assert!(k("Ring coupler, gap 150 nm, radius 10 µm.dat") < k("Ring coupler, gap 100 nm, radius 10 µm.dat"));
}

#[test]
fn the_kit_is_in_the_library_with_its_credit() {
    let library = Library::new();
    let kit: Vec<_> = library.summaries().iter().filter(|s| s.folder == KIT_FOLDER).cloned().collect();
    let files = std::fs::read_dir(DIR).unwrap().filter(|f| !f.as_ref().unwrap().file_name().to_string_lossy().ends_with(".md")).count();
    assert_eq!(kit.len(), files, "every bundled file is a component");
    for s in &kit {
        assert!(matches!(s.asset, AssetRef::Kit(_)) && s.asset.is_built_in());
        assert!(s.entry.description.contains("SiEPIC EBeam PDK") && s.entry.description.contains("MIT"), "{}", s.entry.description);
        let bundle = library.bundle(s.asset).expect("it opens");
        assert_eq!(bundle.documents[0].name, s.entry.name);
    }
    assert!(kit.iter().any(|s| s.entry.name == "Y-branch"));
}
