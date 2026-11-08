//! Optical matrix-vector multiplication with an SVD mesh of MZIs.
//!
//! `cargo run --release --no-default-features --example matmul`

use std::time::Instant;

use shine::photonics::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n = 64;
    let batch = 4096;
    let w = Matrix::from_fn(n, n, |i, j| {
        C64::new(((i * 7 + j * 3) % 11) as f64 - 5.0, 0.0)
    });

    let t = Instant::now();
    let mesh = SvdMesh::decompose(&w)?;
    let mut circuit = Circuit::new();
    let ports = circuit.add_block(&mesh)?;
    let program = circuit.compile(1550.0)?;
    println!(
        "{n}x{n} mesh: {} nodes, decomposed + compiled in {:?}",
        circuit.len(),
        t.elapsed()
    );

    let x = Matrix::from_fn(n, batch, |i, j| C64::new(((i + j) % 5) as f64, 0.0));
    let t = Instant::now();
    let sol = program.run_batch(&ports.inputs, x.as_ref())?;
    let elapsed = t.elapsed();
    println!(
        "ran {batch} input vectors in {elapsed:?} ({:.2} us/vector)",
        elapsed.as_secs_f64() * 1e6 / batch as f64
    );

    let reference = &w * &x;
    let mut err: f64 = 0.0;
    for j in 0..batch {
        for (i, &port) in ports.outputs.iter().enumerate() {
            err = err.max((sol.output_field_at(port, j) * mesh.scale - reference[(i, j)]).norm());
        }
    }
    println!("max abs error vs W*x: {err:.2e}");
    Ok(())
}
