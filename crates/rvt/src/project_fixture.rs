//! A synthetic project for tests: a container with `BasicFileInfo`, the schema, the element table and one or more partitions,
//! holding the elements and levels a test names, written in the two record forms the reader knows (up to 2023 and from 2024).
//! Autodesk's files are never committed; the bytes here follow the layouts in `inventory.rs`, which real files were checked
//! against.

use crate::fixture::{self, Basic, Class};

/// The tags the fixture's schema gives its classes (12 is the first tag a schema has).
pub const OUTLINE: u16 = 12;
pub const PARENTS: u16 = 13;
pub const HEADER: u16 = 14;
pub const PLANE: u16 = 15;

const KIND_PLACED: u32 = 0xffff_ef7f;
const KIND_SYMBOL: u32 = 0xffff_8000;

/// A record: an element of `category`, a placed instance or not (a type's symbol).
#[derive(Clone, Debug)]
pub struct Rec {
    pub id: u32,
    pub category: i64,
    pub placed: bool,
    pub class_tag: u16,
    /// A record that sits inside the body of another record, which names it (the second prologue). From 2024 only.
    pub inside: Option<u32>,
    /// The id the record's own header holds when it has one of its own: set 0 to leave it undeclared in the element table.
    pub declared: bool,
}

impl Rec {
    pub fn new(id: u32, category: i64) -> Self {
        Self { id, category, placed: true, class_tag: 100, inside: None, declared: true }
    }
}

#[derive(Clone, Debug)]
pub struct Lev {
    pub id: u32,
    pub name: String,
    pub feet: f64,
    pub placed: bool,
    /// False leaves out its name block.
    pub named: bool,
    /// False leaves out the second copy of the elevation that confirms the first.
    pub confirmed: bool,
    /// The Building Story setting the block holds, or none for a block with the frame left out.
    pub story: Option<bool>,
    /// A level that sits inside another element, which its block names (from 2024 only).
    pub owner: Option<u32>,
}

impl Lev {
    pub fn new(id: u32, name: &str, feet: f64) -> Self {
        Self { id, name: name.to_string(), feet, placed: true, named: true, confirmed: true, story: None, owner: None }
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    pub release: u32,
    pub recs: Vec<Rec>,
    pub levels: Vec<Lev>,
    /// Ids the element table declares beyond the ones above.
    pub extra_ids: Vec<u32>,
    /// How many gzip members the partition's bytes are cut into (a record can run across two).
    pub members: usize,
    /// The name the file records for its last save (`Username`).
    pub saved_by: String,
}

impl Project {
    pub fn new(release: u32) -> Self {
        Self { release, recs: Vec::new(), levels: Vec::new(), extra_ids: Vec::new(), members: 1, saved_by: String::new() }
    }

    fn wide(&self) -> bool {
        self.release >= 2024
    }

    /// The inflated bytes of the partition.
    pub fn partition_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut blocks = Vec::new();
        if self.wide() {
            let hosts: Vec<u32> = self.recs.iter().filter_map(|r| r.inside).collect();
            for r in self.recs.iter().filter(|r| r.inside.is_none()) {
                out.extend_from_slice(&record64(r.id.into(), r, &[]));
            }
            for host in hosts {
                let inside: Vec<&Rec> = self.recs.iter().filter(|r| r.inside == Some(host)).collect();
                let mut body = Vec::new();
                for r in inside {
                    body.extend_from_slice(&[0x33; 10]);
                    body.extend_from_slice(&frame64(0, r));
                }
                let host_rec = Rec { placed: true, ..Rec::new(host, -2_000_011) };
                out.extend_from_slice(&record64(host.into(), &host_rec, &body));
            }
            for l in &self.levels {
                out.extend_from_slice(&level64(l));
            }
        } else {
            for r in &self.recs {
                out.extend_from_slice(&record32(r));
                out.extend_from_slice(&[0x44; 5]);
            }
            for l in &self.levels {
                out.extend_from_slice(&level32(l));
                out.extend_from_slice(&[0x44; 3]);
            }
        }
        for l in self.levels.iter().filter(|l| l.named) {
            blocks.extend_from_slice(&name_block(l, self.wide()));
        }
        out.extend_from_slice(&blocks);
        out
    }

