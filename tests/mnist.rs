//! The Fashion-MNIST chip kept in the repository (reference/mnist): it still opens
//! as Shine's file format moves on, and takes the 784 pixels `--input` gives.
//! (Simulating it is `examples/mnist.rs` and `examples/mnist.py`: seconds in
//! a release build, too long for a debug test.)

use shine::cli;
use shine::panels::optics::model::{BlockRef, Builtin};
use shine::panels::project::Project;

const SAVED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/reference/mnist/fashion-mnist-chip.shine");

#[test]
fn the_saved_chip_opens_with_784_pixel_inputs_and_a_display() {
    let p = Project::from_bytes(&std::fs::read(SAVED).unwrap()).unwrap();
    let top = p.documents.iter().find(|d| d.component.is_none()).expect("the chip");
    assert_eq!(top.name, "Fashion-MNIST classifier");
    let layout = top.layout();
    assert_eq!(cli::design_inputs(layout).len(), 784);
    assert!(layout.instances.iter().any(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { bits: 4 }))));
    let mesh = p.documents.iter().map(|d| d.layout().instances.len()).max().unwrap();
    assert!(mesh > 300_000, "the 784-mode mesh: {mesh} boxes");
}
