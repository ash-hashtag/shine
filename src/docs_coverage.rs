//! Keeps `docs/` honest: every part, example, shortcut and MCP tool the code
//! offers must be mentioned in the documentation, and every link and image
//! in it must exist. When this fails, update the docs in the same change.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::panels::chips::ChipBrowser;
use crate::panels::optics::examples::{component_examples, demos, layout_examples};
use crate::panels::optics::input::SHORTCUTS;
use crate::panels::optics::library::{SECTIONS, entries};
use crate::panels::project::Catalog;

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs")
}

fn pages() -> Vec<(String, String)> {
    let mut pages: Vec<(String, String)> = std::fs::read_dir(docs_dir())
        .expect("docs/ exists")
        .map(|e| e.expect("readable").path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        // Windows checkouts may use CRLF line endings.
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n")))
        .collect();
    pages.sort();
    pages
}

fn all_text() -> String {
    pages().into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("\n")
}

fn missing<'a>(text: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    names.into_iter().filter(|n| !text.contains(n)).collect()
}

#[test]
fn docs_mention_every_part_and_folder() {
    let text = all_text();
    let entries = entries(&Catalog::default(), true, "");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(missing(&text, names), Vec::<&str>::new(), "parts missing from docs/parts.md");
    assert_eq!(missing(&text, SECTIONS), Vec::<&str>::new(), "library folders missing from the docs");
}

#[test]
fn docs_mention_every_example() {
    let text = all_text();
    let chips = ChipBrowser::default();
    let mut names: Vec<String> = demos().iter().map(|d| d.name.to_owned()).collect();
    names.extend(layout_examples().iter().map(|e| e.name.to_owned()));
    names.extend(component_examples().iter().map(|e| e.name.to_owned()));
    names.extend(chips.examples.iter().map(|e| e.name().to_owned()));
    let gaps: Vec<&str> = missing(&text, names.iter().map(String::as_str));
    assert_eq!(gaps, Vec::<&str>::new(), "examples missing from docs/examples.md");
}

#[test]
fn docs_list_every_shortcut_and_mcp_tool() {
    let editor = std::fs::read_to_string(docs_dir().join("editor.md")).unwrap().replace("\r\n", "\n");
    let rows: Vec<String> = SHORTCUTS.iter().map(|(keys, what)| format!("| {keys} | {what} |")).collect();
    let gaps: Vec<&str> = missing(&editor, rows.iter().map(String::as_str));
    assert_eq!(gaps, Vec::<&str>::new(), "shortcut rows missing from docs/editor.md (copy them from input::SHORTCUTS)");

    let ai = std::fs::read_to_string(docs_dir().join("ai.md")).unwrap().replace("\r\n", "\n");
    let tools = crate::mcp::tools::tools();
    let gaps: Vec<&str> = missing(&ai, tools.iter().map(|t| t.name));
    assert_eq!(gaps, Vec::<&str>::new(), "MCP tools missing from docs/ai.md");
}

#[test]
fn docs_links_and_images_resolve() {
    let dir = docs_dir();
    let mut used = BTreeSet::new();
    for (page, text) in pages() {
        assert!(text.starts_with("---\ntitle: "), "{page}: needs title/description/order front matter");
        for target in text.split("](").skip(1).filter_map(|rest| rest.split(')').next()) {
            let path = target.split('#').next().unwrap();
            if path.starts_with("http") || path.is_empty() {
                continue;
            }
            assert!(dir.join(path).exists(), "{page}: broken link or image “{target}”");
            used.insert(path.to_owned());
        }
    }
    // Every screenshot is used somewhere (stale ones get deleted).
    for image in std::fs::read_dir(dir.join("img")).unwrap() {
        let name = format!("img/{}", image.unwrap().file_name().to_string_lossy());
        assert!(used.contains(&name), "{name} is not used by any page");
    }
}
