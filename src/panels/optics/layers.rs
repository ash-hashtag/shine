//! Matrices as light: a weight layer W (any rows × cols, real or complex)
//! as two MZI meshes with attenuators between them, and a unitary as one
//! mesh. What the optical neural networks are built from, and a tool for
//! anyone's matrices: in the editor (Component → New layer from a
//! matrix…), the library (`Project::new_weight_layer`) and `shine layer`.
//!
//! A layer computes W x / σmax: W = U Σ V† (its singular value
//! decomposition), V† and U each a mesh of MZIs in the rectangular
//! (Clements) arrangement, and Σ/σmax an attenuator per mode, the largest
//! passing everything. Light can't be amplified, so the layer is W scaled
//! by its largest singular value σmax; the meshes' waveguide arms add one
//! more common factor, the same for every path.

use std::str::FromStr;

use super::model::{BlockRef, Builtin, Layout};
use super::reference::{add, link, mesh_named};
use crate::panels::project::{DocId, Project};
use crate::photonics::{C64, Matrix, UnitaryMesh};

/// A matrix as meshes: V† (cols × cols), each mode's amplitude σ/σmax (one
/// per singular value, min(rows, cols) of them) and U (rows × rows), each
/// mesh in the rectangular arrangement; and σmax.
pub struct Svd {
    pub v_adjoint: UnitaryMesh,
    pub attenuation: Vec<f64>,
    pub u: UnitaryMesh,
    pub scale: f64,
}

/// W = U Σ V† as meshes.
pub fn svd(m: &Matrix) -> Result<Svd, String> {
    let (rows, cols) = (m.nrows(), m.ncols());
    if rows == 0 || cols == 0 {
        return Err("the matrix is empty".into());
    }
    if m.col_iter().flat_map(|c| c.iter().copied().collect::<Vec<_>>()).any(|z| !(z.re.is_finite() && z.im.is_finite())) {
        return Err("the matrix has a value that isn't a number".into());
    }
    let s = m.svd().map_err(|_| "the singular value decomposition didn't converge".to_owned())?;
    let k = rows.min(cols);
    let sigma: Vec<f64> = (0..k).map(|i| s.S().column_vector()[i].re).collect();
    let scale = sigma.iter().copied().fold(0.0, f64::max);
    if scale <= 0.0 {
        return Err("the matrix is all zeros".into());
    }
    let u = s.U().to_owned();
    let v_adjoint = s.V().adjoint().to_owned();
    Ok(Svd {
        v_adjoint: UnitaryMesh::decompose_rectangular(&v_adjoint).map_err(|e| e.to_string())?,
        attenuation: sigma.iter().map(|s| s / scale).collect(),
        u: UnitaryMesh::decompose_rectangular(&u).map_err(|e| e.to_string())?,
        scale,
    })
}

/// A weight layer added to a project.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    /// The component: inputs in1…, outputs out1….
    pub id: DocId,
    /// σmax: the layer computes W x / σmax.
    pub scale: f64,
    /// Mesh columns light crosses (both meshes'), each through one arm.
    pub columns: usize,
}

/// Weight matrix `m` (rows × cols) as a component named `name`: inputs
/// in1…in{cols}, outputs out1…out{rows}, computing W x / σmax. Its meshes
/// are components of their own, named after it.
pub fn weight_layer(project: &mut Project, name: &str, m: &Matrix) -> Result<Layer, String> {
    let s = svd(m)?;
    let (rows, cols) = (m.nrows(), m.ncols());
    let v = mesh_named(project, &s.v_adjoint, &format!("{name}: V† ({cols}×{cols} mesh)"), "V†");
    let u = mesh_named(project, &s.u, &format!("{name}: U ({rows}×{rows} mesh)"), "U");
    let mut l = Layout::default();
    let v_box = l.add_instance(BlockRef::Component(v), (3, 0));
    let u_box = l.add_instance(BlockRef::Component(u), (12, 0));
    for j in 0..cols {
        let pin = add(&mut l, Builtin::InputPin { name: format!("in{}", j + 1) }, (0, j as i32));
        link(&mut l, (pin, 0), (v_box, j));
        if let Some(&amplitude) = s.attenuation.get(j) {
            // Mode j's singular value, as an attenuator.
            let loss_db = -20.0 * amplitude.max(1e-12).log10();
            let a = add(&mut l, Builtin::Attenuator { loss_db }, (8, j as i32));
            link(&mut l, (v_box, j), (a, 0));
            link(&mut l, (a, 0), (u_box, j));
        } else {
            // More inputs than outputs: a mode the layer doesn't pass, absorbed.
            let dump = add(&mut l, Builtin::Detector, (8, j as i32));
            link(&mut l, (v_box, j), (dump, 0));
        }
    }
    // More outputs than inputs: U's other inputs stay dark.
    for i in 0..rows {
        let out = add(&mut l, Builtin::OutputPin { name: format!("out{}", i + 1) }, (17, i as i32));
        link(&mut l, (u_box, i), (out, 0));
    }
    let id = project.add_component(
        name,
        "W",
        &format!(
            "A {rows}×{cols} weight matrix in light: a {cols}×{cols} MZI mesh (V†), an attenuator per mode (the singular values, the largest passing everything) and a {rows}×{rows} mesh (U). It computes W x / {:.6}, W's largest singular value.",
            s.scale
        ),
        l,
        10.0,
    );
    Ok(Layer { id, scale: s.scale, columns: s.v_adjoint.depth() + s.u.depth() })
}

