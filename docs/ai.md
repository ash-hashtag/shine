---
title: AI assistants (MCP)
description: Use Shine from Claude and other Model Context Protocol clients, and how language models should approach Shine.
order: 10
---

# AI assistants (MCP)

`shine-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server that
speaks JSON-RPC over stdio. It ships with the desktop release, or build it with
`cargo build --release --bin shine-mcp`.

## Connecting a client

```json
{
  "mcpServers": {
    "shine": { "command": "/path/to/shine-mcp" }
  }
}
```

In Claude Code: `claude mcp add shine /path/to/shine-mcp`.

## Tools

| Tool | What it does |
|---|---|
| `list_blocks` | Lists the building blocks for circuits: type name, ports, parameters with defaults and units. |
| `simulate_circuit` | Solves a circuit exactly at every laser wavelength; returns detector readings, output powers and the field on each connection. |
| `sweep_wavelength` | Drives one open input with unit power and returns output powers over a wavelength range: spectra of filters, rings, interferometers. |
| `decompose_matrix` | Finds MZI phase settings for a matrix: `unitary` gives a triangular (Reck) mesh, `svd` handles any matrix with two meshes and attenuators. |
| `simulate_project` | Opens a `.shine` project and simulates every document: detector readings, display values, signal outputs, component interfaces. |
| `save_project` | Writes a circuit as a `.shine` project that a person can open, inspect and edit in the editor. |

Circuits are JSON: `blocks` (each with an `id`, a `type` from `list_blocks` and optional
`params`) and `connections` from an output `"block.port"` to an input `"block.port"`
(port name or 0-based index). Each port takes one connection. A connection without
`length_um` is ideal (no phase); with it, the phase uses `n_eff` (default 2.4).

```json
{
  "blocks": [
    {"id": "laser", "type": "laser", "params": {"wavelength_nm": 1550}},
    {"id": "dc", "type": "coupler"},
    {"id": "pd", "type": "detector"}
  ],
  "connections": [
    {"from": "laser.out", "to": "dc.in1"},
    {"from": "dc.out2", "to": "pd.in"}
  ]
}
```

## Notes for language models

- Call `list_blocks` first; parameter names and units come from there, not from memory.
- Simulate before claiming a result, and quote the numbers returned.
- Powers are normalised: a laser of amplitude 1 delivers power 1.
- Wavelengths are in nanometres, lengths in micrometres, phases in radians.
- Saved projects open in the editor, where the circuit appears as boxes joined by
  waveguides; tell the user where you saved the file.
- The editor's free-space parts and electronics (registers, function cells) aren't part of
  the MCP circuit format; `simulate_project` does simulate them in saved projects.
- The documentation in plain text: <https://shine.lupyd.com/llms-full.txt>.
