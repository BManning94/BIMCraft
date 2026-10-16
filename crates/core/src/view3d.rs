//! The 3D view: the model as solids (walls with their doors and windows cut out, floor slabs and
//! ceilings with the shafts through them cut out, door and window panels, columns, stairs' steps, roofs' slopes) in an axonometric projection, as Revit's default 3D view shows it. A
//! depth buffer keeps the nearest surface at each pixel; faces are shaded by how they turn to
//! the light, and an edge is drawn wherever the plane a pixel shows changes, so coplanar faces
//! (joined walls' tops, a face's triangles) read as one. The picture keeps which element each
//! pixel shows, for picking. Sections and elevations use the same pictures, clipped at their
//! line and in hidden line, and cut the same solids.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::geom::{self, P2};
use crate::model::{Element, Id, Layer, Model};
use crate::render::{PAPER, Rgb};
use crate::stairs;
use crate::walls::{self, Axis, Plane};

/// A model point in mm: x east, y north, z up.
pub type P3 = [f64; 3];

/// Revit's default 3D view looks from the south-east, from above.
pub const AZIMUTH: f64 = -45.0;
pub const ELEVATION: f64 = 30.0;
/// The largest picture, in px a side.
pub const MAX_SIDE: u32 = 4096;
/// Where faces meet.
pub const EDGE: Rgb = Rgb(48, 48, 48);
const WALL: Rgb = Rgb(236, 236, 232);
const SLAB: Rgb = Rgb(214, 214, 210);
const DOOR: Rgb = Rgb(198, 168, 128);
const GLASS: Rgb = Rgb(166, 200, 228);
const ROOF: Rgb = Rgb(176, 118, 96);
const CEILING: Rgb = Rgb(244, 242, 236);
const RAIL: Rgb = Rgb(120, 122, 128);
/// A component's form made of nothing the library has.
const COMPONENT: Rgb = Rgb(214, 204, 188);
/// Room around a fitted model, px.
const MARGIN: f64 = 30.0;

pub(crate) fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub(crate) fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(crate) fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

/// A triangle of an element's surface; on a curved one, which (its element's, and which of its
/// curved surfaces: a wall's outside or inside, a component's form), so the facets read as one.
struct Tri {
    p: [P3; 3],
    owner: Id,
    color: Rgb,
    curve: Option<(Id, u32)>,
}

/// A wall's plan frame: its start, along it, and square to it.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    o: P2,
    u: P2,
    n: P2,
}

impl Frame {
    /// A box from `a.0` to `a.1` along, `c.0` to `c.1` across and `z.0` to `z.1` up.
    fn cuboid(&self, out: &mut Vec<Tri>, owner: Id, color: Rgb, a: (f64, f64), c: (f64, f64), z: (f64, f64)) {
        let at = |a: f64, c: f64, z: f64| {
            let p = self.o + self.u * a + self.n * c;
            [p.0, p.1, z]
        };
        let v = [
            at(a.0, c.0, z.0),
            at(a.1, c.0, z.0),
            at(a.1, c.1, z.0),
            at(a.0, c.1, z.0),
            at(a.0, c.0, z.1),
            at(a.1, c.0, z.1),
            at(a.1, c.1, z.1),
            at(a.0, c.1, z.1),
        ];
        for [i, j, k, l] in [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            out.push(Tri { p: [v[i], v[j], v[k]], owner, color, curve: None });
            out.push(Tri { p: [v[i], v[k], v[l]], owner, color, curve: None });
        }
    }
}

/// A solid of the model: a box (a door's leaf, a window's glass, a stair's step); a slab: a flat
/// face, level or sloped, and what lies up to `rise` mm above it (a floor, a column, a roof's
/// slope); a prism: a ring in plan from one plane up to another (a wall's part; of a curved wall,
/// with its centre and its centre line's radius, its faces being one surface each); or triangles
/// (a component's form, which sections show and don't cut), past `caps` one curved surface `round`.
pub(crate) enum Solid {
    Box { owner: Id, color: Rgb, frame: Frame, a: (f64, f64), c: (f64, f64), z: (f64, f64) },
    Slab { owner: Id, color: Rgb, face: Vec<P3>, rise: f64 },
    Prism { owner: Id, color: Rgb, ring: Vec<P2>, bottom: Plane, top: Plane, curve: Option<(P2, f64)> },
    Mesh { owner: Id, color: Rgb, tris: Vec<[P3; 3]>, caps: usize, round: Option<u32> },
}

/// A level slab: `ring` from `z.0` up to `z.1`.
fn slab(owner: Id, color: Rgb, ring: &[P2], z: (f64, f64)) -> Solid {
    Solid::Slab { owner, color, face: ring.iter().map(|p| [p.0, p.1, z.0]).collect(), rise: z.1 - z.0 }
}

fn plan(face: &[P3]) -> Vec<P2> {
    face.iter().map(|p| P2(p[0], p[1])).collect()
}

impl Solid {
    pub(crate) fn owner(&self) -> Id {
        match self {
            Solid::Box { owner, .. } | Solid::Slab { owner, .. } | Solid::Prism { owner, .. } | Solid::Mesh { owner, .. } => *owner,
        }
    }

