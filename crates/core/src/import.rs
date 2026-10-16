//! IFC import: an IFC2x3 or IFC4 file, as Revit, other BIM tools and BIMCraft itself write them,
//! read into a model. Storeys become levels. Walls, straight or curved, and the doors and windows
//! in them, floors (level or sloped), ceilings, rooms, columns, grids, stairs and ramps of straight
//! flights with landings between them, railings that have an axis, and roofs (flat, or pitched
//! over a rectangle) come in as BIMCraft elements, each measured from its
//! geometry: an extruded profile, a Brep or a triangle mesh alike, so how a file describes an
//! element doesn't matter. A wall cut to a roof over it comes in attached to it; the openings
//! through floors, ceilings and roofs, as shafts.
//! What BIMCraft has no element for, or what breaks one of its rules, is left out and listed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::PI;

use crate::commands::{micron, next_number, tidy};
use crate::geom::{self, P2};
use crate::model::{
    Ceiling, CeilingType, Column, ColumnType, Element, Extrusion, Floor, FloorType, Grid, Id, Landing, Level, MAX_POINTS, MAX_TEXT, Model,
    Opening, OpeningType, Railing, RailingType, Ramp, Roof, RoofType, Room, Shaft, Side, Stair, TOL, Wall, WallType, culprit, heading,
};
use crate::stairs;
use crate::step::{self, Entity, File, V};
use crate::view3d::{P3, add, cross, dot, mul, sub, unit};
use crate::walls::{self, Axis, Plane};

/// A model read from an IFC file: its schema, what was left out and why, and how far it was moved
/// to bring it near the origin.
pub struct Imported {
    pub model: Model,
    pub schema: String,
    pub skipped: Vec<String>,
    pub moved: Option<P2>,
}

fn real(v: Option<&V>) -> Option<f64> {
    let x = match v? {
        V::Real(x) => *x,
        V::Int(i) => *i as f64,
        V::Typed(_, inner) => return real(Some(inner)),
        _ => return None,
    };
    x.is_finite().then_some(x)
}

fn text(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Str(s) => Some(s.trim()).filter(|s| !s.is_empty()),
        V::Typed(_, inner) => text(Some(inner)),
        _ => None,
    }
}

fn reference(v: Option<&V>) -> Option<u64> {
    match v? {
        V::Ref(r) => Some(*r),
        _ => None,
    }
}

fn list(v: Option<&V>) -> &[V] {
    match v {
        Some(V::List(l)) => l,
        _ => &[],
    }
}

fn refs(v: Option<&V>) -> Vec<u64> {
    list(v).iter().filter_map(|x| reference(Some(x))).collect()
}

fn enumeration(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Enum(e) => Some(e),
        _ => None,
    }
}

/// A list of whole numbers: indices, 1 first.
fn indices(v: Option<&V>) -> Vec<usize> {
    match v {
        Some(V::Typed(_, inner)) => indices(Some(inner)),
        Some(V::List(l)) => l
            .iter()
            .filter_map(|x| match x {
                V::Int(i) => usize::try_from(*i).ok(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `text` cut to what a name may hold.
fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT).collect()
}

/// A number as a name shows it: to the hundredth, without trailing zeros.
fn num(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0 + 0.0)
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    o: P3,
    x: P3,
    y: P3,
    z: P3,
}

const WORLD: Frame = Frame { o: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };

impl Frame {
    fn at(&self, p: P3) -> P3 {
        add(self.o, self.dir(p))
    }

    fn dir(&self, d: P3) -> P3 {
        add(add(mul(self.x, d[0]), mul(self.y, d[1])), mul(self.z, d[2]))
    }

    /// `local`, given in this frame's terms, in its parent's.
    fn then(&self, local: Frame) -> Frame {
        Frame { o: self.at(local.o), x: self.dir(local.x), y: self.dir(local.y), z: self.dir(local.z) }
    }
}

/// Axes at `o`: along `z`, and `x` turned square to it.
fn orthonormal(o: P3, x: P3, z: P3) -> Frame {
    let z = if dot(z, z) > 1e-18 { unit(z) } else { [0.0, 0.0, 1.0] };
    let square = |v: P3| sub(v, mul(z, dot(v, z)));
    let x = if dot(square(x), square(x)) > 1e-12 {
        unit(square(x))
    } else {
        unit(square(if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }))
    };
    Frame { o, x, y: cross(z, x), z }
}

/// A polygon's normal, by Newell's method: as long as the polygon's area, twice over.
fn newell(f: &[P3]) -> P3 {
    f.iter().zip(f.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)))
}

fn normal(f: &[P3]) -> P3 {
    let n = newell(f);
    if dot(n, n) > 1e-12 { unit(n) } else { [0.0; 3] }
}

fn area3(f: &[P3]) -> f64 {
    dot(newell(f), newell(f)).sqrt() / 2.0
}

/// A ring and its copy `up` from it, as faces turned out.
fn prism(out: &mut Vec<Vec<P3>>, bottom: Vec<P3>, up: P3) {
    let top: Vec<P3> = bottom.iter().map(|p| add(*p, up)).collect();
    for ((a, b), (c, d)) in bottom.iter().zip(&top).zip(bottom.iter().zip(&top).cycle().skip(1)) {
        out.push(vec![*a, *c, *d, *b]);
    }
    out.push(bottom.into_iter().rev().collect());
    out.push(top);
}

/// Points less repeats in a row and the closing repeat of the first.
fn open(mut pts: Vec<P2>) -> Vec<P2> {
    pts.dedup_by(|a, b| a.dist(*b) < 1e-6);
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-6) {
        pts.pop();
    }
    pts
}

/// The loops the edges of `faces` no two share make, each face turned counter-clockwise in plan: a
/// region's outline counter-clockwise, its holes clockwise. An edge with another face's corner on it
/// is two edges, there. Each loop starts at its south-west corner, and they come in that order: as
/// the same file reads the same.
fn loops<'a>(faces: impl Iterator<Item = &'a Vec<P3>>) -> Vec<Vec<P2>> {
    // Points by y, then x: in order, south-west first.
    type K = (i64, i64);
    let key = |p: P2| -> K { ((p.1 * 10.0).round() as i64, (p.0 * 10.0).round() as i64) };
    let rings: Vec<Vec<P2>> = faces
        .map(|f| {
            let ring: Vec<P2> = f.iter().map(|p| P2(p[0], p[1])).collect();
            if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
        })
        .collect();
    // ponytail: every corner against every edge, fine for a roof's or a slab's faces.
    let corners: Vec<P2> = rings.iter().flatten().copied().collect();
    let mut edges: BTreeMap<(K, K), P2> = BTreeMap::new();
    for ring in &rings {
        for (a, b) in geom::ring_edges(ring) {
            let mut on: Vec<(f64, P2)> = corners
                .iter()
                .filter(|p| geom::dist_to_segment(**p, a, b) < 0.05 && p.dist(a) > 0.05 && p.dist(b) > 0.05)
                .map(|p| (geom::param_on_segment(*p, a, b), *p))
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            let pts: Vec<P2> = std::iter::once(a).chain(on.into_iter().map(|(_, p)| p)).chain(std::iter::once(b)).collect();
            for w in pts.windows(2) {
                let &[a, b] = w else { continue };
                let (ka, kb) = (key(a), key(b));
                // An edge two faces share runs both ways, and lies inside the outline.
                if ka != kb && edges.remove(&(kb, ka)).is_none() {
                    edges.insert((ka, kb), a);
                }
            }
        }
    }
    let mut next: BTreeMap<K, (K, P2)> = edges.into_iter().map(|((ka, kb), a)| (ka, (kb, a))).collect();
    let mut out = Vec::new();
    while let Some(&start) = next.keys().next() {
        let (mut ring, mut k) = (Vec::new(), start);
        while let Some((kb, a)) = next.remove(&k) {
            ring.push(a);
            k = kb;
        }
        if ring.len() >= 3 {
            out.push(ring);
        }
    }
    out
}

/// The outline of the faces facing up, or down, whose points all lie within half a millimetre of
/// `on`, a level or sloped plane: the largest of their loops, its corners counter-clockwise; and
/// the holes in it, clockwise.
fn outline(faces: &[Vec<P3>], on: Plane, up: bool) -> Option<geom::Region> {
    let picked = faces.iter().filter(|f| {
        let n = normal(f);
        f.len() >= 3 && f.iter().all(|p| (p[2] - on.at(P2(p[0], p[1]))).abs() <= 0.5) && n[2].abs() >= 0.2 && (n[2] > 0.0) == up
    });
    let mut rings: Vec<Vec<P2>> = loops(picked).into_iter().map(|l| geom::corners(&l)).filter(|l| l.len() >= 3).collect();
    rings.sort_by(|a, b| geom::area(b).abs().total_cmp(&geom::area(a).abs()));
    let mut rings = rings.into_iter();
    let mut ring = rings.next()?;
    if geom::area(&ring) < 0.0 {
        ring.reverse();
    }
    let holes = rings.filter(|h| geom::area(h) < 0.0 && h.iter().all(|p| geom::contains(&ring, *p))).collect();
    Some(geom::Region { outer: ring, holes })
}

/// A slab's top, sloped or level: the plane of its largest face turned up, and its outline in
/// plan and the holes in it; and how far its underside, the plane of its largest face turned down,
/// lies below it, plumb.
fn top_of(faces: &[Vec<P3>]) -> Option<(Plane, geom::Region, f64)> {
    let largest = |up: bool| {
        faces
            .iter()
            .filter(|f| f.len() >= 3 && normal(f)[2].abs() > 0.2 && (normal(f)[2] > 0.0) == up)
            .max_by(|a, b| area3(a).total_cmp(&area3(b)))
    };
    let (top, under) = (Plane::of(largest(true)?)?, Plane::of(largest(false)?)?);
    let shape = outline(faces, top, true)?;
    let q = *shape.outer.first()?;
    Some((top, shape, top.at(q) - under.at(q)))
}

/// A point well inside a ring: the middle of the largest of the triangles that make it up.
fn inside(ring: &[P2]) -> Option<P2> {
    let pt = |i: usize| ring.get(i).copied();
    let tris = geom::triangulate(ring).into_iter().filter_map(|[a, b, c]| Some((pt(a)?, pt(b)?, pt(c)?)));
    let size = |(a, b, c): &(P2, P2, P2)| (*b - *a).cross(*c - *a).abs();
    tris.max_by(|x, y| size(x).total_cmp(&size(y))).map(|(a, b, c)| (a + b + c) * (1.0 / 3.0))
}

/// The least and the most of `xs`.
fn span(xs: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    xs.fold(None, |s: Option<(f64, f64)>, x| Some(s.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x)))))
}

/// Whether two rings have the same corners, to the millimetre.
fn same(a: &[P2], b: &[P2]) -> bool {
    a.len() == b.len() && a.iter().all(|p| b.iter().any(|q| q.dist(*p) < 1.0))
}

struct Reader<'a> {
    f: &'a File,
    /// The file's length unit, in mm; its plane angle unit, in radians.
    mm: f64,
    rad: f64,
    /// Taken off every plan point: brings a model placed far out (a site at its map
    /// coordinates) near the origin.
    shift: P2,
}

