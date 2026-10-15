//! Plan views: what a level's plan shows, as a display list of filled rings and polylines in
//! pixels, drawn to SVG or PNG. Drafting conventions: walls and columns the cut plane passes
//! through get a heavy outline and grey poché, and below it a thin one (faint past the view
//! range's bottom); doors a leaf and swing, windows glass lines, stairs their risers up to the cut
//! line and a walk line, roofs their ridge and hips (dashed overhead, below the view range's top),
//! rooms a tag, grids a dashed line with a bubble at each end, dimensions witness lines and ticks
//! (an arc for an angle, an arrow for a radius), door and window tags their mark in a box, text
//! notes their lines, spot elevations a triangle and their height, sections their line and head,
//! elevations a marker; all of it inside the plan's crop region. Sections and elevations draw the
//! same way along their line and up: the cut in poché, levels and grids, notes and spot
//! elevations, over the 3D view's hidden-line picture of what lies beyond.

use std::f64::consts::PI;
use std::fmt::Write;

use tiny_skia::{Color, FillRule, IntSize, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::geom::{self, P2};
use crate::model::{DimKind, Dimension, Element, Id, Model, Note, Opening, PlanRange, Roof, Section, Stair, TOL, Tag, Wall, heading};
use crate::units::Units;
use crate::view3d::{self, Camera, Clip, Image, Scene};
use crate::{font, sheet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const PAPER: Rgb = Rgb(255, 255, 255);
/// Wall poché, the grey of cut walls.
pub const POCHE: Rgb = Rgb(168, 168, 168);
const INK: Rgb = Rgb(25, 25, 25);
const FAINT: Rgb = Rgb(120, 120, 120);
const GRID: Rgb = Rgb(60, 100, 150);
/// Room for grid bubbles and the view title around the drawing, px.
const MARGIN: f64 = 70.0;
const BUBBLE: f32 = 11.0;

pub enum Prim {
    Fill {
        rings: Vec<Vec<(f32, f32)>>,
        color: Rgb,
    },
    Line {
        pts: Vec<(f32, f32)>,
        width: f32,
        color: Rgb,
        dash: Option<[f32; 2]>,
        closed: bool,
    },
    /// A picture, its top left corner `at`, a pixel to a pixel: a section's on a sheet.
    Image {
        at: (f32, f32),
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

impl Prim {
    /// The same, `dx` px right and `dy` down.
    pub(crate) fn moved(self, dx: f32, dy: f32) -> Prim {
        let mv = |pts: Vec<(f32, f32)>| pts.into_iter().map(|(x, y)| (x + dx, y + dy)).collect();
        match self {
            Prim::Fill { rings, color } => Prim::Fill { rings: rings.into_iter().map(mv).collect(), color },
            Prim::Line { pts, width, color, dash, closed } => Prim::Line { pts: mv(pts), width, color, dash, closed },
            Prim::Image { at, width, height, rgba } => Prim::Image { at: (at.0 + dx, at.1 + dy), width, height, rgba },
        }
    }

    /// An image's corners, clockwise from its top left.
    fn corners(at: (f32, f32), width: u32, height: u32) -> Vec<(f32, f32)> {
        let (w, h) = (width as f32, height as f32);
        vec![at, (at.0 + w, at.1), (at.0 + w, at.1 + h), (at.0, at.1 + h)]
    }

    /// What of it lies inside the box from pixel `lo` to `hi`: a fill's rings clipped, a line in
    /// the pieces inside, a picture's whole pixels inside.
    fn cropped(self, lo: (f32, f32), hi: (f32, f32)) -> Vec<Prim> {
        let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
        let px = |p: P2| (p.0 as f32, p.1 as f32);
        match self {
            Prim::Fill { rings, color } => {
                let rings: Vec<Vec<(f32, f32)>> = rings.iter().map(|r| clip_ring(r, lo, hi)).filter(|r| r.len() >= 3).collect();
                if rings.is_empty() { Vec::new() } else { vec![Prim::Fill { rings, color }] }
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let inside = |p: &(f32, f32)| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1);
                if pts.iter().all(inside) {
                    return vec![Prim::Line { pts, width, color, dash, closed }];
                }
                let close = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
                for (a, b) in pts.iter().copied().zip(pts.iter().copied().skip(1)).chain(close) {
                    let Some((p, q)) = clip(mm(a), mm(b), mm(lo), mm(hi)) else { continue };
                    let (p, q) = (px(p), px(q));
                    match runs.last_mut() {
                        Some(run) if run.last() == Some(&p) => run.push(q),
                        _ => runs.push(vec![p, q]),
                    }
                }
                runs.into_iter().map(|pts| Prim::Line { pts, width, color, dash, closed: false }).collect()
            }
            Prim::Image { at, width, height, rgba } => {
                let side = |lo: f32, hi: f32, from: f32, n: u32| {
                    let first = (lo - from).ceil().clamp(0.0, n as f32) as u32;
                    (first, ((hi - from).floor().clamp(0.0, n as f32) as u32).max(first))
                };
                let ((x0, x1), (y0, y1)) = (side(lo.0, hi.0, at.0, width), side(lo.1, hi.1, at.1, height));
                if x1 == x0 || y1 == y0 {
                    return Vec::new();
                }
                let row = (x1 - x0) as usize * 4;
                let mut kept = Vec::with_capacity(row * (y1 - y0) as usize);
                for y in y0..y1 {
                    let from = (y as usize * width as usize + x0 as usize) * 4;
                    kept.extend_from_slice(rgba.get(from..from + row).unwrap_or_default());
                }
                vec![Prim::Image { at: (at.0 + x0 as f32, at.1 + y0 as f32), width: x1 - x0, height: y1 - y0, rgba: kept }]
            }
        }
    }
}

/// Model millimetres (y up) to image pixels (y down). In a section or elevation, the first
/// coordinate runs along its line from its start, the second up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Pixels per millimetre.
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn px(&self, p: P2) -> (f32, f32) {
        (
            (f64::from(self.width) / 2.0 + (p.0 - self.cx) * self.scale) as f32,
            (f64::from(self.height) / 2.0 - (p.1 - self.cy) * self.scale) as f32,
        )
    }

    /// The model point at a pixel: the inverse of [`View::px`].
    pub fn mm(&self, (x, y): (f32, f32)) -> P2 {
        P2(
            self.cx + (f64::from(x) - f64::from(self.width) / 2.0) / self.scale,
            self.cy - (f64::from(y) - f64::from(self.height) / 2.0) / self.scale,
        )
    }
}

pub struct Drawing {
    pub view: View,
    pub prims: Vec<Prim>,
    /// The element each primitive draws, for picking and highlighting.
    pub owners: Vec<Option<Id>>,
    /// The element being drawn now.
    owner: Option<Id>,
}

impl Drawing {
    /// Nothing yet, through `view`.
    pub fn new(view: View) -> Drawing {
        Drawing { view, prims: Vec::new(), owners: Vec::new(), owner: None }
    }
    /// What the primitives pushed next draw.
    pub(crate) fn own(&mut self, owner: Option<Id>) {
        self.owner = owner;
    }
    pub(crate) fn push(&mut self, p: Prim) {
        self.prims.push(p);
        self.owners.push(self.owner);
    }
    pub(crate) fn fill(&mut self, rings: &[Vec<P2>], color: Rgb) {
        if rings.is_empty() {
            return;
        }
        let rings = rings.iter().map(|r| r.iter().map(|p| self.view.px(*p)).collect()).collect();
        self.push(Prim::Fill { rings, color });
    }
    pub(crate) fn line(&mut self, pts: &[P2], width: f32, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.push(Prim::Line { pts, width, color, dash: None, closed });
    }
    /// `text` centred on `at` px, capitals `height` px tall.
    pub(crate) fn text(&mut self, text: &str, at: (f32, f32), height: f32, color: Rgb) {
        self.text_turned(text, at, height, 0.0, color);
    }

    /// What's drawn cropped to the box from pixel `lo` to `hi`, but for the primitives of the
    /// elements `keep` takes.
    pub(crate) fn crop(&mut self, lo: (f32, f32), hi: (f32, f32), keep: impl Fn(Option<Id>) -> bool) {
        let (prims, owners) = (std::mem::take(&mut self.prims), std::mem::take(&mut self.owners));
        for (prim, owner) in prims.into_iter().zip(owners) {
            for part in if keep(owner) { vec![prim] } else { prim.cropped(lo, hi) } {
                self.prims.push(part);
                self.owners.push(owner);
            }
        }
    }

    /// `text` centred on `(x, y)` px, capitals `height` px tall, turned `angle` radians clockwise
    /// on screen (y runs down).
    fn text_turned(&mut self, text: &str, (x, y): (f32, f32), height: f32, angle: f32, color: Rgb) {
        let (s, c) = angle.sin_cos();
        let w = font::width(text, height);
        for pts in font::strokes(text, -w / 2.0, height / 2.0, height) {
            let pts = pts.into_iter().map(|(a, b)| (x + a * c - b * s, y + a * s + b * c)).collect();
            self.push(Prim::Line { pts, width: (height / 10.0).max(1.0), color, dash: None, closed: false });
        }
    }

    /// The element drawn nearest the pixel `at`, within `tol` px. Inside a fill beats lying on a
    /// line, so a grid line along a wall doesn't hide the wall; of equally near ones the last
    /// drawn, which shows on top, wins.
    pub fn pick(&self, at: (f32, f32), tol: f32) -> Option<Id> {
        self.pick_all(at, tol).first().copied()
    }

    /// Every element drawn within `tol` px of `at`, in the order [`Drawing::pick`] prefers them:
    /// what Tab cycles through.
    pub fn pick_all(&self, at: (f32, f32), tol: f32) -> Vec<Id> {
        let p = P2(f64::from(at.0), f64::from(at.1));
        let ring = |pts: &[(f32, f32)]| -> Vec<P2> { pts.iter().map(|&(x, y)| P2(f64::from(x), f64::from(y))).collect() };
        let edges = |pts: &[P2], closed: bool| {
            let open = pts.windows(2).filter_map(|w| match w {
                [a, b] => Some(geom::dist_to_segment(p, *a, *b)),
                _ => None,
            });
            let close = match (closed, pts.first(), pts.last()) {
                (true, Some(a), Some(b)) => Some(geom::dist_to_segment(p, *b, *a)),
                _ => None,
            };
            open.chain(close).fold(f64::INFINITY, f64::min)
        };
        // Each element's nearest primitive: its distance, and how late it's drawn.
        let mut near: std::collections::BTreeMap<Id, (f64, usize)> = std::collections::BTreeMap::new();
        for (order, (prim, owner)) in self.prims.iter().zip(&self.owners).enumerate() {
            let Some(id) = *owner else { continue };
            let d = match prim {
                Prim::Fill { rings, .. } => {
                    let rings: Vec<Vec<P2>> = rings.iter().map(|r| ring(r)).collect();
                    if rings.iter().filter(|r| geom::contains(r, p)).count() % 2 == 1 {
                        -1.0
                    } else {
                        rings.iter().map(|r| edges(r, true)).fold(f64::INFINITY, f64::min)
                    }
                }
                Prim::Line { pts, width, closed, .. } => (edges(&ring(pts), *closed) - f64::from(*width) / 2.0).max(0.0),
                Prim::Image { at, width, height, .. } => {
                    let r = ring(&Prim::corners(*at, *width, *height));
                    if geom::contains(&r, p) { -1.0 } else { edges(&r, true) }
                }
            };
            if d <= f64::from(tol) && near.get(&id).is_none_or(|&(b, _)| d <= b) {
                near.insert(id, (d, order));
            }
        }
        let mut found: Vec<(Id, (f64, usize))> = near.into_iter().collect();
        found.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)));
        found.into_iter().map(|(id, _)| id).collect()
    }

    /// The elements drawn wholly inside the box between pixels `a` and `b`: Revit's window
    /// selection. With `crossing`, the ones it touches too.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let inside = |q: &(f32, f32)| (lo.0..=hi.0).contains(&q.0) && (lo.1..=hi.1).contains(&q.1);
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: std::collections::BTreeMap<Id, (bool, bool)> = std::collections::BTreeMap::new();
        for (prim, owner) in self.prims.iter().zip(&self.owners) {
            let Some(id) = *owner else { continue };
            let corners;
            let (rings, closed): (Vec<&[(f32, f32)]>, bool) = match prim {
                Prim::Fill { rings, .. } => (rings.iter().map(Vec::as_slice).collect(), true),
                Prim::Image { at, width, height, .. } => {
                    corners = Prim::corners(*at, *width, *height);
                    (vec![corners.as_slice()], true)
                }
                Prim::Line { pts, closed, .. } => (vec![pts.as_slice()], *closed),
            };
            let all = rings.iter().all(|r| r.iter().all(inside));
            let any = rings.iter().any(|r| {
                let last = closed.then(|| (r.last(), r.first()));
                r.windows(2)
                    .filter_map(|w| Some((w.first()?, w.get(1)?)))
                    .chain(last.into_iter().filter_map(|(p, q)| Some((p?, q?))))
                    .any(|(p, q)| clips(*p, *q, lo, hi))
                    || r.iter().any(inside)
            });
            let e = seen.entry(id).or_insert((true, false));
            *e = (e.0 && all, e.1 || any);
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }
}

