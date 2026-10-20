//! What an element made from a mass remembers of it, under reserved names in its `params` (as phases and paint are held):
//! a floor `mass_floors` made holds `{mass, level}` under [`FLOOR`], so a second run updates it instead of making another;
//! a wall, floor or roof made by face holds `{mass, face}` under [`FACE`] (an edge number for a wall, `"top"` for a floor or
//! a roof), so `update_by_face` can rebuild it from the mass as it is now.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::{Element, Id, Mass, Model};

pub const FLOOR: &str = "Mass Floor";
pub const FACE: &str = "Mass Face";

/// Whether `name` is one of the parameters a mass's elements hold their mark in: no project or family parameter may take it.
pub fn reserved(name: &str) -> bool {
    matches!(name, FLOOR | FACE)
}

/// What an element's mark says, if `key` is where one is held: a mass of the project, and the level or the face of it.
pub(crate) fn check_value(m: &Model, who: &str, key: &str, v: &Value, e: &Element) -> Option<Result<(), String>> {
    reserved(key).then(|| {
        let shape = if key == FLOOR { "{mass, level}" } else { "{mass, face}" };
        let wrong = || format!("{who}: `{key}` is {shape}, not {v}");
        let o = v.as_object().filter(|o| o.len() == 2).ok_or_else(wrong)?;
        let mass = o.get("mass").and_then(Value::as_u64).ok_or_else(wrong)?;
        if !matches!(m.elements.get(&mass), Some(Element::Mass(_))) {
            return Err(format!("{who}: `{key}` names {mass}, which isn't a mass"));
        }
        if key == FLOOR {
            if !matches!(e, Element::Floor(_)) {
                return Err(format!("{who}: only a floor holds `{key}`"));
            }
            let level = o.get("level").and_then(Value::as_u64).ok_or_else(wrong)?;
            m.level(level).map(|_| ()).ok_or_else(|| format!("{who}: `{key}` names {level}, which isn't a level"))
        } else {
            match (e, o.get("face")) {
                (Element::Wall(_), Some(f)) if f.is_u64() => Ok(()),
                (Element::Floor(_) | Element::Roof(_), Some(f)) if f == "top" => Ok(()),
                _ => Err(format!("{who}: `{key}` is {shape}: a wall's face is an edge number, a floor's or a roof's is \"top\", not {v}")),
            }
        }
    })
}

/// The mass and the face a by-face element was made on, if it holds a mark: a `Some` face is an edge number, `None` the top.
pub fn face_of(e: &Element) -> Option<(Id, Option<usize>)> {
    let o = e.params()?.get(FACE)?.as_object()?;
    Some((o.get("mass")?.as_u64()?, o.get("face")?.as_u64().and_then(|f| usize::try_from(f).ok())))
}

/// The floors `mass_floors` made from `mass` that still lie on the level their mark names, with that level.
pub fn floors_of(m: &Model, mass: Id) -> Vec<(Id, Id)> {
    m.elements
        .iter()
        .filter_map(|(id, e)| {
            let Element::Floor(f) = e else { return None };
            let o = f.params.get(FLOOR)?.as_object()?;
            (o.get("mass")?.as_u64()? == mass && o.get("level")?.as_u64()? == f.level).then_some((*id, f.level))
        })
        .collect()
}

/// `params` without a mass's marks: a copy or a split piece is an ordinary element.
pub fn detach(params: &mut BTreeMap<String, Value>) {
    params.remove(FLOOR);
    params.remove(FACE);
}

/// The marks that name a mass in `gone` taken off, since it's deleted: what was made from it is ordinary from then on.
pub fn forget(m: &mut Model, gone: &BTreeSet<Id>) {
    for params in m.elements.values_mut().filter_map(Element::params_mut) {
        for key in [FLOOR, FACE] {
            if params.get(key).and_then(|v| v.get("mass")).and_then(Value::as_u64).is_some_and(|mass| gone.contains(&mass)) {
                params.remove(key);
            }
        }
    }
}

/// Element `e`, made on `face` of `ms` (an edge number for a wall, none for the top), rebuilt from the mass as it is now:
/// a wall stands on the edge as tall as the mass, a floor or a roof lies on its top face. What was done to it since
/// (its type, a roof's slope) stays. The reason, when the face is gone.
pub fn follow(e: &mut Element, ms: &Mass, mass: Id, face: Option<usize>) -> Result<(), String> {
    let ring = ms.footprint();
    match (e, face) {
        (Element::Wall(w), Some(f)) => {
            let n = ring.len();
            let (Some(&a), Some(&b)) = (ring.get(f), ring.get((f + 1) % n)) else {
                return Err(format!("was made on face {f} of mass {mass}, which has {n} faces now (0 to {})", n.saturating_sub(1)));
            };
            // Counter-clockwise, the mass is to the left of its edge; a wall faces out to its left, so it runs the other way.
            (w.start, w.end, w.level, w.base_offset) = (b, a, ms.level, ms.base_offset);
            w.top_level = ms.top_level;
            if ms.top_level.is_none() {
                w.height = ms.height;
            }
            Ok(())
        }
        (Element::Floor(fl), None) => {
            (fl.boundary, (fl.level, fl.offset)) = (ring, ms.top_face());
            Ok(())
        }
        (Element::Roof(r), None) => {
            (r.boundary, (r.level, r.offset)) = (ring, ms.top_face());
            Ok(())
        }
        (e, _) => Err(format!("is a {}, which holds no such face of mass {mass}", e.category())),
    }
}

/// A mass mirrored in place runs its outline round the other way, so its edge `f` of `n` is edge `n - 2 - f` (mod `n`) now:
/// the faces its walls were made on, numbered again, so they follow the same edges.
pub fn mirrored(m: &mut Model, mass: Id) {
    let Some(Element::Mass(ms)) = m.elements.get(&mass) else { return };
    let n = ms.boundary.len();
    for params in m.elements.values_mut().filter_map(Element::params_mut) {
        if let Some(o) = params.get_mut(FACE).and_then(Value::as_object_mut)
            && o.get("mass").and_then(Value::as_u64) == Some(mass)
            && let Some(f) = o.get("face").and_then(Value::as_u64).and_then(|f| usize::try_from(f).ok()).filter(|f| *f < n)
        {
            o.insert("face".into(), Value::from((2 * n - 2 - f) % n));
        }
    }
}
