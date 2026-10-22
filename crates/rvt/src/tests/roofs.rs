//! Roofs by footprint: flat ones come in, the rest are left out with the reason.

use super::floors::{project, rectangle};
use super::walls::{import, level_named};
use crate::project_fixture::{PROFILE_ROOF, Rec, TypeRec};
use bimcraft_core::geom::area;
use bimcraft_core::model::{Element, Roof};

const ROOFS: i64 = -2_000_035;

fn roofs(i: &bimcraft_core::import::Imported) -> Vec<&Roof> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Roof(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// A project whose ceiling type (id 520, 0.25 ft) serves as the roof type, as a roof type of category roofs.
fn with_roof_type(release: u32) -> crate::project_fixture::Project {
    let mut p = project(release);
    p.types.push(TypeRec::new(530, ROOFS));
    let layers = crate::project_fixture::layers_bytes(&[(0.25, Some(700), 4)], release);
    p.data.push(crate::project_fixture::Block { id: 530, payload: [crate::project_fixture::name_bytes("Warm"), layers].concat() });
    p
}

fn roof(p: &mut crate::project_fixture::Project, id: u32, class: u16, bbox: [f64; 6]) {
    p.recs.push(Rec { class_tag: class, ..Rec::new(id, ROOFS).at(bbox, &[10, 530, u64::from(id)]) });
}

#[test]
fn a_flat_roof_by_footprint_comes_in_with_its_courtyard_type_level_and_underside() {
    for (release, data) in [(2024, true), (2023, false)] {
        let mut p = with_roof_type(release);
        roof(&mut p, 100, PROFILE_ROOF, [0.0, 0.0, 12.0, 20.0, 10.0, 12.25]);
        rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 3.0, data);
        rectangle(&mut p, 100, 210, [5.0, 3.0, 9.0, 7.0], 3.0, data);
        let i = import(&p);
        let [r] = roofs(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        assert_eq!(
            (r.level, r.offset, r.slope, r.type_name.as_str()),
            (level_named(&i, "Level 1"), 3657.6, 0.0, "Warm"), // it names level 1 alone: its underside, 12 ft up, is 3,657.6 mm over it
            "{r:?}"
        );
        assert_eq!(r.courtyards.len(), 1);
        assert!((area(&r.boundary) - 6096.0 * 3048.0).abs() < 1.0);
        assert_eq!(i.model.types.roof["Warm"].thickness, 76.2);
    }
}

#[test]
fn a_roof_that_rises_or_is_not_by_footprint_is_left_out_with_the_reason() {
    let mut p = with_roof_type(2024);
    // Taller than its type is thick: it rises.
    roof(&mut p, 100, PROFILE_ROOF, [0.0, 0.0, 12.0, 20.0, 10.0, 15.0]);
    rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 3.0, true);
    // Flat, but of another class (by extrusion).
    roof(&mut p, 101, 100, [0.0, 0.0, 12.0, 20.0, 10.0, 12.25]);
    rectangle(&mut p, 101, 210, [0.0, 0.0, 20.0, 10.0], 3.0, true);
    let i = import(&p);
    assert!(roofs(&i).is_empty());
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    assert!(said("slopes are not read").contains("100"), "{:?}", i.skipped);
    assert!(said("not a roof by footprint").contains("101"));
}

#[test]
fn the_roof_by_footprint_of_the_architectural_sample_is_that_of_rvt_rss_export() {
    use super::rvt_rs::RAC_ROOFS;
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        let theirs = &RAC_ROOFS[0];
        let found: Vec<(u64, &Roof)> =
            i.model.elements.iter().filter_map(|(m, e)| if let Element::Roof(r) = e { Some((*m, r)) } else { None }).collect();
        let [(m, r)] = found[..] else { panic!("{}: {:?}", path.display(), i.skipped) };
        assert_eq!(from[&m], format!("Roof {}", theirs.id));
        let elevation = i.model.elevation(r.level);
        let bottom = elevation + r.offset;
        assert!((elevation - theirs.storey).abs() < 0.11 && (bottom - theirs.bottom).abs() < 0.11);
        assert!((bottom + i.model.types.roof[&r.type_name].thickness - theirs.top).abs() < 0.11);
        let net = area(&r.boundary).abs() - r.courtyards.iter().map(|c| area(c).abs()).sum::<f64>();
        assert!((net / 1e6 - theirs.area).abs() < 0.005, "{} m2 against {}", net / 1e6, theirs.area);
        // The other roof is by extrusion.
        assert!(i.skipped.iter().any(|s| s.contains("1 roof left out") && s.contains("243274")), "{:?}", i.skipped);
    }
}