impl<'a> Reader<'a> {
    fn get(&self, id: u64) -> Option<&'a Entity> {
        self.f.entities.get(&id)
    }

    fn at(&self, v: Option<&V>) -> Option<&'a Entity> {
        self.get(reference(v)?)
    }

    fn len(&self, v: Option<&V>) -> Option<f64> {
        real(v).map(|x| x * self.mm)
    }

    /// A model point in the plan.
    fn flat(&self, p: P3) -> P2 {
        P2(p[0] - self.shift.0, p[1] - self.shift.1)
    }

    fn coords(&self, v: Option<&V>) -> Vec<f64> {
        list(v).iter().filter_map(|x| real(Some(x))).collect()
    }

    fn point(&self, id: Option<u64>) -> Option<P3> {
        match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => Some([x * self.mm, y * self.mm, 0.0]),
            [x, y, z, ..] => Some([x * self.mm, y * self.mm, z * self.mm]),
            _ => None,
        }
    }

    fn direction(&self, id: Option<u64>) -> Option<P3> {
        let d = match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => [*x, *y, 0.0],
            [x, y, z, ..] => [*x, *y, *z],
            _ => return None,
        };
        (dot(d, d) > 1e-18).then(|| unit(d))
    }

    /// An axis placement, 3D or 2D.
    fn axes(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| reference(e.args.get(i));
        let o = self.point(a(0)).unwrap_or_default();
        match e.name.as_str() {
            "IFCAXIS2PLACEMENT3D" => {
                orthonormal(o, self.direction(a(2)).unwrap_or([1.0, 0.0, 0.0]), self.direction(a(1)).unwrap_or([0.0, 0.0, 1.0]))
            }
            "IFCAXIS2PLACEMENT2D" => orthonormal(o, self.direction(a(1)).unwrap_or([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]),
            _ => WORLD,
        }
    }

    /// Where a placement puts what it places: its chain of local placements.
    fn placement(&self, id: Option<u64>, depth: usize) -> Frame {
        match id.and_then(|i| self.get(i)) {
            // ponytail: a chain is walked anew for each element; memoize if files run to millions of elements.
            Some(e) if e.name == "IFCLOCALPLACEMENT" && depth < 64 => {
                self.placement(reference(e.args.first()), depth + 1).then(self.axes(reference(e.args.get(1))))
            }
            _ => WORLD,
        }
    }

    /// A Cartesian transformation operator: axes, scaled.
    fn operator(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| e.args.get(i);
        let s = real(a(3)).unwrap_or(1.0);
        let (z, k) = match e.name.as_str() {
            "IFCCARTESIANTRANSFORMATIONOPERATOR3D" => (self.direction(reference(a(4))), [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM" => {
                (self.direction(reference(a(4))), [s, real(a(5)).unwrap_or(s), real(a(6)).unwrap_or(s)])
            }
            "IFCCARTESIANTRANSFORMATIONOPERATOR2D" => (None, [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR2DNONUNIFORM" => (None, [s, real(a(4)).unwrap_or(s), s]),
            _ => return WORLD,
        };
        let o = self.point(reference(a(2))).unwrap_or_default();
        let f = orthonormal(o, self.direction(reference(a(0))).unwrap_or([1.0, 0.0, 0.0]), z.unwrap_or([0.0, 0.0, 1.0]));
        Frame { o: f.o, x: mul(f.x, k[0]), y: mul(f.y, k[1]), z: mul(f.z, k[2]) }
    }

    fn point_list(&self, id: Option<u64>) -> Vec<P3> {
        let Some(e) = id.and_then(|i| self.get(i)) else { return Vec::new() };
        list(e.args.first())
            .iter()
            .map(|p| {
                let c = self.coords(Some(p));
                let at = |i: usize| c.get(i).copied().unwrap_or(0.0) * self.mm;
                [at(0), at(1), at(2)]
            })
            .collect()
    }

    /// A curve's points in order, in its own coordinates; an arc as short chords.
    fn curve(&self, id: Option<u64>, depth: usize) -> Option<Vec<P3>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        match e.name.as_str() {
            "IFCPOLYLINE" => Some(refs(a(0)).into_iter().filter_map(|p| self.point(Some(p))).collect()),
            "IFCINDEXEDPOLYCURVE" => {
                let pts = self.point_list(reference(a(0)));
                let segments = list(a(1));
                if segments.is_empty() {
                    return Some(pts);
                }
                let mut out: Vec<P3> = Vec::new();
                for i in segments.iter().flat_map(|s| indices(Some(s))) {
                    if let Some(p) = i.checked_sub(1).and_then(|k| pts.get(k))
                        && out.last() != Some(p)
                    {
                        out.push(*p);
                    }
                }
                Some(out)
            }
            "IFCCOMPOSITECURVE" | "IFCCOMPOSITECURVEONSURFACE" => {
                let mut out: Vec<P3> = Vec::new();
                for seg in refs(a(0)).into_iter().filter_map(|s| self.get(s)) {
                    let Some(mut pts) = self.curve(reference(seg.args.get(2)), depth + 1) else { continue };
                    if enumeration(seg.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    for p in pts {
                        if out.last().is_none_or(|q| dot(sub(*q, p), sub(*q, p)) > 1e-12) {
                            out.push(p);
                        }
                    }
                }
                Some(out)
            }
            "IFCTRIMMEDCURVE" => self.trimmed(e),
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(a(0))), self.len(a(1))?);
                Some((0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => None,
        }
    }

    /// A trimmed line, or a trimmed circle as an arc.
    fn trimmed(&self, e: &Entity) -> Option<Vec<P3>> {
        let basis = self.at(e.args.first())?;
        // A trim: a point, or a parameter along the curve.
        let trim = |v: Option<&V>| {
            let mut out: (Option<P3>, Option<f64>) = (None, None);
            for t in list(v) {
                match t {
                    V::Ref(r) => out.0 = out.0.or(self.point(Some(*r))),
                    V::Typed(_, p) => out.1 = out.1.or(real(Some(p))),
                    _ => {}
                }
            }
            out
        };
        let (t1, t2) = (trim(e.args.get(1)), trim(e.args.get(2)));
        match basis.name.as_str() {
            "IFCLINE" => {
                let p0 = self.point(reference(basis.args.first()))?;
                let v = self.at(basis.args.get(1));
                let d = v.and_then(|v| self.direction(reference(v.args.first()))).unwrap_or([1.0, 0.0, 0.0]);
                let k = v.and_then(|v| self.len(v.args.get(1))).unwrap_or(1.0);
                let at = |(p, t): (Option<P3>, Option<f64>)| p.or_else(|| t.map(|t| add(p0, mul(d, k * t))));
                Some(vec![at(t1)?, at(t2)?])
            }
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(basis.args.first())), self.len(basis.args.get(1))?);
                let angle = |(p, t): (Option<P3>, Option<f64>)| {
                    p.map(|p| dot(sub(p, f.o), f.y).atan2(dot(sub(p, f.o), f.x))).or_else(|| t.map(|t| t * self.rad))
                };
                let (a1, a2) = (angle(t1)?, angle(t2)?);
                let ccw = (a2 - a1).rem_euclid(2.0 * PI);
                let sweep = if enumeration(e.args.get(3)) == Some("F") { ccw - 2.0 * PI } else { ccw };
                Some((0..=12).map(|i| a1 + sweep * f64::from(i) / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => Some(vec![t1.0?, t2.0?]),
        }
    }

    /// A profile's outer edge, in its own plane.
    fn profile(&self, id: Option<u64>, depth: usize) -> Option<Vec<P2>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        let placed = |pts: Vec<P2>| {
            let pos = self.axes(reference(a(2)));
            pts.into_iter().map(|p| pos.at([p.0, p.1, 0.0])).map(|q| P2(q[0], q[1])).collect::<Vec<_>>()
        };
        let rect = |w: f64, h: f64| vec![P2(-w / 2.0, -h / 2.0), P2(w / 2.0, -h / 2.0), P2(w / 2.0, h / 2.0), P2(-w / 2.0, h / 2.0)];
        let ellipse =
            |rx: f64, ry: f64| (0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| P2(rx * t.cos(), ry * t.sin())).collect::<Vec<P2>>();
        match e.name.as_str() {
            "IFCRECTANGLEPROFILEDEF"
            | "IFCROUNDEDRECTANGLEPROFILEDEF"
            | "IFCRECTANGLEHOLLOWPROFILEDEF"
            | "IFCISHAPEPROFILEDEF"
            | "IFCASYMMETRICISHAPEPROFILEDEF" => Some(placed(rect(self.len(a(3))?, self.len(a(4))?))),
            // Steel sections: their bounding rectangle, depth along y.
            "IFCLSHAPEPROFILEDEF" | "IFCUSHAPEPROFILEDEF" | "IFCCSHAPEPROFILEDEF" | "IFCTSHAPEPROFILEDEF" | "IFCZSHAPEPROFILEDEF" => {
                Some(placed(rect(self.len(a(4))?, self.len(a(3))?)))
            }
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = self.len(a(3))?;
                Some(placed(ellipse(r, r)))
            }
            "IFCELLIPSEPROFILEDEF" => Some(placed(ellipse(self.len(a(3))?, self.len(a(4))?))),
            "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                Some(open(self.curve(reference(a(2)), depth + 1)?.into_iter().map(|p| P2(p[0], p[1])).collect()))
            }
            "IFCDERIVEDPROFILEDEF" => {
                let op = self.operator(reference(a(3)));
                Some(
                    self.profile(reference(a(2)), depth + 1)?.into_iter().map(|p| op.at([p.0, p.1, 0.0])).map(|q| P2(q[0], q[1])).collect(),
                )
            }
            _ => None,
        }
    }

    /// The holes in a profile with voids, in its own plane: as many as have at most [`MAX_POINTS`]
    /// points between them.
    fn voids(&self, id: Option<u64>, depth: usize) -> Vec<Vec<P2>> {
        let Some(e) = id.and_then(|i| self.get(i)).filter(|e| e.name == "IFCARBITRARYPROFILEDEFWITHVOIDS") else { return Vec::new() };
        let mut total = 0;
        refs(e.args.get(3))
            .into_iter()
            .filter_map(|c| self.curve(Some(c), depth + 1))
            .map(|pts| open(pts.into_iter().map(|p| P2(p[0], p[1])).collect()))
            .filter(|h| h.len() >= 3)
            .take_while(|h| {
                total += h.len();
                total <= MAX_POINTS
            })
            .collect()
    }

    /// The faces of representation item `id`, as polygons in its own coordinates.
    fn faces(&self, id: u64, depth: usize, out: &mut Vec<Vec<P3>>) {
        let Some(e) = self.get(id) else { return };
        if depth > 16 {
            return;
        }
        let a = |i: usize| e.args.get(i);
        let polygon = |out: &mut Vec<Vec<P3>>, pts: Vec<P3>| {
            if pts.len() >= 3 {
                out.push(pts);
            }
        };
        match e.name.as_str() {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => {
                let (Some(ring), Some(length)) = (self.profile(reference(a(0)), depth + 1), self.len(a(3))) else { return };
                let pos = self.axes(reference(a(1)));
                let up = mul(pos.dir(self.direction(reference(a(2))).unwrap_or([0.0, 0.0, 1.0])), length);
                if ring.len() >= 3 {
                    // Round holes in its profile, in convex pieces.
                    let voids = self.voids(reference(a(0)), depth + 1);
                    for piece in voids.iter().fold(vec![ring], |parts, v| walls::minus(&parts, v)) {
                        prism(out, piece.iter().map(|p| pos.at([p.0, p.1, 0.0])).collect(), up);
                    }
                }
            }
            // What a clipping cuts away doesn't change what the element is.
            "IFCBOOLEANCLIPPINGRESULT" | "IFCBOOLEANRESULT" => {
                if let Some(first) = reference(a(1)) {
                    self.faces(first, depth + 1, out);
                }
                if enumeration(a(0)) == Some("UNION")
                    && let Some(second) = reference(a(2))
                {
                    self.faces(second, depth + 1, out);
                }
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" | "IFCCSGSOLID" => {
                if let Some(inner) = reference(a(0)) {
                    self.faces(inner, depth + 1, out);
                }
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" | "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                for f in refs(a(0)) {
                    self.faces(f, depth + 1, out);
                }
            }
            "IFCFACE" | "IFCFACESURFACE" => {
                let bounds: Vec<&Entity> = refs(a(0)).into_iter().filter_map(|b| self.get(b)).collect();
                let outer = bounds.iter().find(|b| b.name == "IFCFACEOUTERBOUND").or(bounds.first());
                if let Some(bound) = outer {
                    let lp = self.at(bound.args.first());
                    let mut pts: Vec<P3> =
                        lp.map_or_else(Vec::new, |l| refs(l.args.first()).into_iter().filter_map(|p| self.point(Some(p))).collect());
                    if enumeration(bound.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    polygon(out, pts);
                }
            }
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" | "IFCPOLYGONALFACESET" => {
                let pts = self.point_list(reference(a(0)));
                let triangles = e.name != "IFCPOLYGONALFACESET";
                let pn = indices(a(if triangles { 4 } else { 3 }));
                let at = |i: usize| {
                    let i = if pn.is_empty() { i } else { i.checked_sub(1).and_then(|k| pn.get(k)).copied()? };
                    i.checked_sub(1).and_then(|k| pts.get(k)).copied()
                };
                let polys: Vec<Vec<usize>> = if triangles {
                    list(a(3)).iter().map(|t| indices(Some(t))).collect()
                } else {
                    refs(a(2)).into_iter().filter_map(|f| self.get(f)).map(|f| indices(f.args.first())).collect()
                };
                for poly in polys {
                    polygon(out, poly.into_iter().filter_map(at).collect());
                }
            }
            "IFCMAPPEDITEM" => {
                let Some(map) = self.at(a(0)) else { return };
                let mut inner = Vec::new();
                if let Some(rep) = self.at(map.args.get(1)) {
                    for item in refs(rep.args.get(3)) {
                        self.faces(item, depth + 1, &mut inner);
                    }
                }
                let f = self.operator(reference(a(1))).then(self.axes(reference(map.args.first())));
                out.extend(inner.into_iter().map(|face| face.into_iter().map(|p| f.at(p)).collect()));
            }
            // A box: its corner or its placement, and its three sizes.
            "IFCBOUNDINGBOX" | "IFCBLOCK" => {
                let pos = if e.name == "IFCBLOCK" {
                    self.axes(reference(a(0)))
                } else {
                    Frame { o: self.point(reference(a(0))).unwrap_or_default(), ..WORLD }
                };
                if let (Some(x), Some(y), Some(z)) = (self.len(a(1)), self.len(a(2)), self.len(a(3))) {
                    prism(
                        out,
                        [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, y, 0.0], [0.0, y, 0.0]].map(|p| pos.at(p)).to_vec(),
                        pos.dir([0.0, 0.0, z]),
                    );
                }
            }
            _ => {}
        }
    }

    /// A product's shape representations: their identifiers and items.
    fn reps(&self, product: &Entity) -> Vec<(&'a str, Vec<u64>)> {
        let Some(shape) = self.at(product.args.get(6)) else { return Vec::new() };
        refs(shape.args.get(2))
            .into_iter()
            .filter_map(|r| self.get(r))
            .map(|rep| (text(rep.args.get(1)).unwrap_or(""), refs(rep.args.get(3))))
            .collect()
    }

    fn frame(&self, product: &Entity) -> Frame {
        self.placement(reference(product.args.get(5)), 0)
    }

    /// A product's body as faces, in model coordinates: its Body representation, else any but a
    /// line or a footprint.
    fn body(&self, product: &Entity) -> Vec<Vec<P3>> {
        let reps = self.reps(product);
        let items = |keep: &dyn Fn(&str) -> bool| {
            reps.iter().filter(|(i, _)| keep(&i.to_ascii_lowercase())).flat_map(|(_, it)| it.clone()).collect::<Vec<_>>()
        };
        let mut chosen = items(&|i| i == "body" || i == "facetation");
        if chosen.is_empty() {
            chosen = items(&|i| !matches!(i, "axis" | "footprint" | "annotation" | "plan" | "profile" | "reference" | "surface"));
        }
        let mut out = Vec::new();
        for item in chosen {
            self.faces(item, 0, &mut out);
        }
        let f = self.frame(product);
        out.into_iter().map(|face| face.into_iter().map(|p| f.at(p)).collect()).collect()
    }

    /// A product's Axis representation, in model coordinates.
    fn axis(&self, product: &Entity) -> Option<Vec<P3>> {
        let (_, items) = self.reps(product).into_iter().find(|(i, _)| i.eq_ignore_ascii_case("Axis"))?;
        let f = self.frame(product);
        Some(self.curve(items.first().copied(), 0)?.into_iter().map(|p| f.at(p)).collect())
    }
}

