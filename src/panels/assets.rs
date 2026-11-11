//! The asset library: components kept across projects, in folders.
//!
//! Each asset is a component with every component it uses, stored as a
//! small project whose first document is the component (the same bytes as a
//! `.shinecomp` file). Placing one copies it into the open project. The
//! components that ship with Shine are listed as the read-only "Examples"
//! folder, and the SiEPIC EBeam foundry kit's compact models (MIT licence)
//! as the read-only "SiEPIC EBeam" folder. The library lives in a file next to the app's settings on
//! desktop, and in the page's storage in the browser.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::optics::examples::{component_examples, import_component};
use super::project::{CatalogEntry, DocId, Project};

/// Extension of a component file: one component and what it uses.
pub const COMPONENT_EXTENSION: &str = "shinecomp";
/// The library's file, in the app's settings folder.
pub const LIBRARY_FILE: &str = "library.shinelib";
/// Storage key of the library in the browser.
pub const STORAGE_KEY: &str = "shine.library";
/// Folder of the components that ship with Shine.
pub const EXAMPLES_FOLDER: &str = "Examples";
/// Folder of the SiEPIC EBeam kit's compact models that ship with Shine.
pub const KIT_FOLDER: &str = "SiEPIC EBeam";

/// Folders that ship with Shine: read-only, always listed first.
pub const BUILT_IN_FOLDERS: [&str; 2] = [EXAMPLES_FOLDER, KIT_FOLDER];

/// The SiEPIC EBeam PDK's compact models for its nominal process (220 nm
/// silicon, 500 nm strip waveguides, TE, C band), as published in
/// github.com/SiEPIC/SiEPIC_EBeam_PDK under the MIT licence (see
/// library/siepic-ebeam/README.md for where each comes from).
const KIT_FILES: &[(&str, &[u8])] = &[
    ("Broadband 50-50 coupler.sparam", include_bytes!("../../library/siepic-ebeam/Broadband 50-50 coupler.sparam")),
    ("Y-branch.sparam", include_bytes!("../../library/siepic-ebeam/Y-branch.sparam")),
    ("Directional coupler, gap 200 nm, length 0 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 0 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 5 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 5 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 10 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 10 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 15 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 15 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 20 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 20 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 25 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 25 µm.sparam")),
    ("Directional coupler, gap 200 nm, length 30 µm.sparam", include_bytes!("../../library/siepic-ebeam/Directional coupler, gap 200 nm, length 30 µm.sparam")),
    ("Ring coupler, gap 100 nm, radius 10 µm.dat", include_bytes!("../../library/siepic-ebeam/Ring coupler, gap 100 nm, radius 10 µm.dat")),
    ("Ring coupler, gap 100 nm, radius 5 µm.dat", include_bytes!("../../library/siepic-ebeam/Ring coupler, gap 100 nm, radius 5 µm.dat")),
    ("Ring coupler, gap 150 nm, radius 10 µm.dat", include_bytes!("../../library/siepic-ebeam/Ring coupler, gap 150 nm, radius 10 µm.dat")),
];

const KIT_CREDIT: &str = "From the SiEPIC EBeam PDK (FDTD-simulated compact model; MIT licence, © 2016–2020 Lukas Chrostowski and contributors).";

/// The kit's components as bundles (bytes) with their summaries, read once.
fn kit() -> &'static [(Vec<u8>, AssetSummary)] {
    static KIT: std::sync::OnceLock<Vec<(Vec<u8>, AssetSummary)>> = std::sync::OnceLock::new();
    KIT.get_or_init(|| {
        KIT_FILES
            .iter()
            .enumerate()
            .filter_map(|(i, (name, bytes))| {
                let model = super::models::ModelSpec::from_file(name, bytes).map_err(|e| tracing::error!("{name}: {e}")).ok()?;
                let mut bundle = super::models::component(model);
                let root = bundle.documents.first_mut()?;
                if let Some(meta) = root.component.as_mut() {
                    meta.description = format!("{} {KIT_CREDIT}", meta.description);
                }
                let summary = summarize(AssetRef::Kit(i), KIT_FOLDER, &bundle).ok()?;
                Some((bundle.to_bytes(), summary))
            })
            .collect()
    })
}

