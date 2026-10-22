// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_element_records.rs (the
// 64-bit element record, its fixed offsets, the record chain and the second-prologue rule, the BuiltInCategory names),
// src/partition_element_records_2023.rs (the 32-bit form, 2015 to 2023) and src/partition_level_records.rs (Level records and
// their name and elevation blocks).
// Changed: rewritten for BIMCraft; the record marker always comes from the file's own schema, every offset is bounds-checked,
// only the id, category, class and whether the record is a placed instance are kept (no bounding boxes, no reference
// lists), the walk over a partition is one function per record form, and a Level's Building Story setting and the Levels that
// sit inside other elements (RE-51, RE-118) are read from the same blocks.
//! The element inventory: which elements a project holds, by `BuiltInCategory`, and its levels.
//!
//! A partition (`Partitions/NN`) holds one record per element, each opening with a header (the ElementId, the record's size,
//! a constant, a count) and closing with a 16-byte trailer that repeats the size. In 2024 and later a record's
//! frame has fixed offsets from its start: the `BuiltInCategory` (a negative `i64`, Autodesk's published API constant) at
//! `+0x12`, the container reference at `+0x32`, the placement kind at `+0x42`, the class tag at `+0x4a`, a marker at `+0x50`
//! and the bounding box after it. The marker is the schema's tags for `Outline` and `ElementParents` with `ff` ×4 between.
//! Some frames have no ElementId of their own (the second prologue): they sit inside another record's body and take that
//! record's id. Up to 2023 the ElementId is a `u32` 52 bytes before the marker and the category sits 38 bytes before it.
//!
//! A *placed instance* is a record with no container and the placed kind: a family's types and a group's members are
//! records too, and counting only placed instances is what matches the elements Revit's own IFC export writes.
//!
//! A Level has no bounding box, so no `Outline` marker: it is found by its category, and its name and elevation are in a
//! separate block keyed by its ElementId (a run of `ff`, the name as counted UTF-16, then an elevation in feet that appears
//! twice). Past the name the block holds Revit's Building Story setting, which says whether the level is a storey. A Level that
//! sits inside another element has a block whose run of `ff` is broken by that element's id; it is not a project level.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Error, Result};
use crate::schema::Schema;

/// The band `BuiltInCategory` values fall in.
const CATEGORY_MIN: i64 = -2_100_000;
const CATEGORY_MAX: i64 = -1_990_000;
/// `OST_Levels`.
pub const OST_LEVELS: i64 = -2_000_240;

/// Offsets of a 2024 and later frame from its start.
const CATEGORY_AT: usize = 0x12;
const CONTAINER_AT: usize = 0x32;
const KIND_AT: usize = 0x42;
const CLASS_AT: usize = 0x4a;
const MARKER_AT: usize = 0x50;
const BBOX_AT: usize = 0x58;
const FRAME_LEN: usize = BBOX_AT + 48;
/// The kind word of a placed instance, and "no container".
const KIND_PLACED: u32 = 0xffff_ef7f;
/// The kind Revit 2015 to 2018 write on a Level.
const LEVEL_KIND_2015: u32 = 0xffff_ef6f;
const NO_CONTAINER: u64 = u64::MAX;

/// A record chain's header and trailer lengths.
const CHAIN_HEADER: usize = 16;
const CHAIN_TRAILER: usize = 16;

/// 32-bit form (up to 2023), as offsets back from the marker.
const ID_BEFORE_MARKER: usize = 52;
const CATEGORY_BEFORE_MARKER: usize = 38;
const CONTAINER_BEFORE_MARKER: usize = 22;
const KIND_BEFORE_MARKER: usize = 14;
const CLASS_BEFORE_MARKER: usize = 6;
const HEADER_TAG_AFTER_ID: usize = 8;

