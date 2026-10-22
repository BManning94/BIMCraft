//! The records of a partition: boxes, reference lists, types, in both record forms.

use super::*;
use crate::inventory::OST_LEVELS;
use crate::project_fixture::{KIND_MATERIAL, KIND_TYPE, Lev, Project, Rec, TypeRec};
use crate::records::Record;

const WALLS: i64 = -2_000_011;

fn survey(p: &Project) -> (Vec<Record>, Vec<crate::records::TypeRecord>) {
    let s = Revit::from_bytes(p.file(), Limits::default()).unwrap().survey(&|_| true).unwrap();
    (s.records, s.types)
}

#[test]
fn a_record_carries_its_box_and_both_reference_lists_in_both_forms() {
    for release in [2019, 2023, 2024, 2026] {
        let mut p = Project::new(release);
        p.recs = vec![
            Rec::new(100, WALLS).at([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[7, 8, 9, 100, 11]),
            Rec { refs2: vec![30, 31], ..Rec::new(101, WALLS).at([0.0; 6], &[1]) },
        ];
        p.recs[1].bbox = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0];
        let (records, _) = survey(&p);
        let r = records.iter().find(|r| r.id == 100).unwrap();
        assert_eq!(
            (r.bbox, r.refs.clone(), r.refs2.clone(), r.placed, r.category),
            ([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![7, 8, 9, 100, 11], vec![], true, WALLS),
            "Revit {release}"
        );
        assert_eq!(r.extent(), [3.0, 3.0, 3.0]);
        assert_eq!(r.preceding(), Some(9), "the slot before the record's own id: an opening's host");
        let r = records.iter().find(|r| r.id == 101).unwrap();
        assert_eq!((r.refs.clone(), r.refs2.clone(), r.preceding()), (vec![1], vec![30, 31], None), "Revit {release}");
    }
}

#[test]
fn the_slot_before_the_records_own_id_is_a_host_only_when_there_is_one() {
    let r = |refs: &[u64]| Record {
        partition: 1,
        offset: 0,
        id: 100,
        category: WALLS,
        class_tag: 0,
        placed: true,
        bbox: [0.0; 6],
        refs: refs.to_vec(),
        refs2: vec![],
        inside: false,
    };
    assert_eq!(r(&[5, 100]).preceding(), Some(5));
    assert_eq!(r(&[5, 100, 6, 100]).preceding(), Some(6), "the last occurrence");
    assert_eq!(r(&[100, 5]).preceding(), None, "named first: nothing before it");
    assert_eq!(r(&[0, 100]).preceding(), None);
    assert_eq!(r(&[u64::MAX, 100]).preceding(), None);
    assert_eq!(r(&[1, 2]).preceding(), None);
    assert_eq!(r(&[]).preceding(), None);
}

#[test]
fn a_reference_list_that_does_not_check_out_is_no_list() {
    // From 2024 a list of no slots is no list (and no second follows it), one of more than 1,024 or running past the data too;
    // up to 2023 an empty list is an empty list.
    let mut p = Project::new(2024);
    p.recs = vec![Rec::new(100, WALLS).at([0.0; 6], &[7, 100]), Rec::new(101, WALLS).at([0.0; 6], &[8, 101])];
    p.recs[1].refs2 = vec![40];
    let mut bytes = p.partition_bytes();
    let counts: Vec<usize> = memchr::memmem::find_iter(&bytes, &2u32.to_le_bytes()).collect();
    let at = |id: u64| memchr::memmem::find(&bytes, &[&2u32.to_le_bytes()[..], &7u64.to_le_bytes(), &id.to_le_bytes()].concat()).unwrap();
    assert!(!counts.is_empty());
    let first = at(100);
    for (what, count) in [("zero", 0u32), ("past the limit", 1025), ("past the data", u32::MAX)] {
        let mut bad = bytes.clone();
        bad[first..first + 4].copy_from_slice(&count.to_le_bytes());
        let (records, _) = {
            let s = Revit::from_bytes(p.file_from(&bad), Limits::default()).unwrap().survey(&|_| true).unwrap();
            (s.records, ())
        };
        let r = records.iter().find(|r| r.id == 100).unwrap();
        assert!(r.refs.is_empty() && r.refs2.is_empty(), "{what}");
        assert!(records.iter().any(|r| r.id == 101 && r.refs == [8, 101] && r.refs2 == [40]), "the next record is untouched ({what})");
    }
    bytes.clear();
}

#[test]
fn types_and_materials_have_records_without_a_box_that_name_themselves() {
    for release in [2023, 2024] {
        let mut p = Project::new(release);
        p.levels = vec![Lev::new(10, "Level 1", 0.0)];
        p.recs = vec![Rec::new(100, WALLS)];
        p.types = vec![TypeRec::new(500, WALLS), TypeRec { kind: KIND_MATERIAL, ..TypeRec::new(700, -2_000_700) }];
        let (records, types) = survey(&p);
        assert_eq!(records.len(), 1, "a type is not a placed element with a box");
        // (Up to 2023 a level is a box-less record too, of the placed kind; it is no type.)
        let got: Vec<(u32, i64, bool)> =
            types.iter().filter(|t| t.category != OST_LEVELS).map(|t| (t.id, t.category, t.is_type())).collect();
        assert!(types.iter().filter(|t| t.category == OST_LEVELS).all(|t| !t.is_type()));
        assert_eq!(got, [(500, WALLS, true), (700, -2_000_700, false)], "Revit {release}");
        assert_eq!(types.iter().find(|t| t.id == 500).map(|t| t.kind), Some(KIND_TYPE));
    }
    // From 2024 the slot list must hold the record's own id; a count of zero or one that overruns is not a record.
    let mut p = Project::new(2024);
    p.types = vec![TypeRec::new(500, WALLS)];
    let bytes = p.partition_bytes();
    let list = memchr::memmem::find(&bytes, &[&2u32.to_le_bytes()[..], &7u64.to_le_bytes(), &500u64.to_le_bytes()].concat()).unwrap();
    for (what, edit) in [("another id", 24usize), ("a count of 0", 0), ("a count past the limit", 1)] {
        let mut bad = bytes.clone();
        match edit {
            24 => bad[list + 12..list + 20].copy_from_slice(&501u64.to_le_bytes()),
            0 => bad[list..list + 4].copy_from_slice(&0u32.to_le_bytes()),
            _ => bad[list..list + 4].copy_from_slice(&5000u32.to_le_bytes()),
        }
        let (_, types) = {
            let s = Revit::from_bytes(p.file_from(&bad), Limits::default()).unwrap().survey(&|_| true).unwrap();
            ((), s.types)
        };
        assert!(types.is_empty(), "{what}");
    }
}

#[test]
fn a_type_record_without_an_id_of_its_own_takes_the_id_of_the_record_it_sits_in() {
    // Most types sit in the documents of the families a project loads, in the body of another record (the second prologue).
    let mut p = Project::new(2024);
    p.types = vec![TypeRec { inside: true, ..TypeRec::new(500, WALLS) }, TypeRec::new(501, WALLS)];
    let (_, types) = survey(&p);
    let mut got: Vec<(u32, i64)> = types.iter().map(|t| (t.id, t.category)).collect();
    got.sort();
    assert_eq!(got, [(500, WALLS), (501, WALLS)], "{types:?}");
    // The slot list of a frame inside a record must name that record's id.
    let bytes = p.partition_bytes();
    let list = memchr::memmem::find(&bytes, &[&2u32.to_le_bytes()[..], &7u64.to_le_bytes(), &500u64.to_le_bytes()].concat()).unwrap();
    let mut bad = bytes.clone();
    bad[list + 12..list + 20].copy_from_slice(&999u64.to_le_bytes());
    let s = Revit::from_bytes(p.file_from(&bad), Limits::default()).unwrap().survey(&|_| true).unwrap();
    assert_eq!(s.types.iter().map(|t| t.id).collect::<Vec<_>>(), [501]);
}
