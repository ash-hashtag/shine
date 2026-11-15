//! Project folders in the browser, through the File System Access API
//! (Chrome and Edge): a folder the user picks is read and written as on
//! desktop (`panels::project::folder`). Browsers without it (Firefox,
//! Safari) open and download `.shine` files.
//!
//! A folder project's path is `browser-folder:NAME`; the folder's handle is
//! kept here while the page is open (after a reload, saving asks for the
//! folder again). Reading and writing are asynchronous: what they find comes
//! back as [`Event`]s, which the workspace takes each frame.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::panels::project::folder::{Files, MANIFEST, Packing};

const PREFIX: &str = "browser-folder:";

/// What came of reading or writing a folder.
pub enum Event {
    /// A folder was read: its project, as a `.shine` file's bytes.
    Opened { name: String, bytes: Vec<u8> },
    /// A folder was picked to save the project in.
    PickedForSave { name: String },
    /// `count` files were written to the folder.
    Saved { name: String, count: usize, done: Box<dyn FnOnce(&mut crate::panels::workspace::Workspace)> },
    Failed(String),
}

thread_local! {
    /// The open folder: its handle and name.
    static CURRENT: RefCell<Option<(JsValue, String)>> = const { RefCell::new(None) };
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

/// The path a folder project has: `browser-folder:NAME`.
pub fn key_of(name: &str) -> PathBuf {
    PathBuf::from(format!("{PREFIX}{name}"))
}

/// The folder's name, if `path` is a browser folder's.
pub fn folder_name(path: &Path) -> Option<String> {
    path.to_str()?.strip_prefix(PREFIX).map(str::to_owned)
}

/// The open folder's path, if a folder project is open and its folder at hand.
pub fn key() -> Option<PathBuf> {
    CURRENT.with(|c| c.borrow().as_ref().map(|(_, name)| key_of(name)))
}

/// Whether the folder `name` is at hand (picked since the page opened).
pub fn is_open(name: &str) -> bool {
    CURRENT.with(|c| c.borrow().as_ref().is_some_and(|(_, n)| n == name))
}

/// Whether this browser can open and save folders.
pub fn supported() -> bool {
    web_sys::window().is_some_and(|w| Reflect::get(&w, &"showDirectoryPicker".into()).is_ok_and(|f| f.is_function()))
}

/// What came of reading and writing folders since last asked.
pub fn take_events() -> Vec<Event> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

fn push(event: Event) {
    EVENTS.with(|e| e.borrow_mut().push(event));
    CONTEXT.with(|c| {
        if let Some(ctx) = &*c.borrow() {
            ctx.request_repaint();
        }
    });
}

/// The editor's context, to wake it when a folder has been read or written.
pub fn set_context(ctx: egui::Context) {
    CONTEXT.with(|c| *c.borrow_mut() = Some(ctx));
}

fn error(e: JsValue) -> String {
    e.as_string()
        .or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())))
        .unwrap_or_else(|| format!("{e:?}"))
}

/// `object.name(args…)`, awaited if it gives a promise.
async fn call(object: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let f: Function = Reflect::get(object, &name.into()).map_err(error)?.dyn_into().map_err(|_| format!("no {name}"))?;
    let result = f.apply(object, &args.iter().collect::<Array>()).map_err(error)?;
    match result.dyn_into::<Promise>() {
        Ok(p) => JsFuture::from(p).await.map_err(error),
        Err(v) => Ok(v),
    }
}

fn options(create: bool) -> JsValue {
    let o = Object::new();
    let _ = Reflect::set(&o, &"create".into(), &create.into());
    o.into()
}

/// The folder `path`'s files are in, from the top folder `dir`, and the
/// file's name; made as needed if `create`.
async fn locate(dir: &JsValue, path: &str, create: bool) -> Result<(JsValue, String), String> {
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let file = parts.pop().ok_or("an empty path")?.to_owned();
    let mut here = dir.clone();
    for part in parts {
        if part == ".." {
            return Err("it is outside the folder, which browsers can't reach".into());
        }
        here = call(&here, "getDirectoryHandle", &[part.into(), options(create)]).await?;
    }
    Ok((here, file))
}