/// Folder for assets added without one.
pub const DEFAULT_FOLDER: &str = "My components";

const MAGIC: [u8; 4] = *b"SHNL";
const VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetId(pub u64);

/// An asset to place: one of Shine's examples or one from the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetRef {
    Example(usize),
    /// One of the bundled foundry kit's components.
    Kit(usize),
    Saved(AssetId),
}

impl AssetRef {
    /// Whether it ships with Shine (read-only, never moved or deleted).
    pub fn is_built_in(self) -> bool {
        !matches!(self, Self::Saved(_))
    }
}

/// What the library shows about an asset, without decoding it again.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetSummary {
    pub asset: AssetRef,
    pub folder: String,
    /// The component as a catalog entry of its bundle (its name, initials,
    /// description, ports and appearance).
    pub entry: CatalogEntry,
}

impl AssetSummary {
    /// Whether it matches a search: by name, description, folder or a port name.
    pub fn matches(&self, search: &str) -> bool {
        let search = search.trim().to_lowercase();
        if search.is_empty() {
            return true;
        }
        let e = &self.entry;
        let i = &e.interface;
        std::iter::once(&e.name)
            .chain([&e.description, &self.folder, &e.initials])
            .chain(i.inputs.iter().chain(&i.outputs).chain(&i.signal_inputs).chain(&i.signal_outputs))
            .chain(e.parameters.iter().map(|p| &p.name))
            .any(|text| text.to_lowercase().contains(&search))
    }

    pub fn ports(&self) -> String {
        ports_text(&self.entry.interface)
    }
}

/// "2 light in, 2 out · 1 signals in, 0 out", or "no ports".
pub fn ports_text(i: &super::project::Interface) -> String {
    let mut parts = Vec::new();
    if !i.inputs.is_empty() || !i.outputs.is_empty() {
        parts.push(format!("{} light in, {} out", i.inputs.len(), i.outputs.len()));
    }
    if !i.signal_inputs.is_empty() || !i.signal_outputs.is_empty() {
        parts.push(format!("{} signals in, {} out", i.signal_inputs.len(), i.signal_outputs.len()));
    }
    if parts.is_empty() { "no ports".into() } else { parts.join(" · ") }
}

struct Asset {
    id: AssetId,
    folder: String,
    /// The component's project bytes (as in a `.shinecomp` file).
    bundle: Vec<u8>,
    summary: AssetSummary,
}

#[derive(Serialize, Deserialize)]
struct AssetFile {
    id: AssetId,
    folder: String,
    bundle: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    next_id: u64,
    assets: Vec<AssetFile>,
}

pub struct Library {
    assets: Vec<Asset>,
    next_id: u64,
    examples: Vec<AssetSummary>,
    /// Where it is kept (desktop); `None` keeps it in memory, or in the
    /// app's storage (browser).
    pub path: Option<PathBuf>,
    /// Changed since it was last stored.
    dirty: bool,
    /// Every summary, examples first; rebuilt on change.
    summaries: Arc<Vec<AssetSummary>>,
    /// Counts changes, so views can cache what they show.
    generation: u64,
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

/// The summary of a bundle's component, or why it isn't one.
fn summarize(asset: AssetRef, folder: &str, bundle: &Project) -> Result<AssetSummary, String> {
    let root = bundle.documents.first().ok_or("the file has no component")?;
    if root.component.is_none() {
        return Err(format!("“{}” is a layout, not a component", root.name));
    }
    let entry = bundle.catalog().get(root.id).cloned().ok_or("the file has no component")?;
    Ok(AssetSummary { asset, folder: folder.to_owned(), entry })
}

impl Library {
    /// An empty library (with the examples).
    pub fn new() -> Self {
        let examples = (0..component_examples().len())
            .filter_map(|i| {
                let mut p = Project::new();
                let id = import_component(&mut p, i)?;
                summarize(AssetRef::Example(i), EXAMPLES_FOLDER, &p.extract(id)?).ok()
            })
            .chain(kit().iter().map(|(_, s)| s.clone()))
            .collect();
        let mut library = Self { assets: Vec::new(), next_id: 1, examples, path: None, dirty: false, summaries: Arc::default(), generation: 0 };
        library.refresh();
        library
    }

