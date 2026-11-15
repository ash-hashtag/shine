//! Projects as folders: a manifest (`shine-project.pb`) listing the project's
//! designs and components, each in a file of its own, whose boxes refer to the
//! components they place by path, relative to the file. A component may live
//! outside the folder (`../shared/mzi.shinecomp`) and be shared by projects.
//!
//! Opening a folder packs its files into a bundle (what a `.shine` file
//! holds), so it opens like any project; saving writes the files of the
//! documents that changed since the last save, and the manifest.

use std::collections::{HashMap, HashSet, VecDeque};

use shine_format as pb;
use shine_format::mod_Instance::OneOfblock;
use shine_format::mod_ShineFile::OneOfcontent;

use super::protobuf::{folder_of, resolve};

/// The manifest's file name, at the top of a project folder.
pub const MANIFEST: &str = "shine-project.pb";

/// A project folder packed into a bundle: the manifest's documents, then the
/// components they use from elsewhere. `read` reads a file by its path from
/// the folder ("/" separators; `..` for files outside it). A file that's
/// missing is left out (the boxes using it show a missing component).
/// `root` is the folder's full path, if known (see `protobuf::relative`).
pub fn pack(read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>, root: &str) -> Result<pb::Bundle, String> {
    let mut packing = Packing::new(&read(MANIFEST)?, root)?;
    while let Some(path) = packing.next_file() {
        match read(&path) {
            Ok(bytes) => packing.add(path, &bytes)?,
            Err(e) => tracing::warn!("{path}: {e}"),
        }
    }
    Ok(packing.bundle())
}

/// Packing a project folder into a bundle, a file at a time: [`Self::next_file`]
/// says which file to read next, [`Self::add`] takes it. (In steps, so the
/// browser can read its files asynchronously.)
pub struct Packing {
    root: String,
    queue: VecDeque<String>,
    seen: HashSet<String>,
    files: Vec<pb::BundledFile>,
}

impl Packing {
    /// Start from the manifest's contents; `root` as for [`pack`].
    pub fn new(manifest: &[u8], root: &str) -> Result<Self, String> {
        let manifest = match pb::decode(manifest)?.content {
            OneOfcontent::manifest(m) => m,
            _ => return Err(format!("{MANIFEST} isn't a project's manifest")),
        };
        let queue: VecDeque<String> = manifest.documents.iter().map(|p| resolve("", p, root)).collect();
        let seen = queue.iter().cloned().collect();
        Ok(Self { root: root.to_owned(), queue, seen, files: Vec::new() })
    }

    /// The next file to read (a path from the top of the folder), if any.
    pub fn next_file(&mut self) -> Option<String> {
        self.queue.pop_front()
    }

    /// The file at `path`: its document, and the components it uses to read.
    pub fn add(&mut self, path: String, bytes: &[u8]) -> Result<(), String> {
        let document = match pb::decode(bytes).map_err(|e| format!("{path}: {e}"))?.content {
            OneOfcontent::document(d) => d,
            _ => return Err(format!("{path} isn't a design or a component")),
        };
        for b in document.layout.iter().flat_map(|l| &l.boxes) {
            if let OneOfblock::component(used) = &b.block {
                let full = resolve(folder_of(&path), used, &self.root);
                if self.seen.insert(full.clone()) {
                    self.queue.push_back(full);
                }
            }
        }
        self.files.push(pb::BundledFile { path, document: Some(document), id: 0 });
        Ok(())
    }

    pub fn bundle(self) -> pb::Bundle {
        pb::Bundle { files: self.files }
    }
}

/// What a save to a folder wrote, kept once it's written (see
/// [`super::Project::folder_changes`]).
pub struct Written {
    paths: HashMap<super::DocId, String>,
    saved: HashMap<super::DocId, (u64, String)>,
}

impl super::Project {
    /// What saving to the folder `key` (whose full path is `root`) must
    /// write: the files of the documents changed since the last save there
    /// (all the first time, and any `exists` says is gone) and the manifest.
    /// Once they're written, [`Self::folder_written`] records it.
    pub fn folder_changes(&self, key: &std::path::Path, root: &str, exists: &dyn Fn(&str) -> bool) -> (Files, Written) {
        let same_folder = self.saved_files.0.as_deref() == Some(key);
        let saved = &self.saved_files.1;
        let write = |id, path: &str| !same_folder || saved.get(&id).is_none_or(|(f, p)| Some(*f) != self.doc_fingerprint(id) || p != path) || !exists(path);
        let (files, paths) = files(self, root, &write);
        let saved = paths.iter().filter_map(|(&id, p)| Some((id, (self.doc_fingerprint(id)?, p.clone())))).collect();
        (files, Written { paths, saved })
    }