/// Unitary `u` (n × n) as one mesh component named `name`: inputs in1…,
/// outputs out1…, computing u x (times the arms' common loss).
pub fn unitary_mesh(project: &mut Project, name: &str, u: &Matrix) -> Result<DocId, String> {
    if u.nrows() != u.ncols() {
        return Err(format!("a unitary is square; this is {}×{}", u.nrows(), u.ncols()));
    }
    let mesh = UnitaryMesh::decompose_rectangular(u).map_err(|e| e.to_string())?;
    Ok(mesh_named(project, &mesh, name, "U"))
}

/// MZIs the meshes of a weight layer for a rows × cols matrix take: a
/// mesh of n modes has n(n−1)/2.
pub fn layer_mzis(rows: usize, cols: usize) -> usize {
    (rows * rows.saturating_sub(1) + cols * cols.saturating_sub(1)) / 2
}

/// A matrix written as text: a row per line, entries separated by spaces,
/// commas, semicolons or tabs; complex entries as `0.5-0.2i` (or `j`);
/// `#` starts a comment; blank lines are skipped.
pub fn parse_matrix(text: &str) -> Result<Matrix, String> {
    let mut rows: Vec<Vec<C64>> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        let row: Vec<C64> = line
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .filter(|t| !t.is_empty())
            .map(|t| {
                let t = t.replace('j', "i");
                t.parse::<f64>().map(|x| C64::new(x, 0.0)).or_else(|_| C64::from_str(&t)).map_err(|_| format!("line {}: “{t}” isn't a number", n + 1))
            })
            .collect::<Result<_, _>>()?;
        if row.is_empty() {
            continue;
        }
        if let Some(first) = rows.first()
            && first.len() != row.len()
        {
            return Err(format!("line {}: {} entries, but the first row has {}", n + 1, row.len(), first.len()));
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err("no numbers: write a row of the matrix per line".into());
    }
    Ok(Matrix::from_fn(rows.len(), rows[0].len(), |i, j| rows[i][j]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::circuit::sim::{self, Machine};

    /// The layer's transmission, with the arms' common loss divided out.
    fn transmission(p: &Project, layer: &Layer) -> Matrix {
        let d = p.doc(layer.id).unwrap();
        let t = sim::transmission(d.layout(), d.cell_um(), p, &Machine::default(), Some(layer.id), &[super::super::reference::WAVELENGTH_NM])
            .unwrap()
            .matrices
            .remove(0);
        let arms = super::super::digits::arm_amplitude().powi(layer.columns as i32);
        Matrix::from_fn(t.nrows(), t.ncols(), |i, j| t[(i, j)] / arms)
    }

    /// `a` is `b` times one common phase (the arms' propagation, the same
    /// for every path).
    fn close(a: &Matrix, b: &Matrix) -> bool {
        if a.nrows() != b.nrows() || a.ncols() != b.ncols() {
            return false;
        }
        let mut dot = C64::ZERO;
        for i in 0..a.nrows() {
            for j in 0..a.ncols() {
                dot += a[(i, j)] * b[(i, j)].conj();
            }
        }
        let phase = dot / dot.norm();
        let scaled = Matrix::from_fn(b.nrows(), b.ncols(), |i, j| b[(i, j)] * phase);
        (a - scaled).norm_l2() < 1e-9 * b.norm_l2().max(1.0)
    }

    #[test]
    fn layers_of_every_shape_compute_w_over_its_largest_singular_value() {
        let text = "0.5 -1 2\n0.25, 0, 1   # a comment\n\n";
        let wide = parse_matrix(text).unwrap();
        assert_eq!((wide.nrows(), wide.ncols()), (2, 3));
        let tall = wide.transpose().to_owned();
        let complex = parse_matrix("1+0.5i 0.2\n-0.3j 0.8-0.1i").unwrap();
        for (name, m) in [("wide", &wide), ("tall", &tall), ("complex", &complex)] {
            let mut p = Project::new();
            let layer = weight_layer(&mut p, name, m).unwrap();
            let want = Matrix::from_fn(m.nrows(), m.ncols(), |i, j| m[(i, j)] / layer.scale);
            let got = transmission(&p, &layer);
            assert!(close(&got, &want), "{name}: got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn a_unitary_becomes_one_mesh_and_mistakes_are_explained() {
        let u = parse_matrix("0 1\n1 0").unwrap();
        let mut p = Project::new();
        let id = unitary_mesh(&mut p, "Swap", &u).unwrap();
        assert_eq!(p.doc(id).unwrap().name, "Swap");
        assert!(unitary_mesh(&mut p, "Not", &parse_matrix("1 1\n0 1").unwrap()).is_err());
        assert!(parse_matrix("1 2\n3").unwrap_err().contains("line 2: 1 entries"));
        assert!(parse_matrix("1 x").unwrap_err().contains("“x” isn't a number"));
        assert!(weight_layer(&mut p, "Zero", &parse_matrix("0 0").unwrap()).unwrap_err().contains("all zeros"));
        assert_eq!(layer_mzis(64, 784), 2016 + 306_936);
    }
}
