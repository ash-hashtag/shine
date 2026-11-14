---
title: Shine documentation
description: What Shine is, where to start, and a map of every guide.
order: 0
---

# Shine documentation

Shine designs and simulates photonic circuits: free-space optics on a bench, waveguide
components on a chip, and the electronics that drive and read them. It runs in the
browser at [shine.lupyd.com/app](https://shine.lupyd.com/app), as a desktop app on
Linux, Windows and macOS, as a Rust library, and as an MCP server for AI assistants.

![The editor with the 4-channel WDM optical link example open](img/wdm-link.webp)

## Guides

| Guide | What it covers |
|---|---|
| [Getting started](getting-started.md) | Install or open Shine, tour the window, run your first simulation |
| [Editing](editor.md) | Placing, selecting, moving, the add menu, the parts browser, keyboard and mouse |
| [Parts reference](parts.md) | Every part and block: what it does, its ports and parameters |
| [Wiring light and signals](wiring.md) | Waveguides, beams, signal wires, and converting between light and signals |
| [Components](components.md) | Turning a layout into a reusable box, ports and pins, trying a component on its own |
| [Electronics and the clock](electronics.md) | Number and value inputs, displays, registers, the clock, and function-cell expressions |
| [Sweeps](sweeps.md) | Spectra and parameter sweeps of any design, plotted and exported as CSV |
| [Examples](examples.md) | The ready-made circuits and what each one teaches |
| [Validation](validation.md) | Shine's results against textbook formulas and an independent simulator |
| [Optical neural network](neural-network.md) | A digit-reading AI inference chip, simulated end to end and checked against SAX and PhotonForge |
| [Your neural network on a photonic chip](photonic-neural-networks.md) | How to train a network for light, build its chip from the weights, run it and check it |
| [How the simulation works](simulation.md) | What is solved exactly, what is modelled simply, and the limits |
| [Other tools and tapeout](interop.md) | S-parameters for circuit simulators, and a gdsfactory layout for a mask |
| [Projects and files](files.md) | The `.shine` format, the browser and desktop builds, logging |
| [AI assistants (MCP)](ai.md) | Using Shine from Claude and other MCP clients; notes for LLMs |
| [Python](python.md) | Designing and simulating in code with the `shine-photonics` package, and opening the result in the app |
| [TypeScript](typescript.md) | The same in TypeScript and JavaScript, in Node and the browser |
| [Rust library](library.md) | Using Shine from Rust: the library (`shine::api`) and the `photonics` engine |
| [Development](development.md) | Building, testing, keeping these docs current, releases |
| [Roadmap](roadmap.md) | What's being built for 0.1.0 and what comes after |
| [Changelog](changelog.md) | What changed in each version |

On the website, **Search** at the top of every docs page (Ctrl+K) searches all of them.

For language models, the whole documentation is also served as plain text at
[`/llms.txt`](https://shine.lupyd.com/llms.txt) (an index) and
[`/llms-full.txt`](https://shine.lupyd.com/llms-full.txt) (everything in one file).
