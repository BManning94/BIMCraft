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
  and does not show where it came from. The stored-page constants are the exception: they were derived again from the bytes
  alone, by someone who had not seen them (see their section below).
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
| `gzip.rs` | gzip header, bounded inflate, joining a partition's members, the stored-page constants and which streams are paged | Ported in part from rvt-rs `src/compression.rs` at `b6086d4` (the header parse, the list of paged streams, the bounded inflate loop, finding members by their magic), changed (no panics, in-place page stripping, members found by walking from the end of the last one, one inflate cap for a whole stream, trailer checked). The page size and the data in a page are not ported: they are byte observations of the four samples, derived again (see The stored-page constants). The format is RFC 1951 and RFC 1952 |
| `info.rs` | `BasicFileInfo`: release, build, GUID, locale, last saver and path, the `Key: value` block | Ported in part from rvt-rs `src/basic_file_info.rs` at `b6086d4`, changed (the block is found by its first line and each line read from its own start; no scoring of two alignments). rvt-rs says the approach matches Apache Tika and chuongmep/revit-extractor, which are public |
| `atom.rs` | the Atom entry of `PartAtom` (families) and of `ProjectInformation` (a one-entry ZIP): title, save time, product version, OmniClass, categories | Ported in part from rvt-rs `src/part_atom.rs` and `src/project_information.rs` at `b6086d4`, changed (a small scanner of its own instead of quick-xml, size, depth and tag limits). The ZIP is PKWARE's public APPNOTE; the XML is Atom (RFC 4287) with Autodesk's `urn:schemas-autodesk-com:partatom` elements |
| `preview.rs` | finds and checks the PNG in `RevitPreview4.0` | Ported in part from rvt-rs `src/reader.rs` (`preview_png`) at `b6086d4`, changed (every chunk's size and CRC-32 checked, dimensions bounded). The PNG format is the W3C/ISO 15948 specification |
| `schema.rs` | `Formats/Latest`: the list of classes with tag, base, version and field count | Ported in part from rvt-rs `src/formats.rs` (`schema_classes`, the grammar reader) at `b6086d4`, changed (class list only, bounds-checked, nesting capped). The grammar is credited by rvt-rs to STE1200 (Discussion #112), whose basis was not checked |
| `elem_table.rs` | the ElementIds `Global/ElemTable` declares, in the 28-byte or 40-byte record layout | Ported in part from rvt-rs `src/elem_table.rs` at `b6086d4`, changed (ids only, no 12-byte fallback, all counts and offsets checked). rvt-rs credits the frame to STE1200, measured by puzzbobb on 30 files |
| `inventory.rs` | the elements of a project by `BuiltInCategory` (placed instances told from the rest), and its levels with names and elevations | Ported in part from rvt-rs `src/partition_element_records.rs`, `src/partition_element_records_2023.rs` and `src/partition_level_records.rs` at `b6086d4`, changed (marker always from the file's schema, bounds-checked offsets, only id, category, class and placed kept, no bounding boxes or reference lists). The category ids are Autodesk's published `BuiltInCategory` constants; the names are rvt-rs's, measured in Revit's own IFC export of sample files |
| `project_fixture.rs` | a synthetic project (element table, schema, partition records in both forms, levels with name blocks) for tests | BIMCraft, written from the layouts in `inventory.rs` |
| `fixture.rs` | synthetic Compound Files, gzip members, paged and partition streams, `BasicFileInfo`, ZIP, PNG and schema bytes for tests | BIMCraft |

## Format findings

| Finding | Source | Confirmed here on |
|---|---|---|
| A Revit file is a Compound File (signature `D0 CF 11 E0 A1 B1 1A E1`) with a `BasicFileInfo` stream | MS-CFB; rvt-rs `reader.rs` | the four samples |
| The streams of a project: `BasicFileInfo`, `Contents`, `Formats/Latest`, `Global/{ContentDocuments, DocumentIncrementTable, ElemTable, History, Latest, PartitionTable}`, `Partitions/NN`, `ProjectInformation`, `TransmissionData` (a family has `PartAtom` instead of `ProjectInformation`) | rvt-rs `streams.rs` | the four samples list exactly these 12 |
| The number NN of the partition is not fixed by the release: rvt-rs's table gives 66 to 69 for 2023 to 2026, the samples have 20, 21, 20 and 21. The reader finds partitions by the `Partitions/` prefix | byte observation | the four samples |
| A `Global/*` stream is eight bytes and then one gzip member; `Formats/Latest` starts with its member at byte 0 | rvt-rs `compression.rs` (`inflate_at_auto`); observation | the four samples (the member is found in the first 256 bytes) |
| A partition is a 44-byte header and then gzip members of about 128 KiB of output each, with 40 bytes between one and the next: the previous member's CRC-32 and length, then 32 bytes of header the reader does not read; in 52 gaps of each sample there are 188 more bytes (a small structured item with UTF-16 text and length fields, 228 bytes between the members in all) | byte observation (rvt-rs finds members by scanning for the magic and does not describe the gap) | the four samples: the first member at byte 44, 40 bytes between members in 334 of 386, 362 of 414, 363 of 415 and 361 of 413 gaps and 228 in the 52 others, with the pages' runs taken out. The reader does not depend on the 44, the 40 or the 228 |
| The container stores the schema, the `Global` gzip streams and the partitions in pages of 65,249 bytes, counted from byte 0 of the stream: 64,896 of data and a 353-byte run that belongs to no gzip member; inflate runs on the stream with the runs removed | byte observation of the four samples, derived without the numbers (see The stored-page constants). The same numbers are in rvt-rs `compression.rs`, which credits `ahzs645/reviter`'s analysis of a loader and its Discussion #112 (their basis was not checked, see the risk) | the CRC-32 and length of **every** gzip member of every such stream match once the runs are removed, 1,660 members on four samples (1,664 with the four `Contents` members, under a page). A number wrong by one byte breaks hundreds of them |
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
| `Global/ElemTable`: a `u16`, a `u32` count, records from byte 6, 28 bytes up to 2023 and 40 from 2024, the ElementId at +4 or +20 | rvt-rs `elem_table.rs`, credited to STE1200 and puzzbobb | the four samples (13,821 / 13,819 / 13,861 / 13,936 ids, rising, count fits to a short tail) |

### The stored-page constants, and where they come from

`gzip.rs` has two numbers and a list: a stored page is 65,249 bytes, of which the first 64,896 are data and the last 353 a run
that belongs to no gzip member, and the container pages the partitions, the schema and six `Global` streams. The list is
rvt-rs's. The two numbers (and so the 353) are derived from the bytes of the samples, by someone who had not seen them.

**Why a second derivation.** rvt-rs's `compression.rs` credits the numbers to "ahzs645/reviter's ODA loader analysis and
Discussion #112". ODA internals are on the forbidden list and the claim cannot be audited, so the owner had the numbers derived
again. A separate agent session did it on 2026-10-21. It was told not to open `gzip.rs`, any other code that handles pages or trailers, the
earlier rows and text about them in this document, the part 1 hand-over note, rvt-rs's `compression.rs`, or anything of
reviter, ODA or Autodesk until its derivation was written down; it used its own Compound File reader (MS-CFB only), raw inflate
(RFC 1951) and the CRC-32 and length at the end of a gzip member (RFC 1952). It wrote its notes first (`derivation.md` and its
Python programs, kept with the session notes, not in this repository) and read `gzip.rs` and this document after. The earlier
scratch check, which searched a window "chosen around the known answer", is replaced by this.

**What it did, on the four samples** (`rstbasicsampleproject.rvt`, 2023 to 2026; all Compound Files of version 4):

1. Checked every gzip member as stored (header per RFC 1952, raw inflate, CRC-32 and length). The small streams verify. Every
   stream longer than 64,896 bytes does not: in the partition, 300 of 387 members verify in 2023 (324 of 415, 326 of 416 and
   322 of 414 in the others), and `Global/Latest`, `Formats/Latest`, `Global/ContentDocuments` and, from 2024,
   `Global/ElemTable` fail whole. A failing member can still end exactly at its own trailer with too much output (Huffman codes
   resynchronise), so only the CRC and the length count, never "inflate finished".
2. Searched for a run to remove, with no prior. In the first member of each sample's partition that zlib rejects, every cut of 1
   to 4,096 bytes was tried at every offset up to the byte zlib rejects (a cut at or after it leaves that byte as it was). One
   pair makes the member verify, in all four samples: 353 bytes at stream offset 130,145.
3. Cut every failing member by 353 bytes, trying every offset: 86 of 87, 91 of 91, 90 of 90 and 92 of 92 are repaired by one
   cut at one offset (one offset in each has an equal neighbour); the 87th, in 2023, holds two runs.
4. Fitted the spacing: every integer period from 2 to 65,249, the smallest distance between two cuts (two runs cannot be
   closer than the period), was tried against all the cuts. Three fit by congruence, 71, 919 and 65,249 (71 x 919 = 65,249); the
   first two would put a run through members that verify as stored. The first run ends at 1 x 65,249, so pages count from byte 0
   of the stream, the eight-byte prefix of a `Global/*` stream included.
5. Without the fitted model: the single-member streams have a unique cut at 64,896 in 2023 `Global/Latest` and 2024
   `Global/ElemTable`, and a unique second cut at 130,145 in 2023 `Formats/Latest` and `Global/ContentDocuments` once the first
   is removed.
6. Checked the result on everything: remove, from every stream, the bytes at offsets `o` with `o mod 65,249 >= 64,896`, and verify
   every gzip member. **1,664 of 1,664 verify** (36 streams, 388 runs: 93, 98, 98 and 99 per sample), and in the 1,632
   partition members the size in the 32-byte record in front of each (a `u32`, 16 bytes before the magic, equal to the member's
   length from its magic to the end of its trailer, plus 8) equals that length, with no exception. The 388 runs are all different, look random (about 7.4 bits a byte) and all begin with a
   `00` byte; nothing was read into them.

| Sample | Partition | Bytes | Members | Verify as stored | Runs | Verify after removal |
|---|---|---:|---:|---:|---:|---:|
| 2023 | `Partitions/20` | 5,751,736 | 387 | 300 | 88 | 387 |
| 2024 | `Partitions/21` | 5,962,991 | 415 | 324 | 91 | 415 |
| 2025 | `Partitions/20` | 5,998,302 | 416 | 326 | 91 | 416 |
| 2026 | `Partitions/21` | 6,003,729 | 414 | 322 | 92 | 414 |

Moving a number breaks it (members verifying out of 1,664): the layout above 1,664; a start a byte earlier or later 1,293 or
1,290, and 1,289 for the eight-byte prefix; 352 or 354 bytes of run 1,289; a page of 65,248 or 65,250 bytes 1,290 or 1,289; a
page of 65,536 bytes 1,064; no stripping 1,289 (the members that lie inside one page). `a_wrong_page_layout_breaks_the_real_files`
and the other page tests in `crates/rvt/src/tests/mod.rs` keep this: a change of one byte in either constant fails nine tests.

**Compared with the ported values.** Equal: 65,249, 64,896, the 353 run, and stripping from byte 0 of the stored stream. The
list of paged streams is equal where the bytes can tell. Five of its entries are longer than a page in some sample and are
paged (`Partitions/NN`, `Formats/Latest`, `Global/ContentDocuments`, `Global/Latest`, and `Global/ElemTable` from 2024).
`Global/DocumentIncrementTable`, `Global/History` and `Global/PartitionTable` are under 18 KB in every sample, so stripping
removes nothing from them and the samples cannot confirm or refute their place in the list, nor the absence from it of
`Contents`, `BasicFileInfo`, `TransmissionData` and `ProjectInformation` (under 4.1 KB).

**Not derived.**

- What a run holds, or why it is there.
- What a stream that was cut off inside the last 352 bytes of a page looks like (no sample does; the largest remainder of a
  stream length modulo 65,249 below 64,896 is 64,737). `strip_pages` keeps a short last page whole, and a test pins that choice.
- Anything about 32-bit files (there is no such sample), releases before 2023 or after 2026, files that are not this one
  project saved four times, families, or Compound Files of another version than 4.
- Whether `BasicFileInfo`, `TransmissionData` and `ProjectInformation` are paged.
- The head of a partition, the 32-byte records, the 188-byte items and the bytes after a stream's last member. Only the size
  field of the record was used, as a cross-check.

**What this shows about where the numbers come from, and what it does not.** They follow from the bytes of the samples and the
public formats, and the derivation was written down before the ported values were read; they equal rvt-rs's. The code's use of
them therefore rests on observation and not on rvt-rs's second-hand credit. Two limits, plainly. The same numbers are public in
rvt-rs, and the derivation did not see them first, which nobody can verify from the outside. And the agent that wrote this reader
and the one that derived the numbers are language models: whether either met these numbers in its training data cannot be
inspected from here, so no analysis of that can be made. The claim is only that the bytes force these numbers, not that nobody
involved could have known them.

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

The element inventory (`bimcraft-rvt` against the same example of rvt-rs, which decodes every element record and the levels)
is equal on all four files: 5,851 records, 45 categories of element records with the same number of elements and of placed
instances in each (and the levels as a 46th category here: 10 elements, 9 placed), the same nine levels with the same names
and elevations, and the same number of declared ElementIds. 5,861 elements in all, 5,591 of them placed.

## Not verified

- Whether any of this is lawful, or allowed by Autodesk's terms (see the risk, above).
- Anything about releases other than the four samples, and nothing about files that are not Autodesk's own sample projects.