    #[cfg(test)]
    pub(crate) fn color(&self) -> Rgb {
        match self {
            Solid::Box { color, .. } | Solid::Slab { color, .. } | Solid::Prism { color, .. } | Solid::Mesh { color, .. } => *color,
        }
    }

    /// From its bottom to its top.
    pub(crate) fn heights(&self) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                let (lo, hi) = face.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
                (lo, hi + rise)
            }
            Solid::Prism { ring, bottom, top, .. } => {
                ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(bottom.at(*q)), hi.max(top.at(*q))))
            }
            Solid::Mesh { tris, .. } => tris.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2]))),
        }
    }

    /// Its bottom and top over plan point `q`.
    pub(crate) fn z_at(&self, q: P2) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                // The face's plane, by Newell's normal, over `q`.
                let n = face.iter().zip(face.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)));
                let o = face.first().copied().unwrap_or_default();
                let z = if n[2].abs() > 1e-9 { o[2] - (n[0] * (q.0 - o[0]) + n[1] * (q.1 - o[1])) / n[2] } else { o[2] };
                (z, z + rise)
            }
            Solid::Prism { bottom, top, .. } => (bottom.at(q), top.at(q)),
            // Its triangles' heights over `q`, lowest and highest; off it, all of it.
            Solid::Mesh { tris, .. } => {
                let zs = tris.iter().filter_map(|[a, b, c]| {
                    let (pa, pb, pc) = (P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1]));
                    let det = (pb - pa).cross(pc - pa);
                    if det.abs() < 1e-9 {
                        return None;
                    }
                    let (u, v) = ((q - pa).cross(pc - pa) / det, (pb - pa).cross(q - pa) / det);
                    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
                });
                let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
                if lo <= hi { (lo, hi) } else { self.heights() }
            }
        }
    }

    /// Whether plan point `q` lies within its footprint.
    pub(crate) fn over(&self, q: P2) -> bool {
        match self {
            Solid::Box { frame, a, c, .. } => {
                let (s, t) = ((q - frame.o).dot(frame.u), (q - frame.o).dot(frame.n));
                (a.0..=a.1).contains(&s) && (c.0..=c.1).contains(&t)
            }
            Solid::Slab { face, .. } => geom::contains(&plan(face), q),
            Solid::Prism { ring, .. } => geom::contains(ring, q),
            Solid::Mesh { tris, .. } => tris.iter().any(|[a, b, c]| geom::contains(&[P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1])], q)),
        }
    }

    /// Its corners.
    fn corners(&self) -> Vec<P3> {
        let mut out = Vec::new();
        match self {
            Solid::Box { frame, a, c, z, .. } => {
                for (s, t, h) in [a.0, a.1].into_iter().flat_map(|s| [(s, c.0), (s, c.1)]).flat_map(|(s, t)| [(s, t, z.0), (s, t, z.1)]) {
                    let p = frame.o + frame.u * s + frame.n * t;
                    out.push([p.0, p.1, h]);
                }
            }
            Solid::Slab { face, rise, .. } => out.extend(face.iter().flat_map(|p| [*p, [p[0], p[1], p[2] + rise]])),
            Solid::Prism { ring, bottom, top, .. } => {
                out.extend(ring.iter().flat_map(|q| [[q.0, q.1, bottom.at(*q)], [q.0, q.1, top.at(*q)]]))
            }
            Solid::Mesh { tris, .. } => out.extend(tris.iter().flatten()),
        }
        out
    }

    /// The stretches of the line from `p` along unit `u` that lie over it, as distances from `p`.
    pub(crate) fn across(&self, p: P2, u: P2) -> Vec<(f64, f64)> {
        match self {
            Solid::Box { frame, a, c, .. } => {
                // Where the line lies between the box's sides, a pair of them at a time.
                let span = |axis: P2, (lo, hi): (f64, f64)| {
                    let (k0, k1) = ((p - frame.o).dot(axis), u.dot(axis));
                    if k1.abs() < 1e-12 {
                        return (lo..=hi).contains(&k0).then_some((f64::NEG_INFINITY, f64::INFINITY));
                    }
                    let (s0, s1) = ((lo - k0) / k1, (hi - k0) / k1);
                    Some((s0.min(s1), s0.max(s1)))
                };
                match (span(frame.u, *a), span(frame.n, *c)) {
                    (Some(x), Some(y)) if x.0.max(y.0) < x.1.min(y.1) => vec![(x.0.max(y.0), x.1.min(y.1))],
                    _ => Vec::new(),
                }
            }
            Solid::Slab { face, .. } => crossings(&plan(face), p, u),
            Solid::Prism { ring, .. } => crossings(ring, p, u),
            // Shown beyond a section's line, as Revit shows furniture, and never cut.
            Solid::Mesh { .. } => Vec::new(),
        }
    }

    fn tris(&self, out: &mut Vec<Tri>) {
        match self {
            Solid::Box { owner, color, frame, a, c, z } => frame.cuboid(out, *owner, *color, *a, *c, *z),
            Solid::Slab { owner, color, face, rise } => prism(out, *owner, *color, face, *rise),
            Solid::Prism { owner, color, ring, bottom, top, curve } => {
                let (owner, color) = (*owner, *color);
                let at = |q: P2, p: &Plane| [q.0, q.1, p.at(q)];
                for [a, b, c] in geom::triangulate(ring) {
                    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
                    out.push(Tri { p: [at(pt(a), top), at(pt(b), top), at(pt(c), top)], owner, color, curve: None });
                    out.push(Tri { p: [at(pt(a), bottom), at(pt(c), bottom), at(pt(b), bottom)], owner, color, curve: None });
                }
                for (a, b) in geom::ring_edges(ring) {
                    let (a0, b0, a1, b1) = (at(a, bottom), at(b, bottom), at(a, top), at(b, top));
                    // Along a curved wall's face (not across it, where it's cut), which face.
                    let curve = curve
                        .filter(|(c, _)| (b - a).unit().cross((a - *c).unit()).abs() > 0.5)
                        .map(|(c, r)| (owner, u32::from(((a + b) * 0.5).dist(c) > r)));
                    out.push(Tri { p: [a0, b0, b1], owner, color, curve });
                    out.push(Tri { p: [a0, b1, a1], owner, color, curve });
                }
            }
            Solid::Mesh { owner, color, tris, caps, round } => {
                for (i, p) in tris.iter().enumerate() {
                    let curve = round.filter(|_| i >= *caps).map(|k| (*owner, k));
                    out.push(Tri { p: *p, owner: *owner, color: *color, curve });
                }
            }
        }
    }
}

