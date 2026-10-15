//! The engine driven the way every front end drives it: commands with JSON params, and checked
//! through commands too.

use serde_json::{Value, json};

use crate::engine::{Session, commands};
use crate::geom::P2;
use crate::model::{Facing, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn count(s: &mut Session, category: &str) -> u64 {
    run(s, "elements", json!({"category": category}))["count"].as_u64().unwrap()
}

/// An 8 x 6 m box of walls on Level 1; the ids of its walls, bottom one first.
fn house(s: &mut Session) -> Vec<u64> {
    let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn a_closed_wall_chain_makes_one_wall_per_side_up_to_the_next_level() {
    let mut s = Session::new();
    assert_eq!(house(&mut s).len(), 4);
    let w = run(&mut s, "elements", json!({"category": "walls"}));
    let first = &w["elements"][0];
    assert_eq!(first["length"], 8000.0);
    assert_eq!(first["height"], 3000.0);
    assert_eq!(first["top_level"], 2, "Level 2");
}

#[test]
fn doors_and_windows_find_the_wall_under_a_point() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let d = run(&mut s, "door", json!({"at": [2000, 30]}));
    assert_eq!((d["host"].as_u64(), d["offset"].as_f64(), d["mark"].as_str()), (Some(ids[0]), Some(2000.0), Some("1")));
    let w = run(&mut s, "WN", json!({"at": [8000, 3000]}));
    assert_eq!(w["host"].as_u64(), Some(ids[1]));
    let e = s.execute("door", &json!({"at": [4000, 3000]})).unwrap_err();
    assert!(e.contains("no wall"), "{e}");
}

#[test]
fn a_failed_command_changes_nothing() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let before = s.model.clone();
    let e = s.execute("door", &json!({"wall": ids[0], "offset": 100})).unwrap_err();
    assert!(e.contains("fit"), "{e}");
    assert_eq!(*s.model, *before);
    // The first wall of this chain is fine, the second one has no length.
    assert!(s.execute("wall", &json!({"points": [[0, 0], [1000, 0], [1000, 0]]})).is_err());
    assert_eq!(*s.model, *before);
}

#[test]
fn undo_and_redo_step_through_commands() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    assert_eq!(run(&mut s, "undo", json!({}))["undone"], "Door");
    assert_eq!((count(&mut s, "door"), count(&mut s, "wall")), (0, 4));
    assert_eq!(run(&mut s, "redo", json!({}))["redone"], "Door");
    assert_eq!(count(&mut s, "door"), 1);
    assert!(s.execute("redo", &json!({})).is_err());
}

#[test]
fn rooms_need_walls_around_them_and_follow_the_walls() {
    let mut s = Session::new();
    house(&mut s);
    let e = s.execute("room", &json!({"point": [9000, 1000]})).unwrap_err();
    assert!(e.contains("enclosed"), "{e}");
    let r = run(&mut s, "RM", json!({"point": [4000, 3000], "name": "Living"}));
    assert_eq!((r["number"].as_str(), r["area_m2"].as_f64()), (Some("1"), Some(48.0)));
    // A partition splits the box; the room keeps the part around its point.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    let rooms = run(&mut s, "elements", json!({"category": "room"}));
    assert_eq!(rooms["elements"][0]["area_m2"], 30.0);
}

#[test]
fn floors_default_to_the_outline_of_the_walls() {
    let mut s = Session::new();
    house(&mut s);
    assert_eq!(run(&mut s, "floor", json!({}))["area_m2"], 48.0);
    let e = s.execute("floor", &json!({"level": "Level 2"})).unwrap_err();
    assert!(e.contains("boundary"), "{e}");
}

#[test]
fn modify_changes_fields_and_refuses_typos() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "modify", json!({"id": ids[0], "type": "Exterior - 300mm"}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["thickness"], 300.0);
    let e = s.execute("modify", &json!({"id": ids[0], "hieght": 2000})).unwrap_err();
    assert!(e.contains("hieght"), "{e}");
    let e = s.execute("modify", &json!({"id": ids[0], "category": "floor"})).unwrap_err();
    assert!(e.contains("category"), "{e}");
    run(&mut s, "modify", json!({"id": ids[0], "top_level": null, "height": 2500, "level": "Level 1"}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["height"], 2500.0);
}

/// A wall's ends, as `elements` shows them.
fn ends(s: &mut Session, id: u64) -> (Value, Value) {
    let w = run(s, "elements", json!({"category": "wall"}));
    let w = w["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap();
    (w["start"].clone(), w["end"].clone())
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

#[test]
fn moving_a_wall_drags_the_walls_joined_to_it_and_the_rooms_follow() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let part = run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    let room = run(&mut s, "room", json!({"point": [4000, 1500]}))["id"].as_u64().unwrap();
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [8000, 1500]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["area_m2"], 24.0);
    run(&mut s, "MV", json!({"ids": [ids[1]], "by": [1000, 0]}));
    assert_eq!(ends(&mut s, ids[1]), (json!([9000.0, 0.0]), json!([9000.0, 6000.0])));
    assert_eq!(ends(&mut s, ids[0]).1, json!([9000.0, 0.0]), "the corners followed");
    assert_eq!(ends(&mut s, ids[2]).0, json!([9000.0, 6000.0]));
    assert_eq!(ends(&mut s, part).1, json!([9000.0, 3000.0]), "the partition, joined mid-wall, stretched");
    assert_eq!(element(&mut s, room)["area_m2"], 27.0);
    assert_eq!(element(&mut s, door)["at"], json!([2000.0, 0.0]), "a door in a stretched wall stays put");
    assert_eq!(element(&mut s, window)["at"], json!([9000.0, 1500.0]), "a window in the moved wall goes with it");
    // A wall on another level, right above, isn't joined to these.
    let up = run(&mut s, "wall", json!({"points": [[9000, 6000], [9000, 9000]], "level": "Level 2", "height": 3000}))["ids"][0]
        .as_u64()
        .unwrap();
    run(&mut s, "move", json!({"ids": [ids[1]], "by": [-1000, 0]}));
    assert_eq!(ends(&mut s, up).0, json!([9000.0, 6000.0]));
}

#[test]
fn moving_one_end_of_a_wall_stretches_it_and_the_walls_joined_there() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [6000, 0]}))["id"].as_u64().unwrap();
    // The bottom wall's start, at the corner with the left wall, pulled 1000 west: 9 m long now.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [-1000, 0], "end": "start"}));
    assert_eq!(ends(&mut s, ids[0]), (json!([-1000.0, 0.0]), json!([8000.0, 0.0])));
    assert_eq!(ends(&mut s, ids[3]).1, json!([-1000.0, 0.0]), "the left wall's end followed the corner");
    assert_eq!(element(&mut s, door)["at"], json!([6000.0, 0.0]), "its door stayed put");
    let e = s.execute("move", &json!({"ids": [ids[0], ids[1]], "by": [10, 0], "end": "end"})).unwrap_err();
    assert!(e.contains("one wall"), "{e}");
}

#[test]
fn doors_move_along_their_walls_and_copies_bring_their_doors() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [door], "by": [1500, 700]}));
    assert_eq!(element(&mut s, door)["at"], json!([3500.0, 0.0]), "only the part along the wall counts");
    let made = run(&mut s, "CO", json!({"ids": [ids[0]], "by": [0, -2000]}))["ids"].clone();
    let (wall, copy) = (made[0].as_u64().unwrap(), made[1].as_u64().unwrap());
    assert_eq!(ends(&mut s, wall), (json!([0.0, -2000.0]), json!([8000.0, -2000.0])));
    let copy = element(&mut s, copy);
    assert_eq!((copy["host"].as_u64(), copy["mark"].as_str(), copy["at"].clone()), (Some(wall), Some("2"), json!([3500.0, -2000.0])));
    let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Hall"}))["id"].as_u64().unwrap();
    let made = run(&mut s, "copy", json!({"ids": [room], "by": [100, 100]}))["ids"].clone();
    let twin = element(&mut s, made[0].as_u64().unwrap());
    assert_eq!((twin["name"].as_str(), twin["number"].as_str()), (Some("Hall"), Some("2")));
}

fn near(a: &Value, b: [f64; 2]) -> bool {
    (a[0].as_f64().unwrap() - b[0]).abs() < 0.5 && (a[1].as_f64().unwrap() - b[1]).abs() < 0.5
}

