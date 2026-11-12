//! Shine's design model, simulation, analysis and files (feature `core`), and
//! the app built on them (feature `gui`).

#[cfg(feature = "gui")]
pub mod about;
pub mod appearance;
#[cfg(feature = "gui")]
pub mod assets;
pub mod beam_sim;
#[cfg(feature = "gui")]
pub mod chips;
pub mod circuit;
#[cfg(feature = "gui")]
pub mod empty;
#[cfg(feature = "gui")]
pub mod export;
pub mod file_format;
#[cfg(feature = "gui")]
pub mod history;
pub mod interop;
pub mod models;
pub mod names;
pub mod optics;
#[cfg(feature = "gui")]
pub mod perf;
#[cfg(feature = "gui")]
pub mod prefs;
pub mod project;
pub mod schematic;
pub mod script;
#[cfg(feature = "gui")]
pub mod selection;
#[cfg(feature = "gui")]
pub mod session;
#[cfg(feature = "gui")]
pub mod ui_kit;
pub mod units;
#[cfg(feature = "gui")]
pub mod workspace;

/// Things a document editor asks its host (the workspace) to do.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    New,
    Open,
    Save,
    ToggleLibrary,
    ToggleInspector,
    /// Show the inspector (a built-in block's settings are there).
    ShowInspector,
    ShowShortcuts,
    /// Open this document in a tab.
    Inspect(project::DocId),
    /// Start a new component document.
    NewComponent,
    /// Place a component from the asset library (copying it into the
    /// project if needed) at a cell.
    PlaceAsset { asset: assets::AssetRef, cell: (i32, i32) },
    /// Advance the clock one tick.
    Tick,
    /// Run the sweep set up in the editor's sweep window.
    RunSweep,
    /// Save the part of the window under `rect` (in points) as a PNG image.
    SaveImage { name: &'static str, rect: egui::Rect },
}
