//! Plan views: what a level's plan shows, as a display list of filled rings and polylines in
//! pixels, drawn to SVG or PNG. Drafting conventions: walls and columns the cut plane passes
//! through get a heavy outline and grey poché, doors a leaf and swing, windows glass lines, stairs
//! their risers up to the cut line and a walk line, roofs their ridge and hips (dashed overhead
//! from the level below), rooms a tag, grids a dashed line with a bubble at each end, dimensions witness lines and ticks, door and window
//! tags their mark in a box, sections their line and head, elevations a marker. Sections and
//! elevations draw the same way along their line and up: the cut in poché, levels and grids,
//! over the 3D view's hidden-line picture of what lies beyond.

use std::f64::consts::PI;
use std::fmt::Write;

use tiny_skia::{Color, FillRule, IntSize, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::commands::m2;
use crate::font;
use crate::geom::{self, P2};
use crate::model::{CUT_HEIGHT, Dimension, Element, Id, Model, Opening, Roof, Section, Stair, TOL, Tag, Wall, heading};
use crate::view3d::{self, Camera, Clip, Image, Scene};

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
    let z = m.elevation(level) + CUT_HEIGHT;
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

/// The plan of `level` fitted into `width` x `height` px.
pub fn plan(m: &Model, level: Id, width: u32, height: u32) -> Drawing {
    draw(m, level, fit(&plan_extent(m, level), width, height))
}

/// What a plan of `level` shows: its walls, floors, rooms, dimensions and tags, the grids, and
/// the marks of the sections and elevations.
pub(crate) fn plan_extent(m: &Model, level: Id) -> Vec<P2> {
    let mut extent: Vec<P2> = cut_outlines(m, level).into_iter().flat_map(|(_, o)| o).collect();
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
    let ((lo, hi), len) = (heights(m), v.start.dist(v.end));
    let head = m.levels().iter().map(|(_, l)| font::width(&l.name, 10.0)).fold(0.0, f32::max) + 12.0;
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
    Some((cam, Clip { near: 0.0, far: v.depth, x: (column(0.0), column(v.start.dist(v.end))) }))
}

/// What section or elevation `id` sees past its line through `view`, in hidden line: the
/// picture its cut, levels and grids ([`section`]) go over.
pub fn section_image(m: &Model, scene: &Scene, id: Id, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    Some(scene.cut(&cam, &clip, tints))
}

/// What section or elevation `id` draws over its picture through `view`: where its line cuts
/// the solids, in poché; its levels, named past its end; the grids it crosses; and its name.
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
    for (level, l) in m.levels() {
        d.owner = Some(level);
        let (a, b) = (d.view.px(P2(0.0, l.elevation)), d.view.px(P2(len, l.elevation)));
        d.push(Prim::Line { pts: vec![a, b], width: 1.0, color: INK, dash: Some([12.0, 4.0]), closed: false });
        for (text, dy) in [(l.name.clone(), -8.0), (format!("{}", l.elevation), 8.0)] {
            d.text(&text, (b.0 + 8.0 + font::width(&text, 10.0) / 2.0, b.1 + dy), 10.0, INK);
        }
    }
    // Grids running into what it sees: across its line, reaching past it (within its depth).
    let (lo, hi) = heights(m);
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
    for (&id, e) in &m.elements {
        if let Element::Floor(f) = e
            && f.level == level
        {
            d.owner = Some(id);
            d.line(&f.boundary, 1.0, FAINT, true);
        }
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
            opening(&mut d, m, e, o, w, m.elevation(level) + CUT_HEIGHT);
        }
    }
    for (&id, e) in &m.elements {
        d.owner = Some(id);
        match e {
            Element::Grid(g) => grid(&mut d, g.start, g.end, &g.name),
            Element::Room(r) if r.level == level => {
                let area = m.room_region(r).map_or_else(|| "NOT ENCLOSED".to_string(), |g| format!("{:.2} m²", m2(g.area())));
                let (x, y) = d.view.px(r.point);
                d.text(&r.name, (x, y - 16.0), 13.0, INK);
                d.text(&r.number, (x, y), 10.0, INK);
                d.text(&area, (x, y + 14.0), 10.0, INK);
            }
            Element::Stair(st) => stair(&mut d, m, st, level),
            Element::Roof(r) if r.level == level => roof(&mut d, m, r),
            // Overhead, from the level below: its edge, dashed.
            Element::Roof(r) if m.level_above(level) == Some(r.level) => {
                let pts = r.boundary.iter().map(|p| d.view.px(*p)).collect();
                d.push(Prim::Line { pts, width: 1.0, color: INK, dash: Some([10.0, 5.0]), closed: true });
            }
            _ => {}
        }
    }
    // Annotation last, over the model; sections and elevations show on every plan.
    for (&id, e) in &m.elements {
        d.owner = Some(id);
        match e {
            Element::Dimension(dm) if dm.level == level => dimension(&mut d, m, dm),
            Element::Tag(t) if m.level_of(e) == Some(level) => tag(&mut d, m, t),
            Element::Section(v) => section_mark(&mut d, v),
            Element::Elevation(v) => elevation_mark(&mut d, v),
            _ => {}
        }
    }
    d.owner = None;
    let title = m.level(level).map_or_else(String::new, |l| l.name.clone());
    let at = (24.0 + font::width(&title, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text(&title, at, 16.0, INK);
    d
}

/// Whether the segment from `p` to `q` passes through the box from `lo` to `hi` (Liang-Barsky).
fn clips(p: (f32, f32), q: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let d = (q.0 - p.0, q.1 - p.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (pk, qk) in [(-d.0, p.0 - lo.0), (d.0, hi.0 - p.0), (-d.1, p.1 - lo.1), (d.1, hi.1 - p.1)] {
        if pk == 0.0 {
            if qk < 0.0 {
                return false;
            }
        } else if pk < 0.0 {
            t0 = t0.max(qk / pk);
        } else {
            t1 = t1.min(qk / pk);
        }
    }
    t0 <= t1
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
    let cut = if up && each > 0.0 { (CUT_HEIGHT / each).floor() } else { f64::INFINITY };
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
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let head = vec![b, (b.0 - x * 10.0 - y * 4.0, b.1 - y * 10.0 + x * 4.0), (b.0 - x * 10.0 + y * 4.0, b.1 - y * 10.0 - x * 4.0)];
    d.push(Prim::Fill { rings: vec![head], color: INK });
    d.text(label, (a.0 - x * 14.0, a.1 - y * 14.0), 9.0, INK);
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

/// An aligned dimension: a witness line from each reference to the dimension line, a tick where
/// it crosses, and each segment's length over its middle, readable from below or from the right.
fn dimension(d: &mut Drawing, m: &Model, dim: &Dimension) {
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
        d.text_turned(&format!("{}", (s1 - s0).round()), (x + up.0 * 9.0, y + up.1 * 9.0), 10.0, angle as f32, INK);
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
