# The Revit file reader: where every part of it comes from

BIMCraft reads Autodesk Revit files (`.rvt`, `.rfa`) with a reader of its own, `crates/rvt` (package `bimcraft-rvt`). Revit's
format has no public specification, so [CLAUDE.md](../CLAUDE.md) and [ROADMAP.md](../ROADMAP.md) allow it only under the fence
below. This document is the record the fence asks for: every module and every format finding names its source here. A change
to the reader that adds a module or a finding adds its row in the same commit (`crates/rvt/src/tests.rs` fails when a source
file has no row).

## The risk, stated plainly

The project owner chose this on 2026-10-21 and accepted the risk below. None of it is a legal conclusion.

- The agent that wrote this reader is not a lawyer. It did not verify rvt-rs's claim that its work is clean-room, and it makes
  no claim of its own about whether reading Revit's format is lawful in any country, or whether Autodesk's terms for Revit or
  for its sample files allow it. Those are the owner's to decide, with counsel if they want it.
- rvt-rs has one author. Its `CLEANROOM.md` describes a process (accepted and forbidden sources, a declaration on each pull
  request). Whether that process was followed was not audited here, and could not be from the outside.
- Some of rvt-rs's findings are second-hand: it credits Discussion #112 of its own repository (a user, STE1200), puzzbobb's
  measurements across 30 files, and `ahzs645/reviter`, whose "ODA loader analysis" is named as the independent source of the
  stored-page constants in `compression.rs`. What those people used to find them was not checked. Where this reader uses such a
  finding, the row says so and says how it was confirmed here (by the bytes of a sample file), which shows the finding is true
  and does not show where it came from.
- Autodesk's sample projects were downloaded from Autodesk's own public download host (see Sample files). Their terms of use
  were not checked.

## The fence

1. **A crate of its own.** `crates/rvt`, package `bimcraft-rvt`, `license = "Apache-2.0"` (not the repository's
   MIT OR Apache-2.0, because it holds Apache-2.0 code ported from rvt-rs). `bimcraft-core` never depends on it, and neither
   does `crates/update`, `crates/ui-egui` or the MCP server's core path. The command line (`apps/bimcraft-cli`) and the desktop
   (`apps/bimcraft`) take it through the Cargo feature `rvt`, which is on by default; a build without it is
   `--no-default-features`. A test reads the manifests (`crates/rvt/src/tests.rs`).
2. **Attribution, as Apache-2.0 asks.** `crates/rvt/LICENSE` is the licence text; `crates/rvt/NOTICE` carries rvt-rs's
   copyright line (Copyright 2026 Griffin Long) and says what was taken; every file with ported code opens with a header that
   names the rvt-rs source file and commit and says it was changed. A build that includes the crate is distributed under
   Apache-2.0: the README says so, every package (deb, rpm, Arch, AppImage, the AppStream metainfo, the Windows installer and the
   macOS bundle) names Apache-2.0, and each ships the root `NOTICE`, a copy of the crate's that a test keeps equal.
3. **This document.** Every module and every format finding has a source from the accepted list below.
4. **Nothing inside an Autodesk file becomes BIMCraft content.** No family, template, icon or help text is bundled, extracted
   or shipped. The reader reads a file the user gives it. Test fixtures are synthetic files written by the tests; Autodesk's
   files are never committed.
5. **The policy documents say so.** CLAUDE.md's clean-room rule and principle 5 of the roadmap point here.

### Accepted sources

1. Public specifications: MS-CFB (the Compound File Binary format, which Revit files are), RFC 1951 and RFC 1952 (deflate and
   gzip), the PNG specification, and for the schema of the model, IFC4 (ISO 16739).
2. Byte observations of files we are entitled to open: files the user gives the program, and the sample files in the table
   below. An observation says the file and the offsets.
3. rvt-rs's code and documents, under its Apache-2.0 licence, at the commit named below.
4. Autodesk's publicly exported symbol names and its published API constants (the `BuiltInCategory` values such as
   `OST_Walls`). Names and numbers, not behaviour.
5. Autodesk's open-source IFC exporter, `revit-ifc`, to learn what Revit's own IFC export writes. Read, never copied: its code
   is LGPL and would bring that licence in. Part 1 uses none of it.

### Forbidden sources

- Autodesk SDK source, or disassembly or decompilation of Autodesk's binaries beyond their public export names.
- Open Design Alliance SDK source or internals.
- Leaked, breached or NDA material of any kind.
- Copying or translating `revit-ifc`'s code.
- Anyone who has signed an NDA covering Revit internals does not contribute findings that NDA covers.

If a finding's source is not on the accepted list, it does not go in. If one turns out to have come from the forbidden list,
the code goes out, and a different person re-derives it.

## The reference: rvt-rs