    /// The element table.
    pub fn element_table(&self) -> Vec<u8> {
        let mut ids: Vec<u32> = self.recs.iter().filter(|r| r.declared).map(|r| r.id).collect();
        ids.extend(self.recs.iter().filter_map(|r| r.inside));
        ids.extend(self.levels.iter().map(|l| l.id));
        ids.extend(&self.extra_ids);
        // A table of one record fits both record sizes; a real one holds thousands of ids.
        ids.extend(900_001..900_006);
        ids.sort_unstable();
        ids.dedup();
        let mut out = vec![0xaa, 0x05];
        out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
        for id in &ids {
            if self.wide() {
                out.extend_from_slice(&u64::from(*id).to_le_bytes());
                out.extend_from_slice(&[0; 12]);
                out.extend_from_slice(&u64::from(*id).to_le_bytes());
                out.extend_from_slice(&u64::MAX.to_le_bytes());
                out.extend_from_slice(&[0; 4]);
            } else {
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&[0; 16]);
                out.extend_from_slice(&u32::MAX.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0; 19]);
        out
    }

    pub fn schema_bytes() -> Vec<u8> {
        fixture::schema(&[
            Class { name: "Outline", fields: &["m_min"], base: None },
            Class { name: "ElementParents", fields: &["m_ids"], base: None },
            Class { name: "ElementHeader", fields: &[], base: None },
            Class { name: "Plane", fields: &["m_origin"], base: None },
        ])
    }

    /// The container as a Revit file.
    pub fn file(&self) -> Vec<u8> {
        self.file_from(&self.partition_bytes())
    }

    /// The container with these bytes as its partition (cut into the project's number of gzip members).
    pub fn file_from(&self, all: &[u8]) -> Vec<u8> {
        let cut = |k: usize| all.len() * k / self.members.max(1);
        let chunks: Vec<&[u8]> = (0..self.members.max(1)).map(|k| &all[cut(k)..cut(k + 1)]).collect();
        let basic = fixture::basic_file_info(&Basic { release: self.release, username: &self.saved_by, ..Basic::default() });
        fixture::container(&[
            ("BasicFileInfo", &basic),
            ("Formats/Latest", &fixture::gzip(&Self::schema_bytes())),
            ("Global/ElemTable", &fixture::global(&self.element_table())),
            ("Partitions/21", &fixture::partition(&chunks)),
        ])
    }
}

fn marker() -> [u8; 8] {
    let mut m = [0xff; 8];
    m[..2].copy_from_slice(&OUTLINE.to_le_bytes());
    m[6..].copy_from_slice(&PARENTS.to_le_bytes());
    m
}

/// The bounding box: a unit cube, as six `f64`.
fn bbox() -> Vec<u8> {
    [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The fields of a 2024 and later frame from `+0x10` to the end of the box and an empty reference list.
fn frame64(raw_id: u64, r: &Rec) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&raw_id.to_le_bytes());
    f.extend_from_slice(&0x8cu32.to_le_bytes()); // the record's size
    let constant = PARENTS.wrapping_sub(12);
    f.extend_from_slice(&constant.to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes()); // +0x10
    f.extend_from_slice(&r.category.to_le_bytes()); // +0x12
    f.extend_from_slice(&[0xff; 16]); // +0x1a
    f.extend_from_slice(&[0xff; 8]); // +0x2a design option
    f.extend_from_slice(&u64::MAX.to_le_bytes()); // +0x32 container
    f.extend_from_slice(&[0xff; 8]); // +0x3a
    f.extend_from_slice(&(if r.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // +0x42
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&r.class_tag.to_le_bytes()); // +0x4a
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&marker()); // +0x50
    f.extend_from_slice(&bbox()); // +0x58
    f.extend_from_slice(&0u32.to_le_bytes()); // +0x88 reference list
    f
}

/// A chain record: a frame (or, with a `body`, a frame and then more), and the trailer that repeats the size.
fn record64(id: u64, r: &Rec, body: &[u8]) -> Vec<u8> {
    let mut f = frame64(id, r);
    f.extend_from_slice(body);
    let size = f.len() as u32;
    f[8..12].copy_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&size.to_le_bytes());
    f
}

