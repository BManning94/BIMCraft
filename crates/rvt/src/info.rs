// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/basic_file_info.rs (the `Key: value`
// text block, its list of known keys, and the narrow `True`/`False` quirk).
// Changed: rewritten for BIMCraft; the block is found by its first line instead of scoring both byte alignments, each line
// is split on its CR LF bytes and read from its own start (so a narrow value cannot shift the lines after it), the release
// comes from the `Format` or `Revit Build` line only, and nothing is guessed from the binary head.
//! `BasicFileInfo`: the release, build, document GUID and the last save's details.
//!
//! The stream is a binary head (the release and build as counted UTF-16 strings, then more fields) followed by lines of
//! `Key: value` in UTF-16, separated by CR LF. The head has one-byte fields, so the text can start at an odd offset, and one
//! value (`IsSingleUserCloudModel`) is written as a narrow C string inside the wide text, so a line is read from its own
//! first byte and a value that is narrow is unpacked. The last saver's name (`Username`, set only when worksharing is on)
//! and the saved path are in here: `info` shows them, and nothing writes them into a project or an IFC file.

use crate::error::{Error, Result};

/// Lines kept, and the longest value kept, so a hostile stream cannot make the block large.
const MAX_LINES: usize = 256;
const MAX_VALUE_BYTES: usize = 4096;

/// The first line of the block (since Revit 2016), and the first line of the 2016 to 2018 form's `Revit Build`.
const BLOCK_STARTS: [&str; 2] = ["Worksharing:", "Revit Build:"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicFileInfo {
    /// The release year, `2024`.
    pub release: u32,
    /// `20230308_1635(x64)`.
    pub build: Option<String>,
    /// `Unique Document GUID`, the document's identity across saves.
    pub guid: Option<String>,
    /// `Locale when saved`, `ENU`.
    pub locale: Option<String>,
    /// `Not enabled`, or the role of this copy in a workshared project.
    pub worksharing: Option<String>,
    /// The name Revit recorded for the last save; empty (so none) unless the file is workshared.
    pub username: Option<String>,
    /// The full path the file was last saved to, on the saving machine.
    pub last_save_path: Option<String>,
    pub central_model_path: Option<String>,
    /// Every line of the block, in file order, values as written (empty ones too).
    pub properties: Vec<(String, String)>,
}

impl BasicFileInfo {
    /// Reads the stream's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bad = |why: &str| Error::Format(format!("BasicFileInfo {why}"));
        let start = BLOCK_STARTS
            .iter()
            .filter_map(|anchor| memchr::memmem::find(bytes, &wide(anchor)))
            .min()
            .ok_or_else(|| bad("has no block of `Key: value` lines, so it is not what Revit writes"))?;
        let properties = lines(&bytes[start..]);
        let find = |key: &str| properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
        let release = find("Format")
            .and_then(year)
            .or_else(|| find("Revit Build").and_then(|v| v.split_once("Autodesk Revit ")).and_then(|(_, rest)| year(rest)))
            .ok_or_else(|| bad("names no release (no `Format` or `Revit Build` line with a year)"))?;
        let build = find("Build").map(str::to_string).or_else(|| find("Revit Build").and_then(legacy_build));
        let own = |key: &str| find(key).map(str::to_string);
        Ok(Self {
            release,
            build,
            guid: own("Unique Document GUID"),
            locale: own("Locale when saved"),
            worksharing: own("Worksharing"),
            username: own("Username"),
            last_save_path: own("Last Save Path"),
            central_model_path: own("Central Model Path"),
            properties,
        })
    }

    /// The value of a line of the block.
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A release year in the first four digits of `text`, as Revit has written them since 2008.
fn year(text: &str) -> Option<u32> {
    let digits = text.trim().get(..4)?;
    let year: u32 = digits.parse().ok()?;
    (2000..=2100).contains(&year).then_some(year)
}

/// `Autodesk Revit 2016 (Build: 20150220_1515(x64))` gives `20150220_1515(x64)`.
fn legacy_build(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("(Build: ")?;
    let build = rest.trim_end().trim_end_matches(')');
    (!build.is_empty()).then(|| format!("{build}{}", if build.ends_with(')') { "" } else { "(x64)" }))
}

/// The `Key: value` lines of the block, which starts at the first byte of `block`.
fn lines(block: &[u8]) -> Vec<(String, String)> {
    const CRLF: [u8; 4] = [0x0d, 0, 0x0a, 0];
    let mut out = Vec::new();
    let mut at = 0;
    while at < block.len() && out.len() < MAX_LINES {
        let end = memchr::memmem::find(&block[at..], &CRLF).map_or(block.len(), |i| at + i);
        if let Some(line) = line(&block[at..end]) {
            out.push(line);
        }
        at = end + CRLF.len();
    }
    out
}

/// One line: a key of plain characters, a colon, and a value. The key is read as UTF-16 up to its colon; what follows the
/// colon and its space is the value, which is wide text unless it is a narrow ASCII string (a NUL, the text, a NUL).
fn line(raw: &[u8]) -> Option<(String, String)> {
    let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
    let colon = units.iter().position(|&u| u == u16::from(b':'))?;
    let key = String::from_utf16(&units[..colon]).ok()?;
    if !is_label(&key) {
        return None;
    }
    let after = raw.get((colon + 1) * 2..)?;
    let value = match after {
        [] => &[][..],
        [b' ', 0, rest @ ..] => rest,
        _ => return None,
    };
    // The stream's last line ends in a narrow CR LF where the others end in wide ones.
    let value = value.strip_suffix(&[0x0d, 0x0a]).unwrap_or(value);
    let value = &value[..value.len().min(MAX_VALUE_BYTES)];
    Some((key, text(value)))
}

fn is_label(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 96
        && key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '\'' | '-' | '_'))
}

/// A value's text. A narrow string has no NUL inside it, a wide ASCII one has a NUL after every character.
fn text(value: &[u8]) -> String {
    let trimmed = value.strip_prefix(&[0]).unwrap_or(value);
    let trimmed = trimmed.strip_suffix(&[0]).unwrap_or(trimmed);
    if trimmed.len() >= 2 && trimmed.iter().all(|b| (0x20..0x7f).contains(b)) {
        return String::from_utf8_lossy(trimmed).into_owned();
    }
    let units = value.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p));
    let decoded: String = char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect();
    decoded.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
}

impl crate::Revit {
    /// Reads and parses `BasicFileInfo`.
    pub fn basic_file_info(&mut self) -> Result<BasicFileInfo> {
        BasicFileInfo::parse(&self.read_stream(crate::BASIC_FILE_INFO)?)
    }
}
