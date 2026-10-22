//! Roofs by footprint: flat ones come in, the rest are left out with the reason.

use super::floors::{project, rectangle};
use super::walls::{import, level_named};
use crate::project_fixture::{PROFILE_ROOF, Rec, TypeRec};
use bimcraft_core::geom::{P2, area};
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
        let found: Vec<(u64, &Roof)> = i
            .model
            .elements
            .iter()
            .filter_map(|(m, e)| if let Element::Roof(r) = e { Some((*m, r)) } else { None })
            .filter(|(m, _)| from[m] == format!("Roof {}", theirs.id))
            .collect();
        let [(_, r)] = found[..] else { panic!("{}: {:?}", path.display(), i.skipped) };
        let elevation = i.model.elevation(r.level);
        let bottom = elevation + r.offset;
        assert!((elevation - theirs.storey).abs() < 0.11 && (bottom - theirs.bottom).abs() < 0.11);
        assert!((bottom + i.model.types.roof[&r.type_name].thickness - theirs.top).abs() < 0.11);
        let net = area(&r.boundary).abs() - r.courtyards.iter().map(|c| area(c).abs()).sum::<f64>();
        assert!((net / 1e6 - theirs.area).abs() < 0.005, "{} m2 against {}", net / 1e6, theirs.area);
    }
}

#[test]
fn the_roof_by_extrusion_of_the_architectural_sample_comes_in_and_fills_the_box_its_record_holds() {
    // Revit 2023, 2024 and 2025 hold the same roof: a gable of two slopes over 6,360 mm, swept 19.5 m, whose box the profile and its
    // type's 202 mm square to each slope reproduce. rvt-rs does not read it, so its record box is the check (in feet, as the file has it).
    const BOX: [f64; 6] = [
        -21.122_271_924_417_255,
        -11.094_468_433_666_506,
        18.787_785_971_390_942,
        42.854_106_028_338_65,
        9.771_673_298_616_395,
        30.179_387_462_143_346,
    ];
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        assert!(!i.skipped.iter().any(|s| s.contains("roof left out") || s.contains("roofs left out")), "{:?}", i.skipped);
        let (m, r) = roofs_with_ids(&i)
            .into_iter()
            .find(|(m, _)| from[m] == "Roof 243274")
            .unwrap_or_else(|| panic!("{}: {:?}", path.display(), i.skipped));
        let x = r.extrusion.as_ref().unwrap();
        let under = i.model.elevation(r.level) + r.offset;
        assert!((under - BOX[2] * 304.8).abs() < 0.01, "{}: its underside's lowest point {under} is the bottom of its box", path.display());
        let heights: Vec<f64> = x.profile.iter().map(|p| p.1).collect();
        assert_eq!(x.profile.iter().map(|p| p.0).collect::<Vec<_>>(), [0.0, 3294.472, 6360.0], "{}", path.display());
        assert!(heights.iter().zip([17.184, 3186.542, 0.0]).all(|(h, e)| (h - e).abs() < 0.01), "{}: {heights:?}", path.display());
        // The profile runs north along x = 22.506 ft, and it is swept to its left, west: over the box's x.
        let x0 = x.line[0].0;
        assert!((x0 - 22.506_337 * 304.8).abs() < 0.01 && x.line[0].0 == x.line[1].0, "{}", path.display());
        assert!(
            (x.line[0].1 - BOX[1] * 304.8).abs() < 0.01 && (x.line[1].1 - BOX[4] * 304.8).abs() < 0.01,
            "{}: along its box's y",
            path.display()
        );
        assert!(
            (x.from - (x0 - BOX[3] * 304.8)).abs() < 0.01 && (x.to - (x0 - BOX[0] * 304.8)).abs() < 0.01,
            "{}: over its box's x",
            path.display()
        );
        assert_eq!(i.model.types.roof[&r.type_name].thickness, 202.0);
        assert!(i.model.check().is_ok(), "{m}");
    }
}

fn roofs_with_ids(i: &bimcraft_core::import::Imported) -> Vec<(u64, &Roof)> {
    i.model.elements.iter().filter_map(|(m, e)| if let Element::Roof(r) = e { Some((*m, r)) } else { None }).collect()
}

