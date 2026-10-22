//! A sketch's curves closed into regions: what makes a boundary, and what is refused.

use crate::project_fixture::arc_bytes;
use crate::sketch::{Edge, MAX_EDGES, edge_of, from_edges};

fn edge(a: (f64, f64), b: (f64, f64)) -> Edge {
    Edge { a, b, mid: Vec::new() }
}

/// The edges of the closed polygon through `points`.
fn polygon(points: &[(f64, f64)]) -> Vec<Edge> {
    (0..points.len()).map(|i| edge(points[i], points[(i + 1) % points.len()])).collect()
}

#[test]
fn three_curves_close_a_triangle_at_any_scale_and_two_do_not_close_anything() {
    for scale in [1.0, 1e-5, 1000.0] {
        let pieces = from_edges(&polygon(&[(0.0, 0.0), (10.0 * scale, 0.0), (0.0, 10.0 * scale)])).expect("a triangle");
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].outer.len(), 3, "scale {scale}");
        assert!(pieces[0].holes.is_empty());
    }
    assert!(from_edges(&polygon(&[(0.0, 0.0), (10.0, 0.0)])).is_none());
}

#[test]
fn a_vertex_on_the_line_between_its_neighbours_is_dropped_wherever_the_line_runs() {
    // The side from (3, 4) to (23, 16) is drawn as two curves, meeting at (13, 10); the other two sides are corners.
    let pieces = from_edges(&polygon(&[(3.0, 4.0), (13.0, 10.0), (23.0, 16.0), (3.0, 16.0)])).expect("a triangle with a vertex on a side");
    assert_eq!(pieces[0].outer.len(), 3, "{:?}", pieces[0].outer);
    // A bend that is slight, but not nothing, stays.
    let pieces = from_edges(&polygon(&[(0.0, 0.0), (10.0, 0.001), (20.0, 0.0), (10.0, 10.0)])).expect("a bent side");
    assert_eq!(pieces[0].outer.len(), 4);
}

#[test]
fn a_loop_may_hold_a_lens_of_two_curves_between_the_same_ends_but_not_a_curve_twice() {
    let triangle = polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
    let lens = vec![edge((20.0, 0.0), (30.0, 0.0)), Edge { mid: vec![(25.0, 3.0)], ..edge((20.0, 0.0), (30.0, 0.0)) }];
    let pieces = from_edges(&[triangle.clone(), lens].concat()).expect("a triangle and a lens");
    assert_eq!(pieces.len(), 2);
    // The same curve twice, in either direction, is damage.
    let twice = vec![edge((20.0, 0.0), (30.0, 0.0)), edge((20.0, 0.0), (30.0, 0.0))];
    assert!(from_edges(&[triangle.clone(), twice].concat()).is_none());
    let back = vec![edge((20.0, 0.0), (30.0, 0.0)), edge((30.0, 0.0), (20.0, 0.0))];
    assert!(from_edges(&[triangle.clone(), back].concat()).is_none());
    // A curve with its two ends at one point, and three curves meeting at a point.
    assert!(from_edges(&[triangle.clone(), vec![edge((20.0, 0.0), (20.0, 0.0))]].concat()).is_none());
    let mut y = triangle;
    y.push(edge((0.0, 0.0), (-5.0, -5.0)));
    assert!(from_edges(&y).is_none());
}

#[test]
fn a_sketch_holds_up_to_the_most_curves_it_is_read_from() {
    let ring = |n: usize| -> Vec<(f64, f64)> {
        (0..n)
            .map(|k| {
                (1000.0 * (std::f64::consts::TAU * k as f64 / n as f64).cos(), 1000.0 * (std::f64::consts::TAU * k as f64 / n as f64).sin())
            })
            .collect()
    };
    assert_eq!(from_edges(&polygon(&ring(MAX_EDGES))).map(|p| p[0].outer.len()), Some(MAX_EDGES));
    assert!(from_edges(&polygon(&ring(MAX_EDGES + 1))).is_none());
}

#[test]
fn an_arc_that_turns_more_than_a_full_turn_is_no_curve_of_a_sketch() {
    // The frame is turned (x north, y west) so that its arc is not also a line that fits the box: the line it would read as
    // starts at (0, 1) and runs west, out of the box.
    let bbox = [0.0, 0.0, 0.0, 10.0, 10.0, 0.0];
    let arc = |end: f64| arc_bytes([0.0, end], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], 4.0, [6.0, 6.0, 0.0]);
    let some = edge_of(&arc(6.0), &bbox).expect("an arc of nearly a turn");
    assert!(some.mid.len() > 10, "drawn as chords");
    assert!(edge_of(&arc(7.0), &bbox).is_none());
}