/// Level name blocks.
const RUN_64: usize = 56;
const RUN_32: usize = 28;
const MAX_NAME_CHARS: usize = 128;
const ELEVATION_SEARCH: usize = 2048;
const ELEVATION_AFTER_MARKER: usize = 55;
const CONFIRM_LEAD_IN: usize = 24;
const MAX_ELEVATION_FEET: f64 = 1.0e7;
/// Where a level's Building Story setting is looked for past its name, and what frames it: an unset id, `00 01`, a number and
/// the level's own id (`u64` from 2024, `u32` before), then the setting.
const STORY_WINDOW: usize = 0x200;
/// Where the level's name starts from the `u64` ElementId of its block, and the `01 00 00 00` that opens an object keyed by one.
const NAME_AFTER_OWNER: usize = 0x47;
const OBJECT_PREFIX: [u8; 4] = [1, 0, 0, 0];

/// The releases the record forms are known for (rvt-rs's), of which 2023 to 2026 are checked here on sample files.
pub const RELEASES: std::ops::RangeInclusive<u32> = 2015..=2027;

/// One element: its ElementId, its `BuiltInCategory`, its class (the tag in the file's schema) and whether it is a placed
/// instance. An element seen in several records is one element, placed if any of its records is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Element {
    pub category: i64,
    pub id: u32,
    pub class_tag: u16,
    pub placed: bool,
}

/// A level, with the elevation as stored (feet).
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub id: u32,
    pub name: String,
    pub elevation_feet: f64,
    /// Revit's Building Story setting, where the block holds it: only a building story is a storey in Revit's own IFC export.
    pub building_story: Option<bool>,
}

impl Level {
    /// The elevation in millimetres (1 ft is 304.8 mm exactly).
    pub fn elevation_mm(&self) -> f64 {
        self.elevation_feet * 304.8
    }
}

/// How many elements a category has, and how many of them are placed instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub category: i64,
    pub elements: usize,
    pub placed: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    /// ElementIds the element table declares.
    pub declared: usize,
    /// Element records read, before elements seen twice are joined.
    pub records: usize,
    /// One per element and category, ordered by category then id. Levels are here too, under `OST_Levels`.
    pub elements: Vec<Element>,
    /// The levels whose name and elevation could be read, by elevation.
    pub levels: Vec<Level>,
    /// Level elements (placed instances) with no readable name block, not counting those inside other elements.
    pub unread_levels: usize,
    /// Level elements whose block names another element that holds them: not project levels, and not in `levels`.
    pub owned_levels: Vec<u32>,
}

impl Inventory {
    /// Elements per category, ordered by category id.
    pub fn counts(&self) -> Vec<Count> {
        let mut by: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
        for e in &self.elements {
            let c = by.entry(e.category).or_default();
            c.0 += 1;
            c.1 += usize::from(e.placed);
        }
        by.into_iter().map(|(category, (elements, placed))| Count { category, elements, placed }).collect()
    }
}

/// The name of a `BuiltInCategory`, where it is known: the ids are Autodesk's published API constants and the names the ones
/// Revit gives the category (rvt-rs measured them in Revit's own IFC export of sample files).
pub fn category_name(id: i64) -> Option<&'static str> {
    NAMES.binary_search_by_key(&id, |&(known, _)| known).ok().map(|i| NAMES[i].1)
}