#[test]
fn a_mutation_sweep_over_a_project_of_roofs_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = with_roof_type(release);
        roof(&mut p, 100, PROFILE_ROOF, [0.0, 0.0, 12.0, 20.0, 10.0, 12.25]);
        rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 3.0, release >= 2024);
        rectangle(&mut p, 100, 210, [5.0, 3.0, 9.0, 7.0], 3.0, release >= 2024);
        roof(&mut p, 101, 100, [0.0, 0.0, 12.0, 20.0, 10.0, 15.0]);
        p.members = 3;
        super::sweep(&p, 0x0f1e_2d3c_4b5a_6978 ^ u64::from(release), 800, |i, round| assert!(roofs(i).len() <= 2, "round {round}"));
    }
}

#[test]
fn roof_types_of_one_name_are_one_type_when_their_layers_match_and_two_when_they_do_not() {
    let mut p = with_roof_type(2024);
    for (tid, material) in [(531, 700), (532, 701)] {
        p.types.push(TypeRec::new(tid, ROOFS));
        let mut payload = crate::project_fixture::name_bytes("Warm");
        payload.extend(crate::project_fixture::layers_bytes(&[(0.25, Some(material), 4)], 2024));
        p.data.push(crate::project_fixture::Block { id: tid, payload });
    }
    for (k, tid) in [530u64, 531, 532].into_iter().enumerate() {
        let (id, x) = (100 + k as u32, 30.0 * k as f64);
        p.recs.push(Rec {
            class_tag: PROFILE_ROOF,
            ..Rec::new(id, ROOFS).at([x, 0.0, 12.0, x + 20.0, 10.0, 12.25], &[10, tid, u64::from(id)])
        });
        rectangle(&mut p, id, 200 + 10 * k as u32, [x, 0.0, x + 20.0, 10.0], 3.0, true);
    }
    let i = import(&p);
    let names: Vec<&str> = roofs(&i).iter().map(|r| r.type_name.as_str()).collect();
    assert_eq!(names, ["Warm", "Warm", "Warm (2)"], "{:?}", i.skipped);
    assert!(i.skipped.is_empty(), "a sketch of one loop needs no note: {:?}", i.skipped);
}

/// A sketch line of an extrusion roof's profile, in the vertical plane `x = plane`, from `(y, z)` to `(y, z)` (feet), with its curve
/// in its data.
fn profile_line(p: &mut crate::project_fixture::Project, owner: u32, id: u32, plane: f64, a: (f64, f64), b: (f64, f64)) {
    let (dy, dz) = (b.0 - a.0, b.1 - a.1);
    let len = dy.hypot(dz);
    let bbox = [plane, a.0.min(b.0), a.1.min(b.1), plane, a.0.max(b.0), a.1.max(b.1)];
    let mut r = Rec::new(id, crate::floors::OST_SKETCH_LINES).at(bbox, &[1]);
    r.refs2 = vec![30, u64::from(owner)];
    p.recs.push(r);
    let line = crate::project_fixture::line_bytes(0.0, len, [plane, a.0, a.1], [0.0, dy / len, dz / len]);
    p.data.push(crate::project_fixture::Block { id, payload: line });
}

/// A gable roof by extrusion over the roof type "Warm" (0.25 ft thick): its top profile in the plane `x = 10`, eaves at 12 ft at
/// `y = 0` and `y = 20` and the ridge 20 ft up at `y = 10`, swept from `x = 0` to `x = 30`. Its underside is 0.25 ft square to each
/// slope: 0.25 * sqrt(1 + 0.8^2) ft under the top, down the plumb ends, and its box is 11.68 ft to 20 ft tall.
fn gable(release: u32) -> crate::project_fixture::Project {
    let mut p = with_roof_type(release);
    let drop = 0.25 * (1.0f64 + 0.8 * 0.8).sqrt();
    p.recs.push(Rec {
        class_tag: crate::project_fixture::EXTRUSION_ROOF,
        ..Rec::new(100, ROOFS).at([0.0, 0.0, 12.0 - drop, 30.0, 20.0, 20.0], &[10, 530, 100])
    });
    profile_line(&mut p, 100, 200, 10.0, (0.0, 12.0), (10.0, 20.0));
    profile_line(&mut p, 100, 201, 10.0, (20.0, 12.0), (10.0, 20.0));
    p
}