/// The outlines of the walls and columns a plan of `level` cuts.
fn cut_outlines(m: &Model, level: Id) -> Vec<(Id, Vec<P2>)> {
    let z = m.plan_range(level).cut;
    m.elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::Wall(w) if m.cut_by(w, level) => Some((id, wall_outline(m, id, w))),
            Element::Column(col) => {
                let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
                (bottom <= z && z < top).then(|| (id, m.column_outline(col)))
            }
            _ => None,
        })
        .collect()
}

/// The walls and columns a plan sees below its cut plane, down to its view depth: their outlines,
/// and their tops.
fn seen_below(m: &Model, r: &PlanRange) -> Vec<(Id, Vec<P2>, f64)> {
    m.elements
        .iter()
        .filter_map(|(&id, e)| {
            let (outline, top) = match e {
                Element::Wall(w) => (wall_outline(m, id, w), m.top_z(w)),
                Element::Column(col) => (m.column_outline(col), m.span(col.level, col.base_offset, col.top_level, col.height).1),
                _ => return None,
            };
            (r.depth < top && top <= r.cut).then_some((id, outline, top))
        })
        .collect()
}

/// The plan of `level` fitted into `width` x `height` px.
pub fn plan(m: &Model, level: Id, width: u32, height: u32) -> Drawing {
    draw(m, level, fit(&plan_extent(m, level), width, height))
}

/// What a plan of `level` takes on paper: all it shows, or cropped, its crop region and the grids'
/// ends at its edges.
pub(crate) fn plan_extent(m: &Model, level: Id) -> Vec<P2> {
    match m.level(level).and_then(|l| l.crop) {
        None => plan_shows(m, level),
        Some([lo, hi]) => {
            let grids = m.elements.values().filter_map(|e| match e {
                Element::Grid(g) => clip(g.start, g.end, lo, hi),
                _ => None,
            });
            [lo, hi].into_iter().chain(grids.flat_map(|(a, b)| [a, b])).collect()
        }
    }
}

/// Everything a plan of `level` shows, uncropped: its walls, floors, rooms, dimensions, tags,
/// notes and spot elevations, the grids, and the marks of the sections and elevations.
pub(crate) fn plan_shows(m: &Model, level: Id) -> Vec<P2> {
    let r = m.plan_range(level);
    let mut extent: Vec<P2> = cut_outlines(m, level).into_iter().flat_map(|(_, o)| o).collect();
    extent.extend(seen_below(m, &r).into_iter().flat_map(|(_, o, _)| o));
    for e in m.elements.values() {
        match e {
            Element::Floor(f) if f.level == level => extent.extend(&f.boundary),
            Element::Room(r) if r.level == level => extent.push(r.point),
            Element::Grid(g) => extent.extend([g.start, g.end]),
            Element::Dimension(dm) if dm.level == level => extent.push(dm.at),
            Element::Section(v) | Element::Elevation(v) => extent.extend([v.start, v.end]),
            Element::Tag(t) if m.level_of(e) == Some(level) => extent.extend(m.anchor(t.host).map(|a| a + t.offset)),
            Element::Stair(st) if st.level == level || st.top_level == Some(level) => {
                extent.extend([st.start, st.start + heading(st.angle) * m.flight(st).2]);
            }
            Element::Roof(r) if r.level == level || m.level_above(level) == Some(r.level) => extent.extend(&r.boundary),
            Element::Text(n) if n.view == level => extent.extend([Some(n.at), n.leader].into_iter().flatten()),
            Element::Spot(sp) if sp.view == level => extent.push(sp.at),
            _ => {}
        }
    }
    extent
}

/// An element that isn't in the model yet, drawn as a plan would draw it, its primitives owned
/// by `id`: what a tool previews. Annotation, columns, stairs and roofs only.
pub fn sketch(m: &Model, view: View, id: Id, e: &Element) -> Drawing {
    let mut d = Drawing { view, prims: Vec::new(), owners: Vec::new(), owner: Some(id) };
    match e {
        Element::Dimension(dm) => dimension(&mut d, m, dm),
        Element::Tag(t) => tag(&mut d, m, t),
        Element::Text(n) => note(&mut d, n),
        Element::Spot(sp) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
        Element::Section(v) => section_mark(&mut d, v),
        Element::Elevation(v) => elevation_mark(&mut d, v),
        Element::Column(col) => d.line(&m.column_outline(col), 2.0, INK, true),
        Element::Stair(st) => stair(&mut d, m, st, st.level),
        Element::Roof(r) => roof(&mut d, m, r),
        _ => {}
    }
    d
}

/// How far past the lowest and highest heights a section's grids run, mm.
const GRID_PAST: f64 = 600.0;

