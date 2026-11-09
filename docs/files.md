---
title: Projects and files
description: Saving and opening projects, the .shine binary format, desktop and browser builds, and logging.
order: 9
---

# Projects and files

## Projects

A project holds every document you have open in tabs: layouts and components. **File →
Save** writes the whole project to one `.shine` file; **File → Open** reads it back.

- **Desktop:** Save asks for a path the first time, then saves in place (**Save as…** to
  choose again). Ctrl+S, Ctrl+O and Ctrl+N work too.
- **Browser:** Save downloads the file; Open uploads one. Nothing is stored on a server.

## The `.shine` format

A compact binary file: the 4 bytes `SHNE`, a 2-byte format version (little-endian), then
the project encoded with [postcard](https://docs.rs/postcard). It is about five times
smaller than the same project as JSON. Files from older versions are converted when
opened: format 1's separate schematics become layouts of boxes, and format 2's lasers
(which all interfered with each other) go onto shared source A so they simulate as before.
A file from a *newer* Shine than yours is refused with a message to update. Preferences
are not part of a project; they're saved with the app. JSON layouts from early
versions can still be opened.

## Desktop app

Releases contain `shine` (the editor) and `shine-mcp` (the [AI server](ai.md)). Command
line options:

| Option | Effect |
|---|---|
| `--log-file <path>` or `SHINE_LOG_FILE` | also write the log to a file (appended) |
| `--log-level <filter>`, `SHINE_LOG` or `RUST_LOG` | log filter, e.g. `debug` or `shine=trace` |

Logs always go to stderr. The Windows release opens no console window, so use `--log-file` there.

## Browser app

The browser build is the same editor compiled to WebAssembly. Links can open things
directly:

| Query | Effect |
|---|---|
| `?example=<name>` | open an example, e.g. `?example=4-channel-wdm-optical-link` |
| `?tab=<name>` | switch to the document with that name, e.g. `&tab=full-adder` |
| `?examples=demos\|layouts\|chips` | open the examples window on that tab |
| `?chip=<name>` | pick a chip-circuit example, e.g. `?examples=chips&chip=wdm-demultiplexer` |
| `?renderer=webgpu` | use WebGPU instead of WebGL2 |

Names are written in lower case with dashes. If the page fails to start, it shows the
error instead of a blank page.
