//! `shine export`: a saved project's design in other tools' formats.

use std::path::{Path, PathBuf};

use super::{Failure, choose_doc, range};
use crate::panels::interop::{self, gdsfactory, sax, touchstone};
use crate::panels::project::Project;

pub const USAGE: &str = "\
Usage: shine export PROJECT.shine [--format FORMAT] [options]

Writes a design in a format other tools read.

Formats:
  touchstone   S-parameters between its light ports, as a Touchstone .sNp file,
               for circuit simulators and measurement tools
  gdsfactory   a layout description (.pic.yml) that gdsfactory turns into a mask
               layout (GDS or OASIS) with a foundry's process kit
  sax          a netlist for the SAX circuit simulator (.py), with models that
               compute what Shine does; swap them for a foundry kit's

Options:
  --format FORMAT                 touchstone, gdsfactory or sax (default: from
                                  --out's extension, else touchstone)
  --doc NAME                      the document (default: the first with light ports)
  --wavelength FROM:TO[:POINTS]   touchstone, sax: the wavelengths in nm (default
                                  1500:1600:401); sax prints the transmission at them
  --spacing UM                    gdsfactory: room between components in µm (default 40)
  --out FILE                      where to write (default: next to the project, named
                                  after the document)
  -h, --help                      this help

Exit status: 0 on success, 1 if the design can't be exported, 2 for a usage error.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Touchstone,
    Gdsfactory,
    Sax,
}

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub project: String,
    pub format: Option<Format>,
    pub doc: Option<String>,
    pub wavelength: Option<(f64, f64, usize)>,
    pub spacing: Option<f64>,
    pub out: Option<String>,
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
            "--format" => {
                a.format = Some(match value("--format")?.to_lowercase().as_str() {
                    "touchstone" | "snp" => Format::Touchstone,
                    "gdsfactory" | "gf" | "yaml" => Format::Gdsfactory,
                    "sax" | "py" | "python" => Format::Sax,
                    other => return Err(format!("unknown format “{other}” (touchstone, gdsfactory or sax)")),
                })
            }
            "--doc" => a.doc = Some(value("--doc")?),
            "--wavelength" => a.wavelength = Some(range(&value("--wavelength")?, 401)?),
            "--spacing" => {
                let s: f64 = value("--spacing")?.parse().map_err(|_| "--spacing needs a number of µm")?;
                if !(s >= 0.0 && s.is_finite()) {
                    return Err("--spacing can't be negative".into());
                }
                a.spacing = Some(s);
            }
            "--out" | "-o" => a.out = Some(value("--out")?),
            "--log-file" | "--log-level" => {
                if inline.is_none() {
                    it.next();
                }
            }
            f if f.starts_with('-') => return Err(format!("unknown option {f}")),
            path if a.project.is_empty() => a.project = path.to_owned(),
            extra => return Err(format!("unexpected argument “{extra}”")),
        }
    }
    if a.project.is_empty() && !a.help {
        return Err("which project? (shine export PROJECT.shine)".into());
    }
    Ok(a)
}

fn format_of(args: &Args) -> Format {
    args.format.unwrap_or_else(|| match &args.out {
        Some(out) if out.ends_with(".yml") || out.ends_with(".yaml") => Format::Gdsfactory,
        Some(out) if out.ends_with(".py") => Format::Sax,
        _ => Format::Touchstone,
    })
}

/// What was written: the file, and what didn't carry over.
#[derive(Debug, PartialEq)]
pub struct Written {
    pub path: PathBuf,
    pub summary: String,
    pub notes: Vec<String>,
}

pub fn export(args: &Args) -> Result<Written, Failure> {
    let bytes = std::fs::read(&args.project).map_err(|e| Failure::Usage(format!("can't read {}: {e}", args.project)))?;
    let project = Project::from_bytes(&bytes).map_err(|e| Failure::Usage(format!("{}: {e}", args.project)))?;
    let id = match &args.doc {
        Some(_) => choose_doc(&project, args.doc.as_deref()).map_err(Failure::Usage)?,
        None => interop::default_doc(&project).ok_or(Failure::Usage("the project is empty".into()))?,
    };
    let doc = project.doc(id).expect("chosen");
    let source = Path::new(&args.project).file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let next_to_project = |file: String| Path::new(&args.project).parent().unwrap_or(Path::new(".")).join(file);
    let stem = crate::panels::workspace::slug(&doc.name);
    match format_of(args) {
        Format::Touchstone => {
            let (from, to, points) = args.wavelength.unwrap_or((1500.0, 1600.0, 401));
            let (text, ports) =
                interop::touchstone(&project, id, &interop::wavelengths(from, to, points), &source).map_err(|e| Failure::Simulation(format!("“{}”: {e}", doc.name)))?;
            let path = match &args.out {
                Some(out) => {
                    if touchstone::ports_of(out).is_some_and(|n| n != ports) {
                        return Err(Failure::Usage(format!("“{}” has {ports} ports: name the file .s{ports}p", doc.name)));
                    }
                    PathBuf::from(out)
                }
                None => next_to_project(format!("{stem}.s{ports}p")),
            };
            std::fs::write(&path, text).map_err(|e| Failure::Usage(format!("can't write {}: {e}", path.display())))?;
            Ok(Written { summary: format!("S-parameters of “{}”: {ports} ports, {points} wavelengths", doc.name), path, notes: Vec::new() })
        }
        Format::Sax => {
            let (from, to, points) = args.wavelength.unwrap_or((1500.0, 1600.0, 401));
            let e = sax::export(&project, id, &interop::wavelengths(from, to, points), &source).map_err(|e| Failure::Simulation(format!("“{}”: {e}", doc.name)))?;
            let path = args.out.as_ref().map_or_else(|| next_to_project(format!("{}.py", stem.replace('-', "_"))), PathBuf::from);
            std::fs::write(&path, &e.python).map_err(|err| Failure::Usage(format!("can't write {}: {err}", path.display())))?;
            Ok(Written {
                summary: format!("SAX netlist of “{}”: {} components, {} connections, {} ports", doc.name, e.instances, e.connections, e.ports),
                path,
                notes: Vec::new(),
            })
        }
        Format::Gdsfactory => {
            let e = gdsfactory::export(&project, id, args.spacing.unwrap_or(gdsfactory::DEFAULT_SPACING_UM), &source).map_err(Failure::Simulation)?;
            let path = args.out.as_ref().map_or_else(|| next_to_project(format!("{stem}.pic.yml")), PathBuf::from);
            std::fs::write(&path, &e.yaml).map_err(|err| Failure::Usage(format!("can't write {}: {err}", path.display())))?;
            Ok(Written {
                summary: format!("layout of “{}” for gdsfactory: {} components, {} waveguides, {} ports", doc.name, e.instances, e.routes, e.ports),
                path,
                notes: e.notes,
            })
        }
    }
}

/// `shine export …` from the binary (before logging starts, so it prints).
pub fn main(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(a) if a.help => {
            println!("{USAGE}");
            return 0;
        }
        Ok(a) => a,
        Err(m) => {
            eprintln!("shine export: {m}\nTry: shine export --help");
            return 2;
        }
    };
    match export(&parsed) {
        Ok(w) => {
            println!("Wrote {}: {}", w.path.display(), w.summary);
            for n in &w.notes {
                eprintln!("  note: {n}");
            }
            0
        }
        Err(f) => {
            let (Failure::Usage(m) | Failure::Simulation(m)) = &f;
            eprintln!("shine export: {m}");
            f.exit_code()
        }
    }
}
