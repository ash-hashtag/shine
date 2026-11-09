---
title: Development
description: Building from source, running tests, keeping the documentation and screenshots current, and how releases are made.
order: 13
---

# Development

## Build and run

```sh
cargo run --release                         # the editor
cargo run --release --bin shine-mcp         # the MCP server
cargo test                                  # every test, including headless UI tests
cargo clippy --all-targets                  # lints (CI fails on warnings)
```

On Linux the editor needs the usual windowing libraries (`libxkbcommon`, `libxcb`,
Wayland or X11 at runtime).

## Tests

Three layers, all run by CI:

- **Unit tests** next to the code (`cargo test --lib`): the photonics engine against
  closed-form results, the simulator, file formats and upgrades, the MCP tools, and the
  editor's UI driven headlessly with simulated mouse and keyboard input.
- **Integration tests** in `tests/` (`cargo test --test ui`): the whole app through its
  public API with [egui_kittest](https://docs.rs/egui_kittest), clicking menus and buttons
  by their labels and dragging with the mouse; plus the MCP server over stdio.
- **Browser tests** (`scripts/e2e.sh`, after `scripts/build-web.sh`): the WebAssembly
  app in headless Chromium with real mouse events, checked by the pixels on screen.
  Screenshots of failures go to `target/e2e-failures`.

A test also checks that the bundled fonts can draw every character in the UI's text, so
nothing shows as an empty box on machines without the right system fonts. Line coverage
is measured with `cargo llvm-cov --lib`.

## Performance

`cargo run --release --example sweep_bench` times editor sweeps on a few designs (the WDM
link, a ring's parameter, a 16-mode optical DFT); `--example matmul` times the engine on a
large MZI mesh. Run them before and after a change that could affect speed.

## The website and browser build

`web/` is an Astro site: the home page, the section pages (`/features`, `/library`,
`/ai`), these docs at `/docs` (rendered by [Starlight](https://starlight.astro.build)
straight from `docs/*.md`, with Pagefind search built in), `/llms.txt`, and `/app` (the
editor compiled to WebAssembly). It uses pnpm. Search works in built sites, not under
`pnpm dev`.

```sh
scripts/build-web.sh                 # WebAssembly + site into web/dist
scripts/build-web.sh --screenshots   # also regenerate every screenshot
cd web && pnpm dev                   # live preview
cd web && pnpm run deploy            # publish to Cloudflare (maintainers)
```

The browser build needs the `wasm32-unknown-unknown` target and a `wasm-bindgen` CLI
matching the version in `Cargo.lock`; the script checks.

## Keeping the docs current

The documentation lives in `docs/*.md` (readable on GitHub, rendered on the website and
in `/llms-full.txt`). When you change behaviour, update the page that describes it in
the same change.

A test (`cargo test docs`) fails when the docs fall behind the code: every part in the
library, every example, every keyboard shortcut and every MCP tool must be mentioned.
Screenshots in `docs/img/` and `web/public/screenshots/` come from the real app:
`scripts/build-web.sh --screenshots` regenerates them with headless Chromium, including
the interactive ones (menus, the parts browser) driven by `scripts/ui-screenshots.mjs`.
The same run renders a 1200×630 social card for the home page and each docs page
(`scripts/og-cards.py`, into `web/public/og/`), so new pages get one too.

## Search engines and link previews

Every page has a canonical link, Open Graph and Twitter card tags, and schema.org data:
the home page describes the app (`SoftwareApplication`, `WebSite`), and each docs page is a
`TechArticle` with breadcrumbs and its last-modified date from git. `sitemap.xml` lists
the home page and every docs page with their dates and images; `/app` and the 404 page
are `noindex`. Add new indexable pages to `web/src/pages/sitemap.xml.ts`, and their social card to
`scripts/og-cards.py`.

## Continuous integration and releases

- **CI** (`.github/workflows/ci.yml`) runs on every push and pull request: the tests on
  Linux, Windows and macOS, clippy, the WebAssembly and website build, and the browser
  tests.
- **Releases** (`.github/workflows/release.yml`) run only when a version tag is pushed.
  They build `shine` and `shine-mcp` for Linux (x86-64, ARM64), Windows (x86-64) and
  macOS (Apple silicon, Intel), and publish them with checksums on a GitHub release. The
  archives keep the same names in every release (`shine-linux-x86_64.tar.gz`,
  `shine-windows-x86_64.zip`…), so the website's download buttons always fetch the latest.

To release, once a version is final:

```sh
# 1. Cargo.toml's `version` is already the next one (e.g. 0.1.2) while it's built,
#    and docs/changelog.md has its section ("## 0.1.2 (unreleased)"): date it
#    ("## 0.1.2 (2026-11-20)")
# 2. commit, push, then tag that commit and push the tag
git tag v0.1.2
git push origin v0.1.2
```

Versions grow slowly: patch releases (0.1.1, 0.1.2…) carry whatever has landed, and the
minor version changes only for a larger step. The roadmap has goals and milestones, not
versions or dates.

The workflow stops if the tag doesn't match the version in `Cargo.toml` or the changelog
has no section for it, and uses that section as the release notes.

Keep `docs/changelog.md` up to date as changes land (under the next version's heading),
and tick items off in `docs/roadmap.md`.
