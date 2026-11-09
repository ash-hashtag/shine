---
title: Projects and files
description: Saving and opening projects, the .shine binary format, desktop and browser builds, and logging.
order: 11
---

# Projects and files

## Projects

A project holds every document you have open in tabs: layouts and components. **File →
Save** writes the whole project to one `.shine` file; **File → Open** reads it back.

- **Desktop:** Save asks for a path the first time, then saves in place (**Save as…** to
  choose again). Ctrl+S, Ctrl+O and Ctrl+N work too.
- **Browser:** Save downloads the file; Open uploads one. Nothing is stored on a server.

The status bar shows **● unsaved changes** when the project differs from the last save
(on desktop the window title gets a dot too). Opening another project replaces the open
one, so with unsaved changes Shine asks first: save (or download) and open, open without
saving, or cancel. Saving writes the file under a temporary name and then renames it, so a
save that fails part-way never damages the file you had.

### Your session is kept

Shine keeps the open project, its tabs and their views in the app's storage (a settings
file on desktop, the browser's storage on the web) every few seconds and when you leave,
and opens it again next time, unsaved changes included, still marked as unsaved. Closing
the window or the tab, or a crash, loses at most a few seconds of work. If the desktop
app didn't close properly, it says so when it restores your work. **File → New project**
starts over with an empty project (asking first if there are unsaved changes). The kept
session is a safety net, not a substitute for saving: save projects you want to keep as
files.

## The `.shine` format

A compact binary file: the 4 bytes `SHNE`, a 2-byte format version (little-endian), then
the project encoded with [postcard](https://docs.rs/postcard). It is about five times
smaller than the same project as JSON. Files from older versions are converted when
opened: format 1's separate schematics become layouts of boxes, and format 2's lasers
(which all interfered with each other) go onto shared source A so they simulate as before.
Format 4 added waveguides from lasers into boxes; format 5 added group indices to
delay lines and rings (older ones keep a constant index, so they simulate as before); the
current format, 6, added components' exported parameters.
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

`shine simulate PROJECT.shine` simulates a project without opening a window; see
[Command line](command-line.md).

Logs always go to stderr. The Windows release opens no console window, so use `--log-file` there.

**Help → About Shine** shows the version, what's new in it, and links to the docs and to
report a problem. On desktop, **Check for updates** asks GitHub for the latest release
and links to it if it's newer; Shine makes no network requests otherwise. The browser
version is always the latest.

If the app crashes, it writes `crash-report.txt` (the message, where it happened, the
version, the system and a backtrace; no project data) to its settings folder and to the
log. The next start restores your work and shows the report, with **Report on GitHub**
to open an issue with it filled in, and **Copy details**. In the browser, a crash shows
the details on the page, with the same link and a reload that brings your work back.

## Browser app

The browser build is the same editor compiled to WebAssembly. Links can open things
directly:

| Query | Effect |
|---|---|
| `?example=<name>` | open an example, e.g. `?example=4-channel-wdm-optical-link` |
| `?tab=<name>` | switch to the document with that name, e.g. `&tab=full-adder` |
| `?examples=demos\|layouts\|chips` | open the examples window on that tab |
| `?chip=<name>` | pick a chip-circuit example, e.g. `?examples=chips&chip=wdm-demultiplexer` |
| `?sweep` | open the sweep window and run it, e.g. `?example=4-channel-wdm-optical-link&sweep` |
| `?renderer=webgpu` | use WebGPU instead of WebGL2 |

Names are written in lower case with dashes. If the page fails to start, it shows the
error instead of a blank page.