/// The project's length unit in mm, and its plane angle unit in radians.
fn units(f: &File) -> Result<(f64, f64), String> {
    fn si(prefix: Option<&str>) -> f64 {
        match prefix {
            Some("EXA") => 1e18,
            Some("PETA") => 1e15,
            Some("TERA") => 1e12,
            Some("GIGA") => 1e9,
            Some("MEGA") => 1e6,
            Some("KILO") => 1e3,
            Some("HECTO") => 1e2,
            Some("DECA") => 1e1,
            Some("DECI") => 1e-1,
            Some("CENTI") => 1e-2,
            Some("MILLI") => 1e-3,
            Some("MICRO") => 1e-6,
            Some("NANO") => 1e-9,
            _ => 1.0,
        }
    }
    // A unit's kind, and its size in SI base units (metres, radians).
    fn unit(f: &File, id: Option<u64>, depth: usize) -> Option<(String, f64)> {
        let e = f.entities.get(&id?)?;
        let kind = enumeration(e.args.get(1)).unwrap_or("").to_string();
        match e.name.as_str() {
            "IFCSIUNIT" => Some((kind, si(enumeration(e.args.get(2))))),
            "IFCCONVERSIONBASEDUNIT" | "IFCCONVERSIONBASEDUNITWITHOFFSET" if depth < 8 => {
                let measure = f.entities.get(&reference(e.args.get(3))?)?;
                let (_, base) = unit(f, reference(measure.args.get(1)), depth + 1)?;
                Some((kind, real(measure.args.first())? * base))
            }
            _ => None,
        }
    }
    // The first project, the one the name comes from.
    let projects = f.entities.iter().filter(|(_, e)| e.name == "IFCPROJECT");
    let (_, project) = projects.min_by_key(|(id, _)| **id).ok_or("it has no IfcProject")?;
    let assignment = reference(project.args.get(8)).and_then(|a| f.entities.get(&a));
    let (mut mm, mut rad) = (1.0, 1.0);
    for u in refs(assignment.and_then(|a| a.args.first())) {
        match unit(f, Some(u), 0) {
            Some((kind, size)) if kind == "LENGTHUNIT" => mm = size * 1000.0,
            Some((kind, size)) if kind == "PLANEANGLEUNIT" => rad = size,
            _ => {}
        }
    }
    if !(1e-6..=1e9).contains(&mm) || !(rad.is_finite() && rad > 0.0) {
        return Err("its length or angle unit isn't one BIMCraft can use".into());
    }
    Ok((mm, rad))
}

/// How elements relate: where each is, what it's part of, what voids and fills what, and its
/// type's name.
#[derive(Default)]
struct Links {
    within: HashMap<u64, u64>,
    whole: HashMap<u64, u64>,
    parts: HashMap<u64, Vec<u64>>,
    /// An opening's host; the opening a door or window fills.
    host: HashMap<u64, u64>,
    fills: HashMap<u64, u64>,
    type_name: HashMap<u64, String>,
}

impl Links {
    fn read(f: &File, ids: &[u64]) -> Links {
        let mut l = Links::default();
        for e in ids.iter().filter_map(|id| f.entities.get(id)) {
            let a = |i: usize| e.args.get(i);
            match e.name.as_str() {
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    if let Some(s) = reference(a(5)) {
                        l.within.extend(refs(a(4)).into_iter().map(|x| (x, s)));
                    }
                }
                "IFCRELAGGREGATES" => {
                    if let Some(w) = reference(a(4)) {
                        for x in refs(a(5)) {
                            l.whole.insert(x, w);
                            l.parts.entry(w).or_default().push(x);
                        }
                    }
                }
                "IFCRELVOIDSELEMENT" => {
                    if let (Some(h), Some(o)) = (reference(a(4)), reference(a(5))) {
                        l.host.insert(o, h);
                    }
                }
                "IFCRELFILLSELEMENT" => {
                    if let (Some(o), Some(d)) = (reference(a(4)), reference(a(5))) {
                        l.fills.insert(d, o);
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    if let Some(name) = reference(a(5)).and_then(|t| f.entities.get(&t)).and_then(|t| text(t.args.get(2))) {
                        l.type_name.extend(refs(a(4)).into_iter().map(|x| (x, name.to_string())));
                    }
                }
                _ => {}
            }
        }
        l
    }
}

/// A curved wall's faces from how far its body's points are from its centre: the middle one of the
/// points inside its middle, and of those outside it, so a corner where it joins another (cut a
/// little past the arc) doesn't count.
fn arc_faces(radii: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let mut all: Vec<f64> = radii.filter(|r| r.is_finite()).collect();
    all.sort_by(f64::total_cmp);
    let (lo, hi) = (*all.first()?, *all.last()?);
    let (inner, outer): (Vec<f64>, Vec<f64>) = all.iter().partition(|r| **r < (lo + hi) / 2.0);
    Some((*inner.get(inner.len() / 2)?, *outer.get(outer.len() / 2)?))
}

/// A body's highest point.
fn highest(pts: &[P3]) -> P3 {
    pts.iter().copied().max_by(|a, b| a[2].total_cmp(&b[2])).unwrap_or_default()
}

/// A wall as the file has it, before walls that nearly meet are made to meet.
struct RawWall {
    ifc: u64,
    start: P2,
    end: P2,
    arc: f64,
    t: f64,
    z: (f64, f64),
    /// Where its body is highest, in plan.
    peak: P2,
    level: Id,
    type_name: String,
}

/// Walls that nearly meet, made to meet: an end within a wall's thickness of another wall's centre
/// line moves onto it, so corners and T-junctions join as BIMCraft joins walls.
fn meet(walls: &mut [RawWall]) {
    for i in 0..walls.len() {
        for at_end in [false, true] {
            // Curved walls stay as the file has them.
            let Some(w) = walls.get(i).filter(|w| w.arc == 0.0) else { continue };
            let (a, b, p, t, z) = (w.start, w.end, if at_end { w.end } else { w.start }, w.t, w.z);
            if walls.iter().enumerate().any(|(j, o)| j != i && (o.start.dist(p) <= TOL || o.end.dist(p) <= TOL)) {
                continue;
            }
            let mut best: Option<(f64, P2)> = None;
            for o in walls.iter().enumerate().filter(|(j, o)| *j != i && o.arc == 0.0 && o.z.0 < z.1 && z.0 < o.z.1).map(|(_, o)| o) {
                if geom::parallel(a, b, o.start, o.end) {
                    continue;
                }
                let Some(x) = geom::line_intersection(a, b, o.start, o.end) else { continue };
                let reach = t.max(o.t);
                let along = (x - o.start).dot((o.end - o.start).unit());
                let d = x.dist(p);
                if d <= reach && (-reach..=o.start.dist(o.end) + reach).contains(&along) && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, x));
                }
            }
            // Never so far that the wall turns round or vanishes.
            if let Some((_, x)) = best
                && let Some(w) = walls.get_mut(i)
            {
                let (s, e) = if at_end { (w.start, x) } else { (x, w.end) };
                if (e - s).dot(b - a) > TOL * (b - a).length() {
                    (w.start, w.end) = (micron(s), micron(e));
                }
            }
        }
    }
}

/// A type of `map` like `same` says, by `base` or `base (2)`, `base (3)`… made as `new` when
/// there's none.
fn kind<T>(map: &mut BTreeMap<String, T>, base: &str, same: impl Fn(&T) -> bool, new: T) -> String {
    let base = clip(base);
    let mut new = Some(new);
    let mut n = 1;
    loop {
        let name = if n == 1 { base.clone() } else { format!("{base} ({n})") };
        match map.get(&name) {
            Some(t) if same(t) => return name,
            Some(_) => n += 1,
            None => {
                if let Some(t) = new.take() {
                    map.insert(name.clone(), t);
                }
                return name;
            }
        }
    }
}

/// `base`, or `base (2)`… whichever `taken` doesn't hold yet; then it does.
fn fresh(taken: &mut BTreeSet<String>, base: &str) -> String {
    let base = clip(base);
    let name = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") }).find(|n| !taken.contains(n)).unwrap_or(base);
    taken.insert(name.clone());
    name
}

const WALLS: &[&str] = &["IFCWALL", "IFCWALLSTANDARDCASE", "IFCWALLELEMENTEDCASE"];
const OPENINGS: &[&str] = &["IFCDOOR", "IFCDOORSTANDARDCASE", "IFCWINDOW", "IFCWINDOWSTANDARDCASE"];
const SLABS: &[&str] = &["IFCSLAB", "IFCSLABSTANDARDCASE", "IFCSLABELEMENTEDCASE"];
const COLUMNS: &[&str] = &["IFCCOLUMN", "IFCCOLUMNSTANDARDCASE"];

