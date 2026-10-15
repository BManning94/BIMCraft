//! IFC import: an IFC2x3 or IFC4 file, as Revit, other BIM tools and BIMCraft itself write them,
//! read into a model. Storeys become levels. Straight walls and the doors and windows in them,
//! floors, rooms, columns, grids, single straight stairs, and roofs (flat, or pitched over a
//! rectangle) come in as BIMCraft elements, each measured from its geometry: an extruded
//! profile, a Brep or a triangle mesh alike, so how a file describes an element doesn't matter.
//! What BIMCraft has no element for, or what breaks one of its rules, is left out and listed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::PI;

use crate::commands::{micron, next_number};
use crate::geom::{self, P2};
use crate::model::{
    Column, ColumnType, Element, Floor, FloorType, Grid, Id, Level, MAX_TEXT, Model, Opening, OpeningType, Roof, RoofType, Room, Stair,
    TOL, Wall, WallType, culprit,
};
use crate::step::{self, Entity, File, V};
use crate::view3d::{P3, add, cross, dot, mul, sub, unit};

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

/// The outline of the level faces at height `z` (within half a millimetre) facing up, or
/// down: the edges of theirs no two share, chained into loops. The largest loop, its corners
/// counter-clockwise.
fn outline(faces: &[Vec<P3>], z: f64, up: bool) -> Option<Vec<P2>> {
    type K = (i64, i64);
    let key = |p: P2| -> K { ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64) };
    let mut edges: HashMap<(K, K), P2> = HashMap::new();
    for f in faces.iter().filter(|f| f.len() >= 3 && f.iter().all(|p| (p[2] - z).abs() <= 0.5)) {
        let n = normal(f);
        if n[2].abs() < 0.9 || (n[2] > 0.0) != up {
            continue;
        }
        let mut ring: Vec<P2> = f.iter().map(|p| P2(p[0], p[1])).collect();
        if geom::area(&ring) < 0.0 {
            ring.reverse();
        }
        for (a, b) in geom::ring_edges(&ring) {
            let (ka, kb) = (key(a), key(b));
            // An edge two faces share runs both ways, and lies inside the outline.
            if ka != kb && edges.remove(&(kb, ka)).is_none() {
                edges.insert((ka, kb), a);
            }
        }
    }
    let mut next: HashMap<K, (K, P2)> = edges.into_iter().map(|((ka, kb), a)| (ka, (kb, a))).collect();
    let mut best: Option<Vec<P2>> = None;
    while let Some(&start) = next.keys().next() {
        let (mut ring, mut k) = (Vec::new(), start);
        while let Some((kb, a)) = next.remove(&k) {
            ring.push(a);
            k = kb;
        }
        if ring.len() >= 3 && best.as_ref().is_none_or(|b| geom::area(&ring).abs() > geom::area(b).abs()) {
            best = Some(ring);
        }
    }
    let mut ring = geom::corners(&best?);
    if geom::area(&ring) < 0.0 {
        ring.reverse();
    }
    (ring.len() >= 3).then_some(ring)
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
                    prism(out, ring.iter().map(|p| pos.at([p.0, p.1, 0.0])).collect(), up);
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
    let project = f.entities.values().find(|e| e.name == "IFCPROJECT").ok_or("it has no IfcProject")?;
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

/// A wall as the file has it, before walls that nearly meet are made to meet.
struct RawWall {
    ifc: u64,
    start: P2,
    end: P2,
    t: f64,
    z: (f64, f64),
    level: Id,
    type_name: String,
}