async fn read(dir: &JsValue, path: &str) -> Result<Vec<u8>, String> {
    let (folder, name) = locate(dir, path, false).await?;
    let handle = call(&folder, "getFileHandle", &[name.into(), options(false)]).await?;
    let file = call(&handle, "getFile", &[]).await?;
    let buffer = call(&file, "arrayBuffer", &[]).await?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

async fn write(dir: &JsValue, path: &str, bytes: &[u8]) -> Result<(), String> {
    let (folder, name) = locate(dir, path, true).await?;
    let handle = call(&folder, "getFileHandle", &[name.into(), options(true)]).await?;
    // Written to a copy that replaces the file when closed.
    let stream = call(&handle, "createWritable", &[]).await?;
    call(&stream, "write", &[Uint8Array::from(bytes).into()]).await?;
    call(&stream, "close", &[]).await?;
    Ok(())
}

/// The project in folder `dir`, as a `.shine` file's bytes.
async fn pack(dir: &JsValue, name: &str) -> Result<Vec<u8>, String> {
    let manifest = read(dir, MANIFEST).await.map_err(|e| format!("{name} isn't a Shine project folder ({MANIFEST}: {e})"))?;
    let mut packing = Packing::new(&manifest, name)?;
    while let Some(path) = packing.next_file() {
        match read(dir, &path).await {
            Ok(bytes) => packing.add(path, &bytes)?,
            Err(e) => tracing::warn!("{path}: {e}"),
        }
    }
    let file = shine_format::ShineFile { content: shine_format::mod_ShineFile::OneOfcontent::bundle(packing.bundle()) };
    Ok(shine_format::encode(&file))
}

async fn open_handle(dir: JsValue) -> Result<(), String> {
    let name = Reflect::get(&dir, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_else(|| "folder".into());
    let bytes = pack(&dir, &name).await?;
    CURRENT.with(|c| *c.borrow_mut() = Some((dir, name.clone())));
    push(Event::Opened { name, bytes });
    Ok(())
}

async fn pick() -> Result<JsValue, String> {
    let window: JsValue = web_sys::window().ok_or("no window")?.into();
    let o = Object::new();
    let _ = Reflect::set(&o, &"mode".into(), &"readwrite".into());
    call(&window, "showDirectoryPicker", &[o.into()]).await
}

/// The browser's private storage for this site (no picker): a folder in it,
/// for tests (`?open-storage=`, `?save-to-storage=`).
async fn storage(name: &str) -> Result<JsValue, String> {
    let navigator: JsValue = web_sys::window().ok_or("no window")?.navigator().into();
    let storage = Reflect::get(&navigator, &"storage".into()).map_err(error)?;
    let root = call(&storage, "getDirectory", &[]).await?;
    call(&root, "getDirectoryHandle", &[name.into(), options(true)]).await
}

fn spawn(work: impl std::future::Future<Output = Result<(), String>> + 'static) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = work.await {
            // Closing the picker isn't a failure.
            if !e.contains("aborted") {
                push(Event::Failed(e));
            }
        }
    });
}

/// Ask for a folder and open the project in it.
pub fn pick_and_open() {
    spawn(async { open_handle(pick().await?).await });
}

/// Ask for a folder to save the project in.
pub fn pick_for_save() {
    spawn(async {
        let dir = pick().await?;
        let name = Reflect::get(&dir, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_else(|| "folder".into());
        CURRENT.with(|c| *c.borrow_mut() = Some((dir, name.clone())));
        push(Event::PickedForSave { name });
        Ok(())
    });
}

/// Open the project in the folder `name` of the site's private storage.
pub fn open_storage(name: String) {
    spawn(async move { open_handle(storage(&name).await?).await });
}

/// Save the project in the folder `name` of the site's private storage.
pub fn save_to_storage(name: String) {
    spawn(async move {
        let dir = storage(&name).await?;
        CURRENT.with(|c| *c.borrow_mut() = Some((dir, name.clone())));
        push(Event::PickedForSave { name });
        Ok(())
    });
}

/// Read the open folder again.
pub fn reopen() {
    let Some(dir) = CURRENT.with(|c| c.borrow().as_ref().map(|(d, _)| d.clone())) else { return };
    spawn(open_handle(dir));
}

/// Write `files` to the open folder `name`; then `done` runs on the
/// workspace (to count the project as saved).
pub fn save(name: String, files: Files, done: Box<dyn FnOnce(&mut crate::panels::workspace::Workspace)>) {
    let Some(dir) = CURRENT.with(|c| c.borrow().as_ref().filter(|(_, n)| *n == name).map(|(d, _)| d.clone())) else {
        push(Event::Failed(format!("the folder {name} isn't open: choose it again with Save as folder…")));
        return;
    };
    spawn(async move {
        for (path, bytes) in &files {
            write(&dir, path, bytes).await.map_err(|e| format!("{path}: {e}"))?;
        }
        push(Event::Saved { name, count: files.len() - 1, done });
        Ok(())
    });
}