    /// The files [`Self::folder_changes`] gave are written to the folder `key`.
    pub fn folder_written(&mut self, key: &std::path::Path, written: Written) {
        self.paths = written.paths;
        self.saved_files = (Some(key.to_owned()), written.saved);
    }

    /// The project as it is now is what the folder `key` holds: saving there
    /// writes only what changes from here.
    pub fn folder_saved(&mut self, key: &std::path::Path) {
        let documents: Vec<super::DocumentFile> = self.documents.iter().map(super::Document::to_file).collect();
        let paths = super::protobuf::paths(&documents, &self.paths);
        self.paths = documents.iter().map(|d| d.id).zip(paths).collect();
        let saved = self.paths.iter().filter_map(|(&id, p)| Some((id, (self.doc_fingerprint(id)?, p.clone())))).collect();
        self.saved_files = (Some(key.to_owned()), saved);
    }
}

/// Files to write: each one's path in the folder, and its contents.
pub type Files = Vec<(String, Vec<u8>)>;

/// A project's files in a folder: each document's path and contents, for
/// those `write` picks (by id and path), and the manifest last; and every
/// document's path.
pub(crate) fn files(project: &super::Project, root: &str, write: &dyn Fn(super::DocId, &str) -> bool) -> (Files, HashMap<super::DocId, String>) {
    let documents: Vec<super::DocumentFile> = project.documents.iter().map(super::Document::to_file).collect();
    let paths = super::protobuf::paths(&documents, &project.paths);
    let path_of: HashMap<super::DocId, &str> = documents.iter().zip(&paths).map(|(d, p)| (d.id, p.as_str())).collect();
    let mut out: Files = documents
        .iter()
        .zip(&paths)
        .filter(|(d, path)| write(d.id, path))
        .map(|(d, path)| {
            let document = super::protobuf::document_at(d, path, &path_of, root);
            (path.clone(), pb::encode(&pb::ShineFile { content: OneOfcontent::document(document) }))
        })
        .collect();
    let manifest = pb::Manifest { documents: paths.clone() };
    out.push((MANIFEST.to_owned(), pb::encode(&pb::ShineFile { content: OneOfcontent::manifest(manifest) })));
    (out, documents.iter().map(|d| d.id).zip(paths).collect())
}

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use std::path::{Path, PathBuf};

    use super::super::Project;
    use super::MANIFEST;

    /// The project folder `path` names: itself if it is a folder, the
    /// folder it is in if it is the manifest.
    pub fn project_folder(path: &Path) -> Option<PathBuf> {
        if path.is_dir() {
            Some(path.to_owned())
        } else if path.file_name().is_some_and(|f| f == MANIFEST) {
            path.parent().map(Path::to_owned)
        } else {
            None
        }
    }

    /// The project at `path`, a folder (or its manifest) or a `.shine` file,
    /// as a `.shine` file's bytes.
    pub fn read_project(path: &Path) -> Result<Vec<u8>, String> {
        match project_folder(path) {
            Some(dir) => {
                if !dir.join(MANIFEST).exists() {
                    return Err(format!("{} isn't a Shine project folder (it has no {MANIFEST})", dir.display()));
                }
                let root = std::path::absolute(&dir).unwrap_or_else(|_| dir.clone()).to_string_lossy().into_owned();
                let bundle = super::pack(&mut |p| std::fs::read(dir.join(p)).map_err(|e| e.to_string()), &root)?;
                Ok(shine_format::encode(&shine_format::ShineFile { content: shine_format::mod_ShineFile::OneOfcontent::bundle(bundle) }))
            }
            None => std::fs::read(path).map_err(|e| e.to_string()),
        }
    }

    /// Write `bytes` to `path` through a file beside it, renamed over it, so
    /// a failure leaves the old file whole.
    pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".saving");
        let temporary = PathBuf::from(temporary);
        let written = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written
    }

    impl Project {
        /// Save the project as the folder `dir`: the files of the documents
        /// that changed since it was last saved there (all of them the first
        /// time), and the manifest. Files of documents no longer in the
        /// project are left, as another project may use them. Returns how
        /// many documents were written.
        pub fn save_folder(&mut self, dir: &Path) -> std::io::Result<usize> {
            let root = std::path::absolute(dir).unwrap_or_else(|_| dir.to_owned()).to_string_lossy().into_owned();
            let (files, written) = self.folder_changes(dir, &root, &|path| dir.join(path).exists());
            for (path, bytes) in &files {
                write_atomically(&dir.join(path), bytes)?;
            }
            self.folder_written(dir, written);
            Ok(files.len() - 1)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use disk::{project_folder, read_project, write_atomically};

/// In the browser, files only: a `.shine` file's bytes.
#[cfg(target_arch = "wasm32")]
pub fn read_project(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::{DocId, Project};
    use crate::panels::optics::model::{BlockRef, Builtin, Layout};

    /// A design placing a component that places another.
    fn nested() -> (Project, DocId, DocId) {
        let mut p = Project::new();
        let mut inner = Layout::default();
        inner.add_instance(BlockRef::Builtin(Builtin::mzi(1.0, 0.5)), (0, 0));
        let inner = p.add_component("MZI pair", "MP", "", inner, 10.0);
        let mut outer = Layout::default();
        outer.add_instance(BlockRef::Component(inner), (0, 0));
        let outer = p.add_component("Block", "B", "", outer, 10.0);
        let mut top = Layout::default();
        top.add_instance(BlockRef::Component(outer), (2, 0));
        p.add_layout("Chip", top, 10.0);
        (p, inner, outer)
    }

    #[test]
    fn a_folder_saves_only_what_changed_and_opens_the_same() {
        let dir = std::env::temp_dir().join(format!("shine-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (mut p, inner, _) = nested();
        assert_eq!(p.save_folder(&dir).unwrap(), 3, "every document the first time");
        let mut names: Vec<String> = walk(&dir);
        names.sort();
        assert_eq!(names, ["chip.shinedesign", "components/block.shinecomp", "components/mzi-pair.shinecomp", "shine-project.pb"]);
        assert_eq!(p.save_folder(&dir).unwrap(), 0, "nothing changed");

        let q = Project::from_bytes(&super::read_project(&dir).unwrap()).unwrap();
        assert_eq!(q.documents.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["MZI pair", "Block", "Chip"]);
        for (a, b) in p.documents.iter().zip(&q.documents) {
            let ids = |l: &Layout| l.instances.iter().map(|i| matches!(i.block, BlockRef::Component(_))).collect::<Vec<_>>();
            assert_eq!(ids(a.layout()), ids(b.layout()), "{}", a.name);
        }

        // Change the inner component: only its file is written.
        p.doc_mut(inner).unwrap().layout_mut().instances[0].cell = (4, 4);
        assert_eq!(p.save_folder(&dir).unwrap(), 1);
        // Its manifest names a file: the manifest's own folder opens too.
        assert!(super::read_project(&dir.join(super::MANIFEST)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn components_can_be_shared_from_outside_the_folder() {
        let base = std::env::temp_dir().join(format!("shine-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (mut p, inner, _) = nested();
        // The MZI pair lives in a shared folder beside the project's.
        p.paths.insert(inner, "../shared/mzi-pair.shinecomp".into());
        p.save_folder(&base.join("chip")).unwrap();
        assert!(base.join("shared/mzi-pair.shinecomp").exists());
        assert!(!base.join("chip/components/mzi-pair.shinecomp").exists());

        // A second project uses it too, from its own manifest.
        let q = Project::from_bytes(&super::read_project(&base.join("chip")).unwrap()).unwrap();
        let shared = q.documents.iter().find(|d| d.name == "MZI pair").unwrap();
        assert_eq!(q.paths[&shared.id], "../shared/mzi-pair.shinecomp");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_missing_component_file_opens_as_a_missing_component() {
        let dir = std::env::temp_dir().join(format!("shine-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (mut p, _, _) = nested();
        p.save_folder(&dir).unwrap();
        std::fs::remove_file(dir.join("components/mzi-pair.shinecomp")).unwrap();
        let q = Project::from_bytes(&super::read_project(&dir).unwrap()).unwrap();
        assert_eq!(q.documents.len(), 2);
        let block = q.documents.iter().find(|d| d.name == "Block").unwrap();
        let BlockRef::Component(gone) = block.layout().instances[0].block else { panic!() };
        assert!(q.doc(gone).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn walk(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                out.extend(walk(&e.path()).into_iter().map(|n| format!("{name}/{n}")));
            } else {
                out.push(name);
            }
        }
        out
    }
}
