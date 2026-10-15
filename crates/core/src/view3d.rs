//! The 3D view: the model as solids (walls with their doors and windows cut out, floor slabs,
//! door and window panels, columns, stairs' steps, roofs' slopes) in an axonometric projection, as Revit's default 3D view shows it. A
//! depth buffer keeps the nearest surface at each pixel; faces are shaded by how they turn to
//! the light, and an edge is drawn wherever the plane a pixel shows changes, so coplanar faces
//! (joined walls' tops, a face's triangles) read as one. The picture keeps which element each
//! pixel shows, for picking. Sections and elevations use the same pictures, clipped at their
//! line and in hidden line, and cut the same solids.

use std::collections::BTreeMap;

use crate::geom::{self, P2};
use crate::model::{Element, Id, Model, Wall, heading};
use crate::render::{PAPER, Rgb};

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

/// A triangle of an element's surface.
struct Tri {
    p: [P3; 3],
    owner: Id,
    color: Rgb,
}

/// A wall's plan frame: its start, along it, and square to it.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    o: P2,
    u: P2,
    n: P2,
}

impl Frame {
    fn of(w: &Wall) -> Frame {
        let u = (w.end - w.start).unit();
        Frame { o: w.start, u, n: u.perp() }
    }

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
            out.push(Tri { p: [v[i], v[j], v[k]], owner, color });
            out.push(Tri { p: [v[i], v[k], v[l]], owner, color });
        }
    }
}

/// A solid of the model: a box (a wall's part between its openings, a door's leaf, a window's
/// glass, a stair's step) or a slab: a flat face, level or sloped, and what lies up to `rise` mm
/// above it (a floor, a column, a roof's slope).
pub(crate) enum Solid {
    Box { owner: Id, color: Rgb, frame: Frame, a: (f64, f64), c: (f64, f64), z: (f64, f64) },
    Slab { owner: Id, color: Rgb, face: Vec<P3>, rise: f64 },
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
            Solid::Box { owner, .. } | Solid::Slab { owner, .. } => *owner,
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
        }
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
            Solid::Slab { face, .. } => {
                // Where the line crosses the outline, paired off: in, out, in, out.
                // ponytail: a line through a corner it only touches unpairs the rest; nudge it if slabs show gaps.
                let mut cross: Vec<f64> = geom::ring_edges(&plan(face))
                    .filter_map(|(a, b)| {
                        let (e, den) = (b - a, u.cross(b - a));
                        (den.abs() > 1e-12 && (0.0..1.0).contains(&((a - p).cross(u) / den))).then(|| (a - p).cross(e) / den)
                    })
                    .collect();
                cross.sort_by(f64::total_cmp);
                cross.as_chunks::<2>().0.iter().map(|[s0, s1]| (*s0, *s1)).collect()
            }
        }
    }

    fn tris(&self, out: &mut Vec<Tri>) {
        match self {
            Solid::Box { owner, color, frame, a, c, z } => frame.cuboid(out, *owner, *color, *a, *c, *z),
            Solid::Slab { owner, color, face, rise } => prism(out, *owner, *color, face, *rise),
        }
    }
}

/// The model's solids: walls cut around their openings, door leaves and glass, floor slabs,
/// columns, stairs' steps and roofs' slopes.
pub(crate) fn solids(m: &Model) -> Vec<Solid> {
    let mut out = Vec::new();
    for (&id, e) in &m.elements {
        match e {
            Element::Wall(w) => wall(&mut out, m, id, w),
            Element::Floor(f) => {
                let (top, t) = (m.elevation(f.level), m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness));
                out.push(slab(id, SLAB, &f.boundary, (top - t, top)));
            }
            Element::Column(col) => {
                out.push(slab(id, WALL, &m.column_outline(col), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            // Each tread a block from the floor up to it; the last riser climbs to the floor above.
            Element::Stair(st) => {
                let (n, each, _) = m.flight(st);
                let (base, u) = (m.elevation(st.level), heading(st.angle));
                let frame = Frame { o: st.start, u, n: u.perp() };
                for k in 1..n {
                    let k = f64::from(k);
                    let (a, c) = (((k - 1.0) * st.tread, k * st.tread), (-st.width / 2.0, st.width / 2.0));
                    out.push(Solid::Box { owner: id, color: SLAB, frame, a, c, z: (base, base + k * each) });
                }
            }
            Element::Roof(r) => {
                if let Ok((pieces, rise)) = m.roof_pieces(r) {
                    out.extend(pieces.into_iter().map(|face| Solid::Slab { owner: id, color: ROOF, face, rise }));
                }
            }
            // A door's leaf or a window's glass: a 50 mm panel in the middle of the wall.
            Element::Door(o) | Element::Window(o) => {
                if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                    let color = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    let z = m.base_z(w) + o.sill;
                    let a = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
                    out.push(Solid::Box { owner: id, color, frame: Frame::of(w), a, c: (-25.0, 25.0), z: (z, z + t.height) });
                }
            }
            _ => {}
        }
    }
    out
}

/// A wall as boxes: its length cut where its doors and windows start and end, and each stretch
/// cut around the openings over it. Its ends reach past the walls they join, as in plan.
fn wall(out: &mut Vec<Solid>, m: &Model, id: Id, w: &Wall) {
    let f = Frame::of(w);
    let half = m.thickness(w) / 2.0;
    let (a0, a1) = (-m.join_reach(id, w.start), w.start.dist(w.end) + m.join_reach(id, w.end));
    let (z0, z1) = (m.base_z(w), m.top_z(w));
    // The openings: along the wall, and up it.
    let holes: Vec<((f64, f64), (f64, f64))> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Door(o) | Element::Window(o) if o.host == id => {
                let t = m.opening_type(e)?;
                Some(((o.offset - t.width / 2.0, o.offset + t.width / 2.0), (z0 + o.sill, z0 + o.sill + t.height)))
            }
            _ => None,
        })
        .collect();
    let mut cuts: Vec<f64> = holes.iter().flat_map(|((s0, s1), _)| [*s0, *s1]).chain([a0, a1]).filter(|x| (a0..=a1).contains(x)).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    for pair in cuts.windows(2) {
        let [x0, x1] = pair else { continue };
        let mid = (x0 + x1) / 2.0;
        let mut gaps: Vec<(f64, f64)> = holes.iter().filter(|((s0, s1), _)| *s0 <= mid && mid <= *s1).map(|(_, z)| *z).collect();
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut z = z0;
        for (g0, g1) in gaps.into_iter().chain([(z1, z1)]) {
            if g0.min(z1) > z + 1e-6 {
                out.push(Solid::Box { owner: id, color: WALL, frame: f, a: (*x0, *x1), c: (-half, half), z: (z, g0.min(z1)) });
            }
            z = z.max(g1);
        }
    }
}

