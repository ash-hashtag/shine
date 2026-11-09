---
title: Development
description: Building from source, running tests, keeping the documentation and screenshots current, and how releases are made.
order: 12
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

## The website and browser build

`web/` is an Astro site: the home page, these docs at `/docs`, `/llms.txt`, and `/app`
(the editor compiled to WebAssembly). It uses pnpm.

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

## Continuous integration and releases

- **CI** (`.github/workflows/ci.yml`) runs on every push and pull request: tests,
  clippy, the WebAssembly check, and the website build.
- **Releases** (`.github/workflows/release.yml`) run only when a version tag is pushed.
  They build `shine` and `shine-mcp` for Linux (x86-64, ARM64), Windows (x86-64) and
  macOS (Apple silicon, Intel), and publish them with checksums on a GitHub release.

To release, once a version is final:

```sh
# 1. set `version` in Cargo.toml (e.g. 0.2.0), commit, and push
# 2. tag that commit and push the tag
git tag v0.2.0
git push origin v0.2.0
```

The workflow checks that the tag matches the version in `Cargo.toml` and stops if not.
