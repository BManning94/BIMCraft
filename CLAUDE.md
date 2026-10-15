# BIMCraft: instructions for agents

BIMCraft is a clean-room, pure-Rust BIM application following the Revit workflow, modeled on
[storytold/cadcraft](https://github.com/storytold/cadcraft) (the AutoCAD one). Milestone 1, the headless engine with the CLI
and MCP server, and milestone 2, the egui desktop app, are done. What works and what's missing: [README.md](README.md).

## Rules

- **Never crash.** No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` outside tests (the workspace
  lints deny them), and no `unsafe`. Parameters, files and MCP input are hostile: check that numbers are finite and in range,
  and cap sizes (`MAX_POINTS`, `MAX_TEXT`, `MAX_COORD`, `MAX_FILE`). `Session::execute` runs every command under
  `catch_unwind` and restores the model on any failure; `hostile_params_never_panic_or_break_the_model` sweeps every command.
- **Everything is a command** (`ALL` in `crates/core/src/commands.rs`): an id after Revit's tool name, a Revit-style shortcut,
  a params doc written for agents, and a JSON `run`. The CLI, scripts and MCP all go through `Session::execute`, so new
  behaviour is a new command or parameter, with a test.
- **Validate in one place.** Commands build elements; `Model::check` validates the whole model after each command and the
  engine rolls back when it fails. New rules go there, with an error that says what to fix.
- **Clean-room.** Never copy Autodesk content: no families (`.rfa`), templates, icons, help text, or anything from inside
  `.rvt` files. Formats come from public specifications (IFC4 is ISO 16739). Assets are original or openly licensed; the
  lettering is drawn in code (`font.rs`).
- **The core has no UI.** `bimcraft-core` must not depend on a UI toolkit, so front ends stay swappable.
- **The UI is thin.** `crates/ui-egui` reads `session.model` and changes it only through `Session::execute`, never by editing
  the model. Logic any front end needs (what a plan shows, what's under the cursor) belongs in the core: the canvas draws
  `render::draw` and picks with `Drawing::pick`. UI behaviour is tested with egui_kittest: clicks at model points
  (`BimApp::screen`), buttons by label, keys (`crates/ui-egui/src/tests.rs`).
- **Test first.** Write the failing test, watch it fail, then write the code. Drawings and IFC are tested through their output:
  pixels of the PNG, entity references in the STEP file.

## Before you finish

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Looking at the result

- Desktop: `cargo run -p bimcraft -- --sample` (it needs a display; egui 0.36 needs Rust 1.95). After a UI change, look at a
  screenshot of the window, in both themes (View > Dark Mode). Without a display, egui_kittest's `wgpu` feature renders a `Harness` to an image; with no GPU,
  `WGPU_BACKEND=gl` falls back to Mesa's software renderer.
- Plans: `bimcraft-cli run --sample --save /tmp/plan.png`, then read the PNG. After any drawing change, look at it.
- 3D: `bimcraft-cli run --sample --cmd 'render {"view": "{3D}", "path": "/tmp/3d.png"}'` (`azimuth` and `elevation`
  turn it). After touching `view3d.rs`, look at it. Sections and elevations render by name the same way: `"view": "Section 1"`.
- Sheets: `bimcraft-cli run --sample --save /tmp/sheets.pdf` prints every sheet; read them back with poppler (`pdfinfo`,
  `pdftoppm -png`) when it's installed. `render {"view": "A101"}` draws one sheet as a PNG.
- IFC: `bimcraft-cli run --sample --save /tmp/house.ifc`. After touching `ifc.rs`, validate with IfcOpenShell:
  `ifcopenshell.validate.validate(f, logger, express_rules=True)` (needs pytest installed), then build every product's shape
  with `ifcopenshell.geom.create_shape` and compare wall and roof volumes with a hand calculation.
- IFC import: after touching `import.rs` or `step.rs`, open the sample's IFC (`run house.ifc --cmd 'elements {}'`) and
  compare it with the sample, then the same file with its bodies turned into triangle meshes by IfcOpenShell, and a
  building written with IfcOpenShell's API. Render what came in and look at it.

## Conventions

Millimetres; x east, y north, z up. Levels have elevations, and a plan cuts 1200 mm above its level. Wall location lines are
centre lines; walls meeting end to end reach past each other by half the other's thickness so corners close, in plans and in
IFC alike (`Model::join_reach`). Room areas are measured at the wall centre lines.
