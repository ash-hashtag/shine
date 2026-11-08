# Shine

Optics simulator for designing photonic chips

## Running

```sh
cargo run --release                                  # editor
cargo run --release -- --log-file logs/shine.log     # also log to a file
cargo run --release --bin shine-mcp                  # MCP server for AI assistants (stdio)
SHINE_LOG=debug cargo run                            # log filter (or --log-level, RUST_LOG)
cargo run --release --no-default-features --example matmul   # library only
```

Logs always go to stderr; `--log-file <path>` / `SHINE_LOG_FILE` adds a file (appended, directories created).

## Website and browser build

`web/` is an Astro site: a home page with screenshots and `/app`, which runs the editor
compiled to WebAssembly. Links like `/app?example=optical-dft` open an example.

```sh
scripts/build-web.sh                # WASM build + site into web/dist
scripts/build-web.sh --screenshots  # also refresh the screenshots (headless Chromium)
cd web && pnpm dev                 # live-reload the site
```

### Deploying to Cloudflare Workers

The site deploys as Workers static assets (`web/wrangler.jsonc`, no Worker script):

```sh
SITE_URL=https://your.domain scripts/build-web.sh   # build web/dist
cd web && pnpm exec wrangler login  # once
pnpm cf:dev                         # optional: serve dist on the local Workers runtime
pnpm run deploy                     # wrangler deploy (not `pnpm deploy`, a pnpm built-in)
```

`SITE_URL` is the public address; it enables canonical links, `og:url`, absolute social
preview images and `sitemap.xml` (without it the build warns and leaves them out).
Images are lossless WebP; `scripts/screenshots.sh` also renders the 1200×630 Open Graph
card from `scripts/og-card.html`.

`web/public/_headers` sets caching: hashed `/_astro/*` files are immutable, while the
WebAssembly app in `/pkg/*` always revalidates because its file names don't change.

Needs the `wasm32-unknown-unknown` target and a `wasm-bindgen` CLI matching the version in
`Cargo.lock` (`cargo install wasm-bindgen-cli --version <version>`); the script checks this.

## Using Shine from an AI assistant

`shine-mcp` is a Model Context Protocol server (stdio). Point your client at it:

```json
{ "mcpServers": { "shine": { "command": "/path/to/shine-mcp" } } }
```

Tools: `list_blocks`, `simulate_circuit`, `sweep_wavelength`, `decompose_matrix`,
`simulate_project`, `save_project`.

## Editor controls (Blender-style)

| Input | Action |
|---|---|
| Middle drag, Shift+Middle drag / trackpad scroll | Pan |
| Wheel, pinch, Ctrl+Middle drag | Zoom at cursor |
| Ctrl+Wheel / Shift+Wheel | Pan horizontally / vertically |
| Home / `.` | Frame all / selected |
| Click, Shift+click, drag on empty space | Select, extend, box select |
| Drag a component, or G (then X/Y to lock an axis) | Move |
| R / Shift+R, double click | Rotate |
| Shift+D, X/Delete, Shift+A, right click | Duplicate, delete, add menu, context menu |
| A / Alt+A / Ctrl+I | Select all / none / invert |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Tab, double click | Open a component's definition |
| Drag from a port | Draw a waveguide (drag a connected input to move its waveguide) |
| Space, T, N, F1 | Simulate, library, inspector, shortcuts |

## Documents and components

A project holds **free-space layouts** (beams on a grid) and **schematics** (blocks wired
with waveguides), each in its own tab. Any document can be saved as a component
(Component → Save as component…): give it a name, initials and a description. Its ports
are the *Input/Output port* parts of a layout, or the *Input/Output pins* of a schematic,
ordered top to bottom.

In a schematic, components appear as boxes with their initials; hover for the description,
drag from a port to another port to draw a waveguide, and press **Tab** (or double-click)
to open the component's definition. Edits there update every instance. Waveguides are ideal
by default; switch to *From drawing* in the inspector, or give a waveguide a fixed length,
to include propagation phase. Projects save as one compact binary `.shine` file
(File → Save): a `SHNE` header and format version followed by a postcard-encoded body,
about 5× smaller than compact JSON. JSON layouts from earlier versions can still be opened.

## Library

`shine::photonics` is usable without the GUI: build a `Circuit` from components or
reusable blocks (`photonics::blocks`: splitter/combiner trees, all-pass and add-drop
rings, interleavers, MZI switch trees, optical DFT, WDM demux, SVD matrix multiplier),
then `compile` it per wavelength and run batches of inputs.