/// Walls that nearly meet, made to meet: an end within a wall's thickness of another wall's centre
/// line moves onto it, so corners and T-junctions join as BIMCraft joins walls.
fn meet(walls: &mut [RawWall]) {
    for i in 0..walls.len() {
        for at_end in [false, true] {
            let Some(w) = walls.get(i) else { continue };
            let (a, b, p, t, z) = (w.start, w.end, if at_end { w.end } else { w.start }, w.t, w.z);
            if walls.iter().enumerate().any(|(j, o)| j != i && (o.start.dist(p) <= TOL || o.end.dist(p) <= TOL)) {
                continue;
            }
            let mut best: Option<(f64, P2)> = None;
            for o in walls.iter().enumerate().filter(|(j, o)| *j != i && o.z.0 < z.1 && z.0 < o.z.1).map(|(_, o)| o) {
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
        let level = m.insert(Element::Level(Level { name: fresh(&mut taken, n), elevation: *z }));
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
        if axis.as_ref().is_some_and(|a| {
            let (p, q) = (a.first().copied().unwrap_or_default(), a.last().copied().unwrap_or_default());
            a.iter().any(|x| geom::dist_to_segment(r.flat(*x), r.flat(p), r.flat(q)) > 1.0)
        }) {
            leave(&mut skipped, &mut seen, id, "curved walls aren't supported");
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
        let type_name = kind(&mut m.types.wall, &base, |k| (k.thickness - t).abs() < 0.05, WallType { thickness: t });
        raw.push(RawWall { ifc: id, start: micron(a + mid), end: micron(b + mid), t, z: (z0, z1), level: level_for(id, z0), type_name });
    }
    meet(&mut raw);
    // Each wall made: its id, its line, and the height of its base.
    let mut walls: HashMap<u64, (Id, P2, P2, f64)> = HashMap::new();
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
        };
        let id = m.insert(Element::Wall(wall));
        from.insert(id, who(w.ifc));
        walls.insert(w.ifc, (id, w.start, w.end, w.z.0));
        seen.insert(w.ifc);
    }

    // Floors: their top outline, and how thick they are below it.
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
        let Some(ring) = outline(&faces, z1, true) else {
            leave(&mut skipped, &mut seen, id, "its top isn't level");
            continue;
        };
        let t = ((z1 - z0) * 100.0).round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let type_name = kind(&mut m.types.floor, &base, |k| (k.thickness - t).abs() < 0.05, FloorType { thickness: t });
        let level = level_near(z1).unwrap_or_else(|| level_for(id, z1));
        let boundary = ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect();
        let fid = m.insert(Element::Floor(Floor { type_name, level, boundary }));
        from.insert(fid, who(id));
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
        match roof_from(&r, &faces) {
            Ok((boundary, slope, gable, t, eaves)) => {
                let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
                let type_name = kind(&mut m.types.roof, &base, |k| (k.thickness - t).abs() < 0.05, RoofType { thickness: t });
                let level = if slope == 0.0 { level_near(eaves).unwrap_or_else(|| level_for(id, eaves)) } else { level_for(id, eaves) };
                let overhang = if slope == 0.0 { 0.0 } else { (elevation(level) - eaves) / slope.to_radians().tan() };
                let rid = m.insert(Element::Roof(Roof {
                    type_name,
                    level,
                    boundary,
                    slope,
                    overhang: (overhang * 1000.0).round() / 1000.0,
                    gable,
                }));
                from.insert(rid, who(id));
            }
            Err(why) => leave(&mut skipped, &mut seen, id, &why),
        }
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

    // Stairs: one straight flight each, measured from its treads.
    let flights_alone = named(&["IFCSTAIRFLIGHT"]).into_iter().filter(|id| !in_whole(*id, &["IFCSTAIR"]));
    for id in named(&["IFCSTAIR"]).into_iter().filter(|id| !l.whole.contains_key(id)).chain(flights_alone) {
        let parts: Vec<u64> = l.parts.get(&id).cloned().unwrap_or_default();
        seen.insert(id);
        seen.extend(&parts);
        let flights: Vec<u64> = parts.iter().copied().filter(|p| e(*p).is_some_and(|x| x.name == "IFCSTAIRFLIGHT")).collect();
        let source = match flights.as_slice() {
            [] => id,
            [one] => *one,
            more => {
                leave(
                    &mut skipped,
                    &mut seen,
                    id,
                    &format!("it has {} flights, and BIMCraft's stairs are single straight flights", more.len()),
                );
                continue;
            }
        };
        let Some(s) = e(source) else { continue };
        match stair_from(&r, s, &r.body(s)) {
            Ok((start, angle, width, risers, rise, run, z0)) => {
                let level = level_for(id, z0);
                let (top_level, height) = topped(level, z0, z0 + rise);
                let height = (height * 1000.0).round() / 1000.0;
                // Its riser, from the rise the model will measure, so it counts the same risers.
                let climb = top_level.map_or(height, |t| elevation(t) - elevation(level));
                let tread = if risers > 1 { run / f64::from(risers - 1) } else { run };
                let stair = Stair {
                    level,
                    top_level,
                    height,
                    start: micron(start),
                    angle,
                    width: (width * 100.0).round() / 100.0,
                    riser: climb / f64::from(risers),
                    tread: (tread * 1000.0).round() / 1000.0,
                };
                let sid = m.insert(Element::Stair(stair));
                from.insert(sid, who(id));
            }
            Err(why) => leave(&mut skipped, &mut seen, id, &why),
        }
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
        let footprint = floor.and_then(|z0| outline(&faces, z0, false)).and_then(|ring| inside(&ring)).map(|p| r.flat([p.0, p.1, 0.0]));
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

/// A roof's footprint, slope, whether its ends are gables, its thickness, and the height of its
/// underside at the eaves.
type RoofShape = (Vec<P2>, f64, bool, f64, f64);

fn roof_from(r: &Reader, faces: &[Vec<P3>]) -> Result<RoofShape, String> {
    let pts: Vec<P3> = faces.iter().flatten().copied().collect();
    let Some((z0, z1)) = span(pts.iter().map(|p| p[2])) else {
        return Err("it has no body BIMCraft can read".into());
    };
    // Faces turned up, and of them those sloping.
    let up: Vec<(&Vec<P3>, P3)> = faces.iter().map(|f| (f, normal(f))).filter(|(_, n)| n[2] > 0.01).collect();
    let sloped: Vec<&(&Vec<P3>, P3)> = up.iter().filter(|(_, n)| n[2] < 0.9998).collect();
    let Some((main, n)) = sloped.iter().max_by(|a, b| area3(a.0).total_cmp(&area3(b.0))).map(|x| (x.0, x.1)) else {
        let ring = outline(faces, z1, true).ok_or("its top isn't level")?;
        return Ok((
            ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect(),
            0.0,
            false,
            ((z1 - z0) * 100.0).round() / 100.0,
            z0,
        ));
    };
    let slope = (n[2].clamp(-1.0, 1.0).acos().to_degrees() * 100.0).round() / 100.0;
    // Down the slope, and along the eaves.
    let down = P2(n[0], n[1]).unit();
    let eaves = down.perp();
    let plan: Vec<P2> = pts.iter().map(|p| r.flat(*p)).collect();
    let (Some((e0, e1)), Some((d0, d1))) = (span(plan.iter().map(|p| p.dot(eaves))), span(plan.iter().map(|p| p.dot(down)))) else {
        return Err("it has no body BIMCraft can read".into());
    };
    let rect = [(e0, d0), (e1, d0), (e1, d1), (e0, d1)].map(|(a, b)| micron(eaves * a + down * b));
    let mut boundary = rect.to_vec();
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    // Its slopes cover the rectangle, or it isn't one.
    let covered: f64 = up.iter().map(|(f, _)| geom::area(&f.iter().map(|p| P2(p[0], p[1])).collect::<Vec<_>>()).abs()).sum();
    let whole = (e1 - e0) * (d1 - d0);
    if (covered - whole).abs() > 0.02 * whole {
        return Err("BIMCraft pitches roofs over rectangles only".into());
    }
    let hipped = sloped.iter().any(|(_, m)| P2(m[0], m[1]).unit().dot(eaves).abs() > 0.9);
    if !hipped && e1 - e0 < d1 - d0 - 1.0 {
        return Err("BIMCraft's gable roofs slope down to their long sides".into());
    }
    // Its thickness: from its top to the underside parallel to it.
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
    Ok((boundary, slope, !hipped, (t * 100.0).round() / 100.0, z0))
}

/// A straight flight: where its first riser's middle is, the way it climbs (degrees), its width,
/// how many risers, how high they climb, its run, and the height it starts at.
type Flight = (P2, f64, f64, u32, f64, f64, f64);

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
    let angle = (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6;
    Ok((start, angle, c1 - c0, risers, each * f64::from(risers), s1 - s0, z0))
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

    #[test]
    fn a_model_at_map_coordinates_comes_near_the_origin() {
        let i = ifc(meshes("500000000.,5000000000.,0.").as_bytes(), "x").unwrap();
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(0.0, 5000.0)));
        assert_eq!(i.moved, Some(P2(-500_001_000.0, -5_000_002_000.0)));
    }
}
