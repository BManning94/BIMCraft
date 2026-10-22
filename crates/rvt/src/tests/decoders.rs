//! The readers of an element's data, on bytes written by the fixture and on bytes that are not what Revit writes.

use crate::curves::{first_arc, first_line, line_at};
use crate::project_fixture::{arc_bytes, layers_bytes, line_bytes, name_bytes, orientation_bytes};
use crate::types::{find_layers, first_name, layer_layout, unique_type, wall_orientation};
use std::collections::BTreeSet;

const LINE: ([f64; 3], [f64; 3]) = ([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);

#[test]
fn a_line_runs_from_its_origin_by_its_parameters_along_its_direction() {
    let bytes = line_bytes(-2.0, 5.0, LINE.0, LINE.1);
    let l = first_line(&bytes).unwrap();
    assert_eq!((l.start(), l.end()), ([1.0, 0.0, 3.0], [1.0, 7.0, 3.0]));
    // Not a line: a direction that is not a unit, a start at or past the end, a value that is not a number, a cut record.
    assert!(first_line(&line_bytes(0.0, 5.0, LINE.0, [0.0, 2.0, 0.0])).is_none());
    assert!(first_line(&line_bytes(5.0, 5.0, LINE.0, LINE.1)).is_none());
    assert!(first_line(&line_bytes(0.0, f64::NAN, LINE.0, LINE.1)).is_none());
    assert!(first_line(&line_bytes(0.0, 5.0, [f64::INFINITY, 0.0, 0.0], LINE.1)).is_none());
    assert!(first_line(&bytes[..bytes.len() - 1]).is_none());
    assert!(line_at(&bytes, bytes.len()).is_none() && line_at(&[], 0).is_none() && line_at(&bytes, usize::MAX).is_none());
    // The first curve record decides: a line after an arc is not the wall's, and an arc after a line is not either.
    let arc = arc_bytes([0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 3.0, [0.0; 3]);
    assert!(first_line(&[arc.clone(), bytes.clone()].concat()).is_none());
    assert!(first_arc(&[bytes.clone(), arc.clone()].concat()).is_none());
    assert!(first_line(&[b"junk".to_vec(), bytes].concat()).is_some());
    assert!(first_arc(&arc).is_some());
}

#[test]
fn an_arc_needs_unit_square_axes_a_radius_and_a_start_below_its_end() {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let ok = |angles, x, y, r| first_arc(&arc_bytes(angles, x, y, r, [4.0, 5.0, 6.0]));
    let a = ok([0.5, 2.0], x, y, 3.0).unwrap();
    assert!(a.counter_clockwise());
    let (p, q) = (a.point(0.0), a.point(std::f64::consts::FRAC_PI_2));
    assert!((p[0] - 7.0).abs() < 1e-12 && (p[1] - 5.0).abs() < 1e-12 && (q[0] - 4.0).abs() < 1e-12 && (q[1] - 8.0).abs() < 1e-12);
    assert!(!ok([0.5, 2.0], x, [0.0, -1.0, 0.0], 3.0).unwrap().counter_clockwise());
    assert!(ok([2.0, 2.0], x, y, 3.0).is_none() && ok([3.0, 2.0], x, y, 3.0).is_none());
    assert!(ok([0.5, 2.0], x, y, 0.0).is_none() && ok([0.5, 2.0], x, y, -1.0).is_none());
    assert!(ok([0.5, 2.0], [2.0, 0.0, 0.0], y, 3.0).is_none(), "an axis that is not a unit");
    assert!(ok([0.5, 2.0], x, [0.6, 0.8, 0.0], 3.0).is_none(), "axes that are not square to each other");
    assert!(ok([0.5, f64::NAN], x, y, 3.0).is_none());
    // The marker word before the tag must be 1.
    let mut bytes = arc_bytes([0.5, 2.0], x, y, 3.0, [0.0; 3]);
    bytes[0] = 0;
    assert!(first_arc(&bytes).is_none());
    assert!(first_arc(&[1, 0, 0]).is_none() && first_arc(&[]).is_none());
}

fn ids() -> Vec<u32> {
    vec![5, 50, 60]
}

fn materials() -> BTreeSet<u32> {
    BTreeSet::from([50, 60])
}

#[test]
fn layers_are_read_in_each_layout_and_only_where_every_record_is_one_a_type_can_have() {
    let tag = crate::project_fixture::REGIONS.to_le_bytes();
    for release in [2019, 2023, 2024, 2025, 2026, 2027] {
        let layout = layer_layout(release).unwrap();
        let read = |layers: &[(f64, Option<u32>, u32)]| find_layers(&layers_bytes(layers, release), tag, layout, &materials(), &ids());
        let got = read(&[(0.25, Some(50), 4), (0.0, None, 100), (0.5, Some(60), 1)]).unwrap();
        assert_eq!(
            got.iter().map(|l| (l.width_feet, l.material, l.function)).collect::<Vec<_>>(),
            [(0.25, Some(50), 4), (0.0, None, 100), (0.5, Some(60), 1)],
            "Revit {release}"
        );
        // A width of zero only on a membrane or a deck; one that is not a number, negative or over ten feet is no layer.
        assert!(read(&[(0.0, Some(50), 1)]).is_none() && read(&[(0.0, None, 200)]).is_some());
        assert!(
            read(&[(-1.0, Some(50), 1)]).is_none() && read(&[(10.5, Some(50), 1)]).is_none() && read(&[(f64::NAN, Some(50), 1)]).is_none()
        );
        // A material that is not a material of the file, a function nobody has, no layers at all.
        assert!(read(&[(1.0, Some(77), 1)]).is_none() && read(&[(1.0, Some(50), 7)]).is_none() && read(&[]).is_none());
        assert!(read(&vec![(1.0, Some(50), 1); 33]).is_none() && read(&vec![(1.0, Some(50), 1); 32]).is_some());
        // One bad layer spoils the list.
        assert!(read(&[(1.0, Some(50), 1), (1.0, Some(77), 1)]).is_none());
    }
    assert!(layer_layout(2014).is_none() && layer_layout(2028).is_none());
    // The count may instead follow the type's name and a zero word, when no tag frames it.
    let layout = layer_layout(2024).unwrap();
    let mut data = vec![0x11u8; 3];
    data.extend_from_slice(&5u32.to_le_bytes());
    data.extend("Basic".encode_utf16().flat_map(u16::to_le_bytes));
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&layers_bytes(&[(0.5, Some(50), 1)], 2024)[6..]);
    assert_eq!(find_layers(&data, tag, layout, &materials(), &ids()).map(|l| l.len()), Some(1));
    // A window is all that is searched.
    let mut far = vec![0u8; 0x2100];
    far.extend(layers_bytes(&[(0.5, Some(50), 1)], 2024));
    assert!(find_layers(&far, tag, layout, &materials(), &ids()).is_none());
}

#[test]
fn a_wall_orientation_is_its_line_setting_a_word_and_a_flip_that_are_in_range() {
    for (release, word) in [(2019, false), (2023, true), (2024, true)] {
        let o = wall_orientation(&orientation_bytes(3, 1, true, word), release).unwrap();
        assert_eq!((o.location_line, o.word, o.flip), (3, if word { 1 } else { 0 }, true), "Revit {release}");
        assert!(!wall_orientation(&orientation_bytes(5, 2, false, word), release).unwrap().flip);
        assert!(wall_orientation(&orientation_bytes(6, 1, false, word), release).is_none(), "a location line past 5");
    }
    assert!(wall_orientation(&orientation_bytes(0, 3, false, true), 2024).is_none(), "a word past 2");
    let mut bad = orientation_bytes(0, 1, false, true);
    let n = bad.len();
    bad[n - 4] = 2;
    assert!(wall_orientation(&bad, 2024).is_none(), "a flip that is neither 0 nor 1");
    // The second flag: 0 and `00 00`, or 1 and `ff ff`, are both fine; 1 and `00 00` is not.
    let mut second = orientation_bytes(0, 1, true, true);
    let n = second.len();
    second[n - 3..].copy_from_slice(&[1, 0xff, 0xff]);
    assert!(wall_orientation(&second, 2024).is_some());
    second[n - 2] = 0;
    assert!(wall_orientation(&second, 2024).is_none());
    assert!(wall_orientation(&[0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0], 2024).is_none() && wall_orientation(&[], 2024).is_none());
}

#[test]
fn a_name_is_the_first_framed_string_that_is_one() {
    let header = [0xff, 0xff, 0xff, 0xff, 0x10, 0x00, 1, 0, 0, 0];
    assert_eq!(first_name(&name_bytes("Generic - 200mm"), &header, 2024).as_deref(), Some("Generic - 200mm"));
    // Past other frames: the tag ffff (an unset slot), another element's header, an empty or control-laden string.
    let mut data = vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 3, 0, 0, 0, b'a', 0, b'b', 0, b'c', 0];
    data.extend_from_slice(&header);
    data.extend(name_bytes(""));
    data.extend(name_bytes("a\tb"));
    data.extend(name_bytes("   "));
    data.extend(name_bytes("Second"));
    assert_eq!(first_name(&data, &header, 2024).as_deref(), Some("Second"));
    assert!(first_name(&name_bytes("x"), &header, 2024).is_some());
    assert!(first_name(&name_bytes("x"), &header, 2022).is_none(), "one character is an id read as a name before 2023");
    assert!(first_name(&name_bytes("xy"), &header, 2022).is_some());
    // A length past the data, or past the cap, or an unpaired surrogate.
    let mut cut = name_bytes("Generic");
    cut.truncate(cut.len() - 2);
    assert!(first_name(&cut, &header, 2024).is_none());
    let mut long = vec![0xff, 0xff, 0xff, 0xff, 0x02, 0x10];
    long.extend_from_slice(&300u32.to_le_bytes());
    long.extend([b'a', 0].repeat(300));
    assert!(first_name(&long, &header, 2024).is_none());
    let mut lone = name_bytes("ab");
    lone[10..12].copy_from_slice(&0xd800u16.to_le_bytes());
    assert!(first_name(&lone, &header, 2024).is_none());
    assert!(first_name(&[], &header, 2024).is_none());
}

#[test]
fn the_type_a_record_names_is_the_one_type_of_its_category_in_its_list() {
    let types = BTreeSet::from([5, 6]);
    assert_eq!(unique_type(&[1, 2, 5, 9], &types), Some(5));
    assert_eq!(unique_type(&[5, 9, 5], &types), Some(5), "named twice is still one");
    assert_eq!(unique_type(&[5, 6], &types), None);
    assert_eq!(unique_type(&[1, 2], &types), None);
    assert_eq!(unique_type(&[u64::MAX, 5 + (1 << 32)], &types), None, "an id past a u32 is not one");
    assert_eq!(unique_type(&[], &types), None);
}
