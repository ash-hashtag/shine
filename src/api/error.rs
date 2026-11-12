use std::fmt;

/// What went wrong, in words that say what would have worked.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// No design or component of that name; `available` lists the names there are.
    NoSuchDesign { name: String, available: Vec<String> },
    /// No part of that name in a design.
    NoSuchPart { design: String, name: String, available: Vec<String> },
    /// A part has no port of that name.
    NoSuchPort { part: String, port: String, available: Vec<String> },
    /// A part has no parameter of that name.
    NoSuchParameter { part: String, param: String, available: Vec<String> },
    /// Ports that can't be connected, or a part that can't go where it was asked.
    Wiring(String),
    /// A value outside what a block can take.
    Invalid(String),
    /// The simulation couldn't run or didn't settle.
    Simulation(String),
    /// A file that couldn't be read or written.
    File(String),
    /// Something this build can't do (the browser has no files, for example).
    Unsupported(String),
}

fn list(names: &[String]) -> String {
    if names.is_empty() { "none".into() } else { names.join(", ") }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoSuchDesign { name, available } => write!(f, "no design or component “{name}”; there are: {}", list(available)),
            Error::NoSuchPart { design, name, available } => write!(f, "“{design}” has no part “{name}”; it has: {}", list(available)),
            Error::NoSuchPort { part, port, available } => write!(f, "“{part}” has no port “{port}”; it has: {}", list(available)),
            Error::NoSuchParameter { part, param, available } => {
                write!(f, "“{part}” has no parameter “{param}”; it has: {}", list(available))
            }
            Error::Wiring(m) | Error::Invalid(m) | Error::Simulation(m) | Error::File(m) | Error::Unsupported(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