/// From the lowest level or solid to the highest.
fn heights(m: &Model) -> (f64, f64) {
    let solids = view3d::solids(m);
    let zs = m.levels().into_iter().map(|(_, l)| l.elevation).chain(solids.iter().flat_map(|s| {
        let (a, b) = s.heights();
        [a, b]
    }));
    let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
    if lo <= hi { (lo, hi) } else { (0.0, 3000.0) }
}

/// The crop region of view `id` as it shows: a cropped plan's corners; a section's or an
/// elevation's line, along it, and its crop's heights (without one, all of the model's, as far as
/// its grids reach past them). None for a plan that isn't cropped, or another element.
pub fn crop_region(m: &Model, id: Id) -> Option<[P2; 2]> {
    match m.elements.get(&id)? {
        Element::Level(l) => l.crop,
        Element::Section(v) | Element::Elevation(v) => {
            let (lo, hi) = v.crop.map_or_else(
                || {
                    let (lo, hi) = heights(m);
                    (lo - GRID_PAST, hi + GRID_PAST)
                },
                |[bottom, top]| (bottom, top),
            );
            Some([P2(0.0, lo), P2(v.start.dist(v.end), hi)])
        }
        _ => None,
    }
}

/// The heights a section or elevation shows: its crop's, else all of the model's.
fn shown_heights(m: &Model, v: &Section) -> (f64, f64) {
    v.crop.map_or_else(|| heights(m), |[bottom, top]| (bottom, top))
}

/// The view section or elevation `id` fits into `width` x `height` px: along its line from its
/// start, and up, with room for its levels' names and its grids' bubbles.
pub fn section_fit(m: &Model, id: Id, width: u32, height: u32) -> View {
    // At no scale, the level names take no room; at the scale that gives, they do.
    let first = section_extent(m, id, f64::INFINITY).map_or_else(|| fit(&[], width, height), |e| fit(&e, width, height));
    section_extent(m, id, first.scale).map_or(first, |e| fit(&e, width, height))
}

/// What section or elevation `id` shows, along its line and up, at `scale` px per mm: from its
/// start to its end, past its lowest and highest heights, with room past the end for the widest
/// level name.
pub(crate) fn section_extent(m: &Model, id: Id, scale: f64) -> Option<[P2; 2]> {
    let v = m.section(id)?;
    let ((lo, hi), len) = (shown_heights(m, v), v.start.dist(v.end));
    // The widest level name or height.
    let head =
        m.levels().iter().map(|(_, l)| font::width(&l.name, 10.0).max(font::width(&m.units.length(l.elevation), 10.0))).fold(0.0, f32::max)
            + 12.0;
    Some([P2(0.0, lo - GRID_PAST), P2(len + f64::from(head) / scale, hi + GRID_PAST)])
}

/// The camera, and what it keeps, that show what section or elevation `id` sees through `view`.
pub fn section_camera(m: &Model, id: Id, view: View) -> Option<(Camera, Clip)> {
    let v = m.section(id)?;
    let (u, back) = ((v.end - v.start).unit(), v.looking() * -1.0);
    let at = v.start + u * view.cx;
    let cam = Camera {
        azimuth: back.1.atan2(back.0).to_degrees(),
        elevation: 0.0,
        scale: view.scale,
        target: [at.0, at.1, view.cy],
        width: view.width,
        height: view.height,
    };
    let column = |s: f64| f64::from(view.width) / 2.0 + (s - view.cx) * view.scale;
    let row = |z: f64| f64::from(view.height) / 2.0 - (z - view.cy) * view.scale;
    let rows = v.crop.map_or((f64::NEG_INFINITY, f64::INFINITY), |[bottom, top]| (row(top), row(bottom)));
    Some((cam, Clip { near: 0.0, far: v.depth, x: (column(0.0), column(v.start.dist(v.end))), y: rows }))
}

/// What section or elevation `id` sees past its line through `view`, in hidden line: the
/// picture its cut, levels and grids ([`section`]) go over.
pub fn section_image(m: &Model, scene: &Scene, id: Id, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    Some(scene.cut(&cam, &clip, tints))
}