#[test]
fn a_roof_by_extrusion_comes_in_as_its_profile_swept_across_with_its_type_square_to_each_slope() {
    for release in [2024, 2026] {
        let i = import(&gable(release));
        let [r] = roofs(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        let x = r.extrusion.as_ref().expect("an extrusion roof");
        let drop = 0.25 * (1.0f64 + 0.8 * 0.8).sqrt() * 304.8;
        assert_eq!(x.line, [P2(3048.0, 0.0), P2(3048.0, 6096.0)], "along y in the plane x = 10 ft");
        assert_eq!(
            (x.from, x.to),
            (-6096.0, 3048.0),
            "to its left, which is west: x = 30 ft is 20 ft to the right of the plane, x = 0 is 10 to the left"
        );
        let heights: Vec<f64> = x.profile.iter().map(|p| p.1).collect();
        assert_eq!(x.profile.iter().map(|p| p.0).collect::<Vec<_>>(), [0.0, 3048.0, 6096.0]);
        assert_eq!(heights, [0.0, 2438.4, 0.0], "its underside rises 8 ft from each eave to the ridge, as its top does");
        assert!((r.offset - (3657.6 - drop)).abs() < 0.01, "its underside's eave is the bottom of its box: {}", r.offset);
        assert_eq!((r.level, r.type_name.as_str()), (level_named(&i, "Level 1"), "Warm"));
        assert!(r.boundary.is_empty() && r.courtyards.is_empty());
        assert!(i.model.check().is_ok());
        assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    }
}

#[test]
fn a_roof_by_extrusion_the_reader_cannot_check_against_its_box_is_left_out_with_the_reason() {
    let said = |p: &crate::project_fixture::Project, what: &str| {
        let i = import(p);
        assert!(roofs(&i).is_empty(), "{:?}", i.skipped);
        i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_else(|| panic!("`{what}` in {:?}", i.skipped))
    };
    // Its box is a foot lower than its profile and type make it: its thickness is not what the reader assumed.
    let mut p = gable(2024);
    p.recs.iter_mut().find(|r| r.id == 100).unwrap().bbox[2] -= 1.0;
    assert!(said(&p, "box").starts_with("1 roof left out, its box"), "ids 100");
    // A profile line that is an arc.
    let mut p = gable(2024);
    let arc = crate::project_fixture::arc_bytes([0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 10.0, [10.0, 0.0, 12.0]);
    p.data.iter_mut().find(|b| b.id == 200).unwrap().payload = arc;
    said(&p, "straight");
    // A profile in a plane that is not square to an axis (here at 45 degrees): the box says nothing of how far it is swept.
    let mut p = gable(2024);
    for (id, from, to) in [(200, [0.0, 0.0, 12.0], [7.0, 7.0, 20.0]), (201, [14.0, 14.0, 12.0], [7.0, 7.0, 20.0])] {
        let d: [f64; 3] = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        p.data.iter_mut().find(|b| b.id == id).unwrap().payload = crate::project_fixture::line_bytes(0.0, len, from, d.map(|v| v / len));
    }
    said(&p, "axis");
    // Two lines that do not meet.
    let mut p = gable(2024);
    p.data.iter_mut().find(|b| b.id == 201).unwrap().payload = {
        // The second slope ends a foot above where the first does.
        let len = (10.0f64 * 10.0 + 8.0 * 8.0).sqrt();
        crate::project_fixture::line_bytes(0.0, len, [10.0, 20.0, 13.0], [0.0, -10.0 / len, 8.0 / len])
    };
    said(&p, "chain");
}

#[test]
fn a_mutation_sweep_over_a_roof_by_extrusion_never_panics_and_what_comes_in_checks_out() {
    for release in [2024, 2026] {
        let mut p = gable(release);
        p.members = 3;
        super::sweep(&p, 0x1a2b_3c4d_5e6f_7081 ^ u64::from(release), 800, |i, round| assert!(roofs(i).len() <= 1, "round {round}"));
    }
}
