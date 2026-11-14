# Projects as folders, components by path (0.2.0)

Today a project is one `.shine` file holding every document, and a box refers to a
component by a number that means something only inside that file. Using a component in
another project copies it (and everything it uses) in; a `.shinecomp` file carries its own
copies too. Every neural-network project, for example, ends up with its own "MZI cell".

0.2.0 makes components files that projects share:

- **A project is a folder**: one file per design and per component, and boxes refer to
  components by **relative path**. A component used by several projects is one file; fix it
  once and every project using it sees the fix.
- **A `.shine` file is a bundle**: the same files packed into one, for sending, the browser
  and the examples. Opening one works as today; **Export → Bundle** writes one from a folder,
  and **Save as folder** unpacks one.
- **The format is Protocol Buffers** (a `.proto` schema in the repository, read and written
  with `quick-protobuf`), so the format is documented, other tools can read it in any
  language, and it can grow for years without breaking old files (fields are added, never
  renumbered).
- **Shine's own building blocks are built in**, not copied: the MZI (with arms of a length,
  index and loss) is a built-in block, so meshes use it directly.

## Files

```
my-chip/
  shine-project.pb             the manifest: name, format version, designs, open tabs
  classifier.shinedesign       a design (a layout that isn't a component)
  components/
    layer-1.shinecomp          a component, with its boxes' references as paths
    modulator.shinecomp
  ../shared/mesh-64.shinecomp  a component of another folder: paths may leave the project
```

- Every file starts with the 4 bytes `SHPB` and a 2-byte version, then one protobuf message
  (`Document` for designs and components, `Manifest` for the manifest, `Bundle` for a
  `.shine`). Files with `SHNE` are the current postcard format and are still read
  (upgraded on save).
- A box's block is a `oneof`: a built-in (each built-in its own message) or a component
  path, relative to the file the box is in, with `/` separators on every system.
- A bundle holds the manifest and every file, each under its path in the folder; paths that
  leave the folder (`../shared/…`) are kept under `external/`, and unpacking asks where they
  go.
- In memory nothing changes for the editor: documents keep their `DocId`s, assigned when
  files are read; paths live in a table beside them (`DocId ↔ path`), so moving or renaming
  a component file updates the references that point at it when the project is saved.

## Opening and saving

- **Desktop and the command line**: **Open** takes a folder (or its manifest) or a `.shine`.
  Saving a folder project writes only the files whose documents changed. A component
  opened from outside the folder is saved where it is; if it can't be written (read-only,
  an example), Shine offers to copy it into the project.
- **Browser**: in Chrome and Edge, **Open folder** and **Save** use the File System Access
  API; in browsers without it (Firefox, Safari) projects are `.shine` bundles, opened and
  downloaded as today.
- **Components from Shine itself** (the examples, the reference designs) are read-only
  bundles inside the app, referred to by `shine:` paths (`shine:neural/fashion-mnist/layer-1`);
  saving a project that uses one keeps the reference, so it picks up fixes in later versions.
  **Copy into project** makes an editable copy.

## The examples

The Fashion-MNIST classifier joins the examples (its weights are bundled, 1.1 MB; the chip
is built from them when opened), so anyone can open it, or place its layers in their own
designs.

## Order

1. Built-in MZI with arms; meshes and layers use it (no more "MZI cell" component).
2. The Fashion-MNIST classifier in the examples.
3. The `.proto` schema and protobuf reading and writing of documents, with tests that
   every example and reference design survives the round trip, and that postcard files
   still open.
4. Bundles (`.shine` in the new format) replace the postcard project file.
5. Folder projects on desktop and the command line; paths as references; saving only what
   changed.
6. Folders in the browser (File System Access API).
7. `shine:` components from Shine's examples, and the asset library as a folder of
   component files.

Each step keeps every test passing and existing files opening.