/// What section or elevation `id` draws over its picture through `view`: where its line cuts
/// the solids, in poché; its levels, named past its end; the grids it crosses; its notes and spot
/// elevations; and its name. Cropped, only what lies between its crop's heights.
pub fn section(m: &Model, id: Id, view: View) -> Drawing {
    let mut d = Drawing { view, prims: Vec::new(), owners: Vec::new(), owner: None };
    let Some(v) = m.section(id) else { return d };
    let (len, u) = (v.start.dist(v.end), (v.end - v.start).unit());
    // Each element's cut pieces, filled as one so where they meet (a roof's slopes at its ridge)
    // leaves no seam.
    let mut cuts: std::collections::BTreeMap<Id, Vec<Vec<P2>>> = std::collections::BTreeMap::new();
    for s in view3d::solids(m) {
        for (a, b) in s.across(v.start, u) {
            let (a, b) = (a.max(0.0), b.min(len));
            if b - a > 1e-6 {
                // Level, or along a sloped face.
                let ((a0, a1), (b0, b1)) = (s.z_at(v.start + u * a), s.z_at(v.start + u * b));
                cuts.entry(s.owner()).or_default().push(vec![P2(a, a0), P2(b, b0), P2(b, b1), P2(a, a1)]);
            }
        }
    }
    // Outlines, then poché over their inner halves: joined walls show their union, as in plan.
    for (owner, rings) in &cuts {
        d.owner = Some(*owner);
        for r in rings {
            d.line(r, 3.0, INK, true);
        }
    }
    for (owner, rings) in cuts {
        d.owner = Some(owner);
        d.fill(&rings, POCHE);
    }
    let (lo, hi) = shown_heights(m, v);
    if v.crop.is_some() {
        let (a, b) = (d.view.px(P2(0.0, hi)), d.view.px(P2(len, lo)));
        d.crop(a, b, |_| false);
    }
    for (level, l) in m.levels().into_iter().filter(|(_, l)| (lo..=hi).contains(&l.elevation) || v.crop.is_none()) {
        d.owner = Some(level);
        let (a, b) = (d.view.px(P2(0.0, l.elevation)), d.view.px(P2(len, l.elevation)));
        d.push(Prim::Line { pts: vec![a, b], width: 1.0, color: INK, dash: Some([12.0, 4.0]), closed: false });
        for (text, dy) in [(l.name.clone(), -8.0), (m.units.length(l.elevation), 8.0)] {
            d.text(&text, (b.0 + 8.0 + font::width(&text, 10.0) / 2.0, b.1 + dy), 10.0, INK);
        }
    }
    // Grids running into what it sees: across its line, reaching past it (within its depth).
    let past = |q: P2| (q - v.start).dot(v.looking());
    for (&grid_id, e) in &m.elements {
        if let Element::Grid(g) = e
            && let Some(x) = geom::line_intersection(v.start, v.end, g.start, g.end)
            && past(g.start).max(past(g.end)) >= -TOL
            && v.depth.is_none_or(|far| past(g.start).min(past(g.end)) <= far)
            && (0.0..=len).contains(&(x - v.start).dot(u))
        {
            let along = (x - v.start).dot(u);
            d.owner = Some(grid_id);
            grid(&mut d, P2(along, lo - GRID_PAST), P2(along, hi + GRID_PAST), &g.name);
        }
    }
    for (&nid, e) in &m.elements {
        d.owner = Some(nid);
        match e {
            Element::Text(n) if n.view == id => note(&mut d, n),
            Element::Spot(sp) if sp.view == id => {
                let z = m.spot_value(sp).unwrap_or(sp.at.1);
                spot_mark(&mut d, view.px(P2(sp.at.0, z)), Some(z), m.units);
            }
            _ => {}
        }
    }
    d.owner = None;
    let at = (24.0 + font::width(&v.name, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text(&v.name, at, 16.0, INK);
    d
}

/// Section or elevation `id` through `view`, as a PNG: its picture, and its drawing over it.
pub fn section_png(m: &Model, id: Id, view: View) -> Result<Vec<u8>, String> {
    let image = section_image(m, &Scene::new(m), id, view, &[]).ok_or_else(|| format!("{id} is not a section or an elevation"))?;
    let size = tiny_skia::IntSize::from_wh(image.width, image.height).ok_or("the image size is out of range")?;
    let mut pm = Pixmap::from_vec(image.rgba, size).ok_or("the image size is out of range")?;
    paint(&mut pm, &section(m, id, view));
    pm.encode_png().map_err(|e| e.to_string())
}

/// The plan of `level` as `view` shows it: what a canvas that pans and zooms draws.
pub fn draw(m: &Model, level: Id, view: View) -> Drawing {
    let mut d = Drawing { view, prims: Vec::new(), owners: Vec::new(), owner: None };
    let range = m.plan_range(level);
    // Floors whose tops it sees, from the cut plane down to the view depth.
    for (&id, e) in &m.elements {
        if let Element::Floor(f) = e
            && (range.depth..=range.cut).contains(&m.elevation(f.level))
        {
            d.owner = Some(id);
            d.line(&f.boundary, 1.0, FAINT, true);
        }
    }
    // Walls and columns below the cut plane, as seen from it; past the bottom, faint.
    for (id, o, top) in seen_below(m, &range) {
        d.owner = Some(id);
        d.line(&o, 1.0, if top > range.bottom { INK } else { FAINT }, true);
    }
    let outlines = cut_outlines(m, level);
    // Outlines first, then poché over them: each fill covers the inner half of the outlines, so
    // where walls overlap at their joins only the edge of their union stays inked.
    for (id, o) in &outlines {
        d.owner = Some(*id);
        d.line(o, 4.0, INK, true);
    }
    // One fill per wall or column: in one even-odd fill, overlapping walls would cancel out.
    for (id, o) in outlines {
        d.owner = Some(id);
        d.fill(&[o], POCHE);
    }
    for (&id, e) in &m.elements {
        if let Element::Door(o) | Element::Window(o) = e
            && let Some(w) = m.wall(o.host)
            && m.cut_by(w, level)
        {
            d.owner = Some(id);
            opening(&mut d, m, e, o, w, range.cut);
        }
    }
    let crop = m.level(level).and_then(|l| l.crop);
    // Cropped, annotation shows whole while its anchor is inside the crop, and not at all outside it.
    let inside = |p: P2| crop.is_none_or(|[lo, hi]| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1));
    for (&id, e) in &m.elements {
        d.owner = Some(id);
        match e {
            // Cropped, a grid stops at the crop's edge, with its bubbles there.
            Element::Grid(g) => {
                if let Some((a, b)) = crop.map_or(Some((g.start, g.end)), |[lo, hi]| clip(g.start, g.end, lo, hi)) {
                    grid(&mut d, a, b, &g.name);
                }
            }
            Element::Room(r) if r.level == level && inside(r.point) => {
                let area = m.room_region(r).map_or_else(
                    || "NOT ENCLOSED".to_string(),
                    |g| {
                        let (area, unit) = m.units.area(g.area());
                        format!("{area:.2} {unit}")
                    },
                );
                let (x, y) = d.view.px(r.point);
                d.text(&r.name, (x, y - 16.0), 13.0, INK);
                d.text(&r.number, (x, y), 10.0, INK);
                d.text(&area, (x, y + 14.0), 10.0, INK);
            }
            Element::Stair(st) => stair(&mut d, m, st, level),
            Element::Roof(r) if r.level == level => roof(&mut d, m, r),
            // Overhead, from below: its edge, dashed, while its eaves are under the view range's top.
            Element::Roof(r) => {
                let eave = m
                    .roof_pieces(r)
                    .map_or(f64::INFINITY, |(pieces, _)| pieces.iter().flatten().map(|p| p[2]).fold(f64::INFINITY, f64::min));
                if range.cut < eave && eave <= range.top {
                    let pts = r.boundary.iter().map(|p| d.view.px(*p)).collect();
                    d.push(Prim::Line { pts, width: 1.0, color: INK, dash: Some([10.0, 5.0]), closed: true });
                }
            }
            _ => {}
        }
    }
    // Annotation last, over the model; sections and elevations show on every plan.
    for (&id, e) in &m.elements {
        d.owner = Some(id);
        match e {
            Element::Dimension(dm) if dm.level == level && inside(dm.at) => dimension(&mut d, m, dm),
            Element::Tag(t) if m.level_of(e) == Some(level) && m.anchor(t.host).is_some_and(|a| inside(a + t.offset)) => tag(&mut d, m, t),
            Element::Text(n) if n.view == level && inside(n.at) => note(&mut d, n),
            Element::Spot(sp) if sp.view == level && inside(sp.at) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
            // A section's line is cropped as a line is.
            Element::Section(v) => section_mark(&mut d, v),
            Element::Elevation(v) if inside(v.start + (v.end - v.start) * 0.5) => elevation_mark(&mut d, v),
            _ => {}
        }
    }
    // Grids stop at the crop's edge on their own, and annotation is whole or not there: the rest is cut.
    if let Some([lo, hi]) = crop {
        let (a, b) = (d.view.px(lo), d.view.px(hi));
        d.crop((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)), |o| {
            o.and_then(|id| m.elements.get(&id)).is_some_and(|e| {
                matches!(
                    e,
                    Element::Grid(_)
                        | Element::Room(_)
                        | Element::Dimension(_)
                        | Element::Tag(_)
                        | Element::Text(_)
                        | Element::Spot(_)
                        | Element::Elevation(_)
                )
            })
        });
    }
    d.owner = None;
    let title = m.level(level).map_or_else(String::new, |l| l.name.clone());
    let at = (24.0 + font::width(&title, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text(&title, at, 16.0, INK);
    d
}

/// The part of the segment from `p` to `q` inside the box from `lo` to `hi` (Liang-Barsky); its
/// ends as they were where they're inside.
pub(crate) fn clip(p: P2, q: P2, lo: P2, hi: P2) -> Option<(P2, P2)> {
    let d = q - p;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [(-d.0, p.0 - lo.0), (d.0, hi.0 - p.0), (-d.1, p.1 - lo.1), (d.1, hi.1 - p.1)] {
        if pk == 0.0 {
            if qk < 0.0 {
                return None;
            }
        } else if pk < 0.0 {
            t0 = t0.max(qk / pk);
        } else {
            t1 = t1.min(qk / pk);
        }
    }
    (t0 <= t1).then(|| (if t0 > 0.0 { p + d * t0 } else { p }, if t1 < 1.0 { p + d * t1 } else { q }))
}

/// Whether the segment from `p` to `q` passes through the box from `lo` to `hi`.
fn clips(p: (f32, f32), q: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
    clip(mm(p), mm(q), mm(lo), mm(hi)).is_some()
}

/// The part of a ring inside the box from `lo` to `hi`, side by side (Sutherland-Hodgman): exact
/// for the convex rings plans fill.
fn clip_ring(ring: &[(f32, f32)], lo: (f32, f32), hi: (f32, f32)) -> Vec<(f32, f32)> {
    let mut pts = ring.to_vec();
    // Each side of the box: which coordinate, where, and whether inside lies above it.
    for (x, bound, above) in [(true, lo.0, true), (true, hi.0, false), (false, lo.1, true), (false, hi.1, false)] {
        let at = |p: (f32, f32)| if x { p.0 } else { p.1 };
        let inside = |p: (f32, f32)| if above { at(p) >= bound } else { at(p) <= bound };
        let cross = |a: (f32, f32), b: (f32, f32)| {
            let t = (bound - at(a)) / (at(b) - at(a));
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let n = pts.len();
        let mut kept = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (a, b) = (pts[(i + n - 1) % n], pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => kept.push(b),
                (true, false) => kept.push(cross(a, b)),
                (false, true) => kept.extend([cross(a, b), b]),
                (false, false) => {}
            }
        }
        pts = kept;
    }
    pts
}

/// A view fitting `pts` into the image, leaving the margin free.
fn fit(pts: &[P2], width: u32, height: u32) -> View {
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
    let (lo, hi) = if pts.is_empty() { (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)) } else { (lo, hi) };
    let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
    let scale = (room(width) / (hi.0 - lo.0).max(1000.0)).min(room(height) / (hi.1 - lo.1).max(1000.0));
    View { scale, cx: (lo.0 + hi.0) / 2.0, cy: (lo.1 + hi.1) / 2.0, width, height }
}

/// A wall's rectangle in plan, reaching past the ends it shares with other walls so corners
/// close (see [`Model::join_reach`]).
fn wall_outline(m: &Model, id: Id, w: &Wall) -> Vec<P2> {
    let u = (w.end - w.start).unit();
    let n = u.perp() * (m.thickness(w) / 2.0);
    let (a, b) = (w.start - u * m.join_reach(id, w.start), w.end + u * m.join_reach(id, w.end));
    vec![a + n, b + n, b - n, a - n]
}

/// A door or window the cut plane at height `cut` passes through: a gap in the wall, jambs, and
/// a leaf with its swing, or glass.
fn opening(d: &mut Drawing, m: &Model, e: &Element, o: &Opening, w: &Wall, cut: f64) {
    let Some(t) = m.opening_type(e) else { return };
    let bottom = m.base_z(w) + o.sill;
    if !(bottom <= cut && cut < bottom + t.height) {
        return;
    }
    let (u, half) = ((w.end - w.start).unit(), m.thickness(w) / 2.0);
    let at = |along: f64, across: f64| w.start + u * along + u.perp() * across;
    let (s0, s1) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
    // Clear a little past the faces too, where the outlines' outer halves are.
    let past = 3.0 / d.view.scale;
    d.fill(&[vec![at(s0, -half - past), at(s1, -half - past), at(s1, half + past), at(s0, half + past)]], PAPER);
    for s in [s0, s1] {
        d.line(&[at(s, -half), at(s, half)], 2.0, INK, false);
    }
    if matches!(e, Element::Door(_)) {
        let side = if o.flip_facing { -half } else { half };
        let (hinge, free) = if o.flip_hand { (s1, s0) } else { (s0, s1) };
        let pivot = at(hinge, side);
        let open = pivot + u.perp() * (side.signum() * t.width);
        d.line(&[pivot, open], 1.5, INK, false);
        d.line(&arc(pivot, open, at(free, side), 24), 1.0, INK, false);
    } else {
        for across in [-half, 0.0, half] {
            d.line(&[at(s0, across), at(s1, across)], 1.0, INK, false);
        }
    }
}

/// A stair in the plan of `level`. From its own level: its risers up to the cut plane, a cut line
/// across the run, the rest dashed, and its walk line marked UP. From the level it climbs to: all
/// of it, and the walk line marked DN.
fn stair(d: &mut Drawing, m: &Model, st: &Stair, level: Id) {
    let up = st.level == level;
    if !up && st.top_level != Some(level) {
        return;
    }
    let (n, each, length) = m.flight(st);
    let (u, half) = (heading(st.angle), st.width / 2.0);
    let at = |along: f64, across: f64| st.start + u * along + u.perp() * across;
    // The riser the cut plane passes through, from its own level; from above, none.
    let rise = m.plan_range(level).cut - m.elevation(st.level);
    let cut = if up && each > 0.0 { (rise / each).floor() } else { f64::INFINITY };
    let seg = |d: &mut Drawing, a: P2, b: P2, past: bool| {
        let pts = vec![d.view.px(a), d.view.px(b)];
        d.push(Prim::Line { pts, width: 1.0, color: INK, dash: past.then_some([6.0, 4.0]), closed: false });
    };
    for k in 0..n {
        let k = f64::from(k);
        seg(d, at(k * st.tread, -half), at(k * st.tread, half), k > cut);
    }
    let line = ((cut + 0.5) * st.tread).min(length);
    for side in [-half, half] {
        seg(d, at(0.0, side), at(line, side), false);
        seg(d, at(line, side), at(length, side), true);
    }
    if line < length {
        d.line(&[at(line - 0.4 * st.tread, -half), at(line + 0.4 * st.tread, half)], 1.0, INK, false);
    }
    // The walk line up the middle, its arrow at the top; from above, the other way.
    let (from, to, label) = if up { (at(0.0, 0.0), at(length, 0.0), "UP") } else { (at(length, 0.0), at(0.0, 0.0), "DN") };
    d.line(&[from, to], 1.0, INK, false);
    let (a, b) = (d.view.px(from), d.view.px(to));
    let (x, y) = arrow(d, a, b);
    d.text(label, (a.0 - x * 14.0, a.1 - y * 14.0), 9.0, INK);
}

/// An arrowhead at pixel `b`, pointing from `a`; the way it points, on screen.
fn arrow(d: &mut Drawing, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let head = vec![b, (b.0 - x * 10.0 - y * 4.0, b.1 - y * 10.0 + x * 4.0), (b.0 - x * 10.0 + y * 4.0, b.1 - y * 10.0 - x * 4.0)];
    d.push(Prim::Fill { rings: vec![head], color: INK });
    (x, y)
}

/// A roof in the plan of its level: its outline, and where its slopes meet, its ridge and hips.
fn roof(d: &mut Drawing, m: &Model, r: &Roof) {
    let pieces = m.roof_pieces(r).map_or_else(
        |_| vec![r.boundary.clone()],
        |(pieces, _)| pieces.iter().map(|p| p.iter().map(|q| P2(q[0], q[1])).collect()).collect(),
    );
    for p in pieces {
        d.line(&p, 1.0, INK, true);
    }
}

/// The short arc around `center` from `from` to `to`, as `n` segments.
fn arc(center: P2, from: P2, to: P2, n: u32) -> Vec<P2> {
    let (a, b) = (from - center, to - center);
    let start = a.1.atan2(a.0);
    let mut sweep = b.1.atan2(b.0) - start;
    if sweep > PI {
        sweep -= 2.0 * PI;
    } else if sweep < -PI {
        sweep += 2.0 * PI;
    }
    (0..=n)
        .map(|i| {
            let t = start + sweep * f64::from(i) / f64::from(n);
            center + P2(t.cos(), t.sin()) * a.length()
        })
        .collect()
}

/// A dimension, of its kind.
fn dimension(d: &mut Drawing, m: &Model, dim: &Dimension) {
    match dim.kind {
        DimKind::Aligned => aligned(d, m, dim),
        DimKind::Angular => angular(d, m, dim),
        DimKind::Radial => radial(d, m, dim),
    }
}

/// A number as a dimension shows it: to the hundredth, without trailing zeros.
fn figure(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0 + 0.0)
}