/// Reads IFC text into a model, named after the project (or `name`).
pub fn ifc(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let file = step::parse(bytes)?;
    if !file.schema.starts_with("IFC") {
        let schema = if file.schema.is_empty() { "no schema" } else { &file.schema };
        return Err(format!("this STEP file is in {schema}, not IFC"));
    }
    let (mm, rad) = units(&file)?;
    let mut ids: Vec<u64> = file.entities.keys().copied().collect();
    ids.sort_unstable();
    let l = Links::read(&file, &ids);
    // Far from the origin (a site at its map coordinates), the model comes nearer.
    let probe = Reader { f: &file, mm, rad, shift: P2(0.0, 0.0) };
    let origins: Vec<P3> = l.within.keys().filter_map(|id| probe.get(*id)).map(|x| probe.frame(x).o).collect();
    let shift = match (span(origins.iter().map(|p| p[0])), span(origins.iter().map(|p| p[1]))) {
        (Some((x0, x1)), Some((y0, y1))) if x0.abs().max(x1.abs()).max(y0.abs()).max(y1.abs()) > 1e8 => {
            P2((x0 / 1000.0).floor() * 1000.0, (y0 / 1000.0).floor() * 1000.0)
        }
        _ => P2(0.0, 0.0),
    };
    let r = Reader { f: &file, mm, rad, shift };
    let e = |id: u64| r.get(id);
    let named = |names: &[&str]| -> Vec<u64> {
        ids.iter().copied().filter(|id| e(*id).is_some_and(|x| names.contains(&x.name.as_str()))).collect()
    };
    let who = |id: u64| {
        e(id).map_or_else(
            || format!("#{id}"),
            |x| format!("{} #{id}{}", x.name, text(x.args.get(2)).map_or_else(String::new, |n| format!(" '{n}'"))),
        )
    };
    let label = |id: u64| l.type_name.get(&id).cloned().or_else(|| e(id).and_then(|x| text(x.args.get(4))).map(String::from));
    let storey_of = |id: u64| {
        let mut at = id;
        for _ in 0..8 {
            at = *l.within.get(&at).or_else(|| l.whole.get(&at))?;
            if e(at).is_some_and(|x| x.name == "IFCBUILDINGSTOREY") {
                return Some(at);
            }
        }
        None
    };

    let project = ids.iter().filter_map(|id| e(*id)).find(|x| x.name == "IFCPROJECT");
    let title = project.and_then(|p| text(p.args.get(2)).or_else(|| text(p.args.get(5)))).unwrap_or(name);
    let mut m = Model::new(&clip(title));
    m.elements.clear();
    m.next_id = 1;
    let (mut skipped, mut seen): (Vec<String>, HashSet<u64>) = (Vec::new(), HashSet::new());
    // Each element made, and what it was in the file.
    let mut from: HashMap<Id, String> = HashMap::new();

    // Storeys, as levels, lowest first.
    let mut storeys: Vec<(u64, String, f64)> = named(&["IFCBUILDINGSTOREY"])
        .into_iter()
        .filter_map(|id| {
            let s = e(id)?;
            let z = match reference(s.args.get(5)) {
                Some(_) => r.frame(s).o[2],
                None => r.len(s.args.get(9)).unwrap_or(0.0),
            };
            Some((id, text(s.args.get(2)).or_else(|| text(s.args.get(7))).unwrap_or("Level").to_string(), (z * 1000.0).round() / 1000.0))
        })
        .collect();
    storeys.sort_by(|a, b| a.2.total_cmp(&b.2));
    if storeys.is_empty() {
        storeys.push((0, "Level 1".into(), 0.0));
    }
    let mut taken = BTreeSet::new();
    let mut level_of: HashMap<u64, Id> = HashMap::new();
    for (sid, n, z) in &storeys {
        let level = m.insert(Element::Level(Level::new(fresh(&mut taken, n), *z)));
        level_of.insert(*sid, level);
        seen.insert(*sid);
    }
    let levels: Vec<(Id, f64)> = m.levels().into_iter().map(|(id, lv)| (id, lv.elevation)).collect();
    let elevation = |level: Id| levels.iter().find(|(id, _)| *id == level).map_or(0.0, |(_, z)| *z);
    // An element's level: its storey's, else the highest at or below `z`.
    let level_for = |id: u64, z: f64| -> Id {
        storey_of(id)
            .and_then(|s| level_of.get(&s).copied())
            .unwrap_or_else(|| levels.iter().rev().find(|(_, lz)| *lz <= z + 1.0).or(levels.first()).map_or(0, |(id, _)| *id))
    };
    let level_near = |z: f64| levels.iter().find(|(_, lz)| (lz - z).abs() <= 1.0).map(|(id, _)| *id);
    // Up to a level at `top` above `base`, else `top - bottom` tall.
    let topped = |base: Id, bottom: f64, top: f64| -> (Option<Id>, f64) {
        match levels.iter().find(|(id, lz)| *id != base && *lz > elevation(base) + TOL && (lz - top).abs() <= 1.0) {
            Some((id, _)) => (Some(*id), top - bottom),
            None => (None, top - bottom),
        }
    };
    let leave = |skipped: &mut Vec<String>, seen: &mut HashSet<u64>, id: u64, why: &str| {
        skipped.push(format!("{}: {why}", who(id)));
        seen.insert(id);
    };

    // Walls: the line the faces run along, and how thick they are across it.
    let mut raw: Vec<RawWall> = Vec::new();
    for id in named(WALLS) {
        let Some(w) = e(id) else { continue };
        let faces = r.body(w);
        let pts: Vec<P3> = faces.iter().flatten().copied().collect();
        let axis = r.axis(w);
        // A curved axis: the arc through its ends and its middle; the wall's line, the middle of its
        // body round the arc's centre, as thick as the body spreads.
        let bent: Vec<P2> = axis.iter().flatten().map(|x| r.flat(*x)).collect();
        if let [p, .., q] = bent.as_slice()
            && bent.iter().any(|x| geom::dist_to_segment(*x, *p, *q) > 1.0)
        {
            let mid = bent.get(bent.len() / 2).copied().unwrap_or(*p);
            let arc = walls::sweep_through(*p, mid, *q);
            let radii = Axis::new(*p, *q, arc).centre().and_then(|c| Some((c, arc_faces(pts.iter().map(|x| r.flat(*x).dist(c)))?)));
            let (Some((c, (r0, r1))), Some((z0, z1))) = (radii, span(pts.iter().map(|x| x[2]))) else {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            };
            let t = ((r1 - r0) * 100.0).round() / 100.0;
            let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
            let type_name =
                kind(&mut m.types.wall, &base, |k| (k.thickness - t).abs() < 0.05, WallType { thickness: t, layers: Vec::new() });
            let on = |x: P2| micron(c + (x - c).unit() * ((r0 + r1) / 2.0));
            let arc = (arc * 1e6).round() / 1e6;
            raw.push(RawWall {
                ifc: id,
                start: on(*p),
                end: on(*q),
                arc,
                t,
                z: (z0, z1),
                peak: r.flat(highest(&pts)),
                level: level_for(id, z0),
                type_name,
            });
            continue;
        }
        let f = r.frame(w);
        let (a, b) = match axis.as_deref() {
            Some([p, .., q]) if r.flat(*p).dist(r.flat(*q)) > TOL => (r.flat(*p), r.flat(*q)),
            // No axis: along the wall's own x, as far as its body goes.
            _ => {
                let o = r.flat(f.o);
                let x = P2(f.x[0], f.x[1]).unit();
                match span(pts.iter().map(|p| (r.flat(*p) - o).dot(x))) {
                    Some((s0, s1)) if x.length() > 0.5 => (o + x * s0, o + x * s1),
                    _ => (o, o),
                }
            }
        };
        let (u, n) = ((b - a).unit(), (b - a).unit().perp());
        let (Some((c0, c1)), Some((z0, z1))) = (span(pts.iter().map(|p| (r.flat(*p) - a).dot(n))), span(pts.iter().map(|p| p[2]))) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        if a.dist(b) <= TOL || c1 - c0 <= 0.0 || u.length() < 0.5 {
            leave(&mut skipped, &mut seen, id, "it has no length or thickness BIMCraft can read");
            continue;
        }
        let mid = n * ((c0 + c1) / 2.0);
        let t = (c1 - c0) * 100.0;
        let t = t.round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let type_name = kind(&mut m.types.wall, &base, |k| (k.thickness - t).abs() < 0.05, WallType { thickness: t, layers: Vec::new() });
        raw.push(RawWall {
            ifc: id,
            start: micron(a + mid),
            end: micron(b + mid),
            arc: 0.0,
            t,
            z: (z0, z1),
            peak: r.flat(highest(&pts)),
            level: level_for(id, z0),
            type_name,
        });
    }
    meet(&mut raw);
    // Each wall made: its id, its line, and the height of its base; and where it's highest.
    let mut walls: HashMap<u64, (Id, P2, P2, f64)> = HashMap::new();
    let mut peaks: Vec<(Id, P2, f64)> = Vec::new();
    for w in raw {
        let (top_level, height) = topped(w.level, w.z.0, w.z.1);
        let wall = Wall {
            type_name: w.type_name,
            level: w.level,
            start: w.start,
            end: w.end,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset: ((w.z.0 - elevation(w.level)) * 1000.0).round() / 1000.0,
            arc: w.arc,
            location: Default::default(),
            attach_top: None,
            attach_base: None,
        };
        let id = m.insert(Element::Wall(wall));
        from.insert(id, who(w.ifc));
        walls.insert(w.ifc, (id, w.start, w.end, w.z.0));
        peaks.push((id, w.peak, w.z.1));
        seen.insert(w.ifc);
    }

    // The openings through floors, ceilings and roofs: what each voids, its outline in plan at its
    // top, and how high it reaches. Each comes in as a shaft, which takes the hole it cuts in what
    // it voids: a mesh carries the holes cut in it.
    let cuts: Vec<(u64, u64, Vec<P2>, f64, f64)> = named(&["IFCOPENINGELEMENT", "IFCOPENINGSTANDARDCASE"])
        .into_iter()
        .filter_map(|id| {
            let h = *l.host.get(&id)?;
            let host = e(h)?.name.as_str();
            let o = e(id).filter(|_| SLABS.contains(&host) || matches!(host, "IFCCOVERING" | "IFCROOF"))?;
            let faces = r.body(o);
            let (z0, z1) = span(faces.iter().flatten().map(|p| p[2]))?;
            let ring = outline(&faces, Plane::flat(z1), true)?.outer;
            Some((id, h, ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect(), z0, z1))
        })
        .collect();
    // The holes the openings through `id`, or through one of its parts, cut.
    let voided = |id: u64| -> Vec<Vec<P2>> {
        cuts.iter().filter(|(_, h, ..)| *h == id || l.whole.get(h) == Some(&id)).map(|(_, _, ring, ..)| ring.clone()).collect()
    };
    // The holes in the top of slab or ceiling `id` that are its own openings, in plan: less those
    // the openings through it cut, each from its south-west corner, in order: as the same file reads
    // the same.
    let openings_of = |id: u64, holes: Vec<Vec<P2>>| {
        let cut = voided(id);
        let mut openings: Vec<Vec<P2>> = holes
            .into_iter()
            .map(|h| tidy(&h.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect::<Vec<P2>>()))
            .filter(|h| !cut.iter().any(|c| same(h, c)))
            .collect();
        openings
            .sort_by(|a, b| a.first().map(|p| (p.1, p.0)).partial_cmp(&b.first().map(|p| (p.1, p.0))).unwrap_or(std::cmp::Ordering::Equal));
        openings
    };

    // Floors: their top outline and the holes in it, and how thick they are below it.
    let in_whole = |id: u64, names: &[&str]| l.whole.get(&id).and_then(|w| e(*w)).is_some_and(|w| names.contains(&w.name.as_str()));
    let mut roofs: Vec<u64> = named(&["IFCROOF"]).into_iter().filter(|id| !l.whole.contains_key(id)).collect();
    for id in named(SLABS) {
        let Some(s) = e(id) else { continue };
        if in_whole(id, &["IFCROOF", "IFCSTAIR", "IFCRAMP"]) {
            continue;
        }
        match enumeration(s.args.get(8)) {
            Some("ROOF") => {
                roofs.push(id);
                continue;
            }
            Some("LANDING") => {
                leave(&mut skipped, &mut seen, id, "landings aren't supported");
                continue;
            }
            _ => {}
        }
        let faces = r.body(s);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        // Level at its highest, or sloped: then its slope arrow runs up it from its lowest corner,
        // all the way across.
        let shape = outline(&faces, Plane::flat(z1), true).map(|shape| (Plane::flat(z1), shape, z1 - z0)).or_else(|| top_of(&faces));
        let Some((top, geom::Region { outer: ring, holes }, rise)) = shape else {
            leave(&mut skipped, &mut seen, id, "its top isn't flat");
            continue;
        };
        let (tan, flat) = (top.g.length(), |p: P2| micron(r.flat([p.0, p.1, 0.0])));
        let tail = ring.iter().copied().min_by(|a, b| top.at(*a).total_cmp(&top.at(*b))).unwrap_or_default();
        let arrow = (tan > 1e-9).then(|| {
            let u = top.g * (1.0 / tan);
            let reach = ring.iter().map(|q| (*q - tail).dot(u)).fold(0.0, f64::max);
            [flat(tail), flat(tail + u * reach)]
        });
        let t = ((rise * tan.atan().cos()) * 100.0).round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let type_name = kind(&mut m.types.floor, &base, |k| (k.thickness - t).abs() < 0.05, FloorType { thickness: t });
        let z = top.at(tail);
        let level = level_near(z).unwrap_or_else(|| level_for(id, z));
        let floor = Floor {
            type_name,
            level,
            boundary: ring.into_iter().map(flat).collect(),
            openings: openings_of(id, holes),
            offset: ((z - elevation(level)) * 1000.0).round() / 1000.0 + 0.0,
            slope: if arrow.is_some() { (tan.atan().to_degrees() * 1e6).round() / 1e6 } else { 0.0 },
            arrow,
        };
        let fid = m.insert(Element::Floor(floor));
        from.insert(fid, who(id));
        seen.insert(id);
    }

    // Ceilings: coverings that are, their outline and the holes in it, their underside's height and
    // their thickness.
    for id in named(&["IFCCOVERING"]) {
        let Some(c) = e(id).filter(|c| enumeration(c.args.get(8)) == Some("CEILING")) else { continue };
        let faces = r.body(c);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        let Some(geom::Region { outer: ring, holes }) = outline(&faces, Plane::flat(z1), true) else {
            leave(&mut skipped, &mut seen, id, "its top isn't level");
            continue;
        };
        let t = ((z1 - z0) * 100.0).round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let type_name = kind(&mut m.types.ceiling, &base, |k| (k.thickness - t).abs() < 0.05, CeilingType { thickness: t, grid: 0.0 });
        let level = level_for(id, z0);
        let boundary = ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect();
        let offset = ((z0 - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let cid = m.insert(Element::Ceiling(Ceiling { type_name, level, boundary, openings: openings_of(id, holes), offset }));
        from.insert(cid, who(id));
        seen.insert(id);
    }

    // Roofs: flat, else pitched over a rectangle, measured from the faces they turn up.
    for id in roofs {
        let Some(roof) = e(id) else { continue };
        let mut faces = r.body(roof);
        for part in l.parts.get(&id).into_iter().flatten() {
            if let Some(p) = e(*part) {
                faces.extend(r.body(p));
                seen.insert(*part);
            }
        }
        seen.insert(id);
        match roof_from(&r, &faces, &voided(id)) {
            Ok(rd) => {
                let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(rd.t)));
                let type_name = kind(&mut m.types.roof, &base, |k| (k.thickness - rd.t).abs() < 0.05, RoofType { thickness: rd.t });
                let pitched = rd.slope > 0.0;
                let level = if pitched { level_for(id, rd.z) } else { level_near(rd.z).unwrap_or_else(|| level_for(id, rd.z)) };
                // A pitched one reaches its level `overhang` in from its eaves, or its eaves stand
                // `offset` over it; any other sits on it.
                let below = elevation(level) - rd.z;
                let overhang = if pitched && below > 0.0 { below / rd.slope.to_radians().tan() } else { 0.0 };
                let offset = if pitched && below > 0.0 { 0.0 } else { -below };
                let roof = Roof {
                    type_name,
                    level,
                    boundary: rd.boundary,
                    courtyards: rd.courtyards,
                    slope: rd.slope,
                    overhang: (overhang * 1000.0).round() / 1000.0,
                    gable: false,
                    gables: rd.gables,
                    offset: (offset * 100.0).round() / 100.0 + 0.0,
                    openings: rd.openings,
                    join: None,
                    extrusion: rd.extrusion,
                };
                match m.roof_faces(&roof) {
                    Ok(_) => {
                        let rid = m.insert(Element::Roof(roof));
                        from.insert(rid, who(id));
                    }
                    Err(why) => leave(&mut skipped, &mut seen, id, &why),
                }
            }
            Err(why) => leave(&mut skipped, &mut seen, id, &why),
        }
    }

    // A wall whose top follows a roof, as Revit writes attached walls (cut to the roof): its highest
    // point under the roof, and the roof lower than that somewhere over the wall. One rising
    // through a roof is higher than it; one only touching it, along an edge, doesn't reach above it.
    let roofs: Vec<Id> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Roof(_))).map(|(id, _)| *id).collect();
    for (wid, q, top) in peaks {
        let Some(w) = m.wall(wid).cloned() else { continue };
        let axis = Axis::of(&w);
        let along: Vec<P2> = (0..=8).map(|i| axis.at(axis.length() * f64::from(i) / 8.0, 0.0)).collect();
        let under = |rid: Id| {
            walls::underside(&m, rid, q).is_some_and(|z| (z - top).abs() <= 5.0)
                && along.iter().any(|p| walls::underside(&m, rid, *p).is_some_and(|z| z < top - 5.0))
        };
        if let Some(rid) = roofs.iter().copied().find(|rid| under(*rid))
            && let Some(Element::Wall(w)) = m.elements.get_mut(&wid)
        {
            w.attach_top = Some(rid);
        }
    }

    // Shafts: the openings through floors, ceilings and roofs; openings alike (a shaft voids each
    // it passes through), one shaft.
    let mut shafts: Vec<(u64, Vec<P2>, f64, f64)> = Vec::new();
    for (id, _, ring, z0, z1) in cuts {
        let alike = |(_, b, lo, hi): &(u64, Vec<P2>, f64, f64)| (lo - z0).abs() < 1.0 && (hi - z1).abs() < 1.0 && same(b, &ring);
        if !shafts.iter().any(alike) {
            shafts.push((id, ring, z0, z1));
        }
    }
    for (id, boundary, z0, z1) in shafts {
        let level = levels.iter().rev().find(|(_, lz)| *lz <= z0 + 1.0).or(levels.first()).map_or(0, |(id, _)| *id);
        let (top_level, height) = topped(level, z0, z1);
        let base_offset = ((z0 - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let sid = m.insert(Element::Shaft(Shaft { level, boundary, top_level, height: (height * 1000.0).round() / 1000.0, base_offset }));
        from.insert(sid, who(id));
    }

    // Columns: a section across them, and how high they stand.
    for id in named(COLUMNS) {
        let Some(c) = e(id) else { continue };
        let faces = r.body(c);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        // Square to its longest outline edge: a level edge of one of its upright faces (a top
        // made of triangles has diagonals too).
        let f = r.frame(c);
        let mut along = (0.0, P2(f.x[0], f.x[1]).unit());
        for face in faces.iter().filter(|face| normal(face)[2].abs() < 0.1) {
            for (p, q) in face.iter().zip(face.iter().cycle().skip(1)) {
                let d = r.flat(*q) - r.flat(*p);
                if (p[2] - q[2]).abs() < 0.5 && d.length() > along.0 + 1e-6 {
                    along = (d.length(), d.unit());
                }
            }
        }
        let (u, v) = (along.1, along.1.perp());
        let plan: Vec<P2> = faces.iter().flatten().map(|p| r.flat(*p)).collect();
        let (Some((a0, a1)), Some((b0, b1))) = (span(plan.iter().map(|p| p.dot(u))), span(plan.iter().map(|p| p.dot(v)))) else { continue };
        let at = u * ((a0 + a1) / 2.0) + v * ((b0 + b1) / 2.0);
        let radii = span(plan.iter().map(|p| p.dist(at)));
        // Round: many corners, all as far out (a square's four are too).
        let corners: HashSet<(i64, i64)> = plan.iter().map(|p| ((p.0 * 2.0).round() as i64, (p.1 * 2.0).round() as i64)).collect();
        let round = corners.len() >= 12 && radii.is_some_and(|(lo, hi)| hi > 0.0 && (hi - lo) / hi < 0.03);
        let size = |x: f64| (x * 100.0).round() / 100.0;
        let (w, d) = if round {
            (size(2.0 * radii.map_or(0.0, |r| r.1)), size(2.0 * radii.map_or(0.0, |r| r.1)))
        } else {
            (size(a1 - a0), size(b1 - b0))
        };
        if w <= 0.0 || d <= 0.0 {
            leave(&mut skipped, &mut seen, id, "it has no section BIMCraft can read");
            continue;
        }
        let base = label(id)
            .unwrap_or_else(|| if round { format!("Imported Round {}", num(w)) } else { format!("Imported {} x {}", num(w), num(d)) });
        let same = |k: &ColumnType| k.round == round && (k.width - w).abs() < 0.05 && (k.depth - d).abs() < 0.05;
        let type_name = kind(&mut m.types.column, &base, same, ColumnType { width: w, depth: d, round });
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z1);
        let angle = if round { 0.0 } else { (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 };
        let column = Column {
            type_name,
            level,
            at: micron(at),
            angle,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset: ((z0 - elevation(level)) * 1000.0).round() / 1000.0,
        };
        let cid = m.insert(Element::Column(column));
        from.insert(cid, who(id));
        seen.insert(id);
    }

    // Stairs: each flight measured from its treads, a landing between each two and the way the next
    // one turns there. The railings among a stair's parts come in along it.
    let mut railings_on: HashMap<u64, Id> = HashMap::new();
    let flights_alone = named(&["IFCSTAIRFLIGHT"]).into_iter().filter(|id| !in_whole(*id, &["IFCSTAIR"]));
    for id in named(&["IFCSTAIR"]).into_iter().filter(|id| !l.whole.contains_key(id)).chain(flights_alone) {
        let parts: Vec<u64> = l.parts.get(&id).cloned().unwrap_or_default();
        seen.insert(id);
        seen.extend(&parts);
        let of = |name: &str| parts.iter().copied().filter(|p| e(*p).is_some_and(|x| x.name == name)).collect::<Vec<u64>>();
        let flights = of("IFCSTAIRFLIGHT");
        let sources = if flights.is_empty() { vec![id] } else { flights };
        let measured: Result<Vec<Flight>, String> = sources.iter().filter_map(|f| e(*f)).map(|s| stair_from(&r, s, &r.body(s))).collect();
        let mut fl = match measured {
            Ok(fl) if !fl.is_empty() => fl,
            Ok(_) => {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            }
            Err(why) => {
                leave(&mut skipped, &mut seen, id, &why);
                continue;
            }
        };
        fl.sort_by(|a, b| a.6.total_cmp(&b.6));
        let tread = |f: &Flight| if f.3 > 1 { f.5 / f64::from(f.3 - 1) } else { f.5 };
        let Some(&first) = fl.first() else { continue };
        let (start, angle, width, _, _, _, z0) = first;
        if fl.iter().any(|f| (f.2 - width).abs() > 1.0 || (tread(f) - tread(&first)).abs() > 1.0) {
            leave(&mut skipped, &mut seen, id, "its flights aren't all as wide, or their treads as deep");
            continue;
        }
        let risers: u32 = fl.iter().map(|f| f.3).sum();
        let rise: f64 = fl.iter().map(|f| f.4).sum();
        let mut below = 0;
        let landings = fl
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => {
                    below += a.3;
                    Some(Landing { at: f64::from(below), turn: turn_between((a.0, a.1, a.5), (b.0, b.1)) })
                }
                _ => None,
            })
            .collect();
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z0 + rise);
        let height = (height * 1000.0).round() / 1000.0;
        // Its riser, from the rise the model will measure, so it counts the same risers.
        let climb = top_level.map_or(height, |t| elevation(t) - elevation(level));
        let stair = Stair {
            level,
            top_level,
            height,
            start: micron(start),
            angle,
            width: (width * 100.0).round() / 100.0,
            riser: climb / f64::from(risers),
            tread: (tread(&first) * 1000.0).round() / 1000.0,
            landings,
        };
        let sid = m.insert(Element::Stair(stair));
        from.insert(sid, who(id));
        railings_on.extend(of("IFCRAILING").into_iter().map(|p| (p, sid)));
    }

    // Ramps: each flight's slope from its top, landings between them as a stair's.
    for id in named(&["IFCRAMP"]).into_iter().filter(|id| !l.whole.contains_key(id)) {
        let parts: Vec<u64> = l.parts.get(&id).cloned().unwrap_or_default();
        seen.insert(id);
        seen.extend(&parts);
        let of = |name: &str| parts.iter().copied().filter(|p| e(*p).is_some_and(|x| x.name == name)).collect::<Vec<u64>>();
        let flights = of("IFCRAMPFLIGHT");
        let sources = if flights.is_empty() { vec![id] } else { flights };
        let measured: Result<Vec<RampRun>, String> = sources.iter().filter_map(|f| e(*f)).map(|f| ramp_from(&r, &r.body(f))).collect();
        let mut fl = match measured {
            Ok(fl) if !fl.is_empty() => fl,
            Ok(_) => {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            }
            Err(why) => {
                leave(&mut skipped, &mut seen, id, &why);
                continue;
            }
        };
        fl.sort_by(|a, b| a.5.total_cmp(&b.5));
        let Some(&(start, angle, width, _, slope, z0)) = fl.first() else { continue };
        if fl.iter().any(|f| (f.2 - width).abs() > 1.0 || (f.4 - slope).abs() > 0.01) {
            leave(&mut skipped, &mut seen, id, "its flights aren't all as wide, or as steep");
            continue;
        }
        let up = |f: &RampRun| f.5 + f.3 * f.4 / 100.0 - z0;
        let landings = fl
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(Landing { at: (up(a) * 1000.0).round() / 1000.0, turn: turn_between((a.0, a.1, a.3), (b.0, b.1)) }),
                _ => None,
            })
            .collect();
        let rise = fl.last().map_or(0.0, up);
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z0 + rise);
        let ramp = Ramp {
            level,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            start: micron(start),
            angle,
            width: (width * 100.0).round() / 100.0,
            slope: (slope * 1e6).round() / 1e6,
            landings,
        };
        let rid = m.insert(Element::Ramp(ramp));
        from.insert(rid, who(id));
        railings_on.extend(of("IFCRAILING").into_iter().map(|p| (p, rid)));
    }

    // Railings: along their axis, the top of their rail; without one, a straight one along the middle
    // of its body. One among a stair's or a ramp's parts runs along the side its axis starts nearer.
    for id in named(&["IFCRAILING"]) {
        let Some(x) = e(id) else { continue };
        seen.insert(id);
        let Some(axis) = r.axis(x).filter(|a| a.len() >= 2).or_else(|| straight(&r, x)) else {
            leave(&mut skipped, &mut seen, id, "it has no axis for BIMCraft to follow, and it isn't straight");
            continue;
        };
        let first = axis.first().copied().unwrap_or_default();
        let named = label(id);
        let railing = match railings_on.get(&id) {
            Some(&host) => {
                let level = m.elements.get(&host).and_then(|h| m.level_of(h)).unwrap_or(0);
                let on = |side| Railing {
                    type_name: String::new(),
                    level,
                    path: Vec::new(),
                    base_offset: 0.0,
                    host: Some(host),
                    side: Some(side),
                };
                let start = |side| stairs::railing(&m, &on(side)).first().copied();
                let side = [Side::Left, Side::Right]
                    .into_iter()
                    .filter_map(|side| Some((side, start(side)?)))
                    .min_by(|a, b| a.1.0.dist(r.flat(first)).total_cmp(&b.1.0.dist(r.flat(first))));
                let Some((side, (_, z))) = side else { continue };
                let h = first[2] - z;
                let base = named.clone().unwrap_or_else(|| format!("Imported - {}mm", num(h)));
                Railing {
                    type_name: kind(&mut m.types.railing, &base, |t| (t.height - h).abs() < 0.5, RailingType { height: h, spacing: 0.0 }),
                    ..on(side)
                }
            }
            None => {
                let pts: Vec<P3> = r.body(x).into_iter().flatten().collect();
                let span = span(pts.iter().map(|p| p[2])).map_or(0.0, |(lo, hi)| hi - lo);
                let h = named.as_ref().and_then(|n| m.types.railing.get(n)).map_or(span, |t| t.height);
                let base = named.clone().unwrap_or_else(|| format!("Imported - {}mm", num(h)));
                let type_name = kind(&mut m.types.railing, &base, |t| (t.height - h).abs() < 0.5, RailingType { height: h, spacing: 0.0 });
                let level = level_for(id, first[2] - h);
                let path = axis.iter().map(|p| micron(r.flat(*p))).collect();
                let base_offset = ((first[2] - h - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
                Railing { type_name, level, path, base_offset, host: None, side: None }
            }
        };
        let rid = m.insert(Element::Railing(railing));
        from.insert(rid, who(id));
    }

    // Doors and windows: where their openings cut their walls.
    let (mut doors, mut windows) = (0u32, 0u32);
    for id in named(OPENINGS) {
        let Some(o) = e(id) else { continue };
        let is_door = o.name.starts_with("IFCDOOR");
        let opening = l.fills.get(&id).copied();
        let Some(&(host, a, b, base)) = opening.and_then(|op| l.host.get(&op)).and_then(|h| walls.get(h)) else {
            let why = if opening.is_some() { "its wall was left out" } else { "it isn't in a wall" };
            leave(&mut skipped, &mut seen, id, why);
            continue;
        };
        let mut faces = opening.and_then(&e).map(|op| r.body(op)).unwrap_or_default();
        if faces.is_empty() {
            faces = r.body(o);
        }
        let u = (b - a).unit();
        let (Some((s0, s1)), Some((z0, z1))) =
            (span(faces.iter().flatten().map(|p| (r.flat(*p) - a).dot(u))), span(faces.iter().flatten().map(|p| p[2])))
        else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        let size = |x: f64| (x * 100.0).round() / 100.0;
        let width = size(r.len(o.args.get(9)).filter(|w| *w > 0.0).unwrap_or(s1 - s0));
        let height = size(r.len(o.args.get(8)).filter(|h| *h > 0.0).unwrap_or(z1 - z0));
        let sill = size((z0 - base).max(0.0));
        let fallback = format!("Imported {} x {}", num(width), num(height));
        let base_name = label(id).unwrap_or(fallback);
        let same = |k: &OpeningType| (k.width - width).abs() < 0.05 && (k.height - height).abs() < 0.05;
        let new = OpeningType { width, height, sill: if is_door { 0.0 } else { sill } };
        let types = if is_door { &mut m.types.door } else { &mut m.types.window };
        let type_name = kind(types, &base_name, same, new);
        let count = if is_door { &mut doors } else { &mut windows };
        *count += 1;
        let flip_hand = enumeration(o.args.get(11)).is_some_and(|op| op.contains("RIGHT"));
        let op = Opening {
            type_name,
            host,
            offset: micron(P2((s0 + s1) / 2.0, 0.0)).0,
            sill,
            mark: count.to_string(),
            flip_facing: false,
            flip_hand,
        };
        let oid = m.insert(if is_door { Element::Door(op) } else { Element::Window(op) });
        from.insert(oid, who(id));
        seen.insert(id);
    }

    // Rooms: a point inside their footprint.
    let numbers: Vec<String> = named(&["IFCSPACE"])
        .into_iter()
        .filter_map(&e)
        .filter(|s| text(s.args.get(7)).is_some())
        .filter_map(|s| text(s.args.get(2)).map(String::from))
        .collect();
    let mut numbers: BTreeSet<String> = numbers.into_iter().collect();
    for id in named(&["IFCSPACE"]) {
        let Some(s) = e(id) else { continue };
        let faces = r.body(s);
        let floor = span(faces.iter().flatten().map(|p| p[2])).map(|(z0, _)| z0);
        let footprint =
            floor.and_then(|z0| outline(&faces, Plane::flat(z0), false)).and_then(|g| inside(&g.outer)).map(|p| r.flat([p.0, p.1, 0.0]));
        let Some(point) = footprint.or_else(|| faces.is_empty().then(|| r.flat(r.frame(s).o))) else {
            leave(&mut skipped, &mut seen, id, "its footprint isn't one BIMCraft can read");
            continue;
        };
        let (long, short) = (text(s.args.get(7)), text(s.args.get(2)));
        let (room_name, number) = match (long, short) {
            (Some(n), Some(num)) => (n.to_string(), num.to_string()),
            (Some(n), None) | (None, Some(n)) => {
                let num = next_number(numbers.iter().map(String::as_str));
                numbers.insert(num.clone());
                (n.to_string(), num)
            }
            (None, None) => {
                let num = next_number(numbers.iter().map(String::as_str));
                numbers.insert(num.clone());
                ("Room".to_string(), num)
            }
        };
        let level = level_for(id, floor.unwrap_or(0.0));
        let rid = m.insert(Element::Room(Room { level, point: micron(point), name: clip(&room_name), number: clip(&number) }));
        from.insert(rid, who(id));
        seen.insert(id);
    }

    // Grids: each axis a grid line.
    let mut grid_names = BTreeSet::new();
    for id in named(&["IFCGRID"]) {
        let Some(g) = e(id) else { continue };
        let f = r.frame(g);
        seen.insert(id);
        for axis in [7, 8, 9].into_iter().flat_map(|i| refs(g.args.get(i))).filter_map(&e) {
            let Some(pts) = r.curve(reference(axis.args.get(1)), 0) else { continue };
            let (Some(p), Some(q)) = (pts.first(), pts.last()) else { continue };
            let (mut start, mut end) = (micron(r.flat(f.at(*p))), micron(r.flat(f.at(*q))));
            if enumeration(axis.args.get(2)) == Some("F") {
                (start, end) = (end, start);
            }
            let tag = text(axis.args.first()).map_or_else(|| next_number(grid_names.iter().map(String::as_str)), String::from);
            let gid = m.insert(Element::Grid(Grid { name: fresh(&mut grid_names, &tag), start, end }));
            from.insert(gid, who(id));
        }
    }

    // What breaks a rule is left out, with the rule; a wall takes its doors and windows along.
    for _ in 0..=m.elements.len() {
        let Err(error) = m.check() else { break };
        let id = culprit(&error).filter(|id| from.contains_key(id)).ok_or_else(|| error.clone())?;
        let why = error.split_once(": ").map_or(error.as_str(), |(_, why)| why);
        skipped.push(format!("{}: {why}", from.get(&id).cloned().unwrap_or_default()));
        m.elements.remove(&id);
        let hosted: Vec<Id> = m
            .elements
            .iter()
            .filter_map(|(k, x)| match x {
                Element::Door(o) | Element::Window(o) if o.host == id => Some(*k),
                _ => None,
            })
            .collect();
        for h in hosted {
            skipped.push(format!("{}: its wall was left out", from.get(&h).cloned().unwrap_or_default()));
            m.elements.remove(&h);
        }
    }
    m.check()?;

    // The rest of what's in the building, by kind.
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for id in l.within.keys().filter(|id| !seen.contains(id)) {
        if let Some(x) = e(*id) {
            *left.entry(x.name.as_str()).or_default() += 1;
        }
    }
    skipped.extend(
        left.into_iter().map(|(name, n)| format!("{n} {name}: BIMCraft has no element for {}", if n == 1 { "it" } else { "them" })),
    );
    let moved = (r.shift != P2(0.0, 0.0)).then_some(r.shift * -1.0);
    Ok(Imported { model: m, schema: file.schema.clone(), skipped, moved })
}

/// A roof as `roof_from` reads it: over its footprint, round its courtyards, less its openings,
/// flat or each edge but its gables sloping at `slope`; or a profile swept across, its heights up
/// from `z`. Its thickness, and `z`: how high its underside is at its eaves, or its profile's 0.
struct RoofRead {
    boundary: Vec<P2>,
    courtyards: Vec<Vec<P2>>,
    openings: Vec<Vec<P2>>,
    slope: f64,
    gables: Vec<usize>,
    extrusion: Option<Extrusion>,
    t: f64,
    z: f64,
}

fn roof_from(r: &Reader, faces: &[Vec<P3>], cut: &[Vec<P2>]) -> Result<RoofRead, String> {
    let round = |x: f64| (x * 100.0).round() / 100.0;
    let faces: Vec<Vec<P3>> = faces
        .iter()
        .map(|f| {
            f.iter()
                .map(|p| {
                    let q = r.flat(*p);
                    [q.0, q.1, p[2]]
                })
                .collect()
        })
        .collect();
    let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
        return Err("it has no body BIMCraft can read".into());
    };
    // Its faces turned up, each with its plane (none for a sliver of a mesh's): their loops its
    // outline and its openings.
    let up: Vec<(&Vec<P3>, Plane)> =
        faces.iter().filter(|f| f.len() >= 3 && normal(f)[2] > 0.01 && area3(f) > 1.0).filter_map(|f| Some((f, Plane::of(f)?))).collect();
    let mut rings: Vec<Vec<P2>> =
        loops(up.iter().map(|(f, _)| *f)).into_iter().map(|l| geom::corners(&l)).filter(|l| l.len() >= 3).collect();
    rings.sort_by(|a, b| geom::area(b).abs().total_cmp(&geom::area(a).abs()));
    // Each from its south-west corner, the outline counter-clockwise, the openings in order: as
    // the same file reads the same.
    let mut rings = rings.into_iter().map(|l| {
        let mut l: Vec<P2> = l.into_iter().map(micron).collect();
        let first = l.iter().enumerate().min_by(|(_, a), (_, b)| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap_or(std::cmp::Ordering::Equal));
        let first = first.map_or(0, |(i, _)| i);
        l.rotate_left(first);
        l
    });
    let mut boundary = rings.next().ok_or("its top has no outline")?;
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
        boundary.rotate_right(1);
    }
    // Its holes, less those the openings through it `cut`, courtyards or not.
    let mut openings: Vec<Vec<P2>> = rings.filter(|h| !cut.iter().any(|c| same(h, c))).collect();
    openings.sort_by(|a, b| a.first().map(|p| (p.1, p.0)).partial_cmp(&b.first().map(|p| (p.1, p.0))).unwrap_or(std::cmp::Ordering::Equal));
    let sloped: Vec<&(&Vec<P3>, Plane)> = up.iter().filter(|(_, p)| p.g.length() > 0.02).collect();
    let Some(&&(main, along)) = sloped.iter().max_by(|a, b| area3(a.0).total_cmp(&area3(b.0))) else {
        return Ok(RoofRead {
            boundary,
            courtyards: Vec::new(),
            openings,
            slope: 0.0,
            gables: Vec::new(),
            extrusion: None,
            t: round(z1 - z0),
            z: z0,
        });
    };
    // Its thickness: from its largest slope's top to the underside parallel to it.
    let n = normal(main);
    let top = dot(n, main.first().copied().unwrap_or_default());
    let t = faces
        .iter()
        .filter(|f| dot(normal(f), n) < -0.999)
        .map(|f| (top - dot(n, f.first().copied().unwrap_or_default())).abs())
        .filter(|t| *t > 1.0)
        .fold(f64::INFINITY, f64::min);
    if !t.is_finite() {
        return Err("its underside isn't parallel to its top".into());
    }
    let t = round(t);
    let rise = |p: &Plane| t * p.g.length().hypot(1.0);
    // One slope all round: each slope rising from an edge of its footprint at its eaves, where the
    // top is lowest; an edge whose middle is higher stands upright.
    let tan = along.g.length();
    let slope = round(tan.atan().to_degrees());
    let lowest = up.iter().flat_map(|(f, _)| f.iter().map(|p| p[2])).fold(f64::INFINITY, f64::min);
    let touches = |f: &Vec<P3>, q: P2| {
        let ring: Vec<P2> = f.iter().map(|p| P2(p[0], p[1])).collect();
        geom::contains(&ring, q) || geom::ring_edges(&ring).any(|(a, b)| geom::dist_to_segment(q, a, b) < 1.0)
    };
    let top_at = |q: P2| up.iter().filter(|(f, _)| touches(f, q)).map(|(_, p)| p.at(q)).fold(f64::INFINITY, f64::min);
    // A hole with eaves along it is a courtyard, clockwise as the slopes round it rise from it; one
    // cut through the slopes, an opening.
    let (courtyards, openings): (Vec<Vec<P2>>, Vec<Vec<P2>>) =
        openings.into_iter().partition(|h| geom::ring_edges(h).any(|(a, b)| top_at((a + b) * 0.5) <= lowest + 1.0));
    let courtyards: Vec<Vec<P2>> = courtyards
        .into_iter()
        .map(|mut c| {
            if geom::area(&c) > 0.0 {
                c.reverse();
                c.rotate_right(1);
            }
            c
        })
        .collect();
    let rings: Vec<&Vec<P2>> = std::iter::once(&boundary).chain(&courtyards).collect();
    let edges: Vec<(P2, P2)> = rings.iter().flat_map(|r| geom::ring_edges(r)).collect();
    let gables: Vec<usize> =
        (0..edges.len()).filter(|i| edges.get(*i).is_some_and(|(a, b)| top_at((*a + *b) * 0.5) > lowest + 1.0)).collect();
    let eave = |(a, b): &(P2, P2)| {
        let inward = (*b - *a).unit().perp();
        Plane { z: lowest - tan * a.dot(inward), g: inward * tan }
    };
    let eaves: Vec<Plane> = edges.iter().enumerate().filter(|(i, _)| !gables.contains(i)).map(|(_, e)| eave(e)).collect();
    let fits = |(f, p): &&(&Vec<P3>, Plane)| {
        let q = f.first().map_or(P2(0.0, 0.0), |q| P2(q[0], q[1]));
        eaves.iter().any(|e| e.g.dist(p.g) < 1e-4 && (e.at(q) - p.at(q)).abs() < 1.0)
    };
    let speeds: Vec<f64> = (0..edges.len()).map(|i| if gables.contains(&i) { 0.0 } else { 1.0 }).collect();
    let ridge = crate::skeleton::faces(&rings, &speeds)
        .map(|f| f.into_iter().flatten().flatten().map(|(_, k)| lowest + k * tan).fold(lowest, f64::max))
        .unwrap_or(f64::NAN);
    if sloped.iter().all(|(_, p)| (p.g.length() - tan).abs() < 1e-4) && sloped.iter().all(fits) && (ridge - z1).abs() < 1.0 {
        return Ok(RoofRead { boundary, courtyards, openings, slope, gables, extrusion: None, t, z: lowest - rise(&along) });
    }
    // Else one profile swept across: its slopes all one way, its underside's heights along it.
    let u = along.g.unit();
    let u = if u.0 > 1e-9 || (u.0.abs() <= 1e-9 && u.1 > 0.0) { u } else { u * -1.0 };
    if up.iter().any(|(_, p)| p.g.cross(u).abs() > 1e-4) {
        return Err("its slopes don't make a roof BIMCraft draws: one slope all round, or one profile swept across".into());
    }
    let unders: Vec<(f64, f64, Plane)> = up
        .iter()
        .filter_map(|(f, p)| {
            let (s0, s1) = span(f.iter().map(|q| P2(q[0], q[1]).dot(u)))?;
            Some((s0, s1, Plane { z: p.z - rise(p), ..*p }))
        })
        .collect();
    let mut cuts: Vec<f64> = unders.iter().flat_map(|(s0, s1, _)| [*s0, *s1]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    let at = |s: f64, p: &Plane| p.z + p.g.dot(u) * s;
    let mut profile: Vec<P2> = Vec::new();
    for w in cuts.windows(2) {
        let &[a, b] = w else { continue };
        let p = unders
            .iter()
            .find(|(s0, s1, _)| *s0 <= (a + b) / 2.0 && (a + b) / 2.0 <= *s1)
            .map(|(.., p)| *p)
            .ok_or("its slopes leave a gap")?;
        match profile.last() {
            Some(last) if (last.1 - at(a, &p)).abs() > 1.0 => return Err("its underside steps up or down".into()),
            Some(_) => {}
            None => profile.push(P2(a, at(a, &p))),
        }
        // Straight on from the stretch before, one stretch.
        if let [.., x, y] = profile.as_slice()
            && ((y.1 - x.1) / (y.0 - x.0) - p.g.dot(u)).abs() < 1e-6
        {
            profile.pop();
        }
        profile.push(P2(b, at(b, &p)));
    }
    let across: Vec<f64> = up.iter().flat_map(|(f, _)| f.iter().map(|q| P2(q[0], q[1]).dot(u.perp()))).collect();
    let (Some((c0, c1)), Some(first), Some(last)) = (span(across.into_iter()), profile.first().copied(), profile.last().copied()) else {
        return Err("it has no body BIMCraft can read".into());
    };
    if ((last.0 - first.0) * (c1 - c0) - geom::area(&boundary)).abs() > 0.01 * geom::area(&boundary)
        || !openings.is_empty()
        || !courtyards.is_empty()
    {
        return Err("its outline isn't the rectangle its profile sweeps".into());
    }
    let z = profile.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let o = u * first.0 + u.perp() * c0;
    let line = [micron(o), micron(o + u * (last.0 - first.0))];
    let profile = profile.into_iter().map(|p| P2(round(p.0 - first.0), round(p.1 - z))).collect();
    Ok(RoofRead {
        boundary: Vec::new(),
        courtyards: Vec::new(),
        openings,
        slope: 0.0,
        gables: Vec::new(),
        extrusion: Some(Extrusion { line, profile, from: 0.0, to: round(c1 - c0) }),
        t,
        z,
    })
}