/// The stretches of the line from `p` along unit `u` that lie over `ring`, as distances from `p`:
/// where it crosses the ring's edges, paired off, in and out.
pub(crate) fn crossings(ring: &[P2], p: P2, u: P2) -> Vec<(f64, f64)> {
    // ponytail: a line through a corner it only touches unpairs the rest; nudge it if slabs show gaps.
    let mut cross: Vec<f64> = geom::ring_edges(ring)
        .filter_map(|(a, b)| {
            let (e, den) = (b - a, u.cross(b - a));
            (den.abs() > 1e-12 && (0.0..1.0).contains(&((a - p).cross(u) / den))).then(|| (a - p).cross(e) / den)
        })
        .collect();
    cross.sort_by(f64::total_cmp);
    cross.as_chunks::<2>().0.iter().map(|[s0, s1]| (*s0, *s1)).collect()
}

/// The slabs `face`, and what lies `rise` over it, make: less the shafts through it, each piece of
/// it on the face's plane.
fn slabs(out: &mut Vec<Solid>, m: &Model, owner: Id, color: Rgb, face: Vec<P3>, rise: f64) {
    let (pieces, shafts) = m.less_shafts(&face, rise);
    match (shafts.is_empty(), Plane::of(&face)) {
        (false, Some(plane)) => out.extend(pieces.into_iter().map(|ring| Solid::Slab {
            owner,
            color,
            face: ring.iter().map(|q| [q.0, q.1, plane.at(*q)]).collect(),
            rise,
        })),
        _ => out.push(Solid::Slab { owner, color, face, rise }),
    }
}

