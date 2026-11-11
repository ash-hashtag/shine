pub mod about;
pub mod appearance;
pub mod assets;
pub mod beam_sim;
pub mod chips;
pub mod circuit;
pub mod empty;
pub mod export;
pub mod file_format;
pub mod history;
pub mod interop;
pub mod models;
pub mod optics;
pub mod perf;
pub mod prefs;
pub mod project;
pub mod schematic;
pub mod script;
pub mod selection;
pub mod session;
pub mod ui_kit;
pub mod units;
pub mod workspace;

/// Things a document editor asks its host (the workspace) to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    New,
    Open,
    Save,
    ToggleLibrary,
    ToggleInspector,
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