/// A straight flight: where its first riser's middle is, the way it climbs (degrees), its width,
/// how many risers, how high they climb, its run, and the height its first riser starts from.
type Flight = (P2, f64, f64, u32, f64, f64, f64);

/// A ramp's straight flight: the middle of its foot, the way it climbs (degrees), its width, its
/// run, its slope (percent), and the height of its foot.
type RampRun = (P2, f64, f64, f64, f64, f64);

/// How the run from one flight (its start, the way it climbs in degrees, its length) to the next
/// (its start and way) turns at the landing between: straight on, left or right, or back to
/// whichever side it lies on; not a turn BIMCraft makes, not a number.
fn turn_between((a, along, _): (P2, f64, f64), (b, toward): (P2, f64)) -> f64 {
    let d = (toward - along).rem_euclid(360.0);
    let d = if d > 180.0 { d - 360.0 } else { d };
    match d {
        d if d.abs() < 1.0 => 0.0,
        d if (d - 90.0).abs() < 1.0 => 90.0,
        d if (d + 90.0).abs() < 1.0 => -90.0,
        d if d.abs() > 179.0 => {
            if heading(along).cross(b - a) > 0.0 {
                180.0
            } else {
                -180.0
            }
        }
        _ => f64::NAN,
    }
}

/// A railing with no axis, straight and thin: the line along the middle of its body, along its
/// own x or y, at its top.
fn straight(r: &Reader, x: &Entity) -> Option<Vec<P3>> {
    let pts: Vec<P3> = r.body(x).into_iter().flatten().collect();
    let f = r.frame(x);
    let (_, top) = span(pts.iter().map(|p| p[2]))?;
    let spread = |u: P2| span(pts.iter().map(|p| P2(p[0], p[1]).dot(u)));
    let along = [P2(f.x[0], f.x[1]), P2(f.y[0], f.y[1])]
        .into_iter()
        .filter(|u| u.length() > 0.5)
        .map(|u| u.unit())
        .max_by(|a, b| spread(*a).map_or(0.0, |(lo, hi)| hi - lo).total_cmp(&spread(*b).map_or(0.0, |(lo, hi)| hi - lo)))?;
    let ((s0, s1), (c0, c1)) = (spread(along)?, spread(along.perp())?);
    // Thin: no wider than a rail and its balusters could be.
    (c1 - c0 <= 300.0 && s1 - s0 > TOL).then(|| {
        let at = |s: f64| {
            let p = along * s + along.perp() * ((c0 + c1) / 2.0);
            [p.0, p.1, top]
        };
        vec![at(s0), at(s1)]
    })
}

