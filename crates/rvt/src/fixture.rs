//! Synthetic Revit-shaped files for tests. Autodesk's files are never committed: a test builds the container it needs here,
//! out of streams it names, and the bytes are whatever the test says they are.

use std::io::{Cursor, Write};

use flate2::{Compression, Crc, write::DeflateEncoder};

use crate::gzip::{PAGE_PAYLOAD, PAGE_STORED};

/// A gzip member as a standard writer makes it: the 10-byte header, the deflate stream, CRC-32 and the length.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = truncated_gzip(data);
    let mut crc = Crc::new();
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// A gzip member whose deflate stream stores its data as it is, so every byte of `data` can be found in the member.
pub fn stored_gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0x0b];
    let mut deflate = DeflateEncoder::new(&mut out, Compression::none());
    let _ = deflate.write_all(data);
    let _ = deflate.finish();
    out
}

/// A gzip member without its trailer, the form Revit's own readers are said to accept.
pub fn truncated_gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0x0b];
    let mut deflate = DeflateEncoder::new(&mut out, Compression::fast());
    let _ = deflate.write_all(data);
    let _ = deflate.finish();
    out
}

/// What a `Global/*` stream holds: eight bytes, then one gzip member.
pub fn global(data: &[u8]) -> Vec<u8> {
    let mut out = vec![1, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&gzip(data));
    out
}

/// A partition stream: a 44-byte header, then each chunk as a gzip member, its trailer and a 32-byte header.
pub fn partition(chunks: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![9, 0, 0, 0, 0, 0, 0, 0];
    out.resize(44, 0x5a);
    for (i, chunk) in chunks.iter().enumerate() {
        out.extend_from_slice(&gzip(chunk));
        if i + 1 < chunks.len() {
            out.extend_from_slice(&[0x77; 32]);
        }
    }
    out
}

/// What the container stores for a stream Revit writes in pages: after every `PAGE_PAYLOAD` bytes, a trailer that is not data
/// (`PAGE_STORED - PAGE_PAYLOAD` bytes, filled with a pattern a test can look for).
pub fn paged(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + payload.len() / PAGE_PAYLOAD * (PAGE_STORED - PAGE_PAYLOAD));
    for chunk in payload.chunks(PAGE_PAYLOAD) {
        out.extend_from_slice(chunk);
        if chunk.len() == PAGE_PAYLOAD {
            out.resize(out.len() + PAGE_STORED - PAGE_PAYLOAD, 0xa5);
        }
    }
    out
}

/// A Compound File holding these streams (`"Global/Latest"` makes the storage `Global` too), as bytes.
pub fn container(streams: &[(&str, &[u8])]) -> Vec<u8> {
    build(streams).unwrap_or_default()
}

fn build(streams: &[(&str, &[u8])]) -> std::io::Result<Vec<u8>> {
    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new()))?;
    for (name, data) in streams {
        let mut path = String::new();
        let parts: Vec<&str> = name.split('/').collect();
        for storage in &parts[..parts.len() - 1] {
            path.push('/');
            path.push_str(storage);
            if !file.exists(&path) {
                file.create_storage(&path)?;
            }
        }
        file.create_stream(format!("/{name}"))?.write_all(data)?;
    }
    file.flush()?;
    Ok(file.into_inner().into_inner())
}
