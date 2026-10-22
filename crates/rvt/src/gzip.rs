// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/compression.rs (the gzip header
// parse, the list of paged streams, the bounded inflate loop, finding members by their magic).
// Changed: rewritten for BIMCraft with no panics and no anyhow, page stripping done in place, members found by walking
// from the end of the last one instead of inflating every magic hit, the inflate limit applied to a whole stream, and the
// trailer checked against the data (rvt-rs does not read it).
// Not ported: the page size and the data in a page. They are observed in the sample files (see `PAGE_STORED`). rvt-rs has the
// same numbers, credited there to a third party; they were derived before rvt-rs's were read (docs/rvt-provenance.md says
// what that does and does not show).
//! Revit's gzip streams.
//!
//! A stream of a Revit file is gzip members behind a prefix: a `Global/*` stream has eight bytes and then one member,
//! `Formats/Latest` starts with its member, and a partition is a header followed by members of about 128 KiB each, with
//! 40 bytes (the member's CRC-32 and length, then a 32-byte header) between one and the next. The members are standard
//! gzip. Revit is said to leave the CRC and length out of some of them, so a missing or wrong trailer is reported, never
//! an error: the deflate stream decides whether the data is good.
//!
//! The container also stores the large streams in pages: after every 64,896 bytes of data, 353 bytes that are not data.
//! Inflate must run on the stream with those removed ([`strip_pages`]).

use std::io::Read;

use flate2::{Crc, read::DeflateDecoder};

pub const MAGIC: [u8; 3] = [0x1f, 0x8b, 0x08];

// Observed, not ported. In the four sample projects (`rstbasicsampleproject.rvt`, releases 2023 to 2026) the gzip members of
// every stream longer than 64,896 bytes verify (CRC-32 and length) only once the last 353 bytes of each 65,249-byte page are
// removed, pages counted from byte 0 of the stream (the eight-byte prefix of a `Global/*` stream included): 1,664 members in
// 36 streams, 388 runs. How: in the first member of each sample's partition that zlib rejects, every cut of 1 to 4,096 bytes
// at every offset up to the byte it rejects was tried, and exactly one fits (353 bytes at stream offset 130,145). Then every
// failing member of each partition was cut: the runs end at multiples of one period, and 65,249 is the only period up to the
// smallest distance between two cuts that does not also cut members that verify as stored (71 and 919, its factors, do).
// Moving the size, the data or the start by one byte breaks hundreds of members (`a_wrong_page_layout_breaks_the_real_files`).
// The evidence and its limits are in docs/rvt-provenance.md, "The stored-page constants".
/// Bytes a full page takes in the container.
pub const PAGE_STORED: usize = 65_249;
/// Bytes of data in a full page; the other 353 are a run that belongs to no gzip member.
pub const PAGE_PAYLOAD: usize = 64_896;

/// How far into a stream the first member is looked for.
const FIRST_MEMBER_WITHIN: usize = 256;

/// Whether the container stores this stream in pages: the partitions, the schema and the `Global` streams that hold gzip.
///
/// The list is rvt-rs's. The sample bytes confirm it for the five streams that are longer than a page in some sample
/// (`Partitions/NN`, `Formats/Latest`, `Global/ContentDocuments`, `Global/Latest`, and `Global/ElemTable` from 2024, which
/// are cut by the page numbers or do not verify). `Global/DocumentIncrementTable`, `Global/History` and
/// `Global/PartitionTable` are under 18 KB in all four, so stripping them removes nothing and the samples cannot say whether
/// a longer one would be paged; neither can they say it of `Contents`, `BasicFileInfo`, `TransmissionData` and
/// `ProjectInformation`, which are under 4.1 KB and not listed.
pub fn is_paged(stream: &str) -> bool {
    if let Some(rest) = stream.strip_prefix("Partitions/") {
        return !rest.is_empty() && !rest.contains('/');
    }
    matches!(
        stream,
        "Formats/Latest"
            | "Global/ContentDocuments"
            | "Global/DocumentIncrementTable"
            | "Global/ElemTable"
            | "Global/History"
            | "Global/Latest"
            | "Global/PartitionTable"
    )
}

/// Removes each full page's trailer, in place. A last page shorter than a full one is kept whole. No sample ends inside the
/// last 352 bytes of a page, where a stream that was cut off would keep part of a run; a writer that finishes its runs
/// cannot leave one, so this is a choice about damaged files and not an observation.
pub fn strip_pages(mut data: Vec<u8>) -> Vec<u8> {
    let full = data.len() / PAGE_STORED;
    for page in 1..full {
        let from = page * PAGE_STORED;
        data.copy_within(from..from + PAGE_PAYLOAD, page * PAGE_PAYLOAD);
    }
    if full > 0 {
        let tail = full * PAGE_STORED;
        let kept = data.len() - tail;
        data.copy_within(tail.., full * PAGE_PAYLOAD);
        data.truncate(full * PAGE_PAYLOAD + kept);
    }
    data
}