fn level64(l: &Lev) -> Vec<u8> {
    let r = Rec { placed: l.placed, ..Rec::new(l.id, -2_000_240) };
    let mut f = frame64(l.id.into(), &r);
    f.truncate(0x50);
    f.extend_from_slice(&marker()[2..]); // the six bytes of the marker, without the Outline tag
    f.extend_from_slice(&2u16.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    let size = f.len() as u32;
    f[8..12].copy_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&size.to_le_bytes());
    f
}

/// An element record of up to 2023, laid out back from its marker.
fn record32(r: &Rec) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&r.id.to_le_bytes()); // m-52
    f.extend_from_slice(&112u32.to_le_bytes());
    f.extend_from_slice(&HEADER.to_le_bytes()); // m-44
    f.extend_from_slice(&[0; 4]);
    f.extend_from_slice(&r.category.to_le_bytes()); // m-38
    f.extend_from_slice(&[0xff; 8]);
    f.extend_from_slice(&u32::MAX.to_le_bytes()); // m-22 container
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&(if r.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // m-14
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&r.class_tag.to_le_bytes()); // m-6
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&marker()); // m
    f.extend_from_slice(&bbox()); // m+8
    f.extend_from_slice(&0u32.to_le_bytes());
    f
}

fn level32(l: &Lev) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&l.id.to_le_bytes()); // id_at = category - 14
    f.extend_from_slice(&66u32.to_le_bytes());
    f.extend_from_slice(&HEADER.to_le_bytes());
    f.extend_from_slice(&[0; 4]);
    f.extend_from_slice(&(-2_000_240i64).to_le_bytes()); // the category
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&u32::MAX.to_le_bytes()); // +16 container
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&(if l.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // +24
    f.extend_from_slice(&[0; 10]);
    f.extend_from_slice(&marker()[2..]); // +38
    f.extend_from_slice(&[0; 8]);
    f
}

/// The block that names a level and gives its elevation (twice, the second time with the 24 bytes before it).
fn name_block(l: &Lev, wide: bool) -> Vec<u8> {
    let mut b = vec![0x11; 7];
    if let (true, Some(other)) = (wide, l.owner) {
        // The block of a level inside another element: its id, then eight `ff` and the owner's id where a project level has
        // an unbroken run of 56.
        b.extend_from_slice(&[1, 0, 0, 0]);
        b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        b.extend_from_slice(&[0xff; 8]);
        b.extend_from_slice(&u64::from(other).to_le_bytes());
        b.extend_from_slice(&[0xff; 40]);
    } else if wide {
        b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        b.extend_from_slice(&[0xff; 56]);
    } else {
        b.extend_from_slice(&l.id.to_le_bytes());
        b.extend_from_slice(&[0xff; 28]);
    }
    b.extend_from_slice(&[0, 0, 0]);
    b.extend_from_slice(&(l.name.encode_utf16().count() as u32).to_le_bytes());
    b.extend(l.name.encode_utf16().flat_map(u16::to_le_bytes));
    if let Some(story) = l.story {
        // An unset id, `00 01`, a number, the level's id and the setting.
        b.extend_from_slice(&if wide { vec![0xff; 8] } else { vec![0xff; 4] });
        b.extend_from_slice(&[0, 1]);
        b.extend_from_slice(&0.0f64.to_le_bytes());
        if wide {
            b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        } else {
            b.extend_from_slice(&l.id.to_le_bytes());
        }
        b.push(u8::from(story));
    }
    b.extend_from_slice(&[0x22; 10]);
    b.extend_from_slice(&[5, 0, 0, 0]);
    b.extend_from_slice(&PLANE.to_le_bytes());
    let filler: Vec<u8> = (0..49u8).map(|i| 0x30 + i).collect();
    b.extend_from_slice(&filler);
    let feet = l.feet.to_le_bytes();
    b.extend_from_slice(&feet);
    b.extend_from_slice(&[0x55; 100]);
    if l.confirmed {
        b.extend_from_slice(&filler[25..]);
        b.extend_from_slice(&feet);
    }
    // Far enough that the next block's setting is not within reach of this one's search.
    b.extend_from_slice(&[0x66; 0x300]);
    b
}