/// An angular dimension: an arc through `at` between its two lines, witness lines out along them
/// to it, ticks at its ends, and the angle outside its middle.
fn angular(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let Some((o, from, sweep)) = m.angle_sector(dim) else { return };
    let r = dim.at.dist(o);
    if r <= 0.0 {
        return;
    }
    let px = 1.0 / d.view.scale;
    let ray = |t: f64| P2(t.cos(), t.sin());
    let arc: Vec<P2> = (0..=48).map(|i| o + ray(from + sweep * f64::from(i) / 48.0) * r).collect();
    d.line(&arc, 1.0, INK, false);
    let lines: Vec<(P2, P2)> = dim.refs.iter().filter_map(|x| m.reference_line(x).ok()).collect();
    for t in [from, from + sweep] {
        let u = ray(t);
        // How far out along this ray its line reaches: the witness line runs from there to the arc.
        let reach = lines
            .iter()
            .filter(|(a, b)| geom::parallel(*a, *b, o, o + u))
            .flat_map(|(a, b)| [(*a - o).dot(u), (*b - o).dot(u)])
            .fold(0.0, f64::max);
        if r > reach + 3.0 * px {
            d.line(&[o + u * (reach + 3.0 * px), o + u * (r + 4.0 * px)], 1.0, INK, false);
        }
        let (p, k) = (o + u * r, (u + u.perp()) * (3.0 * px));
        d.line(&[p - k, p + k], 2.0, INK, false);
    }
    // Outside the arc's middle, clear of it however the text sits across the ray.
    let (text, mid) = (format!("{}°", figure(sweep.to_degrees())), ray(from + sweep / 2.0));
    let clear = f64::from(font::width(&text, 10.0) / 2.0) * mid.0.abs() + 5.0 * mid.1.abs() + 4.0;
    d.text(&text, d.view.px(o + mid * (r + clear * px)), 10.0, INK);
}

/// A radial dimension: a line from a round element's centre (marked with a cross) toward `at`,
/// an arrow where it meets the edge, and the radius past its end.
fn radial(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let Some(Ok((c, r))) = dim.refs.first().map(|x| m.reference_arc(x)) else { return };
    let u = if dim.at.dist(c) > 0.0 { (dim.at - c).unit() } else { P2(1.0, 0.0) };
    let (edge, end) = (c + u * r, if dim.at.dist(c) > r { dim.at } else { c + u * r });
    d.line(&[c, end], 1.0, INK, false);
    let (a, b) = (d.view.px(c), d.view.px(edge));
    // Inside a circle too small for it, the arrow comes in from outside.
    if r * d.view.scale > 14.0 || end == edge {
        arrow(d, a, b);
    } else {
        arrow(d, d.view.px(end), b);
    }
    for (dx, dy) in [(4.0, 0.0), (0.0, 4.0)] {
        d.push(Prim::Line { pts: vec![(a.0 - dx, a.1 - dy), (a.0 + dx, a.1 + dy)], width: 1.0, color: INK, dash: None, closed: false });
    }
    let text = format!("R {}", m.units.length(r));
    let (x, y) = d.view.px(end);
    let w = font::width(&text, 10.0) / 2.0 + 6.0;
    d.text(&text, (x + u.0 as f32 * w, y - u.1 as f32 * w), 10.0, INK);
}

/// A text note: its lines down from its top left corner, turned as it is, and its leader with an
/// arrow at its end.
pub(crate) fn note(d: &mut Drawing, n: &Note) {
    let h = (n.size * sheet::DENSITY) as f32;
    // Counter-clockwise in the view, clockwise on screen.
    let a = -(n.angle.to_radians() as f32);
    let (sin, cos) = a.sin_cos();
    let at = d.view.px(n.at);
    let turn = |(x, y): (f32, f32)| (at.0 + x * cos - y * sin, at.1 + x * sin + y * cos);
    for (i, line) in n.text.lines().enumerate() {
        let mid = turn((font::width(line, h) / 2.0, h / 2.0 + i as f32 * h * 1.6));
        d.text_turned(line, mid, h, a, INK);
    }
    if let Some(to) = n.leader {
        let (from, to) = (turn((-h * 0.4, h / 2.0)), d.view.px(to));
        d.push(Prim::Line { pts: vec![from, to], width: 1.0, color: INK, dash: None, closed: false });
        arrow(d, from, to);
    }
}

