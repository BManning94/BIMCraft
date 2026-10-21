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
   Apache-2.0 (the README says so).
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

## Compared with rvt-rs

rvt-rs is built from a scratch clone with its own target directory and an example of its own that prints what it finds (not
part of this repository). Equal on all four samples, release by release:

| | 2023 | 2024 | 2025 | 2026 |
|---|---|---|---|---|
| release, build, GUID, locale, saved path | equal | equal | equal | equal |
| schema classes (and whether the parse reached the end) | 4,418 (yes) | 4,492 (yes) | 4,600 (yes) | 4,690 (yes) |
| tags of `Outline`, `ElementParents`, `Plane` | 316, 1410, 565 | 326, 1451, 584 | 345, 1491, 605 | 353, 1521, 613 |

(The element inventory is compared in the same way once it exists.)

## Not verified

- Whether any of this is lawful, or allowed by Autodesk's terms (see the risk, above).
- Anything about releases other than the four samples, and nothing about files that are not Autodesk's own sample projects.