#[test]
fn rotating_turns_elements_about_a_point_and_joined_walls_stay_joined() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let part = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    // The partition turned 10° about its middle: its ends slide back onto the walls it met.
    run(&mut s, "RO", json!({"ids": [part], "about": [4000, 3000], "angle": 10}));
    let (a, b) = ends(&mut s, part);
    assert!(near(&a, [4529.0, 0.0]) && near(&b, [3471.0, 6000.0]), "{a} {b}");
    // The whole box a quarter turn: whole numbers stay whole, and so do the joins.
    run(&mut s, "rotate", json!({"ids": [ids[0], ids[1], ids[2], ids[3]], "about": [4000, 3000], "angle": 90}));
    assert_eq!(ends(&mut s, ids[0]), (json!([7000.0, -1000.0]), json!([7000.0, 7000.0])));
    let door = run(&mut s, "door", json!({"at": [7000, 3000]}))["id"].as_u64().unwrap();
    assert!(s.execute("rotate", &json!({"ids": [door], "about": [0, 0], "angle": 5})).unwrap_err().contains("with its wall"));
}

#[test]
fn mirror_copies_with_doors_swinging_the_mirrored_way() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let made = run(&mut s, "MM", json!({"ids": [ids[0]], "axis": [[10000, -5000], [10000, 5000]], "copy": true}))["ids"].clone();
    let (wall, twin) = (made[0].as_u64().unwrap(), made[1].as_u64().unwrap());
    assert_eq!(ends(&mut s, wall), (json!([20000.0, 0.0]), json!([12000.0, 0.0])));
    let (d, t) = (element(&mut s, door), element(&mut s, twin));
    assert_eq!((t["host"].as_u64(), t["at"].clone()), (Some(wall), json!([18000.0, 0.0])));
    assert_eq!((d["flip_facing"].as_bool(), t["flip_facing"].as_bool()), (Some(false), Some(true)), "a mirror image swings the other way");
    let grid = run(&mut s, "grid", json!({"start": [0, -1000], "end": [0, 7000]}))["id"].as_u64().unwrap();
    run(&mut s, "mirror", json!({"ids": [grid], "axis": [[4000, 0], [4000, 1000]]}));
    assert_eq!(element(&mut s, grid)["start"], json!([8000.0, -1000.0]), "without copy, the grid itself goes");
    assert!(s.execute("mirror", &json!({"ids": [grid], "axis": [[1, 1], [1, 1]]})).unwrap_err().contains("axis"));
}

#[test]
fn trim_to_corner_extends_or_trims_two_walls_to_meet() {
    let mut s = Session::new();
    let a = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}))["ids"][0].as_u64().unwrap();
    let b = run(&mut s, "wall", json!({"points": [[5000, 1000], [5000, 6000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "TR", json!({"ids": [a, b]}));
    assert_eq!((ends(&mut s, a).1, ends(&mut s, b).0), (json!([5000.0, 0.0]), json!([5000.0, 0.0])));
    // Crossing walls: `keep` says which part of each stays.
    let c = run(&mut s, "wall", json!({"points": [[-2000, 3000], [3000, 3000]]}))["ids"][0].as_u64().unwrap();
    let d = run(&mut s, "wall", json!({"points": [[1000, 1500], [1000, 5000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "corner", json!({"ids": [c, d], "keep": [[-1000, 3000], [1000, 4500]]}));
    assert_eq!(ends(&mut s, c), (json!([-2000.0, 3000.0]), json!([1000.0, 3000.0])));
    assert_eq!(ends(&mut s, d), (json!([1000.0, 3000.0]), json!([1000.0, 5000.0])));
    let e = run(&mut s, "wall", json!({"points": [[0, 8000], [4000, 8000]]}))["ids"][0].as_u64().unwrap();
    assert!(s.execute("corner", &json!({"ids": [a, e]})).unwrap_err().contains("parallel"));
}

#[test]
fn split_cuts_a_wall_in_two_and_its_doors_go_with_their_part() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [6000, 0]}))["id"].as_u64().unwrap();
    let pieces = run(&mut s, "SL", json!({"id": ids[0], "at": [4000, 20]}))["ids"].clone();
    let new = pieces[1].as_u64().unwrap();
    assert_eq!(ends(&mut s, ids[0]), (json!([0.0, 0.0]), json!([4000.0, 0.0])));
    assert_eq!(ends(&mut s, new), (json!([4000.0, 0.0]), json!([8000.0, 0.0])));
    assert_eq!((element(&mut s, door)["host"].as_u64(), element(&mut s, window)["host"].as_u64()), (Some(ids[0]), Some(new)));
    assert_eq!(element(&mut s, window)["at"], json!([6000.0, 0.0]));
    assert_eq!(element(&mut s, room)["area_m2"], 48.0);
    assert_eq!(s.model.join_reach(ids[0], P2(4000.0, 0.0)), 0.0, "pieces in line don't overlap");
    assert!(s.execute("split", &json!({"id": ids[0], "at": [2000, 0]})).unwrap_err().contains("in the way"));
    assert!(s.execute("split", &json!({"id": ids[0], "at": [2000, 3000]})).unwrap_err().contains("isn't on"));
}

#[test]
fn modify_edits_many_elements_in_one_undo_step() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let r = run(&mut s, "modify", json!({"ids": [ids[0], ids[2]], "type": "Exterior - 300mm"}));
    assert_eq!(r["modified"], json!([ids[0], ids[2]]));
    let thick = |s: &mut Session| [ids[0], ids[1], ids[2]].map(|id| element(s, id)["thickness"].as_f64().unwrap());
    assert_eq!(thick(&mut s), [300.0, 200.0, 300.0]);
    assert_eq!(s.history().last(), Some(&"Modify"));
    run(&mut s, "undo", json!({}));
    assert_eq!(thick(&mut s), [200.0, 200.0, 200.0]);
}

#[test]
fn move_and_copy_refuse_levels_and_what_breaks_the_model() {
    let mut s = Session::new();
    let ids = house(&mut s);
    assert!(s.execute("move", &json!({"ids": [1], "by": [10, 0]})).unwrap_err().contains("elevation"));
    assert!(s.execute("copy", &json!({"ids": [1], "by": [10, 0]})).unwrap_err().contains("level"));
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    assert!(s.execute("copy", &json!({"ids": [door], "by": [300, 0]})).unwrap_err().contains("overlap"));
    assert!(s.execute("move", &json!({"ids": [ids[0]], "by": [1e300, 0]})).is_err());
    assert!(s.execute("move", &json!({"ids": [ids[0]]})).unwrap_err().contains("by"));
}

#[test]
fn deleting_a_wall_takes_its_openings_and_used_levels_stay() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    let d = run(&mut s, "DE", json!({"ids": [ids[0]]}));
    assert_eq!(d["deleted"].as_array().map(Vec::len), Some(2));
    assert_eq!(count(&mut s, "door"), 0);
    let e = s.execute("delete", &json!({"ids": [1]})).unwrap_err();
    assert!(e.contains("level 1"), "Level 1 still has walls: {e}");
}

#[test]
fn types_are_created_and_changed_everywhere_they_are_used() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "type", json!({"category": "wall", "name": "Brick - 250mm", "thickness": 250}));
    run(&mut s, "modify", json!({"id": ids[0], "type": "Brick - 250mm"}));
    run(&mut s, "type", json!({"category": "wall", "name": "Brick - 250mm", "thickness": 280}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["thickness"], 280.0);
    let e = s.execute("type", &json!({"category": "door", "name": "Wide"})).unwrap_err();
    assert!(e.contains("width"), "a new type needs its sizes: {e}");
}

#[test]
fn levels_and_grids_name_themselves() {
    let mut s = Session::new();
    assert_eq!(run(&mut s, "level", json!({"elevation": 6000}))["name"], "Level 3");
    assert_eq!(run(&mut s, "grid", json!({"start": [0, -1000], "end": [0, 7000]}))["name"], "1");
    assert_eq!(run(&mut s, "GR", json!({"start": [8000, -1000], "end": [8000, 7000]}))["name"], "2");
    assert_eq!(run(&mut s, "grid", json!({"start": [-1000, 0], "end": [9000, 0], "name": "A"}))["name"], "A");
    assert_eq!(run(&mut s, "grid", json!({"start": [-1000, 6000], "end": [9000, 6000]}))["name"], "B");
}

