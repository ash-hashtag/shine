//! The Fashion-MNIST chip, the app's example built from the network in the
//! repository (reference/mnist): it shows a picture the network reads, on its
//! 784 inputs. (Simulating it is `examples/mnist.rs` and `examples/mnist.py`:
//! seconds in a release build, too long for a debug test.)

use shine::cli;
use shine::panels::optics::model::{BlockRef, Builtin};
use shine::panels::project::Project;

#[test]
fn the_example_shows_a_picture_the_network_reads() {
    use shine::panels::optics::digits;
    let w = digits::fashion_weights();
    let samples = digits::fashion_samples();
    assert_eq!(samples.len(), 10);
    assert_eq!(samples.iter().map(|s| s.class.as_str()).collect::<Vec<_>>(), w.classes);
    // The network (computed digitally here) reads most of them; the chip
    // gives the network's class (examples/mnist.rs, benchmark.py).
    let right = samples.iter().enumerate().filter(|(c, s)| digits::argmax(&digits::scores(&w, &s.pixels())) == *c).count();
    assert!(right >= 8, "{right} of 10");

    let start = std::time::Instant::now();
    let mut p = Project::new();
    let top = digits::fashion_demo(&mut p);
    let layout = p.doc(top).unwrap().layout();
    let inputs = cli::design_inputs(layout);
    assert_eq!(inputs.len(), 784);
    let boot = samples[9].pixels();
    let shown: Vec<f64> = inputs.iter().map(|&id| match &layout.instance(id).unwrap().block {
        BlockRef::Builtin(Builtin::Value { value, .. }) => *value,
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(shown, boot, "the ankle boot, on its pixels");
    eprintln!("built in {:?}", start.elapsed());
}
