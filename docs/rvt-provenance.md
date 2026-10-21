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
| `lib.rs` | the crate's documentation and its tests | BIMCraft |

## Format findings

| Finding | Source | Confirmed here on |
|---|---|---|

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

(Filled in as the reader grows.)

## Not verified

- Whether any of this is lawful, or allowed by Autodesk's terms (see the risk, above).
- Anything about releases other than the four samples, and nothing about files that are not Autodesk's own sample projects.