    fn refresh(&mut self) {
        self.generation += 1;
        self.summaries = Arc::new(self.examples.iter().cloned().chain(self.assets.iter().map(|a| a.summary.clone())).collect());
    }

    fn changed(&mut self) {
        self.refresh();
        self.dirty = true;
        if let Some(path) = &self.path {
            let written = write_atomically(path, &self.to_bytes());
            match written {
                Ok(()) => self.dirty = false,
                Err(e) => tracing::error!("Couldn't save the asset library to {}: {e}", path.display()),
            }
        }
    }

    /// Every asset, the examples first, in folder order of insertion.
    pub fn summaries(&self) -> Arc<Vec<AssetSummary>> {
        self.summaries.clone()
    }

    /// Changes whenever the library does.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many assets each folder holds.
    pub fn folder_counts(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for a in &self.assets {
            *counts.entry(a.folder.as_str()).or_default() += 1;
        }
        let mut counts: Vec<(String, usize)> = counts.into_iter().map(|(f, n)| (f.to_owned(), n)).collect();
        counts.sort_by_key(|(f, _)| f.to_lowercase());
        counts
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// The folders of saved assets, sorted (the examples' folder excluded).
    pub fn folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = self.assets.iter().map(|a| a.folder.clone()).collect();
        folders.sort_by_key(|f| f.to_lowercase());
        folders.dedup();
        folders
    }

    /// The component of an asset, as a project with it first.
    pub fn bundle(&self, asset: AssetRef) -> Option<Project> {
        match asset {
            AssetRef::Example(i) => {
                let mut p = Project::new();
                let id = import_component(&mut p, i)?;
                p.extract(id)
            }
            AssetRef::Kit(_) => Project::from_bytes(&kit().iter().find(|k| k.1.asset == asset)?.0).ok(),
            AssetRef::Saved(id) => Project::from_bytes(&self.assets.iter().find(|a| a.id == id)?.bundle).ok(),
        }
    }

    /// The bytes of a `.shinecomp` file for an asset.
    pub fn file_bytes(&self, asset: AssetRef) -> Option<Vec<u8>> {
        match asset {
            AssetRef::Saved(id) => self.assets.iter().find(|a| a.id == id).map(|a| a.bundle.clone()),
            AssetRef::Kit(_) => kit().iter().find(|k| k.1.asset == asset).map(|k| k.0.clone()),
            AssetRef::Example(_) => self.bundle(asset).map(|p| p.to_bytes()),
        }
    }

    /// Add a component (with what it uses) to `folder`. An identical one
    /// already there is kept instead; returns its id and whether it is new.
    pub fn add(&mut self, bundle: &Project, folder: &str) -> Result<(AssetId, bool), String> {
        let added = self.insert(bundle, folder)?;
        if added.1 {
            self.changed();
        }
        Ok(added)
    }

    /// [`Self::add`] without saving, for adding many at once.
    fn insert(&mut self, bundle: &Project, folder: &str) -> Result<(AssetId, bool), String> {
        let folder = if folder.trim().is_empty() { DEFAULT_FOLDER } else { folder.trim() };
        let bytes = bundle.to_bytes();
        if let Some(a) = self.assets.iter().find(|a| a.bundle == bytes) {
            return Ok((a.id, false));
        }
        let id = AssetId(self.next_id);
        let summary = summarize(AssetRef::Saved(id), folder, bundle)?;
        self.next_id += 1;
        self.assets.push(Asset { id, folder: folder.to_owned(), bundle: bytes, summary });
        Ok((id, true))
    }