/// A spot elevation's mark at pixel `at`: a triangle standing on the point, and the height it
/// reads over a line beside it, in `units`.
fn spot_mark(d: &mut Drawing, (x, y): (f32, f32), z: Option<f64>, units: Units) {
    d.push(Prim::Fill { rings: vec![vec![(x, y), (x + 5.0, y - 8.0), (x - 5.0, y - 8.0)]], color: INK });
    let text = z.map_or_else(|| "?".into(), |z| height(units, z));
    let w = font::width(&text, 10.0);
    d.push(Prim::Line { pts: vec![(x - 5.0, y - 8.0), (x + 9.0 + w, y - 8.0)], width: 1.0, color: INK, dash: None, closed: false });
    d.text(&text, (x + 7.0 + w / 2.0, y - 16.0), 10.0, INK);
}

/// A height as a spot elevation reads it, signed: ±0, +3000, -600, +9'-10 1/8".
fn height(units: Units, z: f64) -> String {
    let shown = units.length(z);
    match shown.strip_prefix('-') {
        Some(_) => shown,
        // What rounds to nothing reads as nothing, either side.
        None if units.length(0.0) == shown => format!("±{shown}"),
        None => format!("+{shown}"),
    }
}

/// An aligned dimension: a witness line from each reference to the dimension line, a tick where
/// it crosses, and each segment's length over its middle, readable from below or from the right.
fn aligned(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let (n, cross) = m.crossings(dim);
    let (Some(&(lo, _)), Some(&(hi, _))) = (cross.first(), cross.last()) else { return };
    // Along the dimension line, and along the references; `px` mm make a pixel.
    let u = n.perp();
    let px = 1.0 / d.view.scale;
    let on = |s: f64| dim.at + n * s;
    d.line(&[on(lo - 4.0 * px), on(hi + 4.0 * px)], 1.0, INK, false);
    for &(s, (a, b)) in &cross {
        let c = on(s);
        // The reference's extent along it, from the dimension line: the witness line runs from
        // just off its nearer end to just past the dimension line.
        let (ta, tb) = ((a - c).dot(u), (b - c).dot(u));
        let (from, to) = match (ta.min(tb), ta.max(tb)) {
            (near, _) if near > 0.0 => (near - 3.0 * px, -4.0 * px),
            (_, near) if near < 0.0 => (near + 3.0 * px, 4.0 * px),
            _ => (-4.0 * px, 4.0 * px),
        };
        d.line(&[c + u * from, c + u * to], 1.0, INK, false);
        d.line(&[c - (u + n) * (3.0 * px), c + (u + n) * (3.0 * px)], 2.0, INK, false);
    }
    // The line's angle on screen (y down): it runs east or north, so the text reads from below
    // or from the right.
    let angle = (-n.1).atan2(n.0);
    // The text's up, on screen.
    let up = (angle.sin() as f32, -angle.cos() as f32);
    for pair in cross.windows(2) {
        let [(s0, _), (s1, _)] = pair else { continue };
        let (x, y) = d.view.px(on((s0 + s1) / 2.0));
        d.text_turned(&m.units.length(s1 - s0), (x + up.0 * 9.0, y + up.1 * 9.0), 10.0, angle as f32, INK);
    }
}

/// A door's or window's tag: its mark in a box for a door, in a hexagon for a window, over a
/// paper fill; with a leader, a line to the opening's centre.
fn tag(d: &mut Drawing, m: &Model, t: &Tag) {
    let Some(anchor) = m.anchor(t.host) else { return };
    let head = anchor + t.offset;
    if t.leader {
        d.line(&[anchor, head], 1.0, INK, false);
    }
    let text = m.tag_text(t);
    let (x, y) = d.view.px(head);
    let (w, h) = ((font::width(&text, 10.0) / 2.0 + 5.0).max(8.0), 8.0);
    let ring = if matches!(m.elements.get(&t.host), Some(Element::Window(_))) {
        vec![(x - w - 4.0, y), (x - w, y - h), (x + w, y - h), (x + w + 4.0, y), (x + w, y + h), (x - w, y + h)]
    } else {
        vec![(x - w, y - h), (x + w, y - h), (x + w, y + h), (x - w, y + h)]
    };
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    d.push(Prim::Line { pts: ring, width: 1.0, color: INK, dash: None, closed: true });
    d.text(&text, (x, y), 10.0, INK);
}

/// The way a section or elevation looks, on screen (y down).
fn looking_px(v: &Section) -> (f32, f32) {
    let l = v.looking();
    (l.0 as f32, -l.1 as f32)
}

/// A section's line in plan, dashed, with its head at its start (a flag pointing the way it
/// looks, its name behind it) and a tick at its end.
fn section_mark(d: &mut Drawing, v: &Section) {
    let (a, b) = (d.view.px(v.start), d.view.px(v.end));
    d.push(Prim::Line { pts: vec![a, b], width: 1.5, color: INK, dash: Some([18.0, 5.0]), closed: false });
    let (lx, ly) = looking_px(v);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    d.push(Prim::Fill { rings: vec![vec![a, (a.0 + ux * 12.0, a.1 + uy * 12.0), (a.0 + lx * 12.0, a.1 + ly * 12.0)]], color: INK });
    d.push(Prim::Line { pts: vec![b, (b.0 + lx * 9.0, b.1 + ly * 9.0)], width: 1.5, color: INK, dash: None, closed: false });
    let back = font::width(&v.name, 10.0) / 2.0 + 8.0;
    d.text(&v.name, (a.0 - ux * back, a.1 - uy * back), 10.0, INK);
}

/// An elevation's marker in plan, at its line's middle: a circle with a flag pointing the way it
/// looks, and its name behind it.
fn elevation_mark(d: &mut Drawing, v: &Section) {
    const R: f32 = 9.0;
    let c = d.view.px(v.start + (v.end - v.start) * 0.5);
    let ring: Vec<(f32, f32)> = (0..32)
        .map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + R * t.cos() as f32, c.1 + R * t.sin() as f32)
        })
        .collect();
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    d.push(Prim::Line { pts: ring, width: 1.2, color: INK, dash: None, closed: true });
    let (lx, ly) = looking_px(v);
    let side = (-ly * R * 0.8, lx * R * 0.8);
    let flag = vec![(c.0 + side.0, c.1 + side.1), (c.0 + lx * (R + 7.0), c.1 + ly * (R + 7.0)), (c.0 - side.0, c.1 - side.1)];
    d.push(Prim::Fill { rings: vec![flag], color: INK });
    let back = R + 6.0 + font::width(&v.name, 10.0) / 2.0 * lx.abs() + 6.0 * ly.abs();
    d.text(&v.name, (c.0 - lx * back, c.1 - ly * back), 10.0, INK);
}

/// A grid line, dashed, with a bubble holding its name beyond each end.
fn grid(d: &mut Drawing, start: P2, end: P2, name: &str) {
    let (a, b) = (d.view.px(start), d.view.px(end));
    d.push(Prim::Line { pts: vec![a, b], width: 1.0, color: GRID, dash: Some([14.0, 4.0]), closed: false });
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let dir = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    for ((x, y), k) in [(a, -BUBBLE), (b, BUBBLE)] {
        let c = (x + dir.0 * k, y + dir.1 * k);
        let ring = (0..32).map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + BUBBLE * t.cos() as f32, c.1 + BUBBLE * t.sin() as f32)
        });
        d.push(Prim::Line { pts: ring.collect(), width: 1.2, color: GRID, dash: None, closed: true });
        d.text(name, c, 10.0, GRID);
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// SVG path data for a polyline or ring.
fn path_data(pts: &[(f32, f32)], closed: bool) -> String {
    let mut s = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let _ = write!(s, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { " L" });
    }
    if closed {
        s.push_str(" Z");
    }
    s
}