/// Sorted by id.
const NAMES: &[(i64, &str)] = &[
    (-2_009_030, "Structural Connections"),
    (-2_009_000, "Structural Rebar"),
    (-2_008_132, "Conduits"),
    (-2_008_130, "Cable Trays"),
    (-2_008_128, "Conduit Fittings"),
    (-2_008_126, "Cable Tray Fittings"),
    (-2_008_099, "Sprinklers"),
    (-2_008_087, "Lighting Devices"),
    (-2_008_049, "Pipe Fittings"),
    (-2_008_044, "Pipes"),
    (-2_008_020, "Flex Ducts"),
    (-2_008_013, "Air Terminals"),
    (-2_008_010, "Duct Fittings"),
    (-2_008_000, "Ducts"),
    (-2_006_114, "Design Options"),
    (-2_006_112, "Design Option Sets"),
    (-2_003_400, "Mass"),
    (-2_003_200, "Areas"),
    (-2_001_392, "Floors: Slab Edges"),
    (-2_001_370, "Entourage"),
    (-2_001_360, "Planting"),
    (-2_001_354, "Structural Stiffeners"),
    (-2_001_350, "Specialty Equipment"),
    (-2_001_336, "Structural Trusses"),
    (-2_001_330, "Structural Columns"),
    (-2_001_327, "Structural Beam Systems"),
    (-2_001_320, "Structural Framing"),
    (-2_001_300, "Structural Foundations"),
    (-2_001_263, "Site: Pads"),
    (-2_001_180, "Parking"),
    (-2_001_160, "Plumbing Fixtures"),
    (-2_001_140, "Mechanical Equipment"),
    (-2_001_120, "Lighting Fixtures"),
    (-2_001_100, "Furniture Systems"),
    (-2_001_060, "Electrical Fixtures"),
    (-2_001_040, "Electrical Equipment"),
    (-2_001_000, "Casework"),
    (-2_000_997, "Walls: Rectangular Straight Wall Opening"),
    (-2_000_996, "Shaft Openings"),
    (-2_000_920, "Stairs: Landings"),
    (-2_000_919, "Stairs: Runs"),
    (-2_000_267, "Assemblies"),
    (-2_000_240, "Levels"),
    (-2_000_182, "Walls: Reveals"),
    (-2_000_181, "Walls: Wall Sweeps"),
    (-2_000_171, "Curtain Wall Mullions"),
    (-2_000_170, "Curtain Panels"),
    (-2_000_160, "Rooms"),
    (-2_000_151, "Generic Models"),
    (-2_000_126, "Railings"),
    (-2_000_123, "Stairs: Supports"),
    (-2_000_120, "Stairs"),
    (-2_000_112, "Phases"),
    (-2_000_100, "Columns"),
    (-2_000_080, "Furniture"),
    (-2_000_051, "Lines"),
    (-2_000_045, "Lines: <Sketch>"),
    (-2_000_038, "Ceilings"),
    (-2_000_035, "Roofs"),
    (-2_000_032, "Floors"),
    (-2_000_023, "Doors"),
    (-2_000_014, "Windows"),
    (-2_000_011, "Walls"),
];

fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    let b = buf.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    let b = buf.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(buf: &[u8], at: usize) -> Option<u64> {
    let b = buf.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn f64_at(buf: &[u8], at: usize) -> Option<f64> {
    u64_at(buf, at).map(f64::from_bits)
}

fn in_band(category: i64) -> bool {
    (CATEGORY_MIN..=CATEGORY_MAX).contains(&category)
}

/// The tags of the schema classes the scan reads the file's layout from.
struct Tags {
    outline: u16,
    parents: u16,
    header: u16,
    plane: u16,
}

impl Tags {
    fn of(schema: &Schema) -> Result<Self> {
        let tag = |name: &str| {
            schema
                .tag(name)
                .ok_or_else(|| Error::Format(format!("Formats/Latest has no class {name}, so the element records cannot be found")))
        };
        Ok(Self { outline: tag("Outline")?, parents: tag("ElementParents")?, header: tag("ElementHeader")?, plane: tag("Plane")? })
    }

    /// `[Outline tag] ff ff ff ff [ElementParents tag]`, the marker of an element record.
    fn marker(&self) -> [u8; 8] {
        let mut m = [0xff; 8];
        m[..2].copy_from_slice(&self.outline.to_le_bytes());
        m[6..].copy_from_slice(&self.parents.to_le_bytes());
        m
    }

    /// `05 00 00 00` and the tag of `Plane`: what precedes a Level's elevation block.
    fn elevation_marker(&self) -> [u8; 6] {
        let mut m = [5, 0, 0, 0, 0, 0];
        m[4..].copy_from_slice(&self.plane.to_le_bytes());
        m
    }
}

/// What a scan of one partition finds.
#[derive(Default)]
struct Found {
    records: usize,
    elements: BTreeMap<(i64, u32), (u16, bool)>,
    level_records: BTreeMap<u32, bool>,
    blocks: Vec<Block>,
    /// Declared ids with the block of a level inside another element.
    owned: BTreeSet<u32>,
}

struct Block {
    id: u32,
    name: String,
    elevation_feet: f64,
    building_story: Option<bool>,
}

impl Found {
    fn element(&mut self, category: i64, id: u32, class_tag: u16, placed: bool) {
        self.records += 1;
        let e = self.elements.entry((category, id)).or_insert((class_tag, false));
        e.1 |= placed;
    }

    fn level(&mut self, id: u32, placed: bool) {
        *self.level_records.entry(id).or_insert(false) |= placed;
    }
}

fn declared(ids: &[u32], id: u32) -> bool {
    id != 0 && ids.binary_search(&id).is_ok()
}

/// A record in a partition's chain: where it starts and ends, and its ElementId.
struct Span {
    start: usize,
    end: usize,
    id: u64,
}

/// The leading chain of records of a 2024 and later partition: header (`u64` id, `u32` size, `u16` constant, `u16`
/// count), body, and a 16-byte trailer that repeats the size. The constant is the marker's last tag less 12.
fn chain(buf: &[u8], marker: &[u8; 8]) -> Vec<Span> {
    let constant = u16::from_le_bytes([marker[6], marker[7]]).wrapping_sub(12);
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(id) = u64_at(buf, at) {
        let Some(size) = u32_at(buf, at + 8).map(|v| v as usize) else { break };
        if size < CHAIN_HEADER || u16_at(buf, at + 0x0c) != Some(constant) {
            break;
        }
        let Some(end) = at.checked_add(size) else { break };
        let flags = end.checked_add(8).and_then(|t| u32_at(buf, t));
        let echo = end.checked_add(12).and_then(|t| u32_at(buf, t));
        if !matches!(flags, Some(0 | 0x0100_0000)) || echo.map(|v| v as usize) != Some(size) {
            break;
        }
        out.push(Span { start: at, end, id });
        at = end + CHAIN_TRAILER;
    }
    out
}

/// How many records a partition's chain has (for tests).
#[cfg(test)]
pub(crate) fn chain_len(buf: &[u8], schema: &Schema) -> Option<(usize, usize)> {
    let marker = Tags::of(schema).ok()?.marker();
    let spans = chain(buf, &marker);
    Some((spans.len(), spans.last().map_or(0, |s| s.end + CHAIN_TRAILER)))
}

fn enclosing(chain: &[Span], at: usize) -> Option<&Span> {
    let i = chain.partition_point(|s| s.start <= at);
    let span = chain.get(i.checked_sub(1)?)?;
    (at < span.end).then_some(span)
}

/// A frame with no ElementId of its own: 0, or a value that is not a `u32` ElementId.
fn no_id(raw: u64) -> bool {
    raw == 0 || raw >= u64::from(u32::MAX)
}

fn finite_box(buf: &[u8], at: usize) -> bool {
    let mut b = [0.0f64; 6];
    for (i, v) in b.iter_mut().enumerate() {
        match f64_at(buf, at + i * 8) {
            Some(x) if x.is_finite() => *v = x,
            _ => return false,
        }
    }
    (0..3).all(|a| b[a + 3] >= b[a])
}

/// Elements of a 2024 and later partition.
fn scan_64(buf: &[u8], tags: &Tags, ids: &[u32], found: &mut Found) {
    let marker = tags.marker();
    let spans = chain(buf, &marker);
    for hit in memchr::memmem::find_iter(buf, &marker) {
        let Some(f) = hit.checked_sub(MARKER_AT) else { continue };
        if f + FRAME_LEN > buf.len() || u16_at(buf, f + 0x10) != Some(0) {
            continue;
        }
        let Some(category) = u64_at(buf, f + CATEGORY_AT).map(|v| v as i64).filter(|&c| in_band(c)) else { continue };
        let container = u64_at(buf, f + CONTAINER_AT).unwrap_or(0);
        let kind = u64_at(buf, f + KIND_AT).map_or(0, |v| v as u32);
        let placed = container == NO_CONTAINER && kind == KIND_PLACED;
        let raw = u64_at(buf, f).unwrap_or(0);
        let id = if no_id(raw) {
            // The second prologue: a placed instance inside another record, which gives it its id.
            let host = placed.then(|| enclosing(&spans, f)).flatten().and_then(|s| u32::try_from(s.id).ok());
            match host {
                Some(id) => id,
                None => continue,
            }
        } else {
            raw as u32
        };
        if !declared(ids, id) || !finite_box(buf, f + BBOX_AT) {
            continue;
        }
        found.element(category, id, u16_at(buf, f + CLASS_AT).unwrap_or(0), placed);
    }
}

/// Elements of a partition up to 2023.
fn scan_32(buf: &[u8], tags: &Tags, ids: &[u32], found: &mut Found) {
    for hit in memchr::memmem::find_iter(buf, &tags.marker()) {
        let Some(id_at) = hit.checked_sub(ID_BEFORE_MARKER) else { continue };
        let Some(id) = u32_at(buf, id_at).filter(|&id| declared(ids, id)) else { continue };
        if u16_at(buf, id_at + HEADER_TAG_AFTER_ID) != Some(tags.header) {
            continue;
        }
        let Some(category) = u64_at(buf, hit - CATEGORY_BEFORE_MARKER).map(|v| v as i64).filter(|&c| in_band(c)) else {
            continue;
        };
        if !finite_box(buf, hit + 8) {
            continue;
        }
        let container = u32_at(buf, hit - CONTAINER_BEFORE_MARKER);
        let kind = u32_at(buf, hit - KIND_BEFORE_MARKER);
        let placed = container == Some(u32::MAX) && kind == Some(KIND_PLACED);
        found.element(category, id, u16_at(buf, hit - CLASS_BEFORE_MARKER).unwrap_or(0), placed);
    }
}

/// Level records of a 2024 and later partition: found by their category, with the marker's last six bytes at `+0x50`.
fn levels_64(buf: &[u8], tags: &Tags, release: u32, ids: &[u32], found: &mut Found) {
    let marker = tags.marker();
    let spans = chain(buf, &marker);
    for hit in memchr::memmem::find_iter(buf, &OST_LEVELS.to_le_bytes()) {
        let Some(f) = hit.checked_sub(CATEGORY_AT) else { continue };
        if f + MARKER_AT + 8 > buf.len() || u16_at(buf, f + 0x10) != Some(0) || buf[f + MARKER_AT..f + MARKER_AT + 6] != marker[2..] {
            continue;
        }
        let raw = u64_at(buf, f).unwrap_or(0);
        let id = if no_id(raw) { enclosing(&spans, f).and_then(|s| u32::try_from(s.id).ok()) } else { Some(raw as u32) };
        let Some(id) = id.filter(|&id| declared(ids, id)) else { continue };
        let container = u64_at(buf, f + CONTAINER_AT).unwrap_or(0);
        let kind = u64_at(buf, f + KIND_AT).map_or(0, |v| v as u32);
        found.level(id, container == NO_CONTAINER && is_placed_level(kind, release));
    }
}

/// Level records of a partition up to 2023: the id 14 bytes before the category, the marker 38 after it.
fn levels_32(buf: &[u8], tags: &Tags, release: u32, ids: &[u32], found: &mut Found) {
    let marker = tags.marker();
    for hit in memchr::memmem::find_iter(buf, &OST_LEVELS.to_le_bytes()) {
        let Some(id_at) = hit.checked_sub(14) else { continue };
        let Some(id) = u32_at(buf, id_at).filter(|&id| declared(ids, id)) else { continue };
        if u16_at(buf, id_at + HEADER_TAG_AFTER_ID) != Some(tags.header) || buf.get(hit + 38..hit + 44) != Some(&marker[2..]) {
            continue;
        }
        let container = u32_at(buf, hit + 16);
        let kind = u32_at(buf, hit + 24).unwrap_or(0);
        found.level(id, container == Some(u32::MAX) && is_placed_level(kind, release));
    }
}

fn is_placed_level(kind: u32, release: u32) -> bool {
    kind == KIND_PLACED || (release < 2019 && kind == LEVEL_KIND_2015)
}

/// Name and elevation blocks: an owner id, a run of `ff`, three bytes of padding, the name's length and its UTF-16, then the
/// elevation marker and, [`ELEVATION_AFTER_MARKER`] bytes on, the elevation in feet, which appears again with the 24 bytes
/// before it.
fn blocks(buf: &[u8], tags: &Tags, ids: &[u32], wide_ids: bool, found: &mut Found) {
    let (id_len, run_len) = if wide_ids { (8, RUN_64) } else { (4, RUN_32) };
    let run = vec![0xffu8; run_len];
    let finder = memchr::memmem::Finder::new(&run);
    let mut index = 0usize;
    while let Some(at) = finder.find(&buf[index..]) {
        let mut start = index + at;
        while start > index && buf[start - 1] == 0xff {
            start -= 1;
        }
        index = start;
        while index < buf.len() && buf[index] == 0xff {
            index += 1;
        }
        if let Some(block) = block_at(buf, tags, ids, start, id_len, run_len) {
            found.blocks.push(block);
        }
    }
}

fn block_at(buf: &[u8], tags: &Tags, ids: &[u32], start: usize, id_len: usize, run_len: usize) -> Option<Block> {
    let owner_at = start.checked_sub(id_len)?;
    let owner = if id_len == 4 { u64::from(u32_at(buf, owner_at)?) } else { u64_at(buf, owner_at)? };
    let id = u32::try_from(owner).ok().filter(|&id| declared(ids, id))?;
    // The run ends here, and three bytes of padding follow it (the first 0 or 1).
    let after = start + run_len;
    let pad = buf.get(after..after + 3)?;
    if !matches!(pad[0], 0 | 1) || pad[1..].iter().any(|&b| b != 0) {
        return None;
    }
    let chars = u32_at(buf, after + 3)? as usize;
    if chars == 0 || chars > MAX_NAME_CHARS {
        return None;
    }
    let value = after + 7;
    let end = value.checked_add(chars * 2)?;
    let units: Vec<u16> = buf.get(value..end)?.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
    let name = String::from_utf16(&units).ok().filter(|n| !n.chars().any(char::is_control))?;
    let marker = tags.elevation_marker();
    let window = buf.get(end..(end + ELEVATION_SEARCH).min(buf.len()))?;
    let at = end + memchr::memmem::find(window, &marker)? + ELEVATION_AFTER_MARKER;
    let elevation_feet = f64_at(buf, at).filter(|e| e.is_finite() && e.abs() <= MAX_ELEVATION_FEET)?;
    let copy = buf.get(at.checked_sub(CONFIRM_LEAD_IN)?..at + 8)?;
    let later = buf.get(at + 8..(at + 8 + ELEVATION_SEARCH).min(buf.len()))?;
    memchr::memmem::find(later, copy)?;
    Some(Block { id, name, elevation_feet, building_story: building_story_after(buf, end, id_len == 8) })
}

/// A level's Building Story setting: the byte after the first unset id and `00 01` past its name, a number (0 on most levels)
/// and the level's own id, 1 for a building story and 0 for a level that is not. None where the frame is not found or the
/// byte is neither.
fn building_story_after(buf: &[u8], name_end: usize, wide: bool) -> Option<bool> {
    let unset: &[u8] = if wide { &[0xff; 8] } else { &[0xff; 4] };
    let anchor = [unset, &[0, 1][..]].concat();
    let window = buf.get(name_end..(name_end + STORY_WINDOW).min(buf.len()))?;
    let after = name_end + memchr::memmem::find(window, &anchor)? + anchor.len();
    let id_at = after + 8;
    let setting = if wide {
        u32::try_from(u64_at(buf, id_at)?).ok()?;
        id_at + 8
    } else {
        id_at + 4
    };
    match buf.get(setting)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// The levels inside other elements: objects keyed by an id (`01 00 00 00` and the `u64`) whose block has the owner's id after
/// eight `ff` where a project level's block has its run of 56.
fn owned_64(buf: &[u8], ids: &[u32], found: &mut Found) {
    for at in memchr::memmem::find_iter(buf, &OBJECT_PREFIX) {
        let id_at = at + OBJECT_PREFIX.len();
        let Some(id) = u64_at(buf, id_at).and_then(|v| u32::try_from(v).ok()).filter(|&id| declared(ids, id)) else { continue };
        let chars = u32_at(buf, id_at + NAME_AFTER_OWNER - 4).unwrap_or(0) as usize;
        let sentinel = buf.get(id_at + 8..id_at + 16);
        let owner = u64_at(buf, id_at + 16);
        if (1..=MAX_NAME_CHARS).contains(&chars)
            && sentinel.is_some_and(|run| run.iter().all(|&b| b == 0xff))
            && owner.is_some_and(|o| o != 0 && o < u64::from(u32::MAX))
        {
            found.owned.insert(id);
        }
    }
}

impl crate::Revit {
    /// Reads every element of the project and its levels.
    ///
    /// Needs the file's release to be one the record forms are known for ([`RELEASES`]); a release outside it is an error,
    /// not a guess.
    pub fn inventory(&mut self) -> Result<Inventory> {
        let release = self.basic_file_info()?.release;
        if !RELEASES.contains(&release) {
            return Err(Error::Format(format!(
                "Revit {release}: this reader knows the element records of releases {} to {} and does not guess at others",
                RELEASES.start(),
                RELEASES.end()
            )));
        }
        let schema = self.schema()?;
        let tags = Tags::of(&schema)?;
        let ids = self.element_ids()?;
        let names: Vec<String> = self.stream_names().into_iter().filter(|n| n.starts_with("Partitions/")).map(String::from).collect();
        if names.is_empty() {
            return Err(Error::Format("the file has no Partitions stream, so it holds no elements".into()));
        }
        let wide = release >= 2024;
        let mut elements: BTreeMap<(i64, u32), (u16, bool)> = BTreeMap::new();
        let mut level_records: BTreeMap<u32, bool> = BTreeMap::new();
        let mut named: BTreeMap<u32, (u32, Block)> = BTreeMap::new();
        let mut owned: BTreeSet<u32> = BTreeSet::new();
        let mut records = 0;
        for name in names {
            // One partition at a time: it is inflated, read and dropped.
            let number = name.rsplit('/').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            let buf = self.inflate_all(&name)?.bytes;
            let mut found = Found::default();
            if wide {
                scan_64(&buf, &tags, &ids, &mut found);
                levels_64(&buf, &tags, release, &ids, &mut found);
                owned_64(&buf, &ids, &mut found);
            } else {
                scan_32(&buf, &tags, &ids, &mut found);
                levels_32(&buf, &tags, release, &ids, &mut found);
            }
            blocks(&buf, &tags, &ids, wide, &mut found);
            drop(buf);
            records += found.records;
            owned.extend(found.owned);
            for ((category, id), (class_tag, placed)) in found.elements {
                let e = elements.entry((category, id)).or_insert((class_tag, false));
                e.1 |= placed;
            }
            for (id, placed) in found.level_records {
                *level_records.entry(id).or_insert(false) |= placed;
            }
            // A level edited after an earlier save is written again in a later partition: its newest block is current.
            for block in found.blocks {
                match named.get(&block.id) {
                    Some((held, _)) if *held > number => {}
                    _ => {
                        named.insert(block.id, (number, block));
                    }
                }
            }
        }
        // A level inside another element is no project level, however its block reads.
        owned.retain(|id| level_records.contains_key(id));
        let mut levels: Vec<Level> = level_records
            .iter()
            .filter(|&(id, &placed)| placed && !owned.contains(id))
            .filter_map(|(id, _)| named.get(id))
            .map(|(_, b)| Level { id: b.id, name: b.name.clone(), elevation_feet: b.elevation_feet, building_story: b.building_story })
            .collect();
        levels.sort_by(|a, b| a.elevation_feet.total_cmp(&b.elevation_feet).then(a.id.cmp(&b.id)));
        let placed_levels: BTreeSet<u32> = level_records.iter().filter(|&(id, &p)| p && !owned.contains(id)).map(|(&id, _)| id).collect();
        let unread_levels = placed_levels.len() - levels.len();
        for (&id, &placed) in &level_records {
            let e = elements.entry((OST_LEVELS, id)).or_insert((0, false));
            e.1 |= placed;
        }
        let elements =
            elements.into_iter().map(|((category, id), (class_tag, placed))| Element { category, id, class_tag, placed }).collect();
        Ok(Inventory { declared: ids.len(), records, elements, levels, unread_levels, owned_levels: owned.into_iter().collect() })
    }
}