    /// Add the components of a file: a `.shinecomp` (one component) or a
    /// project (each of its components). Returns how many were new.
    pub fn import_file(&mut self, name: &str, bytes: &[u8], folder: &str) -> Result<usize, String> {
        let added = self.insert_file(name, bytes, folder)?;
        if added > 0 {
            self.changed();
        }
        Ok(added)
    }

    /// Add many files at once, `(name, bytes, folder)` each, saving once.
    /// Returns how many components were new and the files that couldn't be used.
    pub fn import_files(&mut self, files: impl IntoIterator<Item = (String, Vec<u8>, String)>) -> (usize, Vec<String>) {
        let mut added = 0;
        let mut failed = Vec::new();
        for (name, bytes, folder) in files {
            match self.insert_file(&name, &bytes, &folder) {
                Ok(n) => added += n,
                Err(e) => failed.push(format!("{name}: {e}")),
            }
        }
        if added > 0 {
            self.changed();
        }
        (added, failed)
    }

    fn insert_file(&mut self, name: &str, bytes: &[u8], folder: &str) -> Result<usize, String> {
        // S-parameter files become components around their data.
        if super::models::is_model_file(name) && !Project::is_project(bytes) {
            let model = super::models::ModelSpec::from_file(name, bytes).map_err(|e| format!("{name}: {e}"))?;
            let bundle = super::models::component(model);
            return Ok(usize::from(self.insert(&bundle, folder)?.1));
        }
        if !Project::is_project(bytes) {
            return Err(format!("{name} isn't a Shine component or project"));
        }
        let project = Project::from_bytes(bytes)?;
        let roots: Vec<DocId> = if is_component_file(name, &project) {
            project.documents.first().map(|d| d.id).into_iter().collect()
        } else {
            project.documents.iter().filter(|d| d.component.is_some()).map(|d| d.id).collect()
        };
        if roots.is_empty() {
            return Err(format!("{name} has no components (publish a layout as a component first)"));
        }
        let mut added = 0;
        for root in roots {
            let bundle = project.extract(root).ok_or("missing component")?;
            if self.insert(&bundle, folder)?.1 {
                added += 1;
            }
        }
        Ok(added)
    }

    pub fn remove(&mut self, id: AssetId) {
        self.assets.retain(|a| a.id != id);
        self.changed();
    }

    pub fn set_folder(&mut self, id: AssetId, folder: &str) {
        let folder = if folder.trim().is_empty() { DEFAULT_FOLDER } else { folder.trim() };
        if let Some(a) = self.assets.iter_mut().find(|a| a.id == id) {
            a.folder = folder.to_owned();
            a.summary.folder = folder.to_owned();
            self.changed();
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let file = LibraryFile {
            next_id: self.next_id,
            assets: self.assets.iter().map(|a| AssetFile { id: a.id, folder: a.folder.clone(), bundle: a.bundle.clone() }).collect(),
        };
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        postcard::to_extend(&file, bytes).expect("in-memory serialisation cannot fail")
    }

    /// A library from its bytes. Assets that can't be read any more are
    /// skipped (and logged) rather than losing the rest.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 6 || bytes[..4] != MAGIC {
            return Err("not a Shine asset library".into());
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(format!("asset library format {version} isn't supported; update Shine"));
        }
        let file: LibraryFile = postcard::from_bytes(&bytes[6..]).map_err(|e| format!("damaged asset library: {e}"))?;
        let mut library = Self::new();
        library.next_id = file.next_id;
        for a in file.assets {
            let summary = Project::from_bytes(&a.bundle).and_then(|p| summarize(AssetRef::Saved(a.id), &a.folder, &p));
            match summary {
                Ok(summary) => library.assets.push(Asset { id: a.id, folder: a.folder, bundle: a.bundle, summary }),
                Err(e) => tracing::warn!("Skipped an asset that can't be read: {e}"),
            }
        }
        library.next_id = library.next_id.max(library.assets.iter().map(|a| a.id.0 + 1).max().unwrap_or(1));
        library.refresh();
        Ok(library)
    }