#[test]
fn inspect_summarizes_levels_counts_and_warnings() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}));
    run(&mut s, "delete", json!({"ids": [3]}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!(i["counts"]["wall"], 3);
    assert_eq!(i["levels"][0]["name"], "Level 1");
    assert!(i["warnings"][0].as_str().unwrap().contains("not enclosed"), "{i}");
    assert_eq!(i["history"], json!(["Wall", "Room", "Delete"]));
}

#[test]
fn scripts_run_line_by_line_and_name_the_line_that_fails() {
    let mut s = Session::new();
    let out =
        s.script("# a box\nwall {\"points\": [[0,0],[4000,0],[4000,3000],[0,3000]], \"closed\": true}\n\nroom {\"point\": [2000,1500]}\n");
    assert_eq!(out.map(|v| v.len()), Ok(2));
    let e = s.script("level {\"elevation\": 9000}\nnope\n").unwrap_err();
    assert!(e.contains("line 2"), "{e}");
    assert_eq!(count(&mut s, "level"), 3, "the line before the failure stays done");
}

#[test]
fn dimensions_measure_between_parallel_walls_and_grids_and_follow_them() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let (right, left) = (ids[1], ids[3]);
    let grid = run(&mut s, "grid", json!({"start": [3000, -2000], "end": [3000, 8000]}))["id"].as_u64().unwrap();
    // Centre line to grid to centre line: a chain of two, west to east whatever order the refs come in.
    let r = run(&mut s, "DI", json!({"refs": [{"id": right, "face": "centre"}, {"id": grid}, {"id": left}], "at": [0, -1000]}));
    let dim = r["id"].as_u64().unwrap();
    assert_eq!(r["values"], json!([3000.0, 5000.0]));
    // Faces, by points on them. The left wall runs from [0, 6000] down to [0, 0]: its left face is inside.
    let inside = run(&mut s, "dimension", json!({"refs": [[100, 3000], [7900, 3000]], "at": [0, 3000]}));
    assert_eq!(inside["values"], json!([7800.0]));
    let refs = element(&mut s, inside["id"].as_u64().unwrap())["refs"].clone();
    assert_eq!(refs, json!([{"id": left, "face": "left"}, {"id": right, "face": "left"}]));
    // The right wall moves: what measures to it follows.
    run(&mut s, "move", json!({"ids": [right], "by": [1000, 0]}));
    assert_eq!(element(&mut s, dim)["values"], json!([3000.0, 6000.0]));
    // References stay parallel: a turn that breaks that is refused, and so is a dimension across.
    let e = s.execute("rotate", &json!({"ids": [right], "about": [9000, 3000], "angle": 10})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [{"id": left}, {"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [{"id": grid, "face": "left"}, {"id": left}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("face"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [[3000, 3000], [5000, 3000]], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("no wall"), "{e}");
    // Deleting what it measures: the grid drops out of the chain; with one reference left, it goes.
    run(&mut s, "delete", json!({"ids": [grid]}));
    assert_eq!(element(&mut s, dim)["values"], json!([9000.0]));
    let d = run(&mut s, "delete", json!({"ids": [right]}));
    assert!(d["deleted"].as_array().unwrap().contains(&json!(dim)), "{d}");
    assert_eq!(count(&mut s, "dimension"), 0);
}

#[test]
fn mirrored_walls_keep_their_dimensions_on_the_same_faces() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // The bottom wall's outer face (its right, going east) to the top wall's centre line.
    let dim = run(&mut s, "DI", json!({"refs": [[4000, -100], [4000, 6000]], "at": [-1000, 0]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, dim)["values"], json!([6100.0]));
    // The whole box mirrored across x = 10000: the bottom wall now runs west, its outer face on its left.
    run(&mut s, "mirror", json!({"ids": [ids[0], ids[1], ids[2], ids[3], dim], "axis": [[10000, 0], [10000, 1000]]}));
    assert_eq!(element(&mut s, dim)["values"], json!([6100.0]));
    assert_eq!(element(&mut s, dim)["refs"][0], json!({"id": ids[0], "face": "left"}));
}

#[test]
fn tags_name_doors_and_windows_and_go_where_they_go() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "window", json!({"at": [8000, 3000], "mark": ""}));
    // Beside the wall, on the side the door doesn't swing to: outside, below the bottom wall.
    let t = run(&mut s, "TG", json!({"ids": [door]}))["ids"][0].as_u64().unwrap();
    assert_eq!((element(&mut s, t)["text"].clone(), element(&mut s, t)["at"].clone()), (json!("1"), json!([2000.0, -500.0])));
    // Without ids, the doors and windows not tagged yet: Tag All. A window with no mark shows `?`.
    let all = run(&mut s, "tag", json!({}))["ids"].clone();
    assert_eq!(all.as_array().map(Vec::len), Some(1));
    assert_eq!(element(&mut s, all[0].as_u64().unwrap())["text"], "?");
    assert!(s.execute("tag", &json!({})).unwrap_err().contains("nothing to tag"));
    assert!(s.execute("tag", &json!({"ids": [ids[0]]})).unwrap_err().contains("door"));
    // A tag goes with its door's wall, and on its own wherever it's moved.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [0, -1000]}));
    assert_eq!(element(&mut s, t)["at"], json!([2000.0, -1500.0]));
    run(&mut s, "move", json!({"ids": [t], "by": [300, 0]}));
    assert_eq!(element(&mut s, t)["at"], json!([2300.0, -1500.0]));
    // Copied with the wall, the tag names the door's copy.
    let made = run(&mut s, "copy", json!({"ids": [ids[0], t], "by": [0, -3000]}))["ids"].clone();
    let twin = element(&mut s, made.as_array().unwrap().last().unwrap().as_u64().unwrap());
    assert_eq!((twin["category"].as_str(), twin["text"].as_str(), twin["at"].clone()), (Some("tag"), Some("2"), json!([2300.0, -4500.0])));
    // Deleting a door takes its tags.
    run(&mut s, "delete", json!({"ids": [door]}));
    assert_eq!(count(&mut s, "tag"), 2);
}

#[test]
fn hostile_params_never_panic_or_break_the_model() {
    let hostile = [
        json!(null),
        json!([]),
        json!("x"),
        json!({"points": "x"}),
        json!({"points": [[0, 0]]}),
        json!({"points": [[0, 0], [1e308, 1e308]]}),
        json!({"points": [[0, 0], [-1e300, 0]], "closed": true}),
        json!({"at": [1e300, -1e300]}),
        json!({"wall": -1, "offset": 1e300}),
        json!({"wall": 3, "offset": -5e8, "sill": 1e9}),
        json!({"id": u64::MAX}),
        json!({"ids": [0, 1, 2, 3, 3]}),
        json!({"point": [f64::MAX, 0]}),
        json!({"elevation": "high"}),
        json!({"elevation": 1e300}),
        json!({"category": "wall", "name": "", "thickness": -5}),
        json!({"category": "window", "name": "W", "width": 1e12, "height": 1}),
        json!({"boundary": [[0, 0], [1, 0], [0, 0]]}),
        json!({"start": [0, 0], "end": [0, 0]}),
        json!({"level": "nowhere"}),
        json!({"id": 3, "level": [], "start": "x"}),
        json!({"id": 3, "top_level": 3}),
        json!({"limit": -1, "offset": 1e30}),
        json!({"ids": [3, 4], "by": [9.99e8, 0]}),
        json!({"ids": [3, 4, 5, 6], "by": [f64::MAX, 1]}),
        json!({"ids": [4], "by": "x"}),
        json!({"ids": [3, 4], "about": [0, 0], "angle": 1e300}),
        json!({"ids": [3, 4], "about": [0, 0], "angle": 45, "copy": true}),
        json!({"ids": [3, 4], "axis": [[0, 0], [0, 0]]}),
        json!({"ids": [3, 5], "axis": [[0, 0], [1, 1e300]], "copy": true}),
        json!({"ids": [3, 5], "keep": "x"}),
        json!({"ids": [3, 4], "keep": [[0, 0]]}),
        json!({"id": 3, "at": [1e300, 0]}),
        json!({"id": 3, "at": [0, 0]}),
        json!({"refs": "x", "at": [0, 0]}),
        json!({"refs": [[0, 0], [8000, 0]], "at": [1e300, 0]}),
        json!({"refs": [{"id": 3}, {"id": 3}], "at": [0, 0]}),
        json!({"refs": [{"id": 3}, {"id": 4}], "at": [0, 0]}),
        json!({"refs": [{"id": 3, "face": "up"}, {"id": 5}], "at": [0, 0]}),
        json!({"refs": [{"id": u64::MAX}, [0, 6000]], "at": [0, 0]}),
        json!({"ids": [3], "offset": [1e300, 0]}),
        json!({"ids": [3, 4], "leader": 5}),
        json!({"view": "{3D}", "azimuth": 1e300}),
        json!({"view": "{3D}", "elevation": "x", "width": 1e9}),
        json!({"facing": "up"}),
        json!({"at": [1e300, 0]}),
        json!({"start": [0, 0], "end": [1000, 0], "depth": -5}),
        json!({"start": [0, 0], "end": [1000, 0], "name": ""}),
        json!({"sheet": 3, "view": 1}),
        json!({"sheet": "A101", "view": "nowhere", "scale": 0}),
        json!({"size": "A9", "number": ""}),
        json!({"view": 1, "scale": 1e300, "at": [1e300, 0]}),
        json!({"at": [0, 0], "height": -5, "angle": 1e300}),
        json!({"start": [0, 0], "end": [0, 0], "width": 0, "tread": 1e300}),
        json!({"slope": 89.99, "overhang": -1e9}),
        json!({"slope": 1e300, "boundary": [[0, 0], [1, 0], [0, 0]]}),
        json!({"id": 1, "view_range": {"cut": 1e300}}),
        json!({"id": 1, "view_range": {"cut": -5, "bottom": 10, "top": "x"}}),
        json!({"id": 1, "view_range": [1, 2]}),
        json!({"id": 1, "crop": [[5, 5], [0, 0]]}),
        json!({"view": 1, "region": [[0, 0], [0, 0]]}),
        json!({"view": 1, "region": "x"}),
        json!({"view": 1, "region": [[-1e300, 0], [1e300, 1]]}),
        json!({"view": "{3D}", "region": null}),
        json!({"text": "", "at": [0, 0]}),
        json!({"text": "x", "at": [0, 0], "size": 1e300, "angle": f64::MAX, "leader": [1e300, 0]}),
        json!({"text": "x", "at": [0, 0], "view": 3}),
        json!({"at": [4000, 3000], "view": 3, "host": u64::MAX}),
        json!({"kind": "angular", "refs": [{"id": 3}, {"id": 5}], "at": [0, 0]}),
        json!({"kind": "angular", "refs": [{"id": 3}, {"id": 4}, {"id": 6}], "at": [0, 0]}),
        json!({"kind": "radial", "refs": [{"id": 3}], "at": [0, 0]}),
        json!({"kind": 7, "refs": [{"id": 3}, {"id": 4, "face": "end"}], "at": [0, 0]}),
        json!({"sheet": "A101", "view": "Room Schedule", "scale": -1}),
        json!({"sheet": "A101", "view": "{3D}", "azimuth": 1e300, "elevation": 90}),
        json!({"id": 1, "name": "{3D}"}),
        json!({"length": 5}),
        json!({"length": "parsecs"}),
        json!({"length": "ft-in", "width": "10'"}),
        json!({"code": 5}),
        json!({"code": "klingon"}),
        json!({"language": "xx", "name": ""}),
    ];
    for cmd in commands().iter().filter(|c| !matches!(c.id, "open" | "save")) {
        for p in &hostile {
            let mut s = Session::new();
            house(&mut s);
            let _ = s.execute(cmd.id, p);
            assert_eq!(s.model.check(), Ok(()), "{} {p} left a broken model", cmd.id);
        }
    }
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-test-{}-{name}", std::process::id()))
}

#[test]
fn plans_have_a_view_range_that_modify_merges_into() {
    let mut s = Session::new();
    house(&mut s);
    // Revit's: cut 1200 mm up, the top at the level above, the bottom and the view depth at the level.
    assert_eq!(element(&mut s, 1)["view_range"], json!({"top": null, "cut": 1200.0, "bottom": 0.0, "depth": 0.0}));
    run(&mut s, "modify", json!({"id": 1, "view_range": {"cut": 1500, "top": 2800}}));
    run(&mut s, "modify", json!({"id": 1, "view_range": {"depth": -500}}));
    assert_eq!(element(&mut s, 1)["view_range"], json!({"top": 2800.0, "cut": 1500.0, "bottom": 0.0, "depth": -500.0}));
    // Back to the level above, field by field.
    run(&mut s, "modify", json!({"id": 1, "view_range": {"top": null}}));
    assert_eq!(element(&mut s, 1)["view_range"]["top"], Value::Null);
    // Top ≥ cut plane ≥ bottom ≥ view depth.
    let e = s.execute("modify", &json!({"id": 1, "view_range": {"bottom": 2000}})).unwrap_err();
    assert!(e.contains("view range"), "{e}");
    // Rooms keep to their walls whatever a plan cuts: raised over the walls, the cut plane leaves the room as it was.
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": 1, "view_range": {"cut": 3500, "top": 4000}}));
    assert_eq!(element(&mut s, room)["area_m2"], 48.0);
}

#[test]
fn crop_regions_frame_plans_and_sections() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "sheet", json!({}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}))["id"].as_u64().unwrap();
    let whole = element(&mut s, vp)["size"][0].as_f64().unwrap();
    // A plan cropped to its west half takes half the paper.
    run(&mut s, "crop", json!({"view": "Level 1", "region": [[-500, -500], [4000, 6500]]}));
    assert_eq!(element(&mut s, 1)["crop"], json!([[-500.0, -500.0], [4000.0, 6500.0]]));
    let half = element(&mut s, vp)["size"][0].as_f64().unwrap();
    assert!(half < whole - 30.0, "{half} {whole}");
    // Null uncrops; no region crops to all it shows.
    run(&mut s, "crop", json!({"view": "Level 1", "region": null}));
    assert_eq!(element(&mut s, 1)["crop"], Value::Null);
    run(&mut s, "crop", json!({"view": 1}));
    let c = element(&mut s, 1)["crop"].clone();
    assert!(c[0][0].as_f64().unwrap() <= -100.0 && c[1][1].as_f64().unwrap() >= 6100.0, "{c}");
    // A section's crop region is its line's ends, along it, and the heights it shows; what's noted
    // in it stays over the same spot.
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let note = run(&mut s, "text", json!({"text": "Insulated", "at": [3000, 1000], "view": sec}))["id"].as_u64().unwrap();
    run(&mut s, "crop", json!({"view": "Section 1", "region": [[500, -200], [6000, 2500]]}));
    let e = element(&mut s, sec);
    assert_eq!(
        (e["start"].clone(), e["end"].clone(), e["crop"].clone()),
        (json!([-500.0, 3000.0]), json!([5000.0, 3000.0]), json!([-200.0, 2500.0]))
    );
    assert_eq!(element(&mut s, note)["at"], json!([2500.0, 1000.0]));
    // Corners the wrong way round, and views that don't crop, are refused.
    assert!(s.execute("crop", &json!({"view": 1, "region": [[4000, 0], [0, 4000]]})).unwrap_err().contains("south-west"));
    assert!(s.execute("crop", &json!({"view": "A101"})).unwrap_err().contains("A101"));
}