/// A ramp's flight, from its sloping top: up its slope, across it, and how steep.
fn ramp_from(r: &Reader, faces: &[Vec<P3>]) -> Result<RampRun, String> {
    let (top, geom::Region { outer: ring, .. }, _) = top_of(faces).ok_or("its top isn't one flat face BIMCraft can read")?;
    let tan = top.g.length();
    if tan < 1e-6 {
        return Err("its top is level".into());
    }
    let (u, n) = (top.g * (1.0 / tan), (top.g * (1.0 / tan)).perp());
    let (Some((s0, s1)), Some((c0, c1))) = (span(ring.iter().map(|p| p.dot(u))), span(ring.iter().map(|p| p.dot(n)))) else {
        return Err("it has no body BIMCraft can read".into());
    };
    let foot = u * s0 + n * ((c0 + c1) / 2.0);
    let angle = (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 + 0.0;
    Ok((r.flat([foot.0, foot.1, 0.0]), angle, c1 - c0, s1 - s0, tan * 100.0, top.at(foot)))
}

fn stair_from(r: &Reader, s: &Entity, faces: &[Vec<P3>]) -> Result<Flight, String> {
    let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
        return Err("it has no body BIMCraft can read".into());
    };
    // The treads: faces turned up, above the floor, one height each.
    let mut treads: Vec<(f64, P2)> = Vec::new();
    for f in faces.iter().filter(|f| normal(f)[2] > 0.99) {
        let z = f.iter().map(|p| p[2]).sum::<f64>() / f.len() as f64;
        let c = f.iter().fold(P2(0.0, 0.0), |c, p| c + r.flat(*p)) * (1.0 / f.len() as f64);
        if z > z0 + 1.0 && !treads.iter().any(|(tz, _)| (tz - z).abs() < 1.0) {
            treads.push((z, c));
        }
    }
    treads.sort_by(|a, b| a.0.total_cmp(&b.0));
    // A flight's own count, else one riser up to each tread and the last up to the floor above.
    let counted = real(s.args.get(8)).filter(|_| s.name == "IFCSTAIRFLIGHT").filter(|n| (2.0..=200.0).contains(n)).map(|n| n as u32);
    let risers = counted.unwrap_or(u32::try_from(treads.len()).unwrap_or(0) + 1);
    if risers < 2 {
        return Err("BIMCraft can't find its treads".into());
    }
    let each = r.len(s.args.get(10)).filter(|_| counted.is_some()).filter(|h| *h > 0.0).unwrap_or((z1 - z0) / f64::from(risers - 1));
    // It climbs along whichever of its own axes its treads spread out along.
    let f = r.frame(s);
    let axes = [P2(f.x[0], f.x[1]).unit(), P2(f.y[0], f.y[1]).unit()];
    let spread = |u: &P2| span(treads.iter().map(|(_, c)| c.dot(*u))).map_or(0.0, |(lo, hi)| hi - lo);
    let mut u =
        axes.into_iter().filter(|u| u.length() > 0.5).max_by(|a, b| spread(a).total_cmp(&spread(b))).ok_or("it doesn't stand upright")?;
    if let (Some((_, low)), Some((_, high))) = (treads.first(), treads.last())
        && (*high - *low).dot(u) < 0.0
    {
        u = u * -1.0;
    }
    let plan: Vec<P2> = faces.iter().flatten().map(|p| r.flat(*p)).collect();
    let n = u.perp();
    let (Some((s0, s1)), Some((c0, c1))) = (span(plan.iter().map(|p| p.dot(u))), span(plan.iter().map(|p| p.dot(n)))) else {
        return Err("it has no body BIMCraft can read".into());
    };
    let start = u * s0 + n * ((c0 + c1) / 2.0);
    let angle = (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 + 0.0;
    let from = treads.first().map_or(z0, |(z, _)| z - each);
    Ok((start, angle, c1 - c0, risers, each * f64::from(risers), s1 - s0, from))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An IFC4 file in mm whose wall is a triangle mesh with no axis, running north, and whose
    /// column takes its shape from its type through a mapped item; `site` places it all.
    fn meshes(site: &str) -> String {
        let pts = "(0.,-100.,0.),(5000.,-100.,0.),(5000.,100.,0.),(0.,100.,0.),(0.,-100.,2700.),(5000.,-100.,2700.),(5000.,100.,2700.),(0.,100.,2700.)";
        let tris = "(1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)";
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Meshes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT(#62,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINTLIST3D(({pts}));
#11=IFCTRIANGULATEDFACESET(#10,$,.T.,({tris}),$);
#12=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#11));
#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));
#14=IFCCARTESIANPOINT((1000.,2000.,0.));
#15=IFCDIRECTION((0.,1.,0.));
#16=IFCDIRECTION((0.,0.,1.));
#17=IFCAXIS2PLACEMENT3D(#14,#16,#15);
#18=IFCLOCALPLACEMENT(#6,#17);
#19=IFCWALL('w',$,'Wall',$,$,#18,#13,$,$);
#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,300.,500.);
#31=IFCEXTRUDEDAREASOLID(#30,$,#16,3000.);
#32=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#31));
#33=IFCREPRESENTATIONMAP(#5,#32);
#34=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$);
#35=IFCMAPPEDITEM(#33,#34);
#36=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#35));
#37=IFCPRODUCTDEFINITIONSHAPE($,$,(#36));
#38=IFCCARTESIANPOINT((3000.,4000.,0.));
#39=IFCAXIS2PLACEMENT3D(#38,#16,$);
#40=IFCLOCALPLACEMENT(#6,#39);
#41=IFCCOLUMN('c',$,'Column',$,$,#40,#37,$,$);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#19,#41),#7);
#60=IFCCARTESIANPOINT(({site}));
#61=IFCAXIS2PLACEMENT3D(#60,$,$);
#62=IFCLOCALPLACEMENT($,#61);
ENDSEC;END-ISO-10303-21;"
        )
    }

    fn only<'a>(m: &'a Model, cat: &str) -> Vec<&'a Element> {
        m.elements.values().filter(|e| e.category() == cat).collect()
    }

    #[test]
    fn meshes_and_mapped_shapes_come_in_as_extrusions_do() {
        let i = ifc(meshes("0.,0.,0.").as_bytes(), "x").unwrap();
        assert_eq!((i.skipped.len(), i.moved), (0, None), "{:?}", i.skipped);
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        assert_eq!((w.start, w.end, i.model.thickness(w), w.height), (P2(1000.0, 2000.0), P2(1000.0, 7000.0), 200.0, 2700.0));
        let Some(Element::Column(c)) = only(&i.model, "column").first().copied() else { panic!("no column") };
        let t = &i.model.types.column[&c.type_name];
        assert_eq!((c.at, c.angle, t.width, t.depth, t.round, c.height), (P2(3000.0, 4000.0), 90.0, 500.0, 300.0, false, 3000.0));
    }

    /// An IFC4 file in mm with a 6 x 4 m slab 200 mm thick, its profile round two holes, an
    /// opening element voiding it through the east one, as a mesh would carry it: `slab` makes the
    /// slab #35, and what it's part of; the storey holds `within`.
    fn holed(slab: &str, within: u32) -> String {
        let rect = |id: u32, (x0, y0): (f64, f64), (x1, y1): (f64, f64)| {
            let pts: Vec<String> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .enumerate()
                .map(|(i, (x, y))| format!("#{}=IFCCARTESIANPOINT(({x:.1},{y:.1}));", id + 1 + i as u32))
                .collect();
            format!("{}\n#{id}=IFCPOLYLINE((#{},#{},#{},#{},#{}));", pts.join("\n"), id + 1, id + 2, id + 3, id + 4, id + 1)
        };
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Holes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,-200.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,$,$,$,.ELEMENT.,0.);
{}
{}
{}
#30=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#10,(#20,#25));
#31=IFCDIRECTION((0.,0.,1.));
#32=IFCEXTRUDEDAREASOLID(#30,$,#31,200.);
#33=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#32));
#34=IFCPRODUCTDEFINITIONSHAPE($,$,(#33));
{slab}
#40=IFCCARTESIANPOINT((4500.,1500.));
#41=IFCAXIS2PLACEMENT2D(#40,$);
#42=IFCRECTANGLEPROFILEDEF(.AREA.,$,#41,1000.,1000.);
#43=IFCEXTRUDEDAREASOLID(#42,$,#31,200.);
#44=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#43));
#45=IFCPRODUCTDEFINITIONSHAPE($,$,(#44));
#46=IFCOPENINGELEMENT('o',$,'Hole',$,$,#6,#45,$,.OPENING.);
#47=IFCRELVOIDSELEMENT('v',$,$,$,#35,#46);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#{within}),#7);
ENDSEC;END-ISO-10303-21;",
            rect(10, (0.0, 0.0), (6000.0, 4000.0)),
            rect(20, (1000.0, 1000.0), (2500.0, 2000.0)),
            rect(25, (4000.0, 1000.0), (5000.0, 2000.0)),
        )
    }

    #[test]
    fn holes_in_a_slab_come_in_as_its_openings_or_as_the_shafts_through_it() {
        let i = ifc(holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let Some(Element::Floor(f)) = only(&i.model, "floor").first().copied() else { panic!("no floor") };
        assert_eq!(f.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
        let Some(Element::Shaft(sh)) = only(&i.model, "shaft").first().copied() else { panic!("no shaft") };
        assert_eq!(geom::area(&sh.boundary).abs(), 1_000_000.0);
        let floor = i.model.elements.iter().find(|(_, e)| e.category() == "floor").map(|(id, _)| *id);
        assert_eq!(crate::element_json(&i.model, floor.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
    }

    #[test]
    fn holes_in_a_ceiling_come_in_as_its_openings_or_as_the_shafts_through_it() {
        let i = ifc(holed("#35=IFCCOVERING('f',$,'Ceiling',$,$,#6,#34,$,.CEILING.);", 35).as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let Some(Element::Ceiling(c)) = only(&i.model, "ceiling").first().copied() else { panic!("no ceiling") };
        assert_eq!(c.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
        assert_eq!(only(&i.model, "shaft").len(), 1);
        let ceiling = i.model.elements.iter().find(|(_, e)| e.category() == "ceiling").map(|(id, _)| *id);
        assert_eq!(crate::element_json(&i.model, ceiling.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
    }

    #[test]
    fn holes_in_a_roof_come_in_as_its_openings_or_as_the_shafts_through_it() {
        // The slab as a roof, then as the part of one: the opening voids it either way.
        let part = "#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.ROOF.);
#36=IFCROOF('r',$,'Roof',$,$,#6,$,$,.FLAT_ROOF.);
#37=IFCRELAGGREGATES('a',$,$,$,#36,(#35));";
        for (slab, within) in [("#35=IFCROOF('f',$,'Roof',$,$,#6,#34,$,.FLAT_ROOF.);", 35), (part, 36)] {
            let i = ifc(holed(slab, within).as_bytes(), "x").unwrap();
            assert_eq!(i.skipped, Vec::<String>::new());
            let Some(Element::Roof(roof)) = only(&i.model, "roof").first().copied() else { panic!("no roof") };
            assert_eq!(roof.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
            assert_eq!(only(&i.model, "shaft").len(), 1);
        }
    }

    #[test]
    fn a_file_reads_the_same_every_time() {
        // Its slab's and its shaft's rings, and a second project, in metres, after its own: each
        // read hashes afresh, so whatever came in a hash's order would change from read to read.
        let more = "#60=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#61=IFCUNITASSIGNMENT((#60));\n#62=IFCPROJECT('q',$,'Other',$,$,$,$,$,#61);";
        let file = holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).replace("ENDSEC;END", &format!("{more}\nENDSEC;END"));
        // Each read is a project of its own, its uid seeding its own GlobalIds; the rest the same.
        let read = || Model { uid: 0, ..ifc(file.as_bytes(), "x").unwrap().model };
        let first = read();
        for _ in 0..8 {
            assert_eq!(read(), first);
        }
    }

    #[test]
    fn a_model_at_map_coordinates_comes_near_the_origin() {
        let i = ifc(meshes("500000000.,5000000000.,0.").as_bytes(), "x").unwrap();
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(0.0, 5000.0)));
        assert_eq!(i.moved, Some(P2(-500_001_000.0, -5_000_002_000.0)));
    }
}
