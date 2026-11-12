//! `shine layer`: a matrix as a component of MZI meshes, written to a project.

use crate::panels::optics::layers;
use crate::panels::project::Project;

pub const USAGE: &str = "\
Usage: shine layer MATRIX --out PROJECT.shine [--name NAME] [--unitary]

Turns a matrix into a component that multiplies light by it, built from MZI
meshes, and adds it to a project (made if it doesn't exist).

A weight layer computes W x / σmax: W = U Σ V†, two meshes of MZIs (the
rectangular, Clements, arrangement) with an attenuator per mode between them,
divided by W's largest singular value σmax as light can't be amplified. Any
shape, real or complex. With --unitary, a unitary matrix as one mesh.

MATRIX is a text file, or - for standard input: a row per line, entries
separated by spaces, commas or tabs, complex ones as 0.5-0.2i; '#' starts a
comment.

Options:
  --out FILE      the project to add it to (made if it doesn't exist)
  --name NAME     the component's name (default: Weights, or Mesh with --unitary)
  --unitary       one mesh for a unitary matrix, rather than a weight layer
  -h, --help      this help

Exit status: 0 on success, 1 if the matrix can't be built, 2 for a usage error.";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub matrix: String,
    pub out: Option<String>,
    pub name: Option<String>,
    pub unitary: bool,
    pub help: bool,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| inline.clone().or_else(|| it.next().cloned()).ok_or(format!("{name} needs a value"));
        match flag {
            "-h" | "--help" => a.help = true,
            "--out" => a.out = Some(value("--out")?),
            "--name" => a.name = Some(value("--name")?),
            "--unitary" => a.unitary = true,
            "--log-file" | "--log-level" => {
                if inline.is_none() {
                    it.next();
                }
            }
            f if f.starts_with('-') && f != "-" => return Err(format!("unknown option {f}")),
            path if a.matrix.is_empty() => a.matrix = path.to_owned(),
            extra => return Err(format!("unexpected argument “{extra}”")),
        }
    }
    if !a.help {
        if a.matrix.is_empty() {
            return Err("which matrix? (shine layer MATRIX --out PROJECT.shine)".into());
        }
        if a.out.is_none() {
            return Err("where to? (--out PROJECT.shine)".into());
        }
    }
    Ok(a)
}

/// What `shine layer` did, to say.
pub fn run(a: &Args) -> Result<String, super::Failure> {
    use super::Failure;
    let text = if a.matrix == "-" {
        std::io::read_to_string(std::io::stdin()).map_err(|e| Failure::Usage(format!("can't read standard input: {e}")))?
    } else {
        std::fs::read_to_string(&a.matrix).map_err(|e| Failure::Usage(format!("can't read {}: {e}", a.matrix)))?
    };
    let m = layers::parse_matrix(&text).map_err(|e| Failure::Usage(format!("{}: {e}", a.matrix)))?;
    let out = a.out.as_deref().expect("parse checks it");
    let mut project = match std::fs::read(out) {
        Ok(bytes) => Project::from_bytes(&bytes).map_err(|e| Failure::Usage(format!("{out}: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Project::new(),
        Err(e) => return Err(Failure::Usage(format!("can't read {out}: {e}"))),
    };
    let (rows, cols) = (m.nrows(), m.ncols());
    let summary = if a.unitary {
        let id = layers::unitary_mesh(&mut project, a.name.as_deref().unwrap_or("Mesh"), &m).map_err(Failure::Simulation)?;
        format!("“{}”, a {rows}×{rows} mesh of {} MZIs", project.doc(id).expect("added").name, rows * rows.saturating_sub(1) / 2)
    } else {
        let layer = layers::weight_layer(&mut project, a.name.as_deref().unwrap_or("Weights"), &m).map_err(Failure::Simulation)?;
        format!(
            "“{}”, a {rows}×{cols} weight layer of {} MZIs computing W x / {:.6} (W's largest singular value)",
            project.doc(layer.id).expect("added").name,
            layers::layer_mzis(rows, cols),
            layer.scale
        )
    };
    project.write_atomically(std::path::Path::new(out)).map_err(|e| Failure::Usage(format!("can't write {out}: {e}")))?;
    Ok(format!("Added {summary} to {out}"))
}

/// `shine layer …` from the binary.
pub fn main(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(a) if a.help => {
            println!("{USAGE}");
            return 0;
        }
        Ok(a) => a,
        Err(m) => {
            eprintln!("shine layer: {m}\nTry: shine layer --help");
            return 2;
        }
    };
    match run(&parsed) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(f) => {
            let (super::Failure::Usage(m) | super::Failure::Simulation(m)) = &f;
            eprintln!("shine layer: {m}");
            f.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matrix_file_becomes_a_layer_in_a_project() {
        let dir = std::env::temp_dir().join(format!("shine-layer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (matrix, out) = (dir.join("w.txt"), dir.join("layers.shine"));
        std::fs::write(&matrix, "# 2×3\n0.5 -1 2\n0.25 0 1\n").unwrap();
        let args = |extra: &[&str]| -> Vec<String> {
            [matrix.to_str().unwrap(), "--out", out.to_str().unwrap()].iter().chain(extra).map(|s| s.to_string()).collect()
        };
        let said = run(&parse(&args(&[])).unwrap()).unwrap();
        assert!(said.contains("“Weights”, a 2×3 weight layer of 4 MZIs"), "{said}");
        // Again, into the same project: a second layer beside the first.
        run(&parse(&args(&["--name", "Second"])).unwrap()).unwrap();
        let p = Project::from_bytes(&std::fs::read(&out).unwrap()).unwrap();
        assert!(p.find("Weights").is_some() && p.find("Second").is_some());
        std::fs::write(&matrix, "1 1\n0 1\n").unwrap();
        let e = run(&parse(&args(&["--unitary"])).unwrap()).unwrap_err();
        assert!(matches!(e, crate::cli::Failure::Simulation(_)), "{e:?}");
        assert!(parse(&["w.txt".to_owned()]).unwrap_err().contains("--out"));
        std::fs::remove_dir_all(dir).ok();
    }
}