#[test]
fn text_notes_go_in_plans_sections_and_sheets() {
    let mut s = Session::new();
    house(&mut s);
    // In the lowest level's plan by default, 2.5 mm capitals on paper.
    let n = run(&mut s, "TX", json!({"text": "Tiled floor\nfalls to the drain", "at": [1000, 5000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, n);
    assert_eq!((e["view"].as_u64(), e["size"].as_f64(), e["level"].as_u64()), (Some(1), Some(2.5), Some(1)));
    // In a section along it and up, with a leader; on a sheet, in paper mm.
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    run(&mut s, "sheet", json!({}));
    let cut = run(&mut s, "text", json!({"text": "Insulated", "at": [500, 2500], "view": "Section 1", "leader": [1000, 1500]}));
    assert_eq!(element(&mut s, cut["id"].as_u64().unwrap())["leader"], json!([1000.0, 1500.0]));
    run(&mut s, "text", json!({"text": "General notes", "at": [30, 280], "view": "A101", "size": 5}));
    for (p, what) in [
        (json!({"text": " ", "at": [0, 0]}), "text"),
        (json!({"text": "x", "at": [0, 0], "view": "Nowhere"}), "Nowhere"),
        (json!({"text": "x", "at": [0, 0], "size": 0}), "size"),
        (json!({"text": "x"}), "at"),
    ] {
        let e = s.execute("text", &p).unwrap_err();
        assert!(e.contains(what), "{p}: {e}");
    }
    // A note moves like anything else, and goes with its view.
    run(&mut s, "move", json!({"ids": [n], "by": [500, 0]}));
    assert_eq!(element(&mut s, n)["at"], json!([1500.0, 5000.0]));
    run(&mut s, "delete", json!({"ids": [sec]}));
    assert_eq!(count(&mut s, "text"), 2);
}

#[test]
fn dimensions_measure_to_wall_ends_and_openings() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    // From the bottom wall's west end (its outline reaches the corner, 100 mm past the centre line)
    // to the door's centre and its jamb toward the wall's end.
    let r =
        run(&mut s, "DI", json!({"refs": [{"id": ids[0], "face": "start"}, {"id": door}, {"id": door, "face": "end"}], "at": [0, -1000]}));
    assert_eq!(r["values"], json!([2100.0, 450.0]));
    let dim = r["id"].as_u64().unwrap();
    // By points: the door's other jamb, then the bottom wall's east end.
    let r = run(&mut s, "dimension", json!({"refs": [[1550, 50], [8100, 50]], "at": [0, -1500]}));
    assert_eq!(r["values"], json!([6550.0]));
    // The door slides along: what measures to it follows.
    run(&mut s, "move", json!({"ids": [door], "by": [500, 0]}));
    assert_eq!(element(&mut s, dim)["values"], json!([2600.0, 450.0]));
    let e = s.execute("dimension", &json!({"refs": [{"id": door, "face": "left"}, {"id": ids[1]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("jambs"), "{e}");
}

#[test]
fn angular_and_radial_dimensions_measure_angles_and_radii() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let slant = run(&mut s, "wall", json!({"points": [[0, 0], [3000, 3000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    // Between the bottom wall and the slanted one, the angle on `at`'s side.
    let r = run(&mut s, "dimension", json!({"kind": "angular", "refs": [{"id": ids[0]}, {"id": slant}], "at": [2000, 800]}));
    assert_eq!(r["values"], json!([45.0]));
    let r = run(&mut s, "dimension", json!({"kind": "angular", "refs": [[4000, 0], [1500, 1500]], "at": [-2000, 800]}));
    assert_eq!(r["values"], json!([135.0]));
    let e = s.execute("dimension", &json!({"kind": "angular", "refs": [{"id": ids[0]}, {"id": ids[2]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"kind": "spiral", "refs": [{"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("radial"), "{e}");
    // A round column's radius, by id or by a point on its edge; walls have none.
    let col = run(&mut s, "column", json!({"at": [5000, 3000], "type": "Round 300"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "dimension", json!({"kind": "radial", "refs": [{"id": col}], "at": [5600, 3600]}));
    assert_eq!(r["values"], json!([150.0]));
    let radius = r["id"].as_u64().unwrap();
    let r = run(&mut s, "dimension", json!({"kind": "radial", "refs": [[5150, 3000]], "at": [5600, 2400]}));
    assert_eq!(r["values"], json!([150.0]));
    let e = s.execute("dimension", &json!({"kind": "radial", "refs": [{"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("round"), "{e}");
    // It follows its column's type.
    run(&mut s, "type", json!({"category": "column", "name": "Round 300", "width": 500}));
    assert_eq!(element(&mut s, radius)["values"], json!([250.0]));
}

#[test]
fn spot_elevations_read_the_height_of_what_they_stand_on() {
    let mut s = Session::new();
    house(&mut s);
    let floor = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    run(&mut s, "roof", json!({}));
    // In Level 1's plan, the top of the highest thing standing on the level there: the floor.
    let spot = run(&mut s, "EL", json!({"at": [4000, 3000]}));
    assert_eq!((spot["elevation"].as_f64(), spot["host"].as_u64()), (Some(0.0), Some(floor)));
    // In Level 2's, the roof's top 100 mm short of its ridge: 3000 mm in from the walls' faces at 30°, and its thickness upright.
    let roof = run(&mut s, "spot", json!({"at": [4000, 2900], "view": "Level 2"}));
    let (tan, cos) = (30f64.to_radians().tan(), 30f64.to_radians().cos());
    assert!((roof["elevation"].as_f64().unwrap() - (3000.0 + 3000.0 * tan + 300.0 / cos)).abs() < 0.01, "{roof}");
    // In a section, the height of its point.
    run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    assert_eq!(run(&mut s, "spot", json!({"at": [1000, 2700], "view": "Section 1"}))["elevation"], 2700.0);
    assert!(s.execute("spot", &json!({"at": [20000, 0]})).unwrap_err().contains("nothing"));
    // It follows the model: the level goes up, the floor's top with it; the floor goes, and its spot.
    run(&mut s, "modify", json!({"id": 1, "elevation": 100}));
    assert_eq!(element(&mut s, spot["id"].as_u64().unwrap())["elevation"], 100.0);
    run(&mut s, "delete", json!({"ids": [floor]}));
    assert_eq!(count(&mut s, "spot"), 2);
}

#[test]
fn schedules_and_the_3d_view_go_on_sheets() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "sheet", json!({}));
    // A schedule by its name: its view comes to be as it's placed.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Door Schedule"}))["id"].as_u64().unwrap();
    let all = run(&mut s, "elements", json!({"category": "schedule"}));
    assert_eq!((all["count"].as_u64(), all["elements"][0]["of"].as_str()), (Some(1), Some("door")));
    assert_eq!(element(&mut s, vp)["title"], "Door Schedule");
    // The 3D view, from where it's asked to look.
    let v3 = run(&mut s, "place", json!({"sheet": "A101", "view": "{3D}", "azimuth": 30, "elevation": 20}))["id"].as_u64().unwrap();
    let view = element(&mut s, v3)["view"].as_u64().unwrap();
    assert_eq!((element(&mut s, view)["azimuth"].clone(), element(&mut s, view)["elevation"].clone()), (json!(30.0), json!(20.0)));
    // On one sheet each, as every view; a schedule that isn't one is refused.
    run(&mut s, "sheet", json!({}));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "{3D}"})).unwrap_err().contains("A101"));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "Furniture Schedule"})).unwrap_err().contains("Furniture"));
    // Printed with the rest; saved and opened again, the same.
    let pdf = temp("schedules.pdf");
    run(&mut s, "save", json!({"path": pdf}));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    let project = temp("schedules.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let saved = s.model.clone();
    run(&mut s, "open", json!({"path": project}));
    assert_eq!(*s.model, *saved);
    let _ = (std::fs::remove_file(pdf), std::fs::remove_file(project));
}

#[test]
fn base64_matches_the_standard_vectors() {
    use crate::commands::base64;
    assert_eq!([base64(b""), base64(b"M"), base64(b"Ma"), base64(b"Man")], ["", "TQ==", "TWE=", "TWFu"]);
}

#[test]
fn render_returns_a_png_or_writes_svg_and_png_files() {
    let mut s = Session::new();
    house(&mut s);
    let r = run(&mut s, "render", json!({"width": 320, "height": 200}));
    assert!(r["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    assert_eq!((r["width"].as_u64(), r["view"].as_str()), (Some(320), Some("Level 1")));
    let svg = temp("plan.svg");
    run(&mut s, "render", json!({"view": "Level 1", "path": svg}));
    assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
    let png = temp("plan.png");
    run(&mut s, "save", json!({"path": png}));
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    assert_eq!(s.path, None, "exporting an image doesn't make it the project's file");
    let _ = (std::fs::remove_file(svg), std::fs::remove_file(png));
}

#[test]
fn render_draws_the_3d_view_too() {
    let mut s = Session::new();
    house(&mut s);
    let r = run(&mut s, "render", json!({"view": "{3D}", "width": 320, "height": 200, "azimuth": 30, "elevation": 45}));
    assert!(r["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    assert_eq!((r["width"].as_u64(), r["view"].as_str()), (Some(320), Some("{3D}")));
    let e = s.execute("render", &json!({"view": "{3D}", "path": temp("view.svg")})).unwrap_err();
    assert!(e.contains(".png"), "{e}");
}

#[test]
fn sections_and_elevations_are_views_along_a_line() {
    let mut s = Session::new();
    house(&mut s);
    // A section drawn west to east looks north: to the left of its line.
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, section);
    assert_eq!((e["name"].as_str(), e["looking"].clone(), e["length"].clone()), (Some("Section 1"), json!([0.0, 1.0]), json!(10000.0)));
    // The four building elevations, each outside the walls on its side, looking at them.
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let ids: Vec<u64> = ids.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let names: Vec<Value> = ids.iter().map(|id| element(&mut s, *id)["name"].clone()).collect();
    assert_eq!(names, ["North", "South", "East", "West"]);
    let north = element(&mut s, ids[0]);
    assert_eq!(north["looking"], json!([0.0, -1.0]), "the North elevation shows the north side, looking south");
    assert!(north["start"][1].as_f64().unwrap() > 6100.0, "{north}");
    assert!(s.execute("elevation", &json!({})).unwrap_err().contains("already"));
    // One placed south of the house looks north at it.
    let south = run(&mut s, "elevation", json!({"at": [4000, -5000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, south);
    assert_eq!((e["name"].as_str(), e["looking"].clone()), (Some("South 2"), json!([0.0, 1.0])));
    // A view's name is its own: no other view's, nor a level's.
    for name in ["North", "Level 1"] {
        let e = s.execute("section", &json!({"start": [0, 0], "end": [1000, 0], "name": name})).unwrap_err();
        assert!(e.contains("named"), "{e}");
    }
    // Mirrored, a section looks the mirrored way.
    run(&mut s, "mirror", json!({"ids": [section], "axis": [[0, 0], [1000, 0]]}));
    assert_eq!(element(&mut s, section)["looking"], json!([0.0, -1.0]));
    // Each renders as a PNG; only plans make SVGs.
    let png = run(&mut s, "render", json!({"view": "Section 1", "width": 400, "height": 300}));
    assert!(png["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    let e = s.execute("render", &json!({"view": "North", "path": temp("north.svg")})).unwrap_err();
    assert!(e.contains(".png"), "{e}");
}

#[test]
fn sheets_hold_views_at_a_scale_and_print_to_pdf() {
    let mut s = Session::new();
    house(&mut s);
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    let e = element(&mut s, sheet);
    assert_eq!((e["number"].as_str(), e["name"].as_str(), e["paper"].clone()), (Some("A101"), Some("Unnamed"), json!([420.0, 297.0])));
    // Level 1's plan at 1 : 100, by the level's name: its 10 m extent is 100 mm wide, with room around it.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}))["id"].as_u64().unwrap();
    let v = element(&mut s, vp);
    assert_eq!((v["view"].as_u64(), v["scale"].as_f64(), v["title"].as_str()), (Some(1), Some(100.0), Some("Level 1")));
    let w = v["size"][0].as_f64().unwrap();
    assert!((100.0..140.0).contains(&w), "{w}");
    // A view goes on one sheet only.
    let e = s.execute("place", &json!({"sheet": "A101", "view": 1})).unwrap_err();
    assert!(e.contains("A101"), "{e}");
    // The section on a second sheet, at 1 : 50.
    let two = run(&mut s, "sheet", json!({"name": "Sections", "size": "A2"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, two)["number"], "A102");
    run(&mut s, "place", json!({"sheet": two, "view": section, "scale": 50}));
    assert!(s.execute("sheet", &json!({"size": "B5"})).unwrap_err().contains("A4"));
    // A sheet renders as a PNG, or a PDF page; saving to .pdf prints every sheet, a page each.
    assert!(run(&mut s, "render", json!({"view": "A101"}))["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    let pdf = temp("set.pdf");
    run(&mut s, "save", json!({"path": pdf}));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(bytes.windows(6).filter(|w| *w == b"/Page ").count(), 2, "a page a sheet");
    // Deleting a view takes it off its sheet; deleting a sheet takes its views off.
    run(&mut s, "delete", json!({"ids": [section]}));
    assert_eq!(count(&mut s, "viewport"), 1);
    run(&mut s, "delete", json!({"ids": [sheet]}));
    assert_eq!(count(&mut s, "viewport"), 0);
    let _ = std::fs::remove_file(pdf);
}

#[test]
fn columns_stand_from_their_level_to_the_next_and_turn_with_rotate() {
    let mut s = Session::new();
    house(&mut s);
    let c = run(&mut s, "CL", json!({"at": [4000, 3000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, c);
    assert_eq!((e["type"].as_str(), e["height"].as_f64(), e["width"].as_f64()), (Some("Rectangular 400 x 400"), Some(3000.0), Some(400.0)));
    let round = run(&mut s, "column", json!({"at": [2000, 3000], "type": "Round 300", "height": 2500}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, round)["height"], 2500.0);
    run(&mut s, "rotate", json!({"ids": [c], "about": [4000, 3000], "angle": 45}));
    assert!((element(&mut s, c)["angle"].as_f64().unwrap() - 45.0).abs() < 1e-6);
    run(&mut s, "type", json!({"category": "column", "name": "Steel 200 x 300", "width": 200, "depth": 300}));
    run(&mut s, "modify", json!({"id": c, "type": "Steel 200 x 300"}));
    assert_eq!(element(&mut s, c)["depth"], 300.0);
    assert!(s.execute("column", &json!({"at": [0, 0], "type": "Marble"})).unwrap_err().contains("Marble"));
}

#[test]
fn stairs_climb_in_risers_no_higher_than_their_riser() {
    let mut s = Session::new();
    house(&mut s);
    // From [1000, 1000] toward north: 3 m in risers of at most 180 mm is 17 of 176.47 mm, 16 treads of 280 mm.
    let st = run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, st);
    assert_eq!((e["risers"].as_u64(), e["length"].as_f64(), e["angle"].as_f64()), (Some(17), Some(4480.0), Some(90.0)));
    assert!((e["riser_height"].as_f64().unwrap() - 176.47).abs() < 0.01, "{e}");
    assert!(near(&e["end"], [1000.0, 5480.0]), "{e}");
    let e = s.execute("stair", &json!({"start": [0, 0], "end": [1000, 0], "riser": 1})).unwrap_err();
    assert!(e.contains("risers"), "{e}");
    // Moved and turned like anything else.
    run(&mut s, "move", json!({"ids": [st], "by": [500, 0]}));
    assert_eq!(element(&mut s, st)["start"], json!([1500.0, 1000.0]));
}

#[test]
fn roofs_cover_the_walls_below_flat_or_pitched() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]]}));
    // Over the walls of Level 1, on Level 2: hipped at 30°, 500 mm past the walls' faces; its four
    // corners, from the south-west, and none where the partition meets the outer walls.
    let roof = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    let e = element(&mut s, roof);
    assert_eq!((e["level"].as_u64(), e["slope"].as_f64(), e["overhang"].as_f64()), (Some(2), Some(30.0), Some(500.0)));
    assert!(near(&e["boundary"][0], [-600.0, -600.0]) && e["boundary"].as_array().map(Vec::len) == Some(4), "{e}");
    // The ridge: 3100 mm in from the faces at 30°, above Level 2.
    assert!((e["ridge"].as_f64().unwrap() - (3000.0 + 3100.0 * 30f64.to_radians().tan())).abs() < 0.01, "{e}");
    let flat = run(&mut s, "roof", json!({"slope": 0, "boundary": [[0, 0], [4000, 0], [2000, 3000]]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, flat)["ridge"], 3000.0);
    let e = s.execute("roof", &json!({"boundary": [[0, 0], [4000, 0], [2000, 3000]]})).unwrap_err();
    assert!(e.contains("rectangular"), "{e}");
    // In IFC: a column, a stair and the roofs.
    run(&mut s, "column", json!({"at": [4000, 3000]}));
    run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000]}));
    let path = temp("roofed.ifc");
    run(&mut s, "save", json!({"path": path}));
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("IFCCOLUMN(") && ifc.contains("IFCSTAIR(") && ifc.matches("IFCROOF(").count() == 2, "{ifc}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn schedules_come_back_as_rows_or_csv_files() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}));
    let t = run(&mut s, "schedule", json!({"category": "rooms"}));
    assert_eq!(t["rows"][0][1], "Living");
    let csv = temp("rooms.csv");
    run(&mut s, "schedule", json!({"category": "rooms", "path": csv}));
    assert!(std::fs::read_to_string(&csv).unwrap().starts_with("Number,Name,Level"));
    let _ = std::fs::remove_file(csv);
}

#[test]
fn projects_save_and_open_unchanged() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0], "flip_hand": true}));
    let path = temp("house.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let saved = s.model.clone();
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(*t.model, *saved);
    assert_eq!(t.path.as_deref(), Some(path.as_path()));
    std::fs::write(&path, "{\"name\": 3}").unwrap();
    assert!(t.execute("open", &json!({"path": path})).unwrap_err().contains("not a BIMCraft project"));
    assert_eq!(*t.model, *saved, "a failed open keeps the open project");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sample_house_builds_with_four_rooms() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!((i["counts"]["wall"].as_u64(), i["counts"]["door"].as_u64(), i["counts"]["room"].as_u64()), (Some(7), Some(4), Some(4)));
    assert_eq!(i["warnings"], json!([]));
    assert_eq!((i["counts"]["dimension"].as_u64(), i["counts"]["tag"].as_u64()), (Some(3), Some(9)), "every door and window tagged");
    assert_eq!((i["counts"]["elevation"].as_u64(), i["counts"]["section"].as_u64()), (Some(4), Some(1)));
    assert_eq!((i["counts"]["sheet"].as_u64(), i["counts"]["viewport"].as_u64()), (Some(3), Some(6)));
    assert_eq!((i["counts"]["text"].as_u64(), i["counts"]["spot"].as_u64(), i["counts"]["schedule"].as_u64()), (Some(1), Some(1), Some(1)));
    let rooms = run(&mut s, "schedule", json!({"category": "rooms"}));
    let areas: Vec<f64> = rooms["rows"].as_array().unwrap().iter().map(|r| r[3].as_f64().unwrap()).collect();
    assert_eq!(areas, [24.0, 18.0, 14.0, 14.0]);
}

#[test]
fn saving_to_ifc_exports_without_changing_the_project_file() {
    let mut s = Session::new();
    house(&mut s);
    let path = temp("house.ifc");
    assert_eq!(run(&mut s, "save", json!({"path": path}))["format"], "IFC4");
    assert!(std::fs::read_to_string(&path).unwrap().contains("FILE_SCHEMA(('IFC4'));"));
    assert_eq!(s.path, None);
    let _ = std::fs::remove_file(path);
}

/// A value with its numbers to the millimetre, so two models that hold the same building print
/// the same.
fn mm(v: &Value) -> String {
    match v {
        Value::Number(x) => format!("{}", x.as_f64().unwrap_or(f64::NAN).round() + 0.0),
        Value::Array(a) => format!("[{}]", a.iter().map(mm).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// What an element is and where: its level by name, a door's wall by what that wall is.
fn describe(m: &Model, id: u64) -> String {
    let e = crate::element_json(m, id);
    let keys: &[&str] = match e["category"].as_str() {
        Some("wall") => &["start", "end", "thickness", "base_offset", "height", "level", "top_level"],
        Some("door" | "window") => &["host", "offset", "width", "height", "sill"],
        Some("floor") => &["level", "area_m2"],
        Some("room") => &["level", "name", "number", "area_m2"],
        Some("column") => &["level", "at", "width", "depth", "round", "height", "base_offset"],
        Some("stair") => &["level", "start", "angle", "width", "risers", "riser_height", "length"],
        Some("roof") => &["level", "slope", "gable", "ridge", "area_m2", "boundary"],
        Some("grid") => &["name", "start", "end"],
        _ => &[],
    };
    let text = |k: &str, v: &Value| match k {
        "level" | "top_level" => v.as_u64().and_then(|l| m.level(l)).map_or_else(|| "-".to_string(), |l| l.name.clone()),
        "host" => v.as_u64().map_or_else(|| "-".to_string(), |h| format!("({})", describe(m, h))),
        "boundary" => {
            let mut pts: Vec<String> = v.as_array().into_iter().flatten().map(mm).collect();
            pts.sort();
            pts.join(" ")
        }
        "area_m2" | "riser_height" | "slope" | "name" | "number" | "round" | "gable" => v.to_string(),
        _ => mm(v),
    };
    keys.iter().map(|k| format!("{k}={}", text(k, &e[*k]))).collect::<Vec<_>>().join(" ")
}

/// Every element of a category, described, in order.
fn building(m: &Model, category: &str) -> Vec<String> {
    let mut all: Vec<String> = m.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| describe(m, *id)).collect();
    all.sort();
    all
}

#[test]
fn ifc_files_open_as_the_building_they_describe() {
    // The sample, with a column and a stair, out to IFC and back in.
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    run(&mut s, "column", json!({"at": [3000, 5500], "type": "Round 300"}));
    run(&mut s, "column", json!({"at": [8000, 5000], "angle": 30}));
    run(&mut s, "stair", json!({"start": [1000, 2000], "end": [2000, 2000], "width": 900}));
    let path = temp("round-trip.ifc");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!((r["format"].as_str(), r["skipped"].clone()), (Some("IFC4"), json!([])), "{r}");
    assert_eq!(t.path, None, "an import is a new project: saving asks where");
    let levels = |m: &Model| m.levels().into_iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>();
    assert_eq!(levels(&t.model), levels(&s.model));
    for category in ["wall", "door", "window", "floor", "room", "column", "stair", "roof"] {
        let (before, after) = (building(&s.model, category), building(&t.model, category));
        assert!(!before.is_empty(), "{category}");
        assert_eq!(after, before, "{category}");
    }
    // It saves as a project like any other.
    let project = temp("imported.bimcraft");
    run(&mut t, "save", json!({"path": project}));
    let _ = std::fs::remove_file(project);
}

/// A small IFC2x3 file in metres, written the way other BIM tools write them: a storey 3 m up; a
/// wall drawn on its outer face, rotated to run north, with an axis; a partition with no axis
/// stopping at the wall's face; a window in an opening cut sideways through the wall; an L-shaped
/// slab; a room; two grids; and a desk.
const OFFICE: &str = r"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_NAME('office.ifc','2026-10-15T10:00:00',(''),(''),'hand','hand','');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
/* metres */
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCCARTESIANPOINT((0.,0.,0.));
#4=IFCAXIS2PLACEMENT3D(#3,$,$);
#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);
#6=IFCPROJECT('0000000000000000000001',$,'Small \X2\00D6\X0\ffice',$,$,$,$,(#5),#2);
#10=IFCLOCALPLACEMENT($,#4);
#11=IFCSITE('0000000000000000000002',$,'Site',$,$,#10,$,$,.ELEMENT.,$,$,$,$,$);
#12=IFCLOCALPLACEMENT(#10,#4);
#13=IFCBUILDING('0000000000000000000003',$,'Building',$,$,#12,$,$,.ELEMENT.,$,$,$);
#20=IFCLOCALPLACEMENT(#12,#4);
#21=IFCBUILDINGSTOREY('0000000000000000000004',$,'Ground',$,$,#20,$,$,.ELEMENT.,0.);
#22=IFCCARTESIANPOINT((0.,0.,3.));
#23=IFCAXIS2PLACEMENT3D(#22,$,$);
#24=IFCLOCALPLACEMENT(#12,#23);
#25=IFCBUILDINGSTOREY('0000000000000000000005',$,'First',$,$,#24,$,$,.ELEMENT.,3.);
#30=IFCCARTESIANPOINT((1.,2.,0.));
#31=IFCDIRECTION((0.,0.,1.));
#32=IFCDIRECTION((0.,1.,0.));
#33=IFCAXIS2PLACEMENT3D(#30,#31,#32);
#34=IFCLOCALPLACEMENT(#24,#33);
#35=IFCCARTESIANPOINT((0.,0.));
#36=IFCCARTESIANPOINT((5.,0.));
#37=IFCPOLYLINE((#35,#36));
#38=IFCSHAPEREPRESENTATION(#5,'Axis','Curve2D',(#37));
#39=IFCCARTESIANPOINT((0.,0.2));
#40=IFCCARTESIANPOINT((5.,0.2));
#41=IFCPOLYLINE((#35,#36,#40,#39,#35));
#42=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#41);
#43=IFCEXTRUDEDAREASOLID(#42,#4,#31,2.7);
#44=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#43));
#45=IFCPRODUCTDEFINITIONSHAPE($,$,(#38,#44));
#46=IFCWALLSTANDARDCASE('0000000000000000000006',$,'Basic Wall:Exterior:101',$,'Basic Wall:Exterior',#34,#45,'101');
#50=IFCCARTESIANPOINT((5.,4.,0.));
#51=IFCDIRECTION((-1.,0.,0.));
#52=IFCAXIS2PLACEMENT3D(#50,#31,#51);
#53=IFCLOCALPLACEMENT(#24,#52);
#54=IFCCARTESIANPOINT((2.,0.));
#55=IFCAXIS2PLACEMENT2D(#54,$);
#56=IFCRECTANGLEPROFILEDEF(.AREA.,$,#55,4.,0.1);
#57=IFCEXTRUDEDAREASOLID(#56,#4,#31,2.7);
#58=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#57));
#59=IFCPRODUCTDEFINITIONSHAPE($,$,(#58));
#60=IFCWALL('0000000000000000000007',$,'Partition',$,$,#53,#59,$);
#70=IFCCARTESIANPOINT((3.,0.3,0.9));
#71=IFCDIRECTION((0.,-1.,0.));
#72=IFCDIRECTION((1.,0.,0.));
#73=IFCAXIS2PLACEMENT3D(#70,#71,#72);
#74=IFCLOCALPLACEMENT(#34,#4);
#75=IFCCARTESIANPOINT((0.5,0.6));
#76=IFCAXIS2PLACEMENT2D(#75,$);
#77=IFCRECTANGLEPROFILEDEF(.AREA.,$,#76,1.,1.2);
#78=IFCEXTRUDEDAREASOLID(#77,#73,#31,0.4);
#79=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#78));
#80=IFCPRODUCTDEFINITIONSHAPE($,$,(#79));
#81=IFCOPENINGELEMENT('0000000000000000000008',$,'Opening',$,$,#74,#80,$);
#82=IFCRELVOIDSELEMENT('0000000000000000000009',$,$,$,#46,#81);
#83=IFCLOCALPLACEMENT(#74,#4);
#84=IFCWINDOW('000000000000000000000A',$,'Fixed:1000 x 1200:201',$,'Fixed:1000 x 1200',#83,$,'201',1.2,1.);
#85=IFCRELFILLSELEMENT('000000000000000000000B',$,$,$,#81,#84);
#90=IFCCARTESIANPOINT((1.,2.));
#91=IFCCARTESIANPOINT((5.,2.));
#92=IFCCARTESIANPOINT((5.,3.9));
#93=IFCCARTESIANPOINT((1.,3.9));
#94=IFCPOLYLINE((#90,#91,#92,#93,#90));
#95=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#94);
#96=IFCEXTRUDEDAREASOLID(#95,#4,#31,2.5);
#97=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#96));
#98=IFCPRODUCTDEFINITIONSHAPE($,$,(#97));
#99=IFCLOCALPLACEMENT(#24,#4);
#100=IFCSPACE('000000000000000000000C',$,'101',$,$,#99,#98,'Office',.ELEMENT.,.INTERNAL.,$);
#101=IFCRELAGGREGATES('000000000000000000000D',$,$,$,#25,(#100));
#110=IFCCARTESIANPOINT((0.,0.,-0.25));
#111=IFCAXIS2PLACEMENT3D(#110,$,$);
#112=IFCCARTESIANPOINT((0.8,1.9));
#113=IFCCARTESIANPOINT((5.2,1.9));
#114=IFCCARTESIANPOINT((5.2,4.1));
#115=IFCCARTESIANPOINT((2.,4.1));
#116=IFCCARTESIANPOINT((2.,7.1));
#117=IFCCARTESIANPOINT((0.8,7.1));
#118=IFCPOLYLINE((#112,#113,#114,#115,#116,#117,#112));
#119=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#118);
#120=IFCEXTRUDEDAREASOLID(#119,#111,#31,0.25);
#121=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#120));
#122=IFCPRODUCTDEFINITIONSHAPE($,$,(#121));
#123=IFCLOCALPLACEMENT(#24,#4);
#124=IFCSLAB('000000000000000000000E',$,'Floor:Generic 250mm',$,'Floor:Generic 250mm',#123,#122,$,.FLOOR.);
#130=IFCCARTESIANPOINT((0.,-1.));
#131=IFCCARTESIANPOINT((0.,8.));
#132=IFCPOLYLINE((#130,#131));
#133=IFCGRIDAXIS('1',#132,.T.);
#134=IFCCARTESIANPOINT((-1.,0.));
#135=IFCCARTESIANPOINT((6.,0.));
#136=IFCPOLYLINE((#134,#135));
#137=IFCGRIDAXIS('A',#136,.T.);
#138=IFCLOCALPLACEMENT(#12,#4);
#139=IFCGRID('000000000000000000000F',$,'Grid',$,$,#138,$,(#133),(#137),$);
#140=IFCFURNISHINGELEMENT('000000000000000000000G',$,'Desk',$,$,#99,$,$);
#150=IFCRELCONTAINEDINSPATIALSTRUCTURE('000000000000000000000H',$,$,$,(#46,#60,#84,#124,#140),#25);
#151=IFCRELCONTAINEDINSPATIALSTRUCTURE('000000000000000000000I',$,$,$,(#139),#21);
#152=IFCRELAGGREGATES('000000000000000000000J',$,$,$,#13,(#21,#25));
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn ifc_files_from_other_tools_come_in_measured_from_their_geometry() {
    let path = temp("office.ifc");
    std::fs::write(&path, OFFICE).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!((r["format"].as_str(), r["name"].as_str()), (Some("IFC2X3"), Some("Small Öffice")), "{r}");
    let m = &s.model;
    assert_eq!(
        m.levels().into_iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>(),
        [("Ground".into(), 0.0), ("First".into(), 3000.0)]
    );
    // In mm; the wall's centre line 100 mm in from the face it was drawn on; the partition carried on
    // from that face to the wall's centre line, so the two meet.
    assert_eq!(
        building(m, "wall"),
        [
            "start=[5000,4000] end=[900,4000] thickness=100 base_offset=0 height=2700 level=First top_level=-",
            "start=[900,2000] end=[900,7000] thickness=200 base_offset=0 height=2700 level=First top_level=-",
        ]
    );
    assert_eq!(
        building(m, "window"),
        [
            "host=(start=[900,2000] end=[900,7000] thickness=200 base_offset=0 height=2700 level=First top_level=-) offset=3500 width=1000 height=1200 sill=900"
        ]
    );
    assert_eq!(building(m, "floor"), ["level=First area_m2=13.28"]);
    assert_eq!(building(m, "grid"), ["name=\"1\" start=[0,-1000] end=[0,8000]", "name=\"A\" start=[-1000,0] end=[6000,0]"]);
    let room = m.elements.values().find_map(|e| match e {
        crate::model::Element::Room(r) => Some(r.clone()),
        _ => None,
    });
    let room = room.unwrap();
    assert_eq!((room.name.as_str(), room.number.as_str(), m.level(room.level).map(|l| l.name.as_str())), ("Office", "101", Some("First")));
    assert!((1000.0..5000.0).contains(&room.point.0) && (2000.0..3900.0).contains(&room.point.1), "{:?}", room.point);
    // What BIMCraft has no element for is left out, and said so.
    let skipped = r["skipped"].to_string();
    assert!(skipped.contains("IFCFURNISHINGELEMENT"), "{skipped}");
}

#[test]
fn broken_or_hostile_ifc_files_fail_cleanly() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let before = s.model.clone();
    let path = temp("broken.ifc");
    let cases = [
        "this is not STEP at all",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCWALL('x',$,(((((;\n",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('x',$,'unterminated);\nENDSEC;\n",
    ];
    for text in cases {
        std::fs::write(&path, text).unwrap();
        let e = s.execute("open", &json!({"path": path})).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(*s.model, *before, "a failed import keeps the open project: {text}");
    }
    // Nested a thousand deep, and placements that point at each other: an error or a model, never a hang or a crash.
    let deep = format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCX({}1{});\nENDSEC;\n",
        "(".repeat(1000),
        ")".repeat(1000)
    );
    let cycle = OFFICE.replace("#24=IFCLOCALPLACEMENT(#12,#23);", "#24=IFCLOCALPLACEMENT(#34,#23);");
    for text in [deep, cycle] {
        std::fs::write(&path, &text).unwrap();
        let _ = s.execute("open", &json!({"path": path}));
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn project_units_change_what_is_shown_not_the_model() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // Millimetres by default; feet and inches on request, an undo step like any change.
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "mm");
    assert_eq!(run(&mut s, "UN", json!({"length": "ft-in"}))["length"], "ft-in");
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "ft-in");
    // The model, and every command, keep millimetres.
    assert_eq!(element(&mut s, ids[0])["length"], 8000.0);
    assert!(s.execute("units", &json!({"length": "furlongs"})).unwrap_err().contains("ft-in"));
    run(&mut s, "undo", json!({}));
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "mm");
    // Saved with the project.
    run(&mut s, "units", json!({"length": "m"}));
    let path = temp("units.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(run(&mut t, "inspect", json!({}))["project_units"], "m");
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_project_in_portuguese_names_what_it_makes_in_portuguese() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"name": "Casa", "language": "pt"}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!((i["levels"][0]["name"].clone(), i["language"].clone()), (json!("Nível 1"), json!("pt")));
    house(&mut s);
    assert_eq!(run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["name"], "Corte 1");
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let names: Vec<Value> = ids.as_array().unwrap().iter().map(|id| element(&mut s, id.as_u64().unwrap())["name"].clone()).collect();
    assert_eq!(names, ["Norte", "Sul", "Leste", "Oeste"]);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["name"], "Ambiente");
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, sheet)["name"], "Sem nome");
    // A schedule placed by its name in either language is named in the project's.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Room Schedule"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, vp)["title"], "Tabela de ambientes");
    run(&mut s, "sheet", json!({}));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "Tabela de ambientes"})).unwrap_err().contains("A101"));
    // What's named stays named; what comes next follows the language.
    run(&mut s, "language", json!({"code": "en"}));
    assert_eq!(run(&mut s, "level", json!({"elevation": 6000}))["name"], "Level 3");
    assert_eq!(element(&mut s, room)["name"], "Ambiente");
    assert!(s.execute("language", &json!({"code": "klingon"})).unwrap_err().contains("pt"));
}

#[test]
fn a_project_in_spanish_names_what_it_makes_in_spanish() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"language": "es"}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!([&i["name"], &i["levels"][1]["name"], &i["language"]], [&json!("Proyecto"), &json!("Nivel 2"), &json!("es")]);
    house(&mut s);
    assert_eq!(run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["name"], "Sección 1");
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let names: Vec<Value> = ids.as_array().unwrap().iter().map(|id| element(&mut s, id.as_u64().unwrap())["name"].clone()).collect();
    assert_eq!(names, ["Norte", "Sur", "Este", "Oeste"]);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["name"], "Habitación");
    run(&mut s, "sheet", json!({}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "tabla de habitaciones"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, vp)["title"], "Tabla de habitaciones");
    // Sides are named in any language there is.
    assert_eq!(
        [Facing::parse("Sur"), Facing::parse("leste"), Facing::parse("West")],
        [Some(Facing::South), Some(Facing::East), Some(Facing::West)]
    );
}
