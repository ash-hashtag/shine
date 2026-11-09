pub mod beam_sim;
pub mod chips;
pub mod circuit;
pub mod empty;
pub mod file_format;
pub mod history;
pub mod optics;
pub mod prefs;
pub mod project;
pub mod schematic;
pub mod selection;
pub mod ui_kit;
pub mod workspace;

/// Things a document editor asks its host (the workspace) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Place example component `example` (adding its document if needed) at a cell.
    PlaceExample { example: usize, cell: (i32, i32) },
    /// Advance the clock one tick.
    Tick,
    /// Run the sweep set up in the editor's sweep window.
    RunSweep,
}