/// A stream's gzip members, inflated and joined.
#[derive(Debug, Default)]
pub struct Inflated {
    pub bytes: Vec<u8>,
    /// Where each member's output ends in `bytes`.
    pub ends: Vec<usize>,
    pub members: usize,
    /// How many members were followed by a CRC-32 and a length that match their output.
    pub trailers_ok: usize,
    /// Gzip magics found that did not inflate (padding, or a header that is really data).
    pub skipped: usize,
}

/// Why a stream did not inflate; the container turns it into an [`crate::Error`] naming the stream.
#[derive(Debug)]
pub enum Fail {
    NoMember,
    Bad { offset: usize, reason: String },
    TooMuch,
}

/// The length of the gzip header at `at`, with its optional fields; `None` when it is cut short or has a reserved flag set.
fn header_len(data: &[u8], at: usize) -> Option<usize> {
    if data.get(at..at.checked_add(3)?)? != MAGIC {
        return None;
    }
    let flags = *data.get(at.checked_add(3)?)?;
    if flags & 0xe0 != 0 {
        return None;
    }
    let mut pos = at.checked_add(10)?;
    if flags & 0x04 != 0 {
        let xlen = data.get(pos..pos.checked_add(2)?)?;
        pos = pos.checked_add(2)?.checked_add(usize::from(u16::from_le_bytes([xlen[0], xlen[1]])))?;
    }
    for flag in [0x08, 0x10] {
        if flags & flag != 0 {
            let nul = data.get(pos..)?.iter().position(|&b| b == 0)?;
            pos = pos.checked_add(nul)?.checked_add(1)?;
        }
    }
    if flags & 0x02 != 0 {
        pos = pos.checked_add(2)?;
    }
    (pos <= data.len()).then_some(pos - at)
}

/// Inflates the member at `at` onto `out`, which grows by at most `room` bytes (nothing is left on `out` if it fails).
/// Returns where the member ends in `data` and whether the 8 bytes after it are its CRC-32 and length.
fn member_into(data: &[u8], at: usize, out: &mut Vec<u8>, room: usize) -> Result<(usize, bool), Fail> {
    let bad = |reason: String| Fail::Bad { offset: at, reason };
    let body = at + header_len(data, at).ok_or_else(|| bad("the gzip header is cut short".into()))?;
    let start = out.len();
    let mut deflate = DeflateDecoder::new(&data[body..]);
    let mut buf = [0u8; 16 * 1024];
    loop {
        match deflate.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() - start + n > room {
                    out.truncate(start);
                    return Err(Fail::TooMuch);
                }
                out.extend_from_slice(&buf[..n]);
            }
            Err(e) => {
                out.truncate(start);
                return Err(bad(e.to_string()));
            }
        }
    }
    let end = body.saturating_add(usize::try_from(deflate.total_in()).unwrap_or(usize::MAX));
    let mut crc = Crc::new();
    crc.update(&out[start..]);
    let trailer = data
        .get(end..end.saturating_add(8))
        .is_some_and(|t| t[..4] == crc.sum().to_le_bytes() && t[4..] == ((out.len() - start) as u32).to_le_bytes());
    Ok((end, trailer))
}

/// The first member of a stream whose pages are already stripped, looked for in its first bytes.
pub fn inflate_first(data: &[u8], room: usize) -> Result<Inflated, Fail> {
    let mut failure = Fail::NoMember;
    let head = &data[..data.len().min(FIRST_MEMBER_WITHIN)];
    for at in memchr::memmem::find_iter(head, &MAGIC) {
        let mut got = Inflated::default();
        match member_into(data, at, &mut got.bytes, room) {
            Ok((_, trailer)) => {
                got.ends.push(got.bytes.len());
                (got.members, got.trailers_ok) = (1, usize::from(trailer));
                return Ok(got);
            }
            Err(Fail::TooMuch) => return Err(Fail::TooMuch),
            Err(other) => failure = other,
        }
    }
    Err(failure)
}

/// Every member of a stream whose pages are already stripped, joined in order. `room` bounds the whole output.
///
/// Members are found by their magic, and a magic inside a member that has just inflated is data, not a member: the walk
/// resumes at that member's end (past its trailer when it has one). A magic that does not inflate is counted and skipped.
pub fn inflate_members(data: &[u8], room: usize) -> Result<Inflated, Fail> {
    let mut got = Inflated::default();
    let mut floor = 0;
    let mut last = None;
    for at in memchr::memmem::find_iter(data, &MAGIC) {
        if at < floor {
            continue;
        }
        let left = room.saturating_sub(got.bytes.len());
        match member_into(data, at, &mut got.bytes, left) {
            Ok((end, trailer)) => {
                got.ends.push(got.bytes.len());
                got.members += 1;
                got.trailers_ok += usize::from(trailer);
                floor = end + if trailer { 8 } else { 0 };
            }
            Err(Fail::TooMuch) => return Err(Fail::TooMuch),
            Err(fail) => {
                got.skipped += 1;
                last = Some(fail);
            }
        }
    }
    if got.members == 0 {
        return Err(last.unwrap_or(Fail::NoMember));
    }
    Ok(got)
}
