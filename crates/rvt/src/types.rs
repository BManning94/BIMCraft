// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_compound_structure.rs (a
// type's layers and a wall's orientation, RE-53, RE-112), src/partition_names.rs (the name a type gives itself in its data,
// RE-111) and src/partition_type_records.rs (the one type a record's reference list names, RE-28).
// Changed: rewritten for BIMCraft; the readers take the bytes of one element's data, every offset is bounds-checked, the frame
// tags come from the file's own schema instead of a table per release, and a name is looked for only in the type's own data.
//! What an element's data says about its type: the type's name, its layers, and which side of a wall is its exterior.
//!
//! A type has a record of its own, with no box (see [`crate::records::TypeRecord`]), and its serialised data opens with the
//! element-data header and its id. In that data:
//!
//! - its **name** is the first framed string: `ff ff ff ff`, a `u16` field tag (never `0xffff`), a `u32` length in UTF-16 code
//!   units and the name;
//! - its **layers** (a wall, floor, roof or ceiling type) are a `u32` count and a record per layer, exterior (or top) first:
//!   the width in feet (0 only on a membrane), the material's ElementId (all ones: by category), a deck profile id, and a
//!   function (1 structure, 2 substrate, 3 thermal or air, 4 and 5 finish, 100 membrane, 200 structural deck). The count is
//!   framed by `ff ff ff ff` and the tag of `VerticalRegionsStructure`, or follows the type's name and a zero `u32`;
//! - a **wall's** data has, after `ff ff ff ff 01 00 00 00`, three `u32`: its location line (0 to 5), a word of 0 to 2, and a
//!   flip flag. With the flag set the wall's exterior, its first layer, lies to the right of its location line's direction.

use std::collections::BTreeSet;

use crate::records::{declared, u32_at, u64_at};

/// Most layers a type is taken to have.
const MAX_LAYERS: usize = 32;
/// Longest type name, in UTF-16 code units.
const MAX_NAME_UNITS: usize = 256;
/// How far into a type's data its layers, or a wall's orientation, are looked for.
pub const LAYER_WINDOW: usize = 0x2000;
/// How far into an element's data its name is looked for.
const NAME_WINDOW: usize = 0x600;
/// The bytes a wall's location line, a word and its flip flag follow.
const WALL_FLIP_ANCHOR: [u8; 8] = [0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00];

/// Where a layer record's fields are on a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerLayout {
    /// Bytes per record.
    pub record_len: usize,
    /// The `u32` function.
    pub function_at: usize,
    /// The material ElementId.
    pub material_at: usize,
    /// The deck profile ElementId.
    pub deck_at: usize,
    /// Whether the ids are `u64` (Revit 2024 and later) rather than `u32`.
    pub wide_ids: bool,
}

/// The layer record layout of a release.
pub fn layer_layout(release: u32) -> Option<LayerLayout> {
    match release {
        // Revit 2015 to 2022 write 2023's records (rvt-rs RE-180, RE-181).
        2015..=2023 => Some(LayerLayout { record_len: 29, function_at: 8, material_at: 16, deck_at: 20, wide_ids: false }),
        2024 | 2025 => Some(LayerLayout { record_len: 37, function_at: 24, material_at: 8, deck_at: 16, wide_ids: true }),
        // 2026 adds a `u32` after the function, repeating it (RE-124); 2027 writes the same (RE-179).
        2026 | 2027 => Some(LayerLayout { record_len: 41, function_at: 24, material_at: 8, deck_at: 16, wide_ids: true }),
        _ => None,
    }
}

/// One layer of a type's compound structure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompoundLayer {
    /// Feet.
    pub width_feet: f64,
    /// The layer's material; none where the layer takes its category's.
    pub material: Option<u32>,
    /// 1 structure, 2 substrate, 3 thermal or air, 4 and 5 finish, 100 membrane, 200 structural deck; 0 is not named.
    pub function: u32,
}

fn id_at(buf: &[u8], at: usize, wide: bool) -> Option<u64> {
    if wide { u64_at(buf, at) } else { u32_at(buf, at).map(|id| if id == u32::MAX { u64::MAX } else { u64::from(id) }) }
}

/// The layers whose count starts at `count_at`, or none unless every record is one a type can have: a width under 10 ft that
/// is positive (0 only on a membrane or a structural deck), a material among `materials` or by category, a deck profile that
/// is none or a declared id, and a known function.
fn layers_at(buf: &[u8], count_at: usize, layout: LayerLayout, materials: &BTreeSet<u32>, ids: &[u32]) -> Option<Vec<CompoundLayer>> {
    let count = usize::try_from(u32_at(buf, count_at)?).ok()?;
    if !(1..=MAX_LAYERS).contains(&count) {
        return None;
    }
    let mut layers = Vec::with_capacity(count);
    for index in 0..count {
        let at = count_at + 4 + layout.record_len * index;
        let width = f64::from_bits(u64_at(buf, at)?);
        let material = id_at(buf, at + layout.material_at, layout.wide_ids)?;
        let deck = id_at(buf, at + layout.deck_at, layout.wide_ids)?;
        let function = u32_at(buf, at + layout.function_at)?;
        // A membrane has no width, and nor has a structural deck whose profile the layer above it carries.
        let widthless = matches!(function, 100 | 200);
        let width_ok = width.is_finite() && width < 10.0 && (width >= 1e-3 || (widthless && width == 0.0));
        let material = match material {
            u64::MAX => None,
            id => Some(u32::try_from(id).ok().filter(|id| materials.contains(id))?),
        };
        let deck_ok = deck == u64::MAX || u32::try_from(deck).ok().is_some_and(|id| declared(ids, id));
        if !width_ok || !deck_ok || !matches!(function, 0..=5 | 100 | 200) {
            return None;
        }
        layers.push(CompoundLayer { width_feet: width, material, function });
    }
    Some(layers)
}