pub fn svg(d: &Drawing) -> String {
    let (w, h) = (d.view.width, d.view.height);
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{}"/>"#,
        hex(PAPER)
    );
    for p in &d.prims {
        let _ = match p {
            Prim::Fill { rings, color } => {
                let data: Vec<String> = rings.iter().map(|r| path_data(r, true)).collect();
                write!(s, r#"<path d="{}" fill="{}" fill-rule="evenodd"/>"#, data.join(" "), hex(*color))
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let dash = dash.map_or_else(String::new, |[a, b]| format!(r#" stroke-dasharray="{a} {b}""#));
                write!(
                    s,
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"{dash}/>"#,
                    path_data(pts, *closed),
                    hex(*color)
                )
            }
            Prim::Image { at, width, height, rgba } => match picture(*width, *height, rgba).and_then(|pm| pm.encode_png().ok()) {
                Some(png) => write!(
                    s,
                    r#"<image x="{}" y="{}" width="{width}" height="{height}" href="data:image/png;base64,{}"/>"#,
                    at.0,
                    at.1,
                    crate::commands::base64(&png)
                ),
                None => Ok(()),
            },
        };
    }
    s.push_str("</svg>\n");
    s
}

fn path(rings: &[&[(f32, f32)]], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for ring in rings {
        for (i, &(x, y)) in ring.iter().enumerate() {
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}

pub fn png(d: &Drawing) -> Result<Vec<u8>, String> {
    let mut pm = Pixmap::new(d.view.width, d.view.height).ok_or("the image size is out of range")?;
    pm.fill(Color::from_rgba8(PAPER.0, PAPER.1, PAPER.2, 255));
    paint(&mut pm, d);
    pm.encode_png().map_err(|e| e.to_string())
}

/// A picture's pixels as a pixmap.
fn picture(width: u32, height: u32, rgba: &[u8]) -> Option<Pixmap> {
    Pixmap::from_vec(rgba.to_vec(), IntSize::from_wh(width, height)?)
}

/// The display list drawn over what `pm` holds.
fn paint(pm: &mut Pixmap, d: &Drawing) {
    for p in &d.prims {
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        match p {
            Prim::Fill { rings, color } => {
                paint.set_color_rgba8(color.0, color.1, color.2, 255);
                let rings: Vec<&[(f32, f32)]> = rings.iter().map(Vec::as_slice).collect();
                if let Some(path) = path(&rings, true) {
                    pm.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
                }
            }
            Prim::Line { pts, width, color, dash, closed } => {
                paint.set_color_rgba8(color.0, color.1, color.2, 255);
                let stroke = Stroke {
                    width: *width,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    dash: dash.and_then(|[a, b]| StrokeDash::new(vec![a, b], 0.0)),
                    ..Stroke::default()
                };
                if let Some(path) = path(&[pts], *closed) {
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Prim::Image { at, width, height, rgba } => {
                if let Some(img) = picture(*width, *height, rgba) {
                    pm.draw_pixmap(
                        at.0.round() as i32,
                        at.1.round() as i32,
                        img.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}
               room {"point": [4000, 3000], "name": "Living"}
               grid {"start": [0, -2000], "end": [0, 8000]}"#,
        )
        .unwrap();
        s
    }

    fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
        let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
        let c = pm.pixel(x as u32, y as u32).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn plans_fill_cut_walls_with_poche_and_open_them_at_doors() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "solid wall");
        assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (255, 255, 255), "the door's gap");
        assert_eq!(pixel(&png, d.view.px(P2(1000.0, 5000.0))), (255, 255, 255), "the room, away from its tag");
    }

    #[test]
    fn wall_corners_close_solid_without_a_notch() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        // Where the top and right walls overlap, and the square their ends stretch over.
        assert_eq!(pixel(&png, d.view.px(P2(8000.0, 6000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 6050.0))), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn a_plan_of_another_level_leaves_these_walls_out() {
        let s = house();
        let d = plan(&s.model, 2, 800, 600);
        assert!(!d.prims.is_empty(), "the grid and the view title still show");
        assert!(!d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
    }

    #[test]
    fn a_plan_drawn_at_a_given_view_maps_the_model_through_it() {
        let s = house();
        let fitted = plan(&s.model, 1, 800, 600);
        assert_eq!(draw(&s.model, 1, fitted.view).prims.len(), fitted.prims.len(), "the same display list");
        // Zoomed in twice on the house's top right corner: the corner lands mid-image.
        let view = View { scale: fitted.view.scale * 2.0, cx: 8000.0, cy: 6000.0, ..fitted.view };
        let png = png(&draw(&s.model, 1, view)).unwrap();
        assert_eq!(pixel(&png, (400.0, 300.0)), (POCHE.0, POCHE.1, POCHE.2));
        let back = view.mm(view.px(P2(1234.0, -567.0)));
        assert!(back.dist(P2(1234.0, -567.0)) < 0.01, "{back:?}");
    }

    #[test]
    fn every_primitive_names_the_element_it_draws() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(d.owners.len(), d.prims.len());
        // Walls 3 to 6, then the door, the window, the room and the grid.
        for id in 3..=10 {
            assert!(d.owners.contains(&Some(id)), "element {id} draws nothing");
        }
        assert_eq!(d.owners.last(), Some(&None), "the view title belongs to no element");
    }

    #[test]
    fn pick_finds_the_element_drawn_nearest_a_point() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick(at(2000.0, 0.0), 4.0), Some(7), "the door, drawn over its wall");
        assert_eq!(d.pick(at(6000.0, 0.0), 4.0), Some(3), "the wall");
        assert_eq!(d.pick(at(0.0, 3000.0), 4.0), Some(6), "the wall, under a grid line on its centre line");
        let (x, y) = at(6000.0, -100.0);
        assert_eq!(d.pick((x, y + 3.0), 4.0), Some(3), "just outside the wall's face");
        assert_eq!(d.pick(at(8000.0, 3000.0), 4.0), Some(8), "the window");
        assert_eq!(d.pick(at(4000.0, 3000.0), 10.0), Some(9), "the room, by its tag");
        assert_eq!(d.pick(at(0.0, -1500.0), 4.0), Some(10), "the grid line");
        assert_eq!(d.pick(at(4000.0, 1200.0), 4.0), None, "the floor of the room, away from its tag");
    }

    #[test]
    fn pick_all_lists_what_is_near_and_boxes_select_inside_or_crossing() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick_all(at(2000.0, 0.0), 4.0), [7, 3], "the door, then its wall: Tab's order");
        // A box around the window, across the wall it's in.
        let (a, b) = (at(7000.0, 2000.0), at(9000.0, 4000.0));
        assert_eq!(d.in_box(a, b, false), [8], "a window selects what is all inside");
        assert_eq!(d.in_box(b, a, true), [4, 8], "a crossing box also what it touches");
    }

    #[test]
    fn dimensions_and_tags_draw_over_the_plan() {
        let mut s = house();
        // The outer faces of the left and right walls, measured below the house; then every door and window tagged.
        s.script("dimension {\"refs\": [[-100, 3000], [8100, 3000]], \"at\": [4000, -1500]}\ntag {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        assert!(dark(d.view.px(P2(1000.0, -1500.0))), "the dimension line");
        assert!(dark(d.view.px(P2(-100.0, -1000.0))), "a witness line, down from the left wall's outer face");
        assert_eq!(d.pick(d.view.px(P2(1000.0, -1500.0)), 4.0), Some(11));
        // The door's tag, a box outside the wall (the door swings in); the window's, outside too.
        let (x, y) = d.view.px(P2(2000.0, -500.0));
        assert!(dark((x, y - 8.0)), "the top of the door tag's box");
        assert_eq!(d.pick((x, y), 2.0), Some(12));
        assert_eq!(d.pick(d.view.px(P2(8500.0, 3000.0)), 2.0), Some(13));
    }

    /// The house with a section (11) across it, west to east at y = 3000 so it looks north, and
    /// the South elevation (12).
    fn cut() -> Session {
        let mut s = house();
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nelevation {\"facing\": \"south\"}").unwrap();
        s
    }

    #[test]
    fn sections_cut_walls_in_poche_and_show_what_is_beyond() {
        let s = cut();
        let view = section_fit(&s.model, 11, 800, 600);
        let png = section_png(&s.model, 11, view).unwrap();
        // Along the section from its start at x = -1000; up from Level 1.
        let at = |x: f64, z: f64| view.px(P2(x + 1000.0, z));
        assert_eq!(pixel(&png, at(70.0, 1500.0)), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut (beside grid 1, on its centre line)");
        assert_eq!(pixel(&png, at(4000.0, 1500.0)), (255, 255, 255), "the north wall beyond, in hidden line");
        let d = section(&s.model, 11, view);
        assert_eq!(d.pick(at(0.0, 1500.0), 2.0), Some(6), "the cut is the wall's to pick");
        assert_eq!(d.pick(at(4000.0, 3000.0), 3.0), Some(2), "Level 2's line");
        assert_eq!(d.pick(at(0.0, 3700.0), 3.0), Some(10), "grid 1, where it crosses the section");
    }

    #[test]
    fn elevations_show_the_walls_they_look_at_and_cut_nothing() {
        let s = cut();
        let Some(Element::Elevation(e)) = s.model.elements.get(&12) else { panic!("no elevation") };
        let start = e.start;
        let view = section_fit(&s.model, 12, 800, 600);
        let png = section_png(&s.model, 12, view).unwrap();
        // The south wall's face, between the door and the corner, from the south.
        let p = view.px(P2(6000.0 - start.0, 1500.0));
        assert_eq!(pixel(&png, p), (255, 255, 255));
        let d = section(&s.model, 12, view);
        assert!(d.prims.iter().all(|p| !matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "no cut");
        assert_eq!(d.pick(view.px(P2(0.0 - start.0, 3700.0)), 3.0), Some(10), "grid 1 runs into what it sees");
        assert_eq!(
            view3d::Scene::new(&s.model)
                .cut(&section_camera(&s.model, 12, view).unwrap().0, &section_camera(&s.model, 12, view).unwrap().1, &[])
                .pick(f64::from(p.0), f64::from(p.1)),
            Some(3)
        );
    }

    #[test]
    fn plans_mark_sections_and_elevations() {
        let s = cut();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(d.pick(d.view.px(P2(-500.0, 3000.0)), 3.0), Some(11), "the section's line");
        let Some(Element::Elevation(e)) = s.model.elements.get(&12) else { panic!("no elevation") };
        let mid = e.start + (e.end - e.start) * 0.5;
        assert_eq!(d.pick(d.view.px(mid), 3.0), Some(12), "the elevation's marker");
    }

    #[test]
    fn plans_cut_columns_draw_stairs_and_show_roofs_from_both_sides() {
        let mut s = house();
        // Walls 3 to 6, door 7, window 8, room 9, grid 10; then a column 11, a stair 12 and a roof 13.
        s.script("column {\"at\": [6000, 4000]}\nstair {\"start\": [1000, 1500], \"end\": [1000, 2500]}\nroof {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 4000.0))), (POCHE.0, POCHE.1, POCHE.2), "the column, cut");
        // The stair: its first treads, and its arrow up the middle.
        assert_eq!(d.pick(d.view.px(P2(1400.0, 1500.0 + 280.0)), 2.0), Some(12), "a tread");
        // The roof overhead, dashed, from below; seen from its own level, its hips run to the corners.
        assert_eq!(d.pick(d.view.px(P2(4000.0, -600.0)), 2.0), Some(13), "the roof's edge, overhead");
        let up = plan(&s.model, 2, 800, 600);
        let corner = up.view.px(P2(-600.0, -600.0));
        let hip = up.view.px(P2(-600.0 + 600.0, -600.0 + 600.0));
        assert_eq!(up.pick(((corner.0 + hip.0) / 2.0, (corner.1 + hip.1) / 2.0), 2.0), Some(13), "a hip");
    }

    #[test]
    fn sections_cut_roofs_along_their_slope() {
        let mut s = house();
        s.script("roof {}\nsection {\"start\": [4000, -2000], \"end\": [4000, 8000]}").unwrap();
        // The roof 11, the section 12 (running north, looking west): its cut of the roof at the ridge.
        let view = section_fit(&s.model, 12, 800, 600);
        let d = section(&s.model, 12, view);
        let ridge = 3000.0 + 3100.0 * 30f64.to_radians().tan();
        let at = view.px(P2(2000.0 + 3000.0, ridge + 100.0));
        assert_eq!(d.pick(at, 2.0), Some(11));
        let png = section_png(&s.model, 12, view).unwrap();
        assert_eq!(pixel(&png, at), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn plans_draw_what_their_view_range_sees() {
        let mut s = house();
        // A low wall across the room (11), 900 mm tall; a floor (12); the roof over it all (13).
        s.script("wall {\"points\": [[1000, 4500], [5000, 4500]], \"height\": 900}\nfloor {}\nroof {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        // Cut at 1200: the window (900 to 2400 up) is open, and the low wall below the cut is seen from
        // above, its outline thin and no poché.
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 4600.0)), 2.0), Some(11));
        assert_eq!(pixel(&png, d.view.px(P2(3000.0, 4500.0))), (255, 255, 255));
        assert!(d.owners.contains(&Some(13)), "the roof overhead, under the level above");
        // Cut above the window's head, the wall runs solid past it; with the top under the roof's
        // eaves, the roof isn't seen.
        s.execute("modify", &serde_json::json!({"id": 1, "view_range": {"cut": 2500, "top": 2600}})).unwrap();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(pixel(&png_of(&d), d.view.px(P2(8050.0, 3000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert!(!d.owners.contains(&Some(13)));
        // From Level 2, the floor below shows once the view depth reaches down to it.
        assert!(!plan(&s.model, 2, 800, 600).owners.contains(&Some(12)));
        s.execute("modify", &serde_json::json!({"id": 2, "view_range": {"depth": -3000}})).unwrap();
        assert!(plan(&s.model, 2, 800, 600).owners.contains(&Some(12)));
    }

    fn png_of(d: &Drawing) -> Vec<u8> {
        png(d).unwrap()
    }

    #[test]
    fn crop_regions_clip_plans_and_sections() {
        let mut s = house();
        s.execute("crop", &serde_json::json!({"view": 1, "region": [[-1000, -1000], [4000, 4000]]})).unwrap();
        let d = plan(&s.model, 1, 800, 600);
        assert!((d.view.cx - 1500.0).abs() < 1.0 && (d.view.cy - 1500.0).abs() < 1.0, "fitted to the crop: {:?}", d.view);
        assert!(!d.owners.contains(&Some(4)), "the east wall lies outside");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 0.0)), 2.0), Some(3), "the south wall, up to the crop's edge");
        assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 2.0), None);
        // Annotation is whole or not there: the room's tag, its point on the crop's edge, keeps its name whole.
        assert!(d.owners.contains(&Some(9)));
        let name = d.view.px(P2(4000.0 + 300.0, 3000.0));
        assert!(d.pick((name.0, name.1 - 16.0), 3.0) == Some(9), "the name past the crop's edge");
        // Grid 1 stops at the crop's edge, its bubble there.
        let (x, y) = d.view.px(P2(0.0, 4000.0));
        assert_eq!(d.pick((x + 11.0, y - 11.0), 2.0), Some(10));
        // A section cropped to the ground floor's lower part: Level 2 isn't in it, and the cut wall stops.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        s.execute("crop", &serde_json::json!({"view": 11, "region": [[0, -100], [5000, 2000]]})).unwrap();
        let view = section_fit(&s.model, 11, 800, 600);
        let d = section(&s.model, 11, view);
        assert!(!d.owners.contains(&Some(2)) && d.owners.contains(&Some(1)), "only the levels inside");
        let png = section_png(&s.model, 11, view).unwrap();
        // (Beside grid 1, on the wall's centre line.)
        assert_eq!(pixel(&png, view.px(P2(950.0, 1500.0))), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut");
        assert_eq!(pixel(&png, view.px(P2(950.0, 2500.0))), (255, 255, 255), "above the crop");
    }

    #[test]
    fn notes_spot_elevations_and_dimensions_draw_in_their_views() {
        let mut s = house();
        // A note (11), a floor (12) and a spot elevation on it (13); a slanted wall (14) and an angular
        // dimension (15) between it and the south wall; a round column (16) and a radial dimension (17).
        s.script(
            r#"text {"text": "NOTE", "at": [1000, 5000]}
               floor {}
               spot {"at": [2000, 2000]}
               wall {"points": [[0, 0], [3000, 3000]], "type": "Interior - 100mm"}
               dimension {"kind": "angular", "refs": [[4000, 0], [1500, 1500]], "at": [2500, 1000]}
               column {"at": [6000, 4000], "type": "Round 300"}
               dimension {"kind": "radial", "refs": [[6150, 4000]], "at": [6800, 4800]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        // The note: its first letter's upright, just right of and below its top left corner.
        let (x, y) = d.view.px(P2(1000.0, 5000.0));
        assert_eq!(d.pick((x + 1.0, y + 6.0), 2.0), Some(11));
        // The spot elevation's mark: a triangle standing on the point.
        let (x, y) = d.view.px(P2(2000.0, 2000.0));
        assert!(dark((x, y - 4.0)), "its triangle");
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(13));
        // The angular dimension's arc, 1000 mm out from the corner at 45° / 2 round from east.
        let r = P2(2500.0, 1000.0).length();
        let mid = P2(22.5f64.to_radians().cos(), 22.5f64.to_radians().sin()) * r;
        assert_eq!(d.pick(d.view.px(mid), 2.0), Some(15));
        // The radial one's line, from the column's centre toward `at`.
        let along = P2(6000.0, 4000.0) + (P2(6800.0, 4800.0) - P2(6000.0, 4000.0)) * 0.5;
        assert_eq!(d.pick(d.view.px(along), 2.0), Some(17));
        // In a section: a spot elevation at its point, and a note.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nspot {\"at\": [3000, 2000], \"view\": 18}\ntext {\"text\": \"N\", \"at\": [5000, 2500], \"view\": 18}").unwrap();
        let view = section_fit(&s.model, 18, 800, 600);
        let d = section(&s.model, 18, view);
        let (x, y) = view.px(P2(3000.0, 2000.0));
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(19));
        assert!(d.owners.contains(&Some(20)));
    }

    #[test]
    fn spot_elevations_read_signed_heights_in_the_project_units() {
        assert_eq!(
            [height(Units::Millimetres, 0.3), height(Units::Millimetres, 3000.0), height(Units::Millimetres, -600.0)],
            ["±0", "+3000", "-600"]
        );
        assert_eq!([height(Units::Metres, 0.0), height(Units::FeetInches, 3000.0)], ["±0.00", "+9'-10 1/8\""]);
    }

    #[test]
    fn svg_and_png_are_well_formed() {
        let s = house();
        let d = plan(&s.model, 1, 640, 480);
        let svg = svg(&d);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"), "{svg}");
        assert_eq!(svg.matches("<path").count(), d.prims.len());
        let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (640, 480));
    }
}