| | |
|---|---|
| Repository | https://github.com/DrunkOnJava/rvt-rs |
| Commit read and ported from | `b6086d41b9758fd9b92f42f0a47b8345b9c58056` (2026-10-21, after the v0.6.1 tag) |
| Licence | Apache-2.0; `NOTICE` says "Copyright 2026 Griffin Long" |
| What it says of its sources | `CLEANROOM.md`: public byte observations, exported symbol names, previously published work (Apache Tika, chuongmep/revit-extractor, phi-ag/rvt), IFC specifications, and `revit-ifc` for what the export writes; forbids SDK source, ODA source, leaked or NDA material, disassembly beyond exports |
| How it was used here | Read for the container, the streams, the schema and the partition records; code ported file by file as the module table says; and built from a scratch clone with its own target directory, to compare its output with this reader's on the same files. It is a measuring stick, never a dependency of this repository |

Its README's legal claims were not copied and are not relied on.

## Modules

| File | What it does | Source |
|---|---|---|
| `lib.rs` | the crate's documentation and exports | BIMCraft |
| `error.rs` | the errors, each saying what is wrong with the file | BIMCraft |
| `container.rs` | opens a Compound File with limits (file size, stream count, stored size), lists and reads streams, checks it is a Revit file | Ported in part from rvt-rs `src/reader.rs` and `src/streams.rs` at `b6086d4`, changed (read on demand instead of loaded whole, one `Limits` struct, no panics). The container itself is MS-CFB, read with the `cfb` crate |
| `gzip.rs` | gzip header, bounded inflate, joining a partition's members, the stored-page constants and which streams are paged | Ported in part from rvt-rs `src/compression.rs` at `b6086d4`, changed (no panics, in-place page stripping, members found by walking from the end of the last one, one inflate cap for a whole stream, trailer checked). The format is RFC 1951 and RFC 1952 |
| `info.rs` | `BasicFileInfo`: release, build, GUID, locale, last saver and path, the `Key: value` block | Ported in part from rvt-rs `src/basic_file_info.rs` at `b6086d4`, changed (the block is found by its first line and each line read from its own start; no scoring of two alignments). rvt-rs says the approach matches Apache Tika and chuongmep/revit-extractor, which are public |
| `atom.rs` | the Atom entry of `PartAtom` (families) and of `ProjectInformation` (a one-entry ZIP): title, save time, product version, OmniClass, categories | Ported in part from rvt-rs `src/part_atom.rs` and `src/project_information.rs` at `b6086d4`, changed (a small scanner of its own instead of quick-xml, size, depth and tag limits). The ZIP is PKWARE's public APPNOTE; the XML is Atom (RFC 4287) with Autodesk's `urn:schemas-autodesk-com:partatom` elements |
| `preview.rs` | finds and checks the PNG in `RevitPreview4.0` | Ported in part from rvt-rs `src/reader.rs` (`preview_png`) at `b6086d4`, changed (every chunk's size and CRC-32 checked, dimensions bounded). The PNG format is the W3C/ISO 15948 specification |
| `schema.rs` | `Formats/Latest`: the list of classes with tag, base, version and field count | Ported in part from rvt-rs `src/formats.rs` (`schema_classes`, the grammar reader) at `b6086d4`, changed (class list only, bounds-checked, nesting capped). The grammar is credited by rvt-rs to STE1200 (Discussion #112), whose basis was not checked |
| `elem_table.rs` | the ElementIds `Global/ElemTable` declares, in the 28-byte or 40-byte record layout | Ported in part from rvt-rs `src/elem_table.rs` at `b6086d4`, changed (ids only, no 12-byte fallback, all counts and offsets checked). rvt-rs credits the frame to STE1200, measured by puzzbobb on 30 files |
| `inventory.rs` | the elements of a project by `BuiltInCategory` (placed instances told from the rest), and its levels with names, elevations, Building Story setting and the levels that sit inside other elements | Ported in part from rvt-rs `src/partition_element_records.rs`, `src/partition_element_records_2023.rs` and `src/partition_level_records.rs` (including `building_story_after` and `owned_level_ids`) at `b6086d4`, changed (marker always from the file's schema, bounds-checked offsets, only id, category, class and placed kept, no bounding boxes or reference lists). The category ids are Autodesk's published `BuiltInCategory` constants; the names are rvt-rs's, measured in Revit's own IFC export of sample files |
| `records.rs` | the records of a partition: each element's id, category, class, box and its two reference lists, and the box-less records of types and materials, in both record forms | Ported in part from rvt-rs `src/partition_element_records.rs`, `src/partition_element_records_2023.rs` and `src/partition_type_records.rs` at `b6086d4`, changed (every tag from the file's schema, records handed to a callback, one function per form, bounds-checked) |
| `curves.rs` | the bounded line and the arc in an element's data: a wall's location line | Ported in part from rvt-rs `src/partition_beam_axes.rs` at `b6086d4` (RE-49, RE-75), changed (only the record readers; every read bounds-checked) |
| `data.rs` | an element's serialised data (header, id, bytes to the next header), the newest copy, a material's name, a category's material in the object styles | Ported in part from rvt-rs `src/partition_names.rs`, `src/partition_beam_axes.rs` (`scan_first_records`) and `src/partition_materials.rs` at `b6086d4` (RE-58, RE-91, RE-111, RE-115, RE-116, RE-125, and `src/partition_column_nodes.rs` for a column's geometry node, RE-173), changed (the header and the frame tags from the file's schema; every copy kept with its partition) |
| `types.rs` | a type's name and layers, a wall's orientation, the one type a record names | Ported in part from rvt-rs `src/partition_compound_structure.rs`, `src/partition_names.rs` and `src/partition_type_records.rs` at `b6086d4` (RE-28, RE-53, RE-54, RE-111, RE-112), changed (frame tags from the schema, a name only in the type's own data) |
| `walls.rs` | walls: the location line and arc, the side of the exterior, levels and height from the reference list and the box, type and layers | Ported in part from rvt-rs `src/element_record_level_refs.rs`, `src/element_record_wall_joins.rs` and `src/partition_compound_structure.rs` at `b6086d4` (RE-27, RE-29, RE-54, RE-59), changed (builds BIMCraft's parametric wall; its ends are the line's and BIMCraft joins walls where they meet) |
| `sketch.rs` | a sketched element's boundary: its sketch lines (a curve each) closed into regions and their voids, by the curves' ends or, before 2024, by their boxes | Ported in part from rvt-rs `src/element_record_plan_profiles.rs` and `src/partition_beam_axes.rs` (`first_sketch_curve`) at `b6086d4` (RE-25, RE-50, RE-96), changed (one tracer for both ways, arcs drawn as chords within 0.5 mm, every size capped) |
| `floors.rs` | floors and ceilings: the sketch, the one type the reference list names, the level, the offset | BIMCraft, from the findings above (rvt-rs `element_record_level_refs.rs` for the levels, RE-27, RE-59) |
| `names.rs` | the name entries of families and family types (up to 2023 and from 2024), and an id's value in its newest partition | Ported in part from rvt-rs `src/partition_names.rs` at `b6086d4` (RE-38, RE-109, RE-145), changed (both forms in one module, the newest partition's entry wins) |
| `openings.rs` | doors and windows: the host wall, the hole as the instance's box along the wall, the type by its name entry, overlapping neighbours fitted | BIMCraft, from the findings below (rvt-rs `partition_schema_mvp.rs` `opening_instances_from_records` and `bind_opening_hosts` for the host rule, RE-23, RE-85) |
| `placement.rs` | a family instance's placement in its data: a rotation and an origin | Ported in part from rvt-rs `src/partition_instance_transforms.rs` at `b6086d4` (RE-87), changed (the bytes of one instance's data, bounds-checked) |
| `columns.rs` | columns, architectural and structural: place and size from the box (the symbol's envelope when turned), levels and height as a wall's, type by its name entry | BIMCraft, from the findings below (rvt-rs `element_record_column_cuts.rs`, `partition_column_nodes.rs`, RE-26, RE-173) |
| `roofs.rs` | roofs by footprint, flat ones only | BIMCraft, from the findings above (rvt-rs `partition_roof_slopes.rs`, RE-56, is not ported: no roof of the samples sets an edge's slope) |
| `rooms.rs` | rooms: number, name, level, and a point the imported walls enclose | BIMCraft, from the findings below (rvt-rs `partition_room_parameters.rs`, RE-117) |
| `import.rs` | a Revit project read into a `bimcraft-core` model: the levels, the elements by kind, the check-and-drop loop that leaves out what breaks a model rule and says why | BIMCraft, following the pattern of `bimcraft-core`'s IFC importer (`import::ifc`) |
| `project_fixture.rs` | a synthetic project (element table, schema, partition records in both forms, levels with name blocks) for tests | BIMCraft, written from the layouts in `inventory.rs` |
| `fixture.rs` | synthetic Compound Files, gzip members, paged and partition streams, `BasicFileInfo`, ZIP, PNG and schema bytes for tests | BIMCraft |

## Format findings

| Finding | Source | Confirmed here on |
|---|---|---|
| A Revit file is a Compound File (signature `D0 CF 11 E0 A1 B1 1A E1`) with a `BasicFileInfo` stream | MS-CFB; rvt-rs `reader.rs` | the four samples |
| The streams of a project: `BasicFileInfo`, `Contents`, `Formats/Latest`, `Global/{ContentDocuments, DocumentIncrementTable, ElemTable, History, Latest, PartitionTable}`, `Partitions/NN`, `ProjectInformation`, `TransmissionData` (a family has `PartAtom` instead of `ProjectInformation`) | rvt-rs `streams.rs` | the four samples list exactly these 12 |
| The number NN of the partition is not fixed by the release: rvt-rs's table gives 66 to 69 for 2023 to 2026, the samples have 20, 21, 20 and 21. The reader finds partitions by the `Partitions/` prefix | byte observation | the four samples |
| A `Global/*` stream is eight bytes and then one gzip member; `Formats/Latest` starts with its member at byte 0 | rvt-rs `compression.rs` (`inflate_at_auto`); observation | the four samples (the member is found in the first 256 bytes) |
| A partition is a 44-byte header and then gzip members of about 128 KiB of output each, with 40 bytes between one and the next: the previous member's CRC-32 and length, then 32 bytes of header the reader does not read | byte observation (rvt-rs finds members by scanning for the magic and does not describe the gap) | 2024 sample. The reader does not depend on the 44 and the 40 |
| The container stores the schema, the `Global` gzip streams and the partitions in pages of 65,249 bytes, 64,896 of data and a 353-byte trailer; inflate runs on the stream with the trailers removed | rvt-rs `compression.rs`, which credits `ahzs645/reviter`'s analysis of a loader and its Discussion #112 (their basis was not checked, see the risk) | the CRC-32 and length of **every** gzip member of every such stream match, 1,660 members on four samples; a wrong strip would break them on any stream over one page |
| The gzip members are standard, with CRC-32 and length after the deflate stream; rvt-rs calls Revit's streams "truncated" (no trailer). The reader accepts both and reports how many members carried a matching trailer | observation | all 1,660 members of the four samples carry a matching trailer. A file with truncated members is only covered by a synthetic test |
| `BasicFileInfo` is a binary head and then a block of `Key: value` lines in UTF-16LE separated by CR LF, starting at the line `Worksharing:` (some releases put it at an odd byte offset); the value of `IsSingleUserCloudModel` is a narrow `\0False\0` inside the wide text; the stream's last line ends in a narrow CR LF; `Format:` (2019 and later) or `Revit Build: Autodesk Revit 2016 (Build: ...)` (2016 to 2018) gives the release | rvt-rs `basic_file_info.rs`, which says the approach is the one Apache Tika and chuongmep/revit-extractor use. The narrow CR LF at the end is not in rvt-rs: found here | the four samples: all 18 lines of each read, in file order, the same keys on every release. The 2016 to 2018 form and a block at an odd offset are only covered by synthetic tests |
| `Username` is empty unless the file is workshared; the saved path (`Last Save Path`) is the saving machine's, with its user folder in it | byte observation | the four samples (the user folder is in the path; `Username` is empty). `info` shows both and nothing writes them into a project or an IFC file |
| `ProjectInformation` is a ZIP archive with one deflated entry holding the Atom entry (title, update time, `A:product-version`); the entry's name is a temporary path with the saving user's folder in it, so the reader skips it | rvt-rs `project_information.rs` | the four samples (title `rstbasicsampleproject`, product version equal to the release) |
| A family's `PartAtom` is the same Atom entry as plain XML; its OmniClass code is a category term of dotted digits | rvt-rs `part_atom.rs` | **not confirmed on a real file** (the samples are projects): synthetic test only |
| `RevitPreview4.0` is a wrapper of about 300 bytes and then a PNG | rvt-rs `reader.rs` | **not confirmed on a real file**: the four sample projects have no such stream. Synthetic test only |
| `Formats/Latest` is a list of classes in a grammar (zero `u16`, counted name, base reference, version, field count, fields, GUIDs); a class's tag is its definition ordinal starting at 12, inline bases included | rvt-rs `formats.rs`, which credits STE1200's measurements (Discussion #112) | the four samples read to their last byte with no stop, 4,418 / 4,492 / 4,600 / 4,690 classes (2023 to 2026), the count and the tags of `Outline`, `ElementParents` and `Plane` equal to what rvt-rs reads on the same files |
| A partition holds one record per element: a header (`u64` ElementId, `u32` size, `u16` constant, `u16` count), a body, and a 16-byte trailer that repeats the size. The constant is the last tag of the record marker less 12 | rvt-rs `partition_element_records.rs` (RE-35) | 2024, 2025, 2026: the chain, walked from byte 0 until a header or trailer does not fit, has exactly one record for each declared ElementId (13,819 / 13,861 / 13,936), and ends a few MB into a partition of about 40 MB (the rest is the loaded families' own documents) |
| From 2024 an element record's frame has fixed offsets from its start: `BuiltInCategory` (`i64`, negative) at `+0x12`, container at `+0x32`, placement kind at `+0x42`, class tag at `+0x4a`, the marker `[Outline tag] ff ff ff ff [ElementParents tag]` at `+0x50`, a bounding box of six `f64` (feet) at `+0x58`. The tags come from the file's own schema. A record with no container and the kind `0xffffef7f` is a placed instance (the rest are a type's symbols, a group's members) | rvt-rs `partition_element_records.rs` (RE-21, RE-32, RE-80) | 2024, 2025, 2026: 5,851 records per file, equal to rvt-rs's by category |
| Up to 2023 the record is laid out back from the marker: `u32` ElementId 52 bytes before it, the tag of `ElementHeader` at +8 from the id, `BuiltInCategory` 38 before, container (`u32`) 22 before, kind 14 before, class tag 6 before | rvt-rs `partition_element_records_2023.rs` (RE-81, RE-107) | 2023: 5,851 records, equal to rvt-rs's and to the 2024 file's of the same model |
| A frame whose first `u64` is 0 or not an ElementId (the second prologue) is a placed instance inside another record's body and takes that record's ElementId | rvt-rs `partition_element_records.rs` (RE-30, RE-35) | **not confirmed on a real file**: none of the 5,851 records of the four samples is such a frame (rvt-rs attributes 0 as well). rvt-rs measured the rule on Autodesk's Snowdon Towers samples, which were not used here. Synthetic tests only (with the chain it rests on checked above) |
| A Level record is found by `OST_Levels` (-2000240) at `+0x12`, with the last six bytes of the marker at `+0x50` (it has no bounding box); its name and elevation are in a block keyed by its ElementId: a run of `ff` (56 bytes, 28 up to 2023), three bytes of padding, the name as a counted UTF-16 string, then `05 00 00 00` and the tag of `Plane`, and 55 bytes from the start of that marker the elevation in feet, which appears again with the 24 bytes before it. A level rewritten in a later partition keeps its older block: the highest-numbered partition's is current | rvt-rs `partition_level_records.rs` (RE-24, RE-51, RE-107) | the four samples: nine levels each, names and elevations (-2700 mm to 9000 mm) equal to rvt-rs's, none unread |
| A Level's Building Story setting is the byte after the first unset id and `00 01` within 0x200 bytes past its name, a number and the level's own id (`u64` from 2024, `u32` before): 1 for a building story, 0 for a level that is not one | rvt-rs `partition_level_records.rs` (RE-118), measured by rvt-rs against the Building Story parameter of Snowdon Towers' VIM export | Snowdon Towers structural 2024: 12 levels with 1 and the seven that rvt-rs names (Top of Footing and the six "TOS" levels) with 0; Snowdon architectural: 18 of 18 with 1; the architectural basic sample (2024, 2025): 6 of 6 with 1. The structural basic sample (2023 and 2024) reads 0 on all nine levels, which no Revit export was compared with here |
| A Level inside another element has a block opened by `01 00 00 00` and its `u64` ElementId, then eight `ff` and another element's ElementId where a project level has a run of 56 `ff`, and a counted name 0x43 bytes on. It is not a project level | rvt-rs `partition_level_records.rs` (`owned_level_ids`, RE-51) | Snowdon Towers architectural 2024: exactly two (ids 1981744 and 1982117, which rvt-rs also finds), and none on the other five files (the four structural basic samples and the architectural basic ones) |
| A box-less record (a type, a material, a level) has the last six bytes of the marker at `+0x50`, the release's prologue constant (the `ElementParents` tag less 12) at `+0x0c` and, from 2024, a counted `u64` list at `+0x56` that holds the record's own id. A type's placement kind is `0xffff8080`, a material's `0xffff8020` | rvt-rs `partition_type_records.rs` (RE-28) | architectural basic sample 2024: all 1,651 box-less records with a declared id hold their own id in the list; the kind is 0xffff8080 for the 11 wall, 5 floor, 3 roof and 2 ceiling types (and the 2 foundation types), 0xffff8020 for the 174 materials. 2023 (a level's frame, no slot list): the same kinds on the structural sample |
| After the box a record holds two counted lists of ElementIds (`u64`, `u32` up to 2023); the first names the element's type, the levels it is constrained to and the walls it joins | rvt-rs `partition_element_records.rs` (RE-23, RE-26, RE-27, RE-29) | architectural basic sample 2024 to 2025 and structural sample 2023 to 2026: every wall names exactly one wall type and one or two levels |
| An element's serialised data opens with `ff ff ff ff`, the schema's tag of `CellList`, `01 00 00 00` and the element's id (`u64`, or `u32` and four more bytes up to 2023), and runs to the next such header; an element edited after an earlier save is written again in a later partition, which holds the current copy | rvt-rs `partition_names.rs`, `partition_beam_axes.rs` | every wall and wall type of the architectural basic sample (2023, 2024, 2025) has data; the newest-copy rule is tested on a synthetic file only (the samples have one partition) |
| A wall's location line is its data's first curve record: a bounded line (`04 00 08 01`, start and end parameter, origin, unit direction) or, after a `u64` 1, an arc (angles, axes, radius, centre), in feet, and is the wall's centreline whatever its location-line setting | rvt-rs `partition_beam_axes.rs` (RE-49, RE-75), `partition_compound_structure.rs` (RE-54) | architectural basic sample: the line of each of the 47 walls with a type runs down the middle of rvt-rs's body (the thin axis of 44 axis-parallel walls to 0.2 mm), the walls' vertical extents equal rvt-rs's to 0.1 mm, and the union of the import's wall footprints (67.72 m2) is within 0.5% of the union of rvt-rs's (67.88 m2) |
| A wall's data has, after `ff ff ff ff 01 00 00 00`, its location line setting, a word (not before 2021) and a flip: with the flip set the first layer, the exterior, is to the right of the line | rvt-rs `partition_compound_structure.rs` (`wall_orientation`, RE-54, RE-126) | architectural basic sample: for all 47 walls the side of the first layer, read from rvt-rs's `IfcMaterialLayerSetUsage`, is the side the import puts its exterior on (47 of 47) |
| A type's layers are a counted list after `ff ff ff ff` and the schema's tag of `VerticalRegionsStructure`, or after the type's name and a zero word: width in feet, material id, deck id, function (1 structure, 2 substrate, 3 thermal, 4 and 5 finish, 100 membrane, 200 deck); 37 bytes a record in 2024 and 2025, 29 up to 2023 (ids `u32`), 41 in 2026 and 2027 | rvt-rs `partition_compound_structure.rs` (RE-53, RE-112, RE-124) | architectural basic sample 2023, 2024, 2025: the layer widths and materials of all 47 walls equal rvt-rs's export, layer by layer (47 of 47). 2026 and 2027: synthetic only |
| A material is an object `01 00 00 00` + id with the schema's tag of `Material` 0x47 bytes past a `u64` id (0x27 past a `u32`); its name is read by rules in order (its own name field, the string that ends at `PhysicalParamSet`, the first string framed by `PatternHelper`, the `-1001203` entry, and from 2024 the string closed by eight zero bytes, an id and `ff` ×8; up to 2023 also the first framed name in its own data) | rvt-rs `partition_materials.rs` (RE-58, RE-116, RE-125) | architectural basic sample 2023 to 2025: the material names on the layers of the 47 walls equal rvt-rs's, 47 of 47 |
| A layer that names no material takes its category's, written in the object styles: the category id, an unset id, `01 00 00 00`, another unset id and the material (up to 2024); from the `i32` id and `ff` ×4 in 2023 and before | rvt-rs `partition_materials.rs` (`scan_category_material`, RE-91, RE-115) | not met on the samples (the layers of the 47 walls name their materials); synthetic only |
| A sketched element's boundary is its sketch lines: records of the category `OST_SketchLines` (-2000045) whose second reference list ends in the sketched element's id, each with its curve (a bounded line, or an arc not marked by a word before it) as the first curve record of its data; before 2024 only their boxes are read, and a diagonal's two candidate ends are settled only when exactly one open pair fits | rvt-rs `element_record_plan_profiles.rs` (RE-25, RE-50, RE-96), `partition_beam_axes.rs` | architectural basic sample 2023 to 2025: all 8 floors and 2 ceilings close into the boundary rvt-rs's export has (the footprint areas equal to 0.005 m2, their tops and bottoms to 0.1 mm, their storeys); the structural sample's 5 floors at 2023 to 2026 the same. A floor has the level its list names and its top is its box's top; a ceiling's underside is its box's bottom |
| A door's or window's host is a wall its first reference list names: the nearest below the record's own id, else the nearest above, skipping curtain walls; an opening whose only candidates are curtain walls has none | rvt-rs `partition_schema_mvp.rs` (RE-23, RE-85, #439) | architectural basic sample 2023 to 2025: the host of every door and window rvt-rs's export sets in a wall is the wall this rule names (26 of 26), and the 7 openings with no host there name only curtain walls or no wall |
| The hole a door or window cuts in a wall that runs along an axis is its instance's box along the wall (centre and width) and in height (the sill is the box's bottom less the wall's base) | rvt-rs `ifc` export (RE-93, RE-94), which cuts from the record box | architectural basic sample: the centre, width, bottom and top of the hole of each of the 25 openings that come in equal rvt-rs's cut to 0.2 mm, but for the ten windows that overlap their neighbours in the file (by up to 94 mm), which are narrowed to fit |
| A family type's name is a name entry: the type's `u64` id, a counted UTF-16 name and the category; a door's or window's type is the one family symbol of its category its list names that has such an entry | rvt-rs `partition_names.rs` (`resolve_type`, RE-38) | architectural basic sample 2023 to 2025: the type name of each door and window that comes in equals the type part of the name in rvt-rs's export (25 of 25; a window narrowed to fit takes a numbered copy of it) |
| A column's record box is its whole prism: its plan rectangle is the symbol's section for a column along the axes, and a column turned by a placement angle that is no right angle has its symbol's envelope (family axes) turned by that angle about the placement's origin | rvt-rs `element_record_column_cuts.rs` (RE-26, RE-29), `partition_instance_transforms.rs` (RE-87) | architectural basic sample (2023 to 2025: 3 structural columns) and structural sample (2024 to 2026: 30): the centre and size in plan of every column equal rvt-rs's body to 0.1 mm. No sample column is turned: the turned case is synthetic only |
| A column's geometry node (`u64` id, `u64 3`, `u64 -1`, with two boxes of six `f64` at 96 and 48 bytes before the id) holds the column's true z-range where its record box begins at 0.0 | rvt-rs `partition_column_nodes.rs` (RE-173) | structural sample 2024, 2025, 2026: column 1173925 starts at 0.0 where its record box begins 350 mm lower, as in rvt-rs's export. The 2023 file's node is laid out otherwise and is not read: its column keeps the box |
| A room's number and name are the parameter entries `-1006901` and `-1006900` (`i64`, or `i32` up to 2023), each followed by a counted UTF-16 string, in its object (`01 00 00 00` and its id) within 0x400 bytes; the newest partition's copy decides | rvt-rs `partition_room_parameters.rs` (`scan_room_parameter_entries`, RE-117, RE-196) | architectural basic sample 2023 to 2025: the numbers and names of the 9 rooms that come in equal those of rvt-rs's `IfcSpace` (`Name` and `LongName`) |
| A roof by footprint is a record of the class `ProfileRoof`, with sketch lines as a floor's; a roof by extrusion has another class. A footprint roof is flat where its box is as tall as its type is thick | rvt-rs `partition_roof_slopes.rs`, `element_record_plan_profiles.rs` (RE-50, RE-56) | architectural basic sample: the one roof by footprint comes in with rvt-rs's footprint area (0.005 m2), underside and top to 0.1 mm; Snowdon Towers (architectural) has 20 roofs by footprint, all of which rise, and so are left out |
| A type's box-less record may sit inside another record (the second prologue): its frame has no id of its own and no prologue constant, and its slot list names the id of the record it sits in | rvt-rs `partition_type_records.rs` (`decode_second_prologue_at`, RE-30, RE-35) | Snowdon Towers architectural 2024: without it, 1,081 of its 1,100 walls name no wall type; with it, 1,062 of its walls come in |
| `Global/ElemTable`: a `u16`, a `u32` count, records from byte 6, 28 bytes up to 2023 and 40 from 2024, the ElementId at +4 or +20 | rvt-rs `elem_table.rs`, credited to STE1200 and puzzbobb | the four samples (13,821 / 13,819 / 13,861 / 13,936 ids, rising, count fits to a short tail) |

### The stored-page constants, and where they come from

`gzip.rs` has three constants and a list: a stored page is 65,249 bytes, of which 64,896 are data and 353 a trailer, and the
streams the container pages are the partitions, the schema and six `Global` streams. All of it is ported from rvt-rs's
`compression.rs`, and that file credits the constants to "ahzs645/reviter's ODA loader analysis and Discussion #112". ODA
internals are on the forbidden list, so this is exactly what was and was not done with that claim:

- **Ported:** the three numbers and the list of paged streams, from rvt-rs. Nothing from reviter, from ODA or from any
  Autodesk code was read by whoever wrote this reader; the origin claim is rvt-rs's, second-hand, and was not checked.
- **Confirmed from the bytes of the samples:** with those constants the pages are removed, the CRC-32 and length at the end of
  **every** gzip member of every paged stream match, 1,660 members over the four files (the real-file test in
  `tests/mod.rs`). Without them, any member that crosses a page boundary does not inflate to its CRC. A second check, run
  once as a scratch program and not committed: for the first member of the 2024 sample's partition that crosses a boundary,
  every pair of (position of a removed run, its length) with the position in 64,000 to 65,500 and the length in 1 to 1,100
  was tried (1.65 million inflates); exactly one pair makes the member verify, 64,896 and 353. That says the bytes force these
  numbers within that window. It is not a blind derivation: the window was chosen around the known answer.
- **Recommendation:** re-derive the constants from the bytes alone before anyone relies on this reader in a way that makes the
  origin matter, and replace the rvt-rs attribution in this table with the derivation. It is possible: the members are gzip
  with a CRC, so the position and length of a removed run can be searched for with no prior, using the spacing of the members
  and the period between runs; a first attempt here (a blind search near where raw inflate stops) did not finish usefully, and
  a better search was not built in part 1. The constants are facts about a file layout and are verified true; whether
  their provenance through a third party's analysis matters is for the owner to decide.

## Sample files

Downloaded on 2026-10-21, one new folder per file outside the repository, never committed. All four start with the CFB magic
`D0 CF 11 E0 A1 B1 1A E1`, and each size and SHA-256 equals the pin in rvt-rs's `research/autodesk-sample-projects.tsv`.
`BIMCRAFT_RVT_SAMPLES` names the folder that holds them; tests that need them skip when it is unset.

| Release | File | URL | Bytes | SHA-256 |
|---|---|---|---|---|
| 2023 | `rstbasicsampleproject.rvt` | https://revit.downloads.autodesk.com/download/2023RVT_RTM/Docs/InProd/rstbasicsampleproject.rvt | 6344704 | `091ba54f47a2a84b45f925715ef4f4e3f910a16345b7d370150388cdd23c5391` |
| 2024 | `rstbasicsampleproject.rvt` | https://revit.downloads.autodesk.com/download/2024RVT_RTM/Docs/InProd/rstbasicsampleproject.rvt | 6602752 | `aab838f861e0473be1cbb00e3ec57a20bc53fc16e1cec4c5d049bb401fcc616e` |
| 2025 | `rstbasicsampleproject.rvt` | https://revit.downloads.autodesk.com/download/2025RVT_RTM/Docs/InProd/rstbasicsampleproject.rvt | 6651904 | `5089b2c661573cef8fd0741d044535497ae8f52bdb76973ceb47626483131acb` |
| 2026 | `rstbasicsampleproject.rvt` | https://revit.downloads.autodesk.com/download/2026RVT_RTM/Docs/InProd/rstbasicsampleproject.rvt | 6672384 | `e7cf23a9627d3ee97ecb9fdf1ce0e34fd582c1d7ed0080fdc00f9e6563375418` |

The server answered 200 with no sign-in or licence step, and the files are "rst_basic", Autodesk's structural sample project.

Part 2 added these, downloaded on 2026-10-21 from the same host (HTTP 200, no sign-in or licence step), each in a folder of its own outside the repository and never committed. Size and SHA-256 equal the pins in rvt-rs's `research/autodesk-sample-projects.tsv`.
`BIMCRAFT_RVT_SAMPLES` may name several folders, parted like a `PATH`.

| Release | File | URL | Bytes | SHA-256 |
|---|---|---|---|---|
| 2024 | `racbasicsampleproject.rvt` (architectural basic sample) | https://revit.downloads.autodesk.com/download/2024RVT_RTM/Docs/InProd/racbasicsampleproject.rvt | 19722240 | `bb62c161b7991f0c5ec76605d3d9d0407e04001a2e4e6acac1c7fc32fd45ee55` |
| 2025 | `racbasicsampleproject.rvt` | https://revit.downloads.autodesk.com/download/2025RVT_RTM/Docs/InProd/racbasicsampleproject.rvt | 19750912 | `41f1893a62d974edbe19f6077f7b75d0b662fff8c6ed167576e7ff568d3d4293` |
| 2024 | `Snowdon Towers Sample Architectural.rvt` | https://revit.downloads.autodesk.com/download/2024RVT_RTM/Docs/InProd/Samples/Snowdon%20Towers%20Sample%20Architectural.rvt | 94691328 | `33271010919acb2a0d0d040851393a511d86ea7fd9a1f4c054797ae03e5e44a1` |
| 2024 | `Snowdon Towers Sample Structural.rvt` | https://revit.downloads.autodesk.com/download/2024RVT_RTM/Docs/InProd/Samples/Snowdon%20Towers%20Sample%20Structural.rvt | 28057600 | `6dc833f9fe9fa702607265f2e3d581a6d2a928ea56133db8225c9b5e0f79a44f` |

Autodesk's terms for these were not read either.

## Compared with rvt-rs

rvt-rs is built from a scratch clone with its own target directory and an example of its own that prints what it finds (not
part of this repository). Equal on all four samples, release by release:

| | 2023 | 2024 | 2025 | 2026 |
|---|---|---|---|---|
| release, build, GUID, locale, saved path | equal | equal | equal | equal |
| schema classes (and whether the parse reached the end) | 4,418 (yes) | 4,492 (yes) | 4,600 (yes) | 4,690 (yes) |
| tags of `Outline`, `ElementParents`, `Plane` | 316, 1410, 565 | 326, 1451, 584 | 345, 1491, 605 | 353, 1521, 613 |

The element inventory (`bimcraft-rvt` against the same example of rvt-rs, which decodes every element record and the levels)
is equal on all four files: 5,851 records, 45 categories of element records with the same number of elements and of placed
instances in each (and the levels as a 46th category here: 10 elements, 9 placed), the same nine levels with the same names
and elevations, and the same number of declared ElementIds. 5,861 elements in all, 5,591 of them placed.

## Not verified

- Whether any of this is lawful, or allowed by Autodesk's terms (see the risk, above).
- Anything about releases other than the four samples, and nothing about files that are not Autodesk's own sample projects.