/// The model's solids: walls cut around their openings, door leaves and glass, floor slabs and
/// ceilings, columns, stairs' steps and roofs' slopes.
pub(crate) fn solids(m: &Model) -> Vec<Solid> {
    let mut out = Vec::new();
    for (&id, e) in &m.elements {
        match e {
            Element::Wall(_) => out.extend(walls::prisms(m, id, true).into_iter().map(|p| Solid::Prism {
                owner: id,
                color: WALL,
                ring: p.ring,
                bottom: p.bottom,
                top: p.top,
                curve: p.curve,
            })),
            Element::Floor(f) => {
                for (face, rise) in m.floor_slabs(f) {
                    slabs(&mut out, m, id, SLAB, face, rise);
                }
            }
            Element::Ceiling(c) => {
                for (face, rise) in m.ceiling_slabs(c) {
                    slabs(&mut out, m, id, CEILING, face, rise);
                }
            }
            Element::Column(col) => {
                out.push(slab(id, WALL, &m.column_outline(col), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            // Each form in its material's colour; a round one's sides one surface, as a curved wall's are.
            Element::Component(c) => {
                for (k, piece) in (2..).zip(m.component_pieces(c)) {
                    let color = m.materials.get(&piece.material).map_or(COMPONENT, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                    let round = piece.round.then_some(k);
                    out.push(Solid::Mesh { owner: id, color, tris: piece.tris, caps: piece.caps, round });
                }
            }
            // Each tread a block from the floor up to it, each landing a slab up to its top; the last
            // riser of a run climbs onto a landing, or the floor above.
            Element::Stair(st) => {
                let (_, each, _) = m.flight(st);
                let (base, l) = (m.elevation(st.level), stairs::stair(m, st));
                for run in &l.runs {
                    let frame = Frame { o: run.start, u: run.u, n: run.u.perp() };
                    for k in 1..run.risers {
                        let k = f64::from(k);
                        let (a, c) = (((k - 1.0) * st.tread, k * st.tread), (-st.width / 2.0, st.width / 2.0));
                        out.push(Solid::Box { owner: id, color: SLAB, frame, a, c, z: (base, run.z.0 + k * each) });
                    }
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            // Each run solid from its level up to its slope, each landing up to its top.
            Element::Ramp(r) => {
                let (base, l) = (m.elevation(r.level), stairs::ramp(m, r));
                for run in &l.runs {
                    let g = run.u * ((run.z.1 - run.z.0) / run.length.max(1e-9));
                    let top = Plane { z: run.z.0 - g.dot(run.start), g };
                    out.push(Solid::Prism { owner: id, color: SLAB, ring: run.ring(r.width), bottom: Plane::flat(base), top, curve: None });
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            Element::Railing(rl) => out.extend(stairs::railing_parts(m, rl).into_iter().map(|(ring, bottom, top)| Solid::Prism {
                owner: id,
                color: RAIL,
                ring,
                bottom,
                top,
                curve: None,
            })),
            Element::Roof(r) => {
                for (face, rise) in m.roof_slabs(r) {
                    slabs(&mut out, m, id, ROOF, face, rise);
                }
            }
            // A door's leaf or a window's glass: a 50 mm panel in the middle of the wall, square to it.
            Element::Door(o) | Element::Window(o) => {
                if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                    let color = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    let z = m.own_base(w) + o.sill;
                    let axis = Axis::of(w);
                    let u = axis.tangent(o.offset);
                    let frame = Frame { o: axis.at(o.offset, walls::centre(m, w)), u, n: u.perp() };
                    let a = (-t.width / 2.0, t.width / 2.0);
                    out.push(Solid::Box { owner: id, color, frame, a, c: (-25.0, 25.0), z: (z, z + t.height) });
                }
            }
            _ => {}
        }
    }
    out
}

/// A slab: `face`, its copy `rise` above, and the sides between them.
fn prism(out: &mut Vec<Tri>, owner: Id, color: Rgb, face: &[P3], rise: f64) {
    let up = |p: P3| [p[0], p[1], p[2] + rise];
    let pt = |i: usize| face.get(i).copied().unwrap_or_default();
    for [a, b, c] in geom::triangulate(&plan(face)) {
        out.push(Tri { p: [up(pt(a)), up(pt(b)), up(pt(c))], owner, color, curve: None });
        out.push(Tri { p: [pt(a), pt(c), pt(b)], owner, color, curve: None });
    }
    for (a, b) in face.iter().zip(face.iter().cycle().skip(1)) {
        out.push(Tri { p: [*a, *b, up(*b)], owner, color, curve: None });
        out.push(Tri { p: [*a, up(*b), up(*a)], owner, color, curve: None });
    }
}

/// Each triangle's plane, as an index the triangles lying in it (within half a millimetre) share;
/// the facets of one face of a curved wall share one too.
fn planes(tris: &[Tri]) -> Vec<usize> {
    // Planes by their normal, in cells wider than the tolerance, and a millimetre's slab of distance;
    // a match is looked for in the nearer neighbouring cell on each axis and the slabs either side,
    // so the tolerance holds across a cell's edge.
    let mut found: HashMap<[i64; 4], Vec<(P3, f64, usize)>> = HashMap::new();
    let mut count = 0;
    let mut curves: BTreeMap<(Id, u32), usize> = BTreeMap::new();
    tris.iter()
        .map(|t| {
            if let Some(c) = t.curve {
                let next = tris.len() + curves.len();
                return *curves.entry(c).or_insert(next);
            }
            let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            count += 1;
            // No area, no plane to share: its own.
            if dot(n, n) < 0.5 {
                return count - 1;
            }
            let d = dot(n, t.p[0]);
            // A plane and its flip are one: each faces where its first ratio that isn't zero is positive.
            let (n, d) = if n.iter().find(|x| x.abs() > 1e-9).is_some_and(|x| *x < 0.0) { (mul(n, -1.0), -d) } else { (n, d) };
            let cell = n.map(|x| x * 1e4);
            let q = cell.map(|x| x.round() as i64);
            let near = |k: usize| [q[k], q[k] + if cell[k] < q[k] as f64 { -1 } else { 1 }];
            let slab = d.floor() as i64;
            for a in near(0) {
                for b in near(1) {
                    for c in near(2) {
                        for s in slab - 1..=slab + 1 {
                            let same = found
                                .get(&[a, b, c, s])
                                .and_then(|v| v.iter().find(|(m, e, _)| dot(*m, n) > 1.0 - 1e-9 && (e - d).abs() < 0.5));
                            if let Some(&(_, _, id)) = same {
                                count -= 1;
                                return id;
                            }
                        }
                    }
                }
            }
            found.entry([q[0], q[1], q[2], slab]).or_default().push((n, d, count - 1));
            count - 1
        })
        .collect()
}

/// The model's solids as its pictures show them: an element of a compound type split into its
/// layers, each in its material's colour; a slab's from the top down, a wall's from its exterior in.
pub(crate) fn shaded(m: &Model) -> Vec<Solid> {
    solids(m).into_iter().flat_map(|s| layered(m, s)).collect()
}

fn layered(m: &Model, s: Solid) -> Vec<Solid> {
    let layers = match m.elements.get(&s.owner()) {
        Some(Element::Wall(w)) => m.type_layers("wall", &w.type_name),
        Some(Element::Floor(f)) => m.type_layers("floor", &f.type_name),
        Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
        Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
        _ => &[],
    };
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    if total <= 0.0 {
        return vec![s];
    }
    let colour = |l: &Layer, or: Rgb| m.materials.get(&l.material).map_or(or, |x| Rgb(x.color[0], x.color[1], x.color[2]));
    match s {
        Solid::Slab { owner, color, face, rise } => {
            let mut out = Vec::new();
            let mut above = 0.0;
            for l in layers {
                let r = rise * l.thickness / total;
                if r > 1e-9 {
                    let lift = rise - above - r;
                    let face = face.iter().map(|p| [p[0], p[1], p[2] + lift]).collect();
                    out.push(Solid::Slab { owner, color: colour(l, color), face, rise: r });
                }
                above += r;
            }
            out
        }
        // Across the wall from its exterior face in, each layer between two of its boundaries; the
        // outermost and innermost reach whatever lies past them.
        Solid::Prism { owner, color, ring, bottom, top, curve } => {
            let Some(w) = m.wall(owner) else { return vec![Solid::Prism { owner, color, ring, bottom, top, curve }] };
            let (axis, b) = (Axis::of(w), walls::boundaries(m, w));
            let off = |q: P2| axis.project(q).1;
            let n = layers.len();
            layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.thickness > 1e-9)
                .filter_map(|(k, l)| {
                    let mut part = ring.clone();
                    if let Some(hi) = b.get(k).filter(|_| k > 0) {
                        part = geom::clip(&part, |q| hi - off(q));
                    }
                    if let Some(lo) = b.get(k + 1).filter(|_| k + 1 < n) {
                        part = geom::clip(&part, |q| off(q) - lo);
                    }
                    (part.len() >= 3 && geom::area(&part).abs() > 1e-6).then(|| Solid::Prism {
                        owner,
                        color: colour(l, color),
                        ring: part,
                        bottom,
                        top,
                        curve,
                    })
                })
                .collect()
        }
        other => vec![other],
    }
}

/// The model as triangles, each with its plane: what every picture of it is drawn from.
pub struct Scene {
    tris: Vec<Tri>,
    planes: Vec<usize>,
}

impl Scene {
    pub fn new(m: &Model) -> Scene {
        let mut tris = Vec::new();
        for solid in shaded(m) {
            solid.tris(&mut tris);
        }
        let planes = planes(&tris);
        Scene { tris, planes }
    }

    /// A view of the whole model from `azimuth` and `elevation` (degrees), fitted into the picture.
    pub fn fit(&self, azimuth: f64, elevation: f64, width: u32, height: u32) -> Camera {
        let mut cam = Camera { azimuth, elevation: elevation.clamp(-89.0, 89.0), scale: 1.0, target: [0.0; 3], width, height };
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
        let pts: Vec<P3> = self.tris.iter().flat_map(|t| t.p).collect();
        let Some((target, w, h)) = extent(&cam, &pts) else {
            cam.scale = room(width).min(room(height)) / 10_000.0;
            return cam;
        };
        cam.target = target;
        cam.scale = (room(width) / w.max(1000.0)).min(room(height) / h.max(1000.0)).clamp(1e-6, 50.0);
        cam
    }

    /// The picture `cam` takes. Elements in `tints` show halfway to their tint, and their edges in it.
    pub fn render(&self, cam: &Camera, tints: &[(Id, Rgb)]) -> Image {
        self.picture(cam, None, tints)
    }

    /// A section's or an elevation's picture: what `clip` keeps, in hidden line (faces the paper's
    /// colour, edges dark), for the cut to be drawn over.
    pub fn cut(&self, cam: &Camera, clip: &Clip, tints: &[(Id, Rgb)]) -> Image {
        self.picture(cam, Some(clip), tints)
    }

    fn picture(&self, cam: &Camera, clip: Option<&Clip>, tints: &[(Id, Rgb)]) -> Image {
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let (back, right, up) = cam.axes();
        let mut depth = vec![f32::NEG_INFINITY; w * h];
        let mut hit = vec![u32::MAX; w * h];
        for (i, t) in self.tris.iter().enumerate() {
            let [a, b, c] = t.p.map(|p| cam.project(p));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            // Edge-on (or not a number): nothing to fill.
            if area.is_nan() || area.abs() <= 1e-12 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
            let (y0, y1) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
            if !(x1 >= 0.0 && x0 < w as f64 && y1 >= 0.0 && y0 < h as f64) {
                continue;
            }
            for py in y0.max(0.0) as usize..=y1.min(h as f64 - 1.0) as usize {
                for px in x0.max(0.0) as usize..=x1.min(w as f64 - 1.0) as usize {
                    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                    // Barycentric weights; a hair's tolerance so neighbours leave no gap between them.
                    let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / area;
                    let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / area;
                    let wc = 1.0 - wa - wb;
                    if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                        continue;
                    }
                    let z = wa * a.2 + wb * b.2 + wc * c.2;
                    if clip.is_some_and(|k| {
                        z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1 || y < k.y.0 || y > k.y.1
                    }) {
                        continue;
                    }
                    let z = z as f32;
                    let k = py * w + px;
                    if let (Some(d), Some(s)) = (depth.get_mut(k), hit.get_mut(k))
                        && z > *d
                    {
                        (*d, *s) = (z, i as u32);
                    }
                }
            }
        }
        // Lit from over the viewer's left shoulder; a face turned away from the light keeps 62%.
        let light = unit(add(add(mul(right, -0.35), mul(up, 0.55)), mul(back, 0.75)));
        let shade: Vec<Rgb> = self
            .tris
            .iter()
            .map(|t| {
                let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
                let n = if dot(n, back) < 0.0 { mul(n, -1.0) } else { n };
                let k = 0.62 + 0.38 * dot(n, light).max(0.0);
                let c = |v: u8| (f64::from(v) * k).round() as u8;
                if clip.is_some() { PAPER } else { Rgb(c(t.color.0), c(t.color.1), c(t.color.2)) }
            })
            .collect();
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        let tri = |k: usize| hit.get(k).map(|&t| t as usize).filter(|t| *t < self.tris.len());
        let plane = |k: usize| tri(k).and_then(|t| self.planes.get(t)).copied();
        let mut rgba = Vec::with_capacity(w * h * 4);
        for py in 0..h {
            for px in 0..w {
                let k = py * w + px;
                let here = plane(k);
                let edge = (px + 1 < w && plane(k + 1) != here) || (py + 1 < h && plane(k + w) != here);
                let tint = tri(k).and_then(|t| self.tris.get(t)).and_then(|t| tints.get(&t.owner));
                let c = match (edge, tri(k).and_then(|t| shade.get(t))) {
                    (true, _) => tint.copied().unwrap_or(EDGE),
                    (false, Some(s)) => tint.map_or(*s, |c| mix(*s, *c)),
                    (false, None) => PAPER,
                };
                rgba.extend([c.0, c.1, c.2, 255]);
            }
        }
        Image { width: w as u32, height: h as u32, rgba, hit, owners: self.tris.iter().map(|t| t.owner).collect() }
    }
}

/// What a section's or an elevation's picture keeps: what lies beyond its plane, depths (toward
/// the camera) below `near`, and no more than `far` past it; between pixel columns `x.0` and `x.1`,
/// and rows `y.0` and `y.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub near: f64,
    pub far: Option<f64>,
    pub x: (f64, f64),
    pub y: (f64, f64),
}

/// Where `pts` lie as `cam` looks at them: the point in their middle, and how far they spread
/// across the picture and up it, mm.
fn extent(cam: &Camera, pts: &[P3]) -> Option<(P3, f64, f64)> {
    let first = *pts.first()?;
    let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
    });
    let middle = mul(add(lo, hi), 0.5);
    let (_, right, up) = cam.axes();
    let (mut x, mut y) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
    for p in pts {
        let d = sub(*p, middle);
        let (px, py) = (dot(d, right), dot(d, up));
        (x, y) = ((x.0.min(px), x.1.max(px)), (y.0.min(py), y.1.max(py)));
    }
    Some((add(middle, add(mul(right, (x.0 + x.1) / 2.0), mul(up, (y.0 + y.1) / 2.0))), x.1 - x.0, y.1 - y.0))
}

/// The whole model from `azimuth` and `elevation` at `scale` px per mm: the picture as large as
/// that makes it (up to [`MAX_SIDE`] a side), `room` px clear around it. A 3D view on a sheet.
pub fn camera(m: &Model, azimuth: f64, elevation: f64, scale: f64, room: f64) -> Camera {
    let mut cam = Camera { azimuth, elevation: elevation.clamp(-89.0, 89.0), scale, target: [0.0; 3], width: 1, height: 1 };
    let pts: Vec<P3> = solids(m).iter().flat_map(Solid::corners).collect();
    if let Some((target, w, h)) = extent(&cam, &pts) {
        let side = |d: f64| (d * scale + 2.0 * room).ceil().clamp(1.0, f64::from(MAX_SIDE)) as u32;
        (cam.target, cam.width, cam.height) = (target, side(w), side(h));
    }
    cam
}

/// Each element's top over plan point `p`: the highest top of its solids standing over it.
pub(crate) fn tops_at(m: &Model, p: P2) -> BTreeMap<Id, f64> {
    let mut tops = BTreeMap::new();
    for s in solids(m).iter().filter(|s| s.over(p)) {
        let z = s.z_at(p).1;
        let top = tops.entry(s.owner()).or_insert(z);
        *top = f64::max(*top, z);
    }
    tops
}

/// Halfway from `a` to `b`.
fn mix(a: Rgb, b: Rgb) -> Rgb {
    let m = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Where the 3D view looks from, and how much of the model its picture holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Degrees counter-clockwise from east, around the model, of where it looks from.
    pub azimuth: f64,
    /// Degrees above the horizon it looks down from, within ±89.
    pub elevation: f64,
    /// Pixels per mm.
    pub scale: f64,
    /// The model point in the middle of the picture.
    pub target: P3,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    /// Toward the camera, and the picture's right and up, in the model.
    fn axes(&self) -> (P3, P3, P3) {
        let (a, e) = (self.azimuth.to_radians(), self.elevation.clamp(-89.0, 89.0).to_radians());
        let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
        let right = [-a.sin(), a.cos(), 0.0];
        (back, right, cross(back, right))
    }

    /// The pixel `p` shows at, and how near the camera it is (larger is nearer).
    pub fn project(&self, p: P3) -> (f64, f64, f64) {
        let (back, right, up) = self.axes();
        let d = sub(p, self.target);
        (f64::from(self.width) / 2.0 + dot(d, right) * self.scale, f64::from(self.height) / 2.0 - dot(d, up) * self.scale, dot(d, back))
    }

    /// The model point at pixel `(x, y)` on the plane through the target, square to the view.
    pub fn unproject(&self, (x, y): (f64, f64)) -> P3 {
        let (_, right, up) = self.axes();
        let (dx, dy) = ((x - f64::from(self.width) / 2.0) / self.scale, (y - f64::from(self.height) / 2.0) / self.scale);
        add(self.target, sub(mul(right, dx), mul(up, dy)))
    }

    /// Turned `around` degrees about the target and raised `up` degrees. The azimuth stays within
    /// (-180, 180].
    pub fn orbit(self, around: f64, up: f64) -> Camera {
        let a = (self.azimuth + around).rem_euclid(360.0);
        Camera { azimuth: if a > 180.0 { a - 360.0 } else { a }, elevation: (self.elevation + up).clamp(-89.0, 89.0), ..self }
    }

    /// Moved so the model point under pixel `from` comes under pixel `to`.
    pub fn pan(self, from: (f64, f64), to: (f64, f64)) -> Camera {
        Camera { target: add(self.target, sub(self.unproject(from), self.unproject(to))), ..self }
    }

    /// Zoomed by `factor` about pixel `at`, which keeps its model point.
    pub fn zoom(self, factor: f64, at: (f64, f64)) -> Camera {
        let p = self.unproject(at);
        let zoomed = Camera { scale: (self.scale * factor).clamp(1e-6, 50.0), ..self };
        zoomed.pan(zoomed.project_xy(p), at)
    }

    fn project_xy(&self, p: P3) -> (f64, f64) {
        let (x, y, _) = self.project(p);
        (x, y)
    }
}

/// A picture of the 3D view: its pixels, and which element each one shows.
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row by row: red, green, blue and alpha.
    pub rgba: Vec<u8>,
    /// Each pixel's triangle (`u32::MAX` for none), and each triangle's element.
    hit: Vec<u32>,
    owners: Vec<Id>,
}

impl Image {
    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        (x >= 0.0 && y >= 0.0 && x < w && y < h).then(|| y as usize * self.width as usize + x as usize)
    }

    /// The element the pixel at `(x, y)` shows.
    pub fn pick(&self, x: f64, y: f64) -> Option<Id> {
        let t = *self.hit.get(self.index(x, y)?)?;
        self.owners.get(t as usize).copied()
    }

    /// Every element the picture shows.
    pub fn shows(&self) -> BTreeSet<Id> {
        self.hit.iter().filter_map(|t| self.owners.get(*t as usize).copied()).collect()
    }

    /// The colour of the pixel at `(x, y)`; the paper's, off the picture.
    pub fn color(&self, x: f64, y: f64) -> Rgb {
        match self.index(x, y).and_then(|k| self.rgba.get(4 * k..4 * k + 3)) {
            Some([r, g, b]) => Rgb(*r, *g, *b),
            _ => PAPER,
        }
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let size = tiny_skia::IntSize::from_wh(self.width, self.height).ok_or("the picture's size is out of range")?;
        let pm = tiny_skia::Pixmap::from_vec(self.rgba.clone(), size).ok_or("the picture's size is out of range")?;
        pm.encode_png().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coplanar_triangles_share_a_plane_and_degenerate_ones_each_their_own() {
        let tri = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, curve: None };
        // A face's triangles a micron off its plane, as a file rounds them, are one plane;
        // triangles with no area are each their own, and many of them stay quick.
        let mut tris: Vec<Tri> = (0..200)
            .map(|k| {
                let (x, e) = (f64::from(k) * 100.0, f64::from(k % 7) * 1e-4);
                tri([[x, 0.0, 1000.0 + e], [x + 100.0, 0.0, 1000.0], [x, 100.0, 1000.0 - e]])
            })
            .collect();
        tris.extend((0..20_000).map(|k| tri([[f64::from(k), 0.0, 0.0]; 3])));
        let started = std::time::Instant::now();
        let ids = planes(&tris);
        assert!(started.elapsed().as_secs_f64() < 1.0, "{:?}", started.elapsed());
        assert!(ids.iter().take(200).all(|id| *id == ids[0]), "one plane");
        let degenerate: std::collections::BTreeSet<usize> = ids.iter().skip(200).copied().collect();
        assert_eq!(degenerate.len(), 20_000);
    }
    use crate::engine::Session;

    /// The 8 x 6 m box of 200 mm walls on Level 1 (walls 3 south, 4 east, 5 north, 6 west), a door
    /// (7) in the south wall, a window (8) in the east one, and a floor (9).
    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}
               floor {}"#,
        )
        .unwrap();
        s
    }

    fn view(s: &Session) -> (Camera, Image) {
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let img = scene.render(&cam, &[]);
        (cam, img)
    }

    fn at(cam: &Camera, p: P3) -> (f64, f64) {
        let (x, y, _) = cam.project(p);
        (x, y)
    }

    #[test]
    fn each_pixel_shows_the_nearest_surface_and_the_element_it_belongs_to() {
        let s = house();
        let (cam, img) = view(&s);
        let pick = |p: P3| {
            let (x, y) = at(&cam, p);
            img.pick(x, y)
        };
        assert_eq!(pick([5000.0, -100.0, 1500.0]), Some(3), "the south wall's outer face");
        assert_eq!(pick([2000.0, -25.0, 1000.0]), Some(7), "the door, seen through its opening");
        assert_eq!(pick([8025.0, 3000.0, 1650.0]), Some(8), "the window's glass");
        assert_eq!(pick([2000.0, 4500.0, 0.0]), Some(9), "the floor, seen over the walls");
        assert_eq!(pick([4000.0, 1000.0, 0.0]), Some(3), "the floor behind the south wall: the wall hides it");
        assert_eq!(img.pick(1.0, 1.0), None, "the background");
    }

    #[test]
    fn faces_shade_evenly_and_edges_mark_where_they_meet() {
        let s = house();
        let (cam, img) = view(&s);
        let color = |p: P3| {
            let (x, y) = at(&cam, p);
            img.color(x, y)
        };
        // Along the south wall's face past the door: one shade, no seams from the triangles under it.
        let row: Vec<Rgb> = (0..20).map(|i| color([3000.0 + 140.0 * f64::from(i), -100.0, 1800.0])).collect();
        assert!(row.iter().all(|c| *c == row[0]) && row[0] != EDGE, "{row:?}");
        // Over the corner where the south and east walls overlap, their tops make one face: zoomed in
        // on it, across it, away from its outer edges.
        let close = cam.zoom(4.0, at(&cam, [8000.0, 0.0, 3000.0]));
        let img4 = Scene::new(&s.model).render(&close, &[]);
        let top: Vec<Rgb> = (0..9)
            .map(|i| {
                let (x, y) = at(&close, [7860.0 + 20.0 * f64::from(i), 40.0, 3000.0]);
                img4.color(x, y)
            })
            .collect();
        assert!(top.iter().all(|c| *c == top[0]) && top[0] != EDGE, "{top:?}");
        // Where the south wall's top meets its face, an edge.
        let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
        let near: Vec<Rgb> = [-1.0, 0.0, 1.0].iter().flat_map(|dx| [-1.0, 0.0, 1.0].map(|dy| img.color(x + dx, y + dy))).collect();
        assert!(near.contains(&EDGE), "{near:?}");
    }

    #[test]
    fn tinted_elements_show_in_their_tint() {
        let s = house();
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let (x, y) = at(&cam, [5000.0, -100.0, 1500.0]);
        let plain = scene.render(&cam, &[]).color(x, y);
        let tinted = scene.render(&cam, &[(3, Rgb(37, 99, 235))]).color(x, y);
        assert!(plain.2 <= plain.0 && tinted.2 > tinted.0 + 40, "{plain:?} {tinted:?}");
    }

    #[test]
    fn the_camera_fits_the_model_and_orbits_pans_and_zooms() {
        let s = house();
        let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 400, 300);
        for (x, y) in [(0.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (0.0, 6000.0)] {
            for z in [0.0, 3000.0] {
                let (px, py) = at(&cam, [x, y, z]);
                assert!((0.0..400.0).contains(&px) && (0.0..300.0).contains(&py), "[{x}, {y}, {z}] at {px}, {py}");
            }
        }
        // Orbiting turns about the target, which stays in the middle of the image.
        let turned = cam.orbit(90.0, 100.0);
        assert_eq!((turned.azimuth, turned.elevation), (45.0, 89.0), "never quite straight down");
        let (x, y) = at(&turned, turned.target);
        assert!((x - 200.0).abs() < 1e-9 && (y - 150.0).abs() < 1e-9);
        // A pan carries the point under one pixel to another; a zoom keeps the point under the cursor.
        let p = cam.unproject((100.0, 80.0));
        let (x, y) = at(&cam.pan((100.0, 80.0), (150.0, 120.0)), p);
        assert!((x - 150.0).abs() < 1e-6 && (y - 120.0).abs() < 1e-6);
        let zoomed = cam.zoom(2.0, (100.0, 80.0));
        let (x, y) = at(&zoomed, p);
        assert!((x - 100.0).abs() < 1e-6 && (y - 80.0).abs() < 1e-6 && zoomed.scale == cam.scale * 2.0);
    }

    #[test]
    fn the_picture_saves_as_png() {
        let s = house();
        let (_, img) = view(&s);
        let pm = tiny_skia::Pixmap::decode_png(&img.png().unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (400, 300));
    }
}
