// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/reader.rs (`preview_png`: the
// stream `RevitPreview4.0` is a wrapper of about 300 bytes and then a PNG, found by its signature).
// Changed: rewritten for BIMCraft; the PNG is walked chunk by chunk (each chunk's size capped and its CRC-32 checked) from
// IHDR to IEND, its dimensions are bounded, and what is returned ends at IEND.
//! The preview image Revit saves in `RevitPreview4.0`: a wrapper of about 300 bytes, then a PNG.
//!
//! The PNG is checked before it is handed back: the signature, an `IHDR` chunk first with sane dimensions, every chunk's
//! size within a cap and its CRC-32 right, and an `IEND` last.

use flate2::Crc;

use crate::error::{Error, Result};

const SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";
/// The most a preview may weigh, one chunk of it, and a side of it.
pub const MAX_PNG: usize = 4 << 20;
const MAX_CHUNK: usize = MAX_PNG;
const MAX_SIDE: u32 = 8192;
const MAX_CHUNKS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    /// The PNG, from its signature to the end of IEND.
    pub png: Vec<u8>,
}

/// Finds and checks the PNG in the stream's bytes.
pub fn find(stream: &[u8]) -> Result<Preview> {
    let bad = |why: String| Error::Format(format!("the preview image {why}"));
    let start = memchr::memmem::find(stream, &SIGNATURE).ok_or_else(|| bad("has no PNG in it".into()))?;
    let png = &stream[start..];
    let mut at = SIGNATURE.len();
    let (mut width, mut height) = (0, 0);
    for index in 0..MAX_CHUNKS {
        let Some(head) = png.get(at..at + 8) else {
            return Err(bad(if index == 0 { "is cut short".into() } else { "has no IEND chunk".into() }));
        };
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = [head[4], head[5], head[6], head[7]];
        if len > MAX_CHUNK {
            return Err(bad(format!("has a chunk of {len} bytes, over the limit of {MAX_CHUNK}")));
        }
        if index == 0 && (&kind != b"IHDR" || len != 13) {
            return Err(bad("does not open with an IHDR chunk of 13 bytes".into()));
        }
        let end = at + 8 + len + 4;
        let (Some(data), Some(crc)) = (png.get(at + 8..at + 8 + len), png.get(at + 8 + len..end)) else {
            return Err(bad("is cut short in a chunk".into()));
        };
        let mut sum = Crc::new();
        sum.update(&kind);
        sum.update(data);
        if sum.sum().to_be_bytes() != crc {
            return Err(bad(format!("has a chunk {} whose checksum is wrong", String::from_utf8_lossy(&kind))));
        }
        if index == 0 {
            width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
                return Err(bad(format!("has dimensions {width} by {height}, outside 1 to {MAX_SIDE}")));
            }
        }
        at = end;
        if at > MAX_PNG {
            return Err(bad(format!("is over {MAX_PNG} bytes")));
        }
        if &kind == b"IEND" {
            return Ok(Preview { width, height, png: png[..at].to_vec() });
        }
    }
    Err(bad(format!("has more than {MAX_CHUNKS} chunks")))
}

impl crate::Revit {
    /// The preview image, if the file has one. The sample projects of 2023 to 2026 have none.
    pub fn preview(&mut self) -> Result<Option<Preview>> {
        if self.stream_len("RevitPreview4.0").is_none() {
            return Ok(None);
        }
        find(&self.read_stream("RevitPreview4.0")?).map(Some)
    }
}
