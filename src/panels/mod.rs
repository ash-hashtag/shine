pub mod beam_sim;
pub mod chips;
pub mod empty;
pub mod file_format;
pub mod history;
pub mod optics;
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
}
