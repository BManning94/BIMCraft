//! Rooms: a name and a number at a level, and a point inside the walls round them.

use super::walls::{WALLS, base, import, level_named};
use crate::project_fixture::{Block, Project, Rec, line_bytes, orientation_bytes};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Room};

const ROOMS: i64 = -2_000_160;

fn rooms(i: &Imported) -> Vec<&Room> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// A 20 by 10 ft enclosure of four walls from level 1 to level 2.
fn walled(release: u32) -> Project {
    let mut p = base(release);
    let word = !(2015..=2020).contains(&release);
    let walls = [
        (100, [0.0, 0.0], [1.0, 0.0], 20.0),
        (101, [20.0, 0.0], [0.0, 1.0], 10.0),
        (102, [20.0, 10.0], [-1.0, 0.0], 20.0),
        (103, [0.0, 10.0], [0.0, -1.0], 10.0),
    ];
    for (id, o, d, len) in walls {
        let (o, d, len): ([f64; 2], [f64; 2], f64) = (o, d, len);
        let (x0, x1) = (o[0].min(o[0] + d[0] * len) - 0.4, o[0].max(o[0] + d[0] * len) + 0.4);
        let (y0, y1) = (o[1].min(o[1] + d[1] * len) - 0.4, o[1].max(o[1] + d[1] * len) + 0.4);
        p.recs.push(Rec::new(id, WALLS).at([x0, y0, 0.0, x1, y1, 10.0], &[10, 11, 500, u64::from(id)]));
        let mut b = line_bytes(0.0, len, [o[0], o[1], 0.0], [d[0], d[1], 0.0]);
        b.extend(orientation_bytes(0, 1, false, word));
        p.data.push(Block { id, payload: b });
    }
    p
}

fn room(p: &mut Project, id: u32, number: &str, name: &str, bbox: [f64; 6]) {
    p.recs.push(Rec::new(id, ROOMS).at(bbox, &[10, 11, u64::from(id)]));
    p.room_names.push((id, number.into(), name.into()));
}

