//! `shine network`: a trained two-layer neural network as a photonic chip.

use crate::panels::optics::digits::{self, Weights};
use crate::panels::optics::layers;
use crate::panels::project::Project;

pub const USAGE: &str = "\
Usage: shine network WEIGHTS.json --out CHIP.shine [--name NAME]

Builds a trained two-layer neural network as a photonic chip and saves it as
a project: a laser split into one light per input, a modulator per input
driven by a value input, the first layer as MZI meshes, a photodetector and
electronics per neuron, the second layer, and a display showing the class.
The chip computes

    h = clamp(|W1 x|² / H, 0, 1)
    o = |W2 h|²
    class = argmax(o)

so the network must be trained for that (see the guide, \"Your neural network
on a photonic chip\"). Its inputs, in order, are set with
`shine simulate CHIP.shine --input -`.

WEIGHTS.json holds
  w1             the first layer, hidden × inputs (a row per neuron)
  w2             the second layer, classes × hidden
  hidden_scale   H
  classes        the classes' names (optional)
  dataset        what it was trained on, naming the chip (optional)
  test_accuracy  (optional)

Options:
  --out FILE     where to save the chip (a new project; replaced if it exists)
  --name NAME    the chip's name (default: from the dataset)
  -h, --help     this help

Exit status: 0 on success, 1 if the network can't be built, 2 for a usage error.";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub weights: String,
    pub out: Option<String>,
    pub name: Option<String>,
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
            "--log-file" | "--log-level" => {
                if inline.is_none() {
                    it.next();
                }
            }
            f if f.starts_with('-') => return Err(format!("unknown option {f}")),
            path if a.weights.is_empty() => a.weights = path.to_owned(),
            extra => return Err(format!("unexpected argument “{extra}”")),
        }
    }
    if !a.help {
        if a.weights.is_empty() {
            return Err("which weights? (shine network WEIGHTS.json --out CHIP.shine)".into());
        }
        if a.out.is_none() {
            return Err("where to? (--out CHIP.shine)".into());
        }
    }
    Ok(a)
}

/// What `shine network` did, to say.
pub fn run(a: &Args) -> Result<String, super::Failure> {
    use super::Failure;
    let text = std::fs::read_to_string(&a.weights).map_err(|e| Failure::Usage(format!("can't read {}: {e}", a.weights)))?;
    let w = Weights::from_json(&text).map_err(|e| Failure::Usage(format!("{}: {e}", a.weights)))?;
    let (inputs, hidden, classes) = w.sizes();
    let mut project = Project::new();
    let name = a.name.clone().unwrap_or_else(|| digits::chip_name(&w));
    digits::chip_named(&mut project, &name, &w, &vec![0.0; inputs]).map_err(Failure::Simulation)?;
    let out = a.out.as_deref().expect("parse checks it");
    project.write_atomically(std::path::Path::new(out)).map_err(|e| Failure::Usage(format!("can't write {out}: {e}")))?;
    let mzis = layers::layer_mzis(hidden, inputs) + layers::layer_mzis(classes, hidden);
    Ok(format!(
        "Wrote “{name}” to {out}: a {inputs} → {hidden} → {classes} network on {mzis} MZIs. Its {inputs} inputs, in order: shine simulate {out} --inputs"
    ))
}

/// `shine network …` from the binary.
pub fn main(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(a) if a.help => {
            println!("{USAGE}");
            return 0;
        }
        Ok(a) => a,
        Err(m) => {
            eprintln!("shine network: {m}\nTry: shine network --help");
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
            eprintln!("shine network: {m}");
            f.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;

    #[test]
    fn weights_become_a_chip_that_classifies_as_the_network_does() {
        let dir = std::env::temp_dir().join(format!("shine-network-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (weights, chip) = (dir.join("weights.json"), dir.join("chip.shine"));
        std::fs::write(
            &weights,
            r#"{"w1": [[0.9, -0.4, 0.2, 0.1, -0.6], [0.3, 0.8, -0.5, 0.4, 0.2], [-0.2, 0.1, 0.7, -0.9, 0.5]],
                "w2": [[1.0, -0.5, 0.3], [-0.4, 0.9, 0.6]], "hidden_scale": 1.5}"#,
        )
        .unwrap();
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let said = run(&parse(&args(&[weights.to_str().unwrap(), "--out", chip.to_str().unwrap()])).unwrap()).unwrap();
        assert!(said.contains("“Optical neural network”") && said.contains("a 5 → 3 → 2 network on 17 MZIs"), "{said}");
        // Three samples through the chip: the display shows the network's class.
        let samples = dir.join("samples.txt");
        std::fs::write(&samples, "1 0 0.5 0 0.2\n0 1 0 0.8 0\n0.3 0.3 0.9 0 1\n").unwrap();
        let csv = cli::simulate(&cli::parse(&args(&[chip.to_str().unwrap(), "--input", samples.to_str().unwrap(), "--only", "Display"])).unwrap()).unwrap();
        let classes: Vec<&str> = csv.lines().skip(1).map(|l| l.rsplit(',').next().unwrap()).collect();
        assert_eq!(classes, ["0", "1", "1"], "{csv}");
        // Layers may widen: 2 → 3 → 2, with the network's classes.
        std::fs::write(&weights, r#"{"w1": [[1.0, 0.2], [0.3, -0.8], [0.5, 0.5]], "w2": [[1.0, 0.0, 0.2], [0.0, 1.0, 0.1]], "hidden_scale": 1.0, "dataset": "Toy"}"#).unwrap();
        let said = run(&parse(&args(&[weights.to_str().unwrap(), "--out", chip.to_str().unwrap()])).unwrap()).unwrap();
        assert!(said.contains("“Toy classifier”") && said.contains("2 → 3 → 2"), "{said}");
        std::fs::write(&samples, "1 0
0 1
").unwrap();
        let csv = cli::simulate(&cli::parse(&args(&[chip.to_str().unwrap(), "--input", samples.to_str().unwrap(), "--only", "Display"])).unwrap()).unwrap();
        let classes: Vec<&str> = csv.lines().skip(1).map(|l| l.rsplit(',').next().unwrap()).collect();
        // x = (1, 0): h = (1, 0.09, 0.25), o = (1.05², 0.115²): class 0; x = (0, 1): h = (0.04, 0.64, 0.25), o = (0.09², 0.665²): class 1.
        assert_eq!(classes, ["0", "1"], "{csv}");
        // Mistakes are explained.
        assert!(parse(&args(&["w.json"])).unwrap_err().contains("--out"));
        std::fs::write(&weights, "{}").unwrap();
        assert!(matches!(run(&parse(&args(&[weights.to_str().unwrap(), "--out", chip.to_str().unwrap()])).unwrap()), Err(cli::Failure::Usage(_))));
        std::fs::remove_dir_all(dir).ok();
    }
}
