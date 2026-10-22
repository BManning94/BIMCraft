# bimcraft-rvt

The reader of Autodesk Revit files (`.rvt`, `.rfa`), which puts a project's elements into a BIMCraft model. Policy comes first:
the crate is Apache-2.0 only, ported in part from rvt-rs and fenced by [docs/rvt-provenance.md](../../docs/rvt-provenance.md)
(what may be read from where, and what is never copied). Autodesk's files are never committed: tests build synthetic ones
(`fixture.rs`, `project_fixture.rs`) and the real samples are read only where `BIMCRAFT_RVT_SAMPLES` names a folder of them.

- **Read only what a record says.** Anything the reader cannot decode (a curtain wall, a sloped floor, stairs, groups, families)
  is left out and listed in the import's `skipped`, never guessed; a rule of the model (`Model::check`) a made element breaks
  leaves that element out with the rule.
- **A module for each part of a file** (`container`, `gzip`, `records`, `schema`, `inventory`, `elem_table`, `atom`...) and **for
  each element kind** it imports (`walls`, `floors`, `roofs`, `columns`, `openings`, `rooms`), joined by `import.rs`. A new kind
  is a module with its own tests, a line in `import.rs`, and its place in the write-up
  [Revit files and the Revit import](../../docs/areas/revit-import.md).
- Lengths and counts a file claims are bounded before anything is allocated (`Limits`), and the sweep of hostile files is part of
  the tests.
- References to levels and hosts in the model are GUIDs (`Model::guid`), and what the import puts in the library is given its ids
  by `sync_library` before the model is checked.
- `cargo test -p bimcraft-rvt` runs in about twenty seconds; filter by module (`walls::`, `floors::`, `container::`).