/// Whether the layer count at `count_at` is framed as a type's layers are: `ff ff ff ff` and the schema's tag of
/// `VerticalRegionsStructure`, or the type's name (`u32 k`, `k` UTF-16 code units) and a zero `u32`.
fn framed(buf: &[u8], count_at: usize, tag: [u8; 2]) -> bool {
    let tagged =
        count_at >= 6 && buf.get(count_at - 6..count_at - 2) == Some(&[0xff; 4][..]) && buf.get(count_at - 2..count_at) == Some(&tag[..]);
    if tagged {
        return true;
    }
    if count_at < 4 || u32_at(buf, count_at - 4) != Some(0) {
        return false;
    }
    (1..=MAX_NAME_UNITS).any(|k: usize| {
        let Some(start) = count_at.checked_sub(4 + 2 * k + 4) else { return false };
        u32_at(buf, start) == Some(k as u32)
            && (0..k).all(|i| {
                buf.get(start + 4 + 2 * i..start + 6 + 2 * i)
                    .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                    .is_some_and(|unit| unit >= 0x20)
            })
    })
}

/// The first framed layer list in a type's data.
pub fn find_layers(data: &[u8], tag: [u8; 2], layout: LayerLayout, materials: &BTreeSet<u32>, ids: &[u32]) -> Option<Vec<CompoundLayer>> {
    let data = data.get(..LAYER_WINDOW.min(data.len()))?;
    (6..data.len().saturating_sub(4)).filter(|&at| framed(data, at, tag)).find_map(|at| layers_at(data, at, layout, materials, ids))
}

/// What a wall's data says about its location line and its sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallOrientation {
    /// Revit's location-line setting, 0 to 5. The stored line is the wall's centreline whatever it is.
    pub location_line: u32,
    /// A word of 0 to 2. Revit 2015 to 2020 do not write it, and it reads 0 there.
    pub word: u32,
    /// Set puts the wall's exterior to the right of its location line's direction; clear, to the left.
    pub flip: bool,
}

/// Whether a wall's data holds [`WallOrientation::word`] on a release.
fn has_word(release: u32) -> bool {
    !(2015..=2020).contains(&release)
}

/// A wall's orientation, from its data: none without the anchor or with values a wall does not take. The flip is one byte;
/// the byte after it is a second flag, 0 followed by `00 00` or 1 followed by `ff ff`, which is not read.
pub fn wall_orientation(data: &[u8], release: u32) -> Option<WallOrientation> {
    let data = data.get(..LAYER_WINDOW.min(data.len()))?;
    let at = memchr::memmem::find(data, &WALL_FLIP_ANCHOR)? + WALL_FLIP_ANCHOR.len();
    let location_line = u32_at(data, at)?;
    let (word, flip_at) = if has_word(release) { (u32_at(data, at + 4)?, at + 8) } else { (0, at + 4) };
    let flip = match data.get(flip_at..flip_at + 4)? {
        [flip @ (0 | 1), 0, 0, 0] | [flip @ (0 | 1), 1, 0xff, 0xff] => *flip == 1,
        _ => return None,
    };
    (location_line <= 5 && word <= 2).then_some(WallOrientation { location_line, word, flip })
}

/// The first framed name in an element's data: `ff ff ff ff`, a `u16` tag (never `0xffff`), a `u32` length and the name. The
/// element-data `header` of another element is not a frame. A name of one character is not taken on releases before 2023:
/// there it is a one-id list's id read as a name.
pub fn first_name(data: &[u8], header: &[u8; 10], release: u32) -> Option<String> {
    let window = data.get(..NAME_WINDOW.min(data.len()))?;
    for at in memchr::memmem::find_iter(window, &[0xff; 4]) {
        let Some(tag) = window.get(at + 4..at + 6) else { continue };
        if window.get(at..at + header.len()) == Some(&header[..]) || tag == [0xff, 0xff] {
            continue;
        }
        let units = u32_at(window, at + 6).map(|n| n as usize);
        let Some(units) = units.filter(|n| (1..=MAX_NAME_UNITS).contains(n)) else { continue };
        let Some(bytes) = window.get(at + 10..at + 10 + units * 2) else { continue };
        let code: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
        let Ok(name) = String::from_utf16(&code) else { continue };
        // U+FFFF and U+FFFE fill unset slots; they are never part of a name.
        if name.trim().is_empty() || name.chars().any(|c| c.is_control() || matches!(c, '\u{fffd}' | '\u{fffe}' | '\u{ffff}')) {
            continue;
        }
        if release < 2023 && name.chars().nth(1).is_none() {
            continue;
        }
        return Some(name);
    }
    None
}

/// The one id among `refs` that is a type of the category (`types`): a record's reference list names exactly one of the type
/// it is placed from. None where it names none or more than one.
pub fn unique_type(refs: &[u64], types: &BTreeSet<u32>) -> Option<u32> {
    let mut found = None;
    for id in refs.iter().filter_map(|&v| u32::try_from(v).ok()).filter(|id| types.contains(id)) {
        match found {
            None => found = Some(id),
            Some(seen) if seen == id => {}
            Some(_) => return None,
        }
    }
    found
}
