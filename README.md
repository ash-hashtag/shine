# Shine

Design and simulate photonic circuits: free-space optics, waveguide components, and the
electronics that drive and read them. Light is solved exactly (interference, resonances,
several wavelengths at once); reusable components nest; registers, a clock and function
cells close the loop.

- **Use it in the browser:** <https://shine.lupyd.com/app>
- **Download for Linux, Windows or macOS:** [releases](https://github.com/ash-hashtag/shine/releases)
- **Documentation:** [`docs/`](docs/README.md), also at <https://shine.lupyd.com/docs>
  and as plain text for language models at <https://shine.lupyd.com/llms-full.txt>

![A 4-channel WDM optical link in Shine](docs/img/wdm-link.webp)

## What's inside

- **Editor** (`shine`): one grid for free-space parts, component boxes, waveguides and
  signal wires, with Blender-style controls. See [Getting started](docs/getting-started.md).
- **MCP server** (`shine-mcp`): lets AI assistants build, simulate and save circuits. See
  [AI assistants](docs/ai.md).
- **Library** (`shine::photonics`): the simulator on its own, with SIMD kernels and
  compiled circuits. See [Rust library](docs/library.md).

## Build from source

```sh
cargo run --release                   # the editor
cargo run --release --bin shine-mcp   # the MCP server
cargo test                            # tests, including headless UI and docs checks
scripts/build-web.sh                  # the website, docs and WebAssembly app
```

More in [Development](docs/development.md): the website, screenshots, keeping the docs
current, CI and releases.

## License

See [LICENSE](LICENSE).