#[test]
fn a_room_comes_in_with_its_number_name_level_and_a_point_inside_its_walls() {
    for release in [2019, 2023, 2024, 2026] {
        let mut p = walled(release);
        room(&mut p, 300, "101", "Kitchen ☃", [0.2, 0.2, 0.0, 19.8, 9.8, 8.0]);
        let i = import(&p);
        let [r] = rooms(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        assert_eq!((r.number.as_str(), r.name.as_str(), r.level), ("101", "Kitchen ☃", level_named(&i, "Level 1")), "Revit {release}");
        let region = i.model.room_region(r).expect("enclosed");
        assert!((region.area() - 6096.0 * 3048.0).abs() < 1.0, "{}", region.area());
    }
}

#[test]
fn a_rooms_point_is_moved_off_a_wall_it_would_fall_on_and_a_room_with_no_name_is_called_room() {
    // A partition across the middle of the room, along the room's middle: the middle is on a wall, a grid point beside it is not. A
    // partition makes two rooms of the enclosure, so the room is the half its point is in, and it has the half's box.
    let mut p = walled(2024);
    p.recs.push(Rec::new(104, WALLS).at([9.6, 0.0, 0.0, 10.4, 10.0, 10.0], &[10, 11, 500, 104]));
    let mut b = line_bytes(0.0, 10.0, [10.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    b.extend(orientation_bytes(0, 1, false, true));
    p.data.push(Block { id: 104, payload: b });
    p.recs.push(Rec::new(300, ROOMS).at([0.2, 0.2, 0.0, 9.8, 9.8, 8.0], &[10, 11, 300]));
    let i = import(&p);
    let [r] = rooms(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((r.name.as_str(), r.number.as_str()), ("Room", ""));
    assert!(r.point.0 > 0.0 && r.point.0 < 3048.0, "{:?}", r.point);
    // The box of the whole enclosure does not match the half a point finds: the room is left out.
    let mut q = p.clone();
    q.recs.retain(|r| r.id != 300);
    room(&mut q, 301, "1", "Hall", [0.2, 0.2, 0.0, 19.8, 9.8, 8.0]);
    q.recs.push(Rec::new(104, WALLS).at([9.6, 0.0, 0.0, 10.4, 10.0, 10.0], &[10, 11, 500, 104]));
    let i = import(&q);
    assert!(rooms(&i).is_empty() && i.skipped.iter().any(|s| s.contains("do not enclose its box") && s.contains("301")), "{:?}", i.skipped);
}

#[test]
fn rooms_without_walls_or_without_a_level_are_left_out_with_the_reason() {
    let mut p = base(2024);
    room(&mut p, 300, "1", "Open", [0.0, 0.0, 0.0, 10.0, 10.0, 8.0]);
    let i = import(&p);
    assert!(rooms(&i).is_empty() && i.skipped.iter().any(|s| s.contains("room") && s.contains("300")), "{:?}", i.skipped);
}

#[test]
fn the_newest_partitions_number_and_name_win_even_when_empty() {
    use crate::fixture::{self, container};
    let mut older = walled(2024);
    room(&mut older, 300, "101", "Old", [0.2, 0.2, 0.0, 19.8, 9.8, 8.0]);
    let mut newer = older.clone();
    newer.room_names = vec![(300, "102".into(), String::new())];
    let basic = fixture::basic_file_info(&fixture::Basic { release: 2024, ..Default::default() });
    let schema = fixture::gzip(&Project::schema_bytes());
    let table = fixture::global(&newer.element_table());
    let (a, b) = (fixture::partition(&[&older.partition_bytes()]), fixture::partition(&[&newer.partition_bytes()]));
    let streams: [(&str, &[u8]); 5] = [
        ("BasicFileInfo", &basic),
        ("Formats/Latest", &schema),
        ("Global/ElemTable", &table),
        ("Partitions/21", &a),
        ("Partitions/22", &b),
    ];
    let i = crate::import(&container(&streams), "x").unwrap();
    let [r] = rooms(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((r.number.as_str(), r.name.as_str()), ("102", "Room"), "the newer partition says 102 and no name");
}

#[test]
fn the_rooms_of_the_architectural_sample_have_rvt_rss_numbers_and_names() {
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let i = crate::import_path(&path, "x").unwrap();
        let mut got: Vec<(String, String)> = rooms(&i).iter().map(|r| (r.number.clone(), r.name.clone())).collect();
        got.sort();
        // Nine of the fourteen rooms are enclosed by walls that come in; the rest are open to curtain walls or room separation lines.
        let want: Vec<(&str, &str)> = vec![
            ("102", "Mech."),
            ("103", "Bath"),
            ("201", "Entry Hall"),
            ("202", "Bedroom"),
            ("203", "Bath"),
            ("204", "Bedroom"),
            ("205", "Bath"),
            ("207", "Master Bath"),
            ("208", "Linen"),
        ];
        assert_eq!(got.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), want, "{}: {:?}", path.display(), i.skipped);
        assert!(i.skipped.iter().any(|s| s.contains("5 rooms left out") && s.contains("do not enclose")), "{:?}", i.skipped);
    }
}

#[test]
fn a_mutation_sweep_over_a_project_of_rooms_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = walled(release);
        room(&mut p, 300, "101", "Kitchen", [0.2, 0.2, 0.0, 19.8, 9.8, 8.0]);
        room(&mut p, 301, "102", "Hall", [0.2, 0.2, 0.0, 9.8, 9.8, 8.0]);
        p.members = 3;
        super::sweep(&p, 0x2468_ace0_1357_9bdf ^ u64::from(release), 800, |i, round| assert!(rooms(i).len() <= 2, "round {round}"));
    }
}

#[test]
fn a_value_far_beyond_revits_limits_is_damage_and_never_reaches_the_geometry() {
    // One wall of the enclosure starts a googol of feet away: a length the reader does not take, so the wall has no line, and the room
    // (with only three walls) is left out. Nothing may panic on the way: the room's point is found among the walls made.
    let mut p = walled(2024);
    p.data.retain(|b| b.id != 100);
    let mut b = line_bytes(0.0, 1e300, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    b.extend(orientation_bytes(0, 1, false, true));
    p.data.push(Block { id: 100, payload: b });
    room(&mut p, 300, "101", "Kitchen", [0.2, 0.2, 0.0, 19.8, 9.8, 8.0]);
    let i = import(&p);
    assert_eq!(super::walls::walls(&i).len(), 3, "{:?}", i.skipped);
    assert!(rooms(&i).is_empty() && i.skipped.iter().any(|s| s.contains("no location line") && s.contains("100")), "{:?}", i.skipped);
}
