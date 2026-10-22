// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_beam_axes.rs (the bounded
// line and arc records in an element's data, RE-49, RE-75).
// Changed: rewritten for BIMCraft; only the record readers are kept (not the beam solver or the scans over the file), every
// read is bounds-checked, and the readers take the bytes of one element's data.
//! The curves an element's data holds: a wall's location line or arc.
//!
//! A bounded line is `04 00 08 01`, a start and an end parameter (`f64`, feet), an origin (three `f64`) and a unit direction
//! (three `f64`): the line runs from `origin + start · direction` to `origin + end · direction`. An arc is the same tag
//! preceded by a `u64` 1 (a line's is 0): a start and an end angle (radians), the arc's X axis and Y axis (unit vectors, square
//! to each other), a radius (feet) and a centre. It runs from `centre + radius · (cos a · X + sin a · Y)` at the start angle to
//! the end angle. A wall's first curve record is its location line, which is its centreline whatever its location-line setting.

use crate::records::{f64_at, u64_at};

/// The bytes a bounded line or arc starts with.
const TAG: [u8; 4] = [0x04, 0x00, 0x08, 0x01];

/// A bounded line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundedLine {
    pub start_parameter: f64,
    pub end_parameter: f64,
    pub origin: [f64; 3],
    pub direction: [f64; 3],
}

impl BoundedLine {
    fn point(&self, parameter: f64) -> [f64; 3] {
        [0, 1, 2].map(|a| self.origin[a] + parameter * self.direction[a])
    }

    /// The line's first end, model feet.
    pub fn start(&self) -> [f64; 3] {
        self.point(self.start_parameter)
    }

    /// The line's second end, model feet.
    pub fn end(&self) -> [f64; 3] {
        self.point(self.end_parameter)
    }
}

/// The values of a curve record, `n` of them after the tag, all finite and within [`crate::records::MAX_FEET`].
fn values<const N: usize>(buf: &[u8], at: usize) -> Option<[f64; N]> {
    if buf.get(at..at.checked_add(4)?)? != TAG {
        return None;
    }
    let mut out = [0.0; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = f64_at(buf, at + 4 + 8 * i).filter(|x| x.is_finite() && x.abs() <= crate::records::MAX_FEET)?;
    }
    Some(out)
}

fn unit(v: [f64; 3]) -> bool {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - 1.0).abs() <= 1e-9
}

/// The bounded line whose tag starts at `at`: finite values, a unit direction and a start parameter below the end one.
pub fn line_at(buf: &[u8], at: usize) -> Option<BoundedLine> {
    let [start_parameter, end_parameter, ox, oy, oz, dx, dy, dz] = values::<8>(buf, at)?;
    (unit([dx, dy, dz]) && start_parameter < end_parameter).then_some(BoundedLine {
        start_parameter,
        end_parameter,
        origin: [ox, oy, oz],
        direction: [dx, dy, dz],
    })
}

/// The first bounded line in `data`. None where the data's first curve record is an arc: its axes would read as a line.
pub fn first_line(data: &[u8]) -> Option<BoundedLine> {
    let first = memchr::memmem::find(data, &TAG)?;
    if arc_at(data, first).is_some() {
        return None;
    }
    memchr::memmem::find_iter(data, &TAG).find_map(|at| line_at(data, at))
}

/// A circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundedArc {
    pub start_angle: f64,
    pub end_angle: f64,
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub radius: f64,
    pub centre: [f64; 3],
}

impl BoundedArc {
    /// The point at `angle` on the arc, model feet.
    pub fn point(&self, angle: f64) -> [f64; 3] {
        let (sin, cos) = angle.sin_cos();
        [0, 1, 2].map(|a| self.centre[a] + self.radius * (cos * self.x_axis[a] + sin * self.y_axis[a]))
    }

    /// Whether the angle runs counter-clockwise seen from above: the axes' cross product points up.
    pub fn counter_clockwise(&self) -> bool {
        self.x_axis[0] * self.y_axis[1] - self.x_axis[1] * self.y_axis[0] > 0.0
    }
}

/// The arc whose tag starts at `at`: the `u64` before it is 1, the values are finite, the axes are unit and square to each
/// other, the radius is positive and the start angle is below the end one.
pub fn arc_at(buf: &[u8], at: usize) -> Option<BoundedArc> {
    if u64_at(buf, at.checked_sub(8)?)? != 1 {
        return None;
    }
    arc_values_at(buf, at)
}

/// The record at `at` read as an arc whatever the word before it: a sketch line's arc is not marked.
pub fn arc_values_at(buf: &[u8], at: usize) -> Option<BoundedArc> {
    let [start_angle, end_angle, xx, xy, xz, yx, yy, yz, radius, cx, cy, cz] = values::<12>(buf, at)?;
    let (x_axis, y_axis) = ([xx, xy, xz], [yx, yy, yz]);
    (unit(x_axis) && unit(y_axis) && (xx * yx + xy * yy + xz * yz).abs() <= 1e-9 && radius > 0.0 && start_angle < end_angle)
        .then_some(BoundedArc { start_angle, end_angle, x_axis, y_axis, radius, centre: [cx, cy, cz] })
}

/// The arc a wall's data stores where its first curve record is one.
pub fn first_arc(data: &[u8]) -> Option<BoundedArc> {
    arc_at(data, memchr::memmem::find(data, &TAG)?)
}
