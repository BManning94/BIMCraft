//! Roofs by footprint: a sketched boundary, flat, at a level, of a type.
//!
//! A footprint roof (class `ProfileRoof`) is read as a floor or a ceiling is: the loops its sketch lines close, the one roof type
//! its reference list names, the level it stands on and its underside, which is the bottom of its box. It comes in only where it
//! is flat: its box is as tall as its type is thick. A roof that rises, whether its edges set a slope, its type is thick across a
//! slope or it is sloped some other way, is left out and listed, and so is a roof of another class (by extrusion, by face): the
//! slope blocks of the edges (rvt-rs RE-56) are not read, since no roof of the samples this crate was checked against sets one.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::model::{Element, Roof, RoofType};

use crate::floors::{ROOF, ring, sketch_lines, sketched};
use crate::import::{Facts, Import, kind};
use crate::inventory::Gathered;

/// `OST_Roofs`.
pub(crate) const OST_ROOFS: i64 = -2_000_035;

pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    let sketches = sketch_lines(f);
    for r in f.of_category(OST_ROOFS) {
        out.insert(r.id);
        out.extend(sketches.get(&r.id).into_iter().flatten().map(|l| l.id));
    }
    out.extend(f.types.get(&OST_ROOFS).into_iter().flatten());
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let sketches = sketch_lines(f);
    let mut made: BTreeMap<u32, String> = BTreeMap::new();
    for r in f.of_category(OST_ROOFS) {
        if f.tags.profile_roof != Some(r.class_tag) {
            cx.left_out("roof", r.id, "it is not a roof by footprint (by extrusion or by face), which is not described");
            continue;
        }
        let name = |cx: &mut Import, tid: u32, typ: &crate::walls::LayeredType| {
            made.entry(tid)
                .or_insert_with(|| {
                    let base = typ.name.clone().unwrap_or_else(|| format!("Imported - {}mm", typ.thickness));
                    kind(
                        &mut cx.model.types.roof,
                        &base,
                        |k| (k.thickness - typ.thickness).abs() < 0.05 && k.layers == typ.layers,
                        RoofType { thickness: typ.thickness, layers: typ.layers.clone() },
                    )
                })
                .clone()
        };
        match sketched(cx, f, g, r, ROOF, &sketches, name) {
            Ok(s) => {
                if s.pieces.len() > 1 {
                    cx.note("roof", r.id, "its sketch is several separate loops: one roof for each");
                }
                for p in &s.pieces {
                    let roof = Roof {
                        type_name: s.type_name.clone(),
                        level: s.level,
                        boundary: ring(&p.outer),
                        courtyards: p.holes.iter().map(|h| ring(h)).collect(),
                        offset: s.offset,
                        ..Roof::default()
                    };
                    let id = cx.model.insert(Element::Roof(roof));
                    cx.from.insert(id, format!("Roof {}", r.id));
                }
            }
            Err(why) => cx.left_out(
                "roof",
                r.id,
                &why.replace("sloped or stepped roof, which is not described", "sloped or stepped roof, whose slopes are not read"),
            ),
        }
    }
}