/// A slab: `face`, its copy `rise` above, and the sides between them.
fn prism(out: &mut Vec<Tri>, owner: Id, color: Rgb, face: &[P3], rise: f64) {
    let up = |p: P3| [p[0], p[1], p[2] + rise];
    let pt = |i: usize| face.get(i).copied().unwrap_or_default();
    for [a, b, c] in geom::triangulate(&plan(face)) {
        out.push(Tri { p: [up(pt(a)), up(pt(b)), up(pt(c))], owner, color });
        out.push(Tri { p: [pt(a), pt(c), pt(b)], owner, color });
    }
    for (a, b) in face.iter().zip(face.iter().cycle().skip(1)) {
        out.push(Tri { p: [*a, *b, up(*b)], owner, color });
        out.push(Tri { p: [*a, up(*b), up(*a)], owner, color });
    }
}

/// Each triangle's plane, as an index the triangles lying in it (within half a millimetre) share.
fn planes(tris: &[Tri]) -> Vec<usize> {
    let mut found: Vec<(P3, f64)> = Vec::new();
    tris.iter()
        .map(|t| {
            let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            let d = dot(n, t.p[0]);
            // ponytail: a linear search per triangle; hash planes if models reach tens of thousands of faces.
            let same = |(m, e): &(P3, f64)| {
                let c = dot(*m, n);
                (c > 1.0 - 1e-9 && (e - d).abs() < 0.5) || (c < -1.0 + 1e-9 && (e + d).abs() < 0.5)
            };
            found.iter().position(same).unwrap_or_else(|| {
                found.push((n, d));
                found.len() - 1
            })
        })
        .collect()
}

/// The model as triangles, each with its plane: what every picture of it is drawn from.
pub struct Scene {
    tris: Vec<Tri>,
    planes: Vec<usize>,
}

impl Scene {
    pub fn new(m: &Model) -> Scene {
        let mut tris = Vec::new();
        for solid in solids(m) {
            solid.tris(&mut tris);
        }
        let planes = planes(&tris);
        Scene { tris, planes }
    }

    /// A view of the whole model from `azimuth` and `elevation` (degrees), fitted into the picture.
    pub fn fit(&self, azimuth: f64, elevation: f64, width: u32, height: u32) -> Camera {
        let mut cam = Camera { azimuth, elevation: elevation.clamp(-89.0, 89.0), scale: 1.0, target: [0.0; 3], width, height };
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
        let mut pts = self.tris.iter().flat_map(|t| t.p);
        let Some(first) = pts.next() else {
            cam.scale = room(width).min(room(height)) / 10_000.0;
            return cam;
        };
        let (lo, hi) = pts.fold((first, first), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        });
        let middle = mul(add(lo, hi), 0.5);
        let (_, right, up) = cam.axes();
        // The extent across and up the picture, from the middle of the box around the model.
        let (mut x, mut y) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
        for p in self.tris.iter().flat_map(|t| t.p) {
            let d = sub(p, middle);
            let (px, py) = (dot(d, right), dot(d, up));
            (x, y) = ((x.0.min(px), x.1.max(px)), (y.0.min(py), y.1.max(py)));
        }
        cam.target = add(middle, add(mul(right, (x.0 + x.1) / 2.0), mul(up, (y.0 + y.1) / 2.0)));
        cam.scale = (room(width) / (x.1 - x.0).max(1000.0)).min(room(height) / (y.1 - y.0).max(1000.0)).clamp(1e-6, 50.0);
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
                    if clip.is_some_and(|k| z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1) {
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
/// the camera) below `near`, and no more than `far` past it; between pixel columns `x.0` and `x.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub near: f64,
    pub far: Option<f64>,
    pub x: (f64, f64),
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