    /// The library kept at `path` (empty if there's none yet), saved there
    /// on every change.
    pub fn open_file(path: PathBuf) -> Self {
        let mut library = match std::fs::read(&path) {
            Ok(bytes) => Self::from_bytes(&bytes).unwrap_or_else(|e| {
                tracing::error!("Couldn't read the asset library {}: {e}", path.display());
                // Keep the unreadable file for recovery rather than overwrite it.
                let mut aside = path.as_os_str().to_owned();
                aside.push(".unreadable");
                let _ = std::fs::rename(&path, std::path::PathBuf::from(aside));
                Self::new()
            }),
            Err(_) => Self::new(),
        };
        library.path = Some(path);
        library
    }

    /// The library for the app's storage, if it changed since last asked.
    pub fn take_changes(&mut self) -> Option<Vec<u8>> {
        std::mem::take(&mut self.dirty).then(|| self.to_bytes())
    }
}

/// Whether a file holds one component (rather than a whole project): by its
/// extension, or, without one, when its first document is a component and
/// the rest are what it uses.
pub fn is_component_file(name: &str, project: &Project) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with(&format!(".{COMPONENT_EXTENSION}")) {
        return true;
    }
    if lower.ends_with(&format!(".{}", super::file_format::EXTENSION)) {
        return false;
    }
    project.documents.first().is_some_and(|root| {
        root.component.is_some() && project.extract(root.id).is_some_and(|b| b.documents.len() == project.documents.len())
    })
}

