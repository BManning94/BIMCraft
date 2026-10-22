// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/elem_table.rs (the record layouts of
// `Global/ElemTable` and the rule that picks one, which rvt-rs credits to STE1200's frame measured by puzzbobb).
// Changed: rewritten for BIMCraft; only the ElementIds are read (not the owners), the 12-byte fallback layout is not
// supported (a table neither size fits is an error), and every count and offset is checked before it is used.
//! `Global/ElemTable`: every ElementId in the file, in ascending order.
//!
//! After a `u16` (the tag of the class `ElemTable` in the file's schema) and a `u32` record count at byte 2, the records
//! start at byte 6. A record is 28 bytes up to Revit 2023 (`[u32 id][u32 id][16 bytes][u32 owner]`) and 40 bytes from 2024
//! (`[u64 id][12 bytes][u64 id][u64 owner][u32]`). The second id is the element's ElementId (the first equals it on nearly
//! every record). The size is found, not assumed: the one at which the stated number of records fits and their ElementIds
//! rise strictly all the way, and if both fit, the one that leaves the shorter tail.

use crate::error::{Error, Result};

const ORIGIN: usize = 6;
/// `(record size, offset of the ElementId)`, longest first.
const LAYOUTS: [(usize, usize); 2] = [(40, 20), (28, 4)];

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The ElementIds the inflated `Global/ElemTable` declares, ascending.
pub fn parse(data: &[u8]) -> Result<Vec<u32>> {
    let bad = |why: &str| Error::Format(format!("Global/ElemTable {why}"));
    let count = u32_at(data, 2).ok_or_else(|| bad("is too short to hold its record count"))? as usize;
    if count == 0 {
        return Err(bad("lists no elements"));
    }
    let mut best: Option<(usize, usize, usize)> = None; // (tail, size, id offset)
    for (size, id_at) in LAYOUTS {
        let Some(end) = count.checked_mul(size).and_then(|n| n.checked_add(ORIGIN)).filter(|&end| end <= data.len()) else {
            continue;
        };
        let mut previous: Option<u32> = None;
        let rises = (0..count).all(|k| match u32_at(data, ORIGIN + k * size + id_at) {
            Some(id) if previous.is_none_or(|p| id > p) => {
                previous = Some(id);
                true
            }
            _ => false,
        });
        if rises && best.is_none_or(|(tail, ..)| data.len() - end < tail) {
            best = Some((data.len() - end, size, id_at));
        }
    }
    let (_, size, id_at) = best
        .ok_or_else(|| bad(&format!("has no record layout (28 or 40 bytes) in which its {count} records fit with ElementIds that rise")))?;
    Ok((0..count).filter_map(|k| u32_at(data, ORIGIN + k * size + id_at)).collect())
}

impl crate::Revit {
    /// The ElementIds `Global/ElemTable` declares.
    pub fn element_ids(&mut self) -> Result<Vec<u32>> {
        parse(&self.inflate("Global/ElemTable")?.bytes)
    }
}
