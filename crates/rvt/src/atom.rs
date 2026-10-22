// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/part_atom.rs (which elements of the
// Atom entry hold the title, the OmniClass term and the categories) and src/project_information.rs (the one-entry ZIP a
// project keeps the same entry in, and that its entry name is never read out).
// Changed: rewritten for BIMCraft with a small scanner of its own instead of quick-xml, hard limits on size, nesting and
// tags, and no copy of the raw XML kept.
//! The Atom entry: a family's `PartAtom` stream, or a project's `ProjectInformation` (a ZIP holding the same entry).
//!
//! It holds the document's title, the save time, the product version and, for a family, its categories and OmniClass code
//! (`23.40.20.14.17` is furniture). The ZIP entry's own name is a temporary path that embeds the saving user's Windows
//! folder, so it is skipped and never returned.

use std::io::Read;

use crate::error::{Error, Result};

/// The most XML read, and how deep it may nest, and how many tags are looked at.
pub const MAX_ATOM: usize = 1 << 20;
const MAX_DEPTH: usize = 64;
const MAX_TAGS: usize = 100_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Atom {
    /// The entry's own `<title>`: the document's name.
    pub title: Option<String>,
    pub updated: Option<String>,
    /// `<A:product-version>`, the release the entry was written by.
    pub product_version: Option<String>,
    /// The first category term that is a dotted number.
    pub omniclass: Option<String>,
    /// Every category's term, in order.
    pub categories: Vec<String>,
}

impl Atom {
    pub fn parse(xml: &[u8]) -> Result<Self> {
        let bad = |why: &str| Error::Format(format!("the Atom entry {why}"));
        if xml.len() > MAX_ATOM {
            return Err(bad(&format!("is {} bytes, over the limit of {MAX_ATOM}", xml.len())));
        }
        let xml = std::str::from_utf8(xml).map_err(|_| bad("is not UTF-8"))?;
        let mut atom = Atom::default();
        let mut stack: Vec<&str> = Vec::new();
        let mut text = String::new();
        let mut rest = xml;
        let mut tags = 0;
        while let Some(open) = rest.find('<') {
            text.push_str(&rest[..open]);
            rest = &rest[open..];
            let (skip_to, kind) = if rest.starts_with("<!--") {
                ("-->", None)
            } else if rest.starts_with("<![CDATA[") {
                ("]]>", None)
            } else if rest.starts_with("<?") {
                ("?>", None)
            } else if rest.starts_with("<!") {
                (">", None)
            } else {
                (">", Some(()))
            };
            let close = rest.find(skip_to).ok_or_else(|| bad("is cut short in the middle of markup"))?;
            let body = &rest[1..close];
            rest = &rest[close + skip_to.len()..];
            if kind.is_none() {
                continue;
            }
            tags += 1;
            if tags > MAX_TAGS {
                return Err(bad("has too many tags"));
            }
            if let Some(name) = body.strip_prefix('/') {
                let name = name.trim();
                if stack.pop() != Some(name) {
                    return Err(bad(&format!("closes `{name}` where it is not open")));
                }
                atom.element_ended(&stack, name, &decode(text.trim()));
                text.clear();
                continue;
            }
            let empty = body.ends_with('/');
            let body = body.trim_end_matches('/');
            let name = body.split_whitespace().next().unwrap_or("");
            if name == "category"
                && let Some(term) = attribute(body, "term")
            {
                atom.category(&term);
            }
            text.clear();
            if !empty {
                if stack.len() >= MAX_DEPTH {
                    return Err(bad(&format!("nests deeper than {MAX_DEPTH} levels")));
                }
                stack.push(name);
            }
        }
        if !stack.is_empty() {
            return Err(bad("is cut short: an element is never closed"));
        }
        Ok(atom)
    }

    /// An element ended with `text` inside it, under the elements in `parents`.
    fn element_ended(&mut self, parents: &[&str], name: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        let local = name.rsplit(':').next().unwrap_or(name);
        let plain = !name.contains(':');
        match (local, parents) {
            ("title", [entry]) if plain && *entry == "entry" => {
                self.title.get_or_insert_with(|| text.to_string());
            }
            ("updated", [entry]) if plain && *entry == "entry" => {
                self.updated.get_or_insert_with(|| text.to_string());
            }
            ("product-version", _) => {
                self.product_version.get_or_insert_with(|| text.to_string());
            }
            ("term", [.., "category"]) => self.category(text),
            _ => {}
        }
    }

    fn category(&mut self, term: &str) {
        if term.contains('.') && term.chars().all(|c| c.is_ascii_digit() || c == '.') {
            self.omniclass.get_or_insert_with(|| term.to_string());
        }
        self.categories.push(term.to_string());
    }
}

/// `name="value"` inside a tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}="))?;
    let rest = &tag[at + name.len() + 1..];
    let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let end = rest[1..].find(quote)?;
    Some(decode(&rest[1..1 + end]))
}

/// The five entities and numeric references.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let name = &rest[1..semi];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((ch, semi + 1))
        });
        match decoded {
            Some((ch, used)) => {
                out.push(ch);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The first entry of the ZIP a project keeps in `ProjectInformation`, parsed as an Atom entry. `max_inflated` caps the
/// entry's size once inflated.
pub fn project_information(bytes: &[u8], max_inflated: usize) -> Result<Atom> {
    let bad = |why: &str| Error::Format(format!("ProjectInformation {why}"));
    let number = |at: usize, len: usize| -> Option<usize> {
        let b = bytes.get(at..at + len)?;
        Some(b.iter().rev().fold(0usize, |n, &x| n << 8 | usize::from(x)))
    };
    if bytes.get(..4) != Some(b"PK\x03\x04") {
        return Err(bad("is not a ZIP archive"));
    }
    let (method, size) = (number(8, 2), number(18, 4));
    let (name_len, extra_len) = (number(26, 2), number(28, 2));
    let (Some(method), Some(size), Some(name_len), Some(extra_len)) = (method, size, name_len, extra_len) else {
        return Err(bad("is cut short in its first entry's header"));
    };
    // The entry's name is skipped, never read.
    let data = bytes.get(30 + name_len + extra_len..).ok_or_else(|| bad("has no data after its first entry's header"))?;
    let limit = max_inflated.min(MAX_ATOM);
    let body = match method {
        0 => {
            let stored = data.get(..size).ok_or_else(|| bad("is cut short in its stored entry"))?;
            if stored.len() > limit {
                return Err(bad(&format!("holds an entry over the limit of {limit} bytes")));
            }
            stored.to_vec()
        }
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(data)
                .take(limit as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| bad(&format!("does not inflate: {e}")))?;
            if out.len() > limit {
                return Err(bad(&format!("holds an entry that inflates past the limit of {limit} bytes")));
            }
            out
        }
        other => return Err(bad(&format!("uses compression method {other}, which this reader does not know"))),
    };
    Atom::parse(&body)
}

impl crate::Revit {
    /// The document's Atom entry, from `PartAtom` (a family) or `ProjectInformation` (a project); none if the file has neither.
    pub fn document_atom(&mut self) -> Result<Option<Atom>> {
        if self.stream_len("PartAtom").is_some() {
            return Atom::parse(&self.read_stream("PartAtom")?).map(Some);
        }
        if self.stream_len("ProjectInformation").is_some() {
            let stored = self.read_stream("ProjectInformation")?;
            return project_information(&stored, MAX_ATOM).map(Some);
        }
        Ok(None)
    }
}