/// The component and project files in `dir` and its subfolders (not hidden
/// ones, at most 8 deep), each with the subfolders it is in.
#[cfg(not(target_arch = "wasm32"))]
pub fn component_files_in(dir: &std::path::Path) -> Vec<(PathBuf, Vec<String>)> {
    fn walk(dir: &std::path::Path, sub: &[String], out: &mut Vec<(PathBuf, Vec<String>)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        // A folder's own files first, then its subfolders.
        entries.sort_by_key(|e| (e.path().is_dir(), e.file_name()));
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                if sub.len() < 8 {
                    let mut deeper = sub.to_vec();
                    deeper.push(name);
                    walk(&path, &deeper, out);
                }
            } else {
                let lower = name.to_lowercase();
                if lower.ends_with(&format!(".{COMPONENT_EXTENSION}")) || lower.ends_with(&format!(".{}", super::file_format::EXTENSION)) || super::models::is_model_file(&lower) {
                    out.push((path, sub.to_vec()));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &[], &mut out);
    out
}

fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".saving");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path)).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::model::{BlockRef, Layout};

    fn component_project() -> (Project, DocId) {
        let mut p = Project::new();
        let fa = import_component(&mut p, 0).unwrap();
        let mut outer = Layout::default();
        outer.add_instance(BlockRef::Component(fa), (0, 0));
        let id = p.add_component("Adder pair", "AP", "Two-bit adder", outer, 10.0);
        (p, id)
    }

    #[test]
    fn examples_are_listed_first_and_searchable() {
        let library = Library::new();
        let all = library.summaries();
        let examples = component_examples().len();
        assert_eq!(all.len(), examples + KIT_FILES.len(), "the examples, then the foundry kit");
        assert!(all[..examples].iter().all(|s| s.folder == EXAMPLES_FOLDER && matches!(s.asset, AssetRef::Example(_))));
        assert!(all[examples..].iter().all(|s| s.folder == KIT_FOLDER && matches!(s.asset, AssetRef::Kit(_))));
        assert!(library.is_empty(), "none of them are the user's");
        let mz: Vec<_> = all.iter().filter(|s| s.matches("phase")).map(|s| s.entry.name.as_str()).collect();
        assert_eq!(mz, vec!["Mach–Zehnder interferometer"], "by exported parameter");
        assert!(all.iter().any(|s| s.matches("EXAMPLES")), "by folder, any case");
        assert_eq!(all[0].ports(), "3 signals in, 2 out");
    }

    #[test]
    fn components_are_added_once_and_kept_in_folders() {
        let (p, id) = component_project();
        let mut library = Library::new();
        let (a, new) = library.add(&p.extract(id).unwrap(), "").unwrap();
        assert!(new);
        assert_eq!(library.add(&p.extract(id).unwrap(), "Elsewhere").unwrap(), (a, false), "no duplicates");
        assert_eq!(library.folders(), vec![DEFAULT_FOLDER.to_owned()]);
        library.set_folder(a, "Adders");
        assert_eq!(library.folders(), vec!["Adders".to_owned()]);

        // Its bundle places like any component, with the adder it uses.
        let mut target = Project::new();
        let here = target.import(&library.bundle(AssetRef::Saved(a)).unwrap()).unwrap();
        assert_eq!(target.documents.len(), 2);
        assert!(target.simulate(here, &Default::default()).error.is_none());

        // A layout isn't an asset.
        let mut plain = Project::new();
        let l = plain.add_layout("Just a layout", Layout::default(), 10.0);
        assert!(library.add(&plain.extract(l).unwrap(), "").is_err());
    }

    #[test]
    fn files_add_one_component_or_every_component_of_a_project() {
        let (p, id) = component_project();
        let mut library = Library::new();
        let file = p.extract(id).unwrap().to_bytes();
        assert_eq!(library.import_file("pair.shinecomp", &file, "").unwrap(), 1);
        assert_eq!(library.len(), 1, "only the component the file is about");
        // The whole project: its other component (the adder) too.
        assert_eq!(library.import_file("project.shine", &p.to_bytes(), "").unwrap(), 1);
        assert_eq!(library.len(), 2);
        assert!(library.import_file("notes.txt", b"hello", "").is_err());
        let mut empty = Project::new();
        empty.add_layout("Layout", Layout::default(), 10.0);
        assert!(library.import_file("empty.shine", &empty.to_bytes(), "").unwrap_err().contains("no components"));
    }

    #[test]
    fn s_parameter_files_become_library_components() {
        let mut library = Library::new();
        let s3p = crate::panels::models::tests_support::splitter_s3p();
        let (added, failed) = library.import_files(vec![
            ("y-branch.s3p".into(), s3p.into_bytes(), "Foundry kit".into()),
            ("broken.s2p".into(), b"1 2".to_vec(), "Foundry kit".into()),
        ]);
        assert_eq!(added, 1);
        assert_eq!(failed.len(), 1, "{failed:?}");
        let a = library.summaries().iter().find(|a| a.entry.name == "y-branch").cloned().expect("listed");
        assert_eq!(a.folder, "Foundry kit");
        assert_eq!(a.ports(), "1 light in, 2 out");
    }

    #[test]
    fn the_library_is_kept_in_a_file() {
        let dir = std::env::temp_dir().join(format!("shine-library-{}", std::process::id()));
        let path = dir.join(LIBRARY_FILE);
        let (p, id) = component_project();
        {
            let mut library = Library::open_file(path.clone());
            assert!(library.is_empty());
            let (a, _) = library.add(&p.extract(id).unwrap(), "Adders").unwrap();
            library.set_folder(a, "Logic");
        }
        let reopened = Library::open_file(path.clone());
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.folders(), vec!["Logic".to_owned()]);
        assert_eq!(reopened.summaries().last().unwrap().entry.name, "Adder pair");

        // A damaged file is set aside, not overwritten.
        std::fs::write(&path, b"SHNLxx").unwrap();
        let fresh = Library::open_file(path.clone());
        assert!(fresh.is_empty());
        assert!(dir.join(format!("{LIBRARY_FILE}.unreadable")).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn changes_reach_the_browser_storage_once() {
        let (p, id) = component_project();
        let mut library = Library::new();
        assert!(library.take_changes().is_none());
        library.add(&p.extract(id).unwrap(), "").unwrap();
        let bytes = library.take_changes().expect("changed");
        assert!(library.take_changes().is_none());
        assert_eq!(Library::from_bytes(&bytes).unwrap().len(), 1);
    }
}
