// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_names.rs (the name entries
// of families and family types, RE-38, RE-109, RE-145).
// Changed: rewritten for BIMCraft; both entry forms are read by one module, every offset is bounds-checked, and the newest
// partition's entries win.
//! The names of families and family types.
//!
//! From 2024 a partition holds a name entry for every loaded family and family type: the element's `u64` id, the length in
//! UTF-16 code units, the name, and the element's `BuiltInCategory`. Found from the category, whose top five bytes are `ff`.
//! Up to 2023 an entry is `01 00 00 00`, a `u32` id, a `u32` length and the name, with no category. An element edited after an
//! earlier save has its entry again in a later partition: the newest partition's entry is current.

use std::collections::BTreeMap;

use crate::records::{declared, in_band, u32_at, u64_at};

const MAX_UNITS: usize = 256;
const MAX_UNITS_32: usize = 128;

/// One name entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    /// The `BuiltInCategory` of the element: none in the 32-bit form.
    pub category: Option<i64>,
}

fn units(buf: &[u8], at: usize, n: usize) -> Option<String> {
    let bytes = buf.get(at..at.checked_add(n.checked_mul(2)?)?)?;
    String::from_utf16(&bytes.as_chunks::<2>().0.iter().map(|u| u16::from_le_bytes(*u)).collect::<Vec<_>>()).ok()
}

/// The entries of a 2024 or later partition whose ids the element table declares.
pub(crate) fn entries_64(buf: &[u8], ids: &[u32], out: &mut Vec<Entry>) {
    for hit in memchr::memmem::find_iter(buf, &[0xff; 5]) {
        let Some(at) = hit.checked_sub(3) else { continue };
        let Some(category) = u64_at(buf, at).map(|v| v as i64).filter(|&c| in_band(c)) else { continue };
        for n in 1..=MAX_UNITS {
            let Some(start) = at.checked_sub(2 * n) else { break };
            let Some(length_at) = start.checked_sub(4) else { break };
            if u32_at(buf, length_at) != Some(n as u32) {
                continue;
            }
            let Some(id_at) = length_at.checked_sub(8) else { break };
            let Some(id) = u64_at(buf, id_at).and_then(|v| u32::try_from(v).ok()).filter(|&id| declared(ids, id)) else { continue };
            let Some(name) = units(buf, start, n) else { continue };
            if name.trim().is_empty() || name.chars().any(char::is_control) {
                continue;
            }
            out.push(Entry { id, name, category: Some(category) });
            break;
        }
    }
}

/// Whether `name` is one character outside ASCII: in 32-bit data a list of one id reads as a one-unit name, the id's low half.
fn lone_non_ascii(name: &str) -> bool {
    let mut chars = name.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_ascii())
}

/// The entries of a partition up to 2023.
pub(crate) fn entries_32(buf: &[u8], ids: &[u32], out: &mut Vec<Entry>) {
    for at in memchr::memmem::find_iter(buf, &[1u8, 0, 0, 0]) {
        let (Some(id), Some(n)) = (u32_at(buf, at + 4), u32_at(buf, at + 8)) else { continue };
        let n = n as usize;
        if !(1..=MAX_UNITS_32).contains(&n) || !declared(ids, id) {
            continue;
        }
        let Some(name) = units(buf, at + 12, n) else { continue };
        let printable = name.chars().all(|c| !c.is_control() && !('\u{e000}'..='\u{f8ff}').contains(&c) && c < '\u{fff0}');
        if printable && !lone_non_ascii(&name) {
            out.push(Entry { id, name, category: None });
        }
    }
}

/// An id's value as the newest partition holds it, none where that partition's copies disagree.
pub(crate) struct Latest<V> {
    found: BTreeMap<u32, (u32, Option<V>)>,
}

impl<V> Default for Latest<V> {
    fn default() -> Self {
        Self { found: BTreeMap::new() }
    }
}

impl<V: PartialEq> Latest<V> {
    pub fn offer(&mut self, id: u32, partition: u32, value: V) {
        match self.found.get_mut(&id) {
            Some((held_at, held)) if *held_at == partition => {
                if held.as_ref() != Some(&value) {
                    *held = None;
                }
            }
            Some((held_at, _)) if *held_at > partition => {}
            _ => {
                self.found.insert(id, (partition, Some(value)));
            }
        }
    }

    pub fn into_values(self) -> BTreeMap<u32, V> {
        self.found.into_iter().filter_map(|(id, (_, v))| Some((id, v?))).collect()
    }
}
