//! The plan canvas: a level's floor plan as `render::draw` lays it out, panned and zoomed. Over it:
//! selection (click, Ctrl adds, Shift takes away, Tab for what's underneath, boxes), dragging,
//! the tools' points, temporary dimensions, and previews of what a click would do.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{
    Column, DEFAULT_HEIGHT, DimKind, Dimension, Element, Face, Id, Line, MAX_COORD, Model, NOTE_SIZE, Note, RISER, Reference, Roof,
    Section, Spot, Stair, TOL, TREAD, Tag,
};
use bimcraft_core::render::{self, Drawing, Prim, Rgb, View};
use bimcraft_core::roof_footprint;
use egui::{
    Align, Align2, Color32, Event, FontId, Key, Layout, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, TextEdit, TextureId, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::{Icon, pal};
use crate::{BimApp, Sheet, Status, Tool, icon_button, top_params};

/// How near the cursor an element is picked, in px.
pub(crate) const PICK: f32 = 6.0;
/// How near the cursor a click snaps to a wall's end or a point of the chain, in px.
const SNAP: f64 = 10.0;
/// Zoom limits, px per mm: from 1000 km to 2 cm across 1000 px.
const SCALE: (f64, f64) = (1e-6, 50.0);

/// A level's view: px per mm and the model point at the canvas centre.
#[derive(Clone, Copy, Debug)]
struct Camera {
    scale: f64,
    cx: f64,
    cy: f64,
}

/// A drag with the primary button.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Moving the selection, from the model point first pressed.
    Move(P2),
    /// A selection box, from the pixel first pressed.
    Box((f32, f32)),
}

pub struct Canvas {
    cams: BTreeMap<Id, Camera>,
    /// Where the plan was last shown, in points.
    rect: Rect,
    /// The level shown last.
    level: Option<Id>,
    /// The model point under the cursor.
    pub cursor: Option<P2>,
    /// The model point a middle-button drag holds under the cursor.
    grab: Option<P2>,
    drag: Option<Drag>,
    /// What's under the cursor, as `Drawing::pick_all` orders it, and how far Tab has gone.
    under: Vec<Id>,
    cycle: usize,
    /// A crop region's grip being dragged: the view, and which edge (see [`grips`]).
    crop_drag: Option<(Id, usize)>,
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas {
            cams: BTreeMap::new(),
            rect: Rect::ZERO,
            level: None,
            cursor: None,
            grab: None,
            drag: None,
            under: Vec::new(),
            cycle: 0,
            crop_drag: None,
        }
    }
}

impl Canvas {
    fn view(&self, level: Id) -> Option<View> {
        let c = self.cams.get(&level)?;
        Some(View {
            scale: c.scale,
            cx: c.cx,
            cy: c.cy,
            width: self.rect.width().max(1.0) as u32,
            height: self.rect.height().max(1.0) as u32,
        })
    }

    /// Where model point `p` shows on screen.
    pub fn screen(&self, p: P2) -> Pos2 {
        match self.level.and_then(|l| self.view(l)) {
            Some(v) => self.rect.min + Vec2::from(v.px(p)),
            None => self.rect.center(),
        }
    }

    /// Whether the cursor was over the plan last frame.
    pub fn hovering(&self) -> bool {
        self.cursor.is_some()
    }

    /// Show all of `level` again, the next time it's drawn.
    pub fn fit(&mut self, level: Id) {
        self.cams.remove(&level);
    }

    /// View `id` (a plan's level, a section or an elevation) in `rect`, fitted by `fit` the first
    /// time it shows: the wheel zooms about the cursor and the middle button pans. The view, and the
    /// cursor's pixel in it while it's over it.
    pub(crate) fn navigate(
        &mut self,
        ui: &Ui,
        response: &Response,
        rect: Rect,
        id: Id,
        fit: impl FnOnce(u32, u32) -> View,
    ) -> Option<(View, Option<(f32, f32)>)> {
        self.rect = rect;
        self.level = Some(id);
        self.cams.entry(id).or_insert_with(|| {
            let v = fit(rect.width().max(1.0) as u32, rect.height().max(1.0) as u32);
            Camera { scale: v.scale, cx: v.cx, cy: v.cy }
        });
        let (hover, wheel, pinch, middle) = ui.input(|i| {
            (
                i.pointer.hover_pos().filter(|_| response.hovered()),
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                (i.pointer.button_pressed(PointerButton::Middle), i.pointer.button_down(PointerButton::Middle)),
            )
        });
        if let (Some(at), Some(v)) = (hover, self.view(id)) {
            let at = at - rect.min;
            // A wheel notch (about 50 points) zooms by a fifth.
            let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
            if factor.is_finite() && factor != 1.0 {
                self.hold(id, v.mm(at.into()), at, v.scale * factor.clamp(0.2, 5.0));
            }
            if middle.0 {
                self.grab = Some(v.mm(at.into()));
            }
        }
        match (self.grab, ui.input(|i| i.pointer.hover_pos()), self.view(id)) {
            (Some(g), Some(at), Some(v)) if middle.1 => self.hold(id, g, at - rect.min, v.scale),
            _ => self.grab = None,
        }
        Some((self.view(id)?, hover.map(|p| (p.x - rect.min.x, p.y - rect.min.y))))
    }

    /// View `id` moved `dx` mm along, to keep what's on screen in place when its coordinates move
    /// under it (a section's start moving along its line).
    pub(crate) fn slide(&mut self, id: Id, dx: f64) {
        if let Some(c) = self.cams.get_mut(&id) {
            c.cx += dx;
        }
    }

    /// Zoom by `factor` about the middle of the view.
    fn zoom(&mut self, level: Id, factor: f64) {
        if let Some(v) = self.view(level) {
            let mid = self.rect.size() / 2.0;
            self.hold(level, v.mm(mid.into()), mid, v.scale * factor);
        }
    }

    /// Keep model point `p` at pixel `at` (canvas coordinates) with `scale` px per mm.
    fn hold(&mut self, level: Id, p: P2, at: Vec2, scale: f64) {
        let scale = scale.clamp(SCALE.0, SCALE.1);
        let (w, h) = (f64::from(self.rect.width()), f64::from(self.rect.height()));
        let cx = (p.0 - (f64::from(at.x) - w / 2.0) / scale).clamp(-MAX_COORD, MAX_COORD);
        let cy = (p.1 + (f64::from(at.y) - h / 2.0) / scale).clamp(-MAX_COORD, MAX_COORD);
        if cx.is_finite() && cy.is_finite() {
            self.cams.insert(level, Camera { scale, cx, cy });
        }
    }
}

/// A drawing's colour on screen: as drawn, or in the dark theme with its lightness turned over,
/// so ink reads light on a dark ground and the grid stays blue.
pub(crate) fn color(c: Rgb) -> Color32 {
    if !crate::style::dark() {
        return Color32::from_rgb(c.0, c.1, c.2);
    }
    let (r, g, b) = (i32::from(c.0), i32::from(c.1), i32::from(c.2));
    let shift = 255 - r.max(g).max(b) - r.min(g).min(b);
    let over = |v: i32| (26 + (v + shift).clamp(0, 255) * 206 / 255) as u8;
    Color32::from_rgb(over(r), over(g), over(b))
}

fn pos(origin: Pos2, (x, y): (f32, f32)) -> Pos2 {
    origin + vec2(x, y)
}

/// Where a drawing's pixels go on screen: its (0, 0) at `origin`, `k` screen px a drawing px (a
/// sheet is drawn at paper density, and scaled as its view zooms). On `paper`, its colours stay
/// as drawn in either theme.
#[derive(Clone, Copy)]
pub(crate) struct Place {
    pub origin: Pos2,
    pub k: f32,
    pub paper: bool,
}

impl Place {
    pub(crate) fn at(origin: Pos2) -> Place {
        Place { origin, k: 1.0, paper: false }
    }
    pub(crate) fn pos(self, (x, y): (f32, f32)) -> Pos2 {
        self.origin + vec2(x, y) * self.k
    }
}

/// The display list, with the selected and hovered elements in the accent colours; with `thin`,
/// every line one pixel wide. Its pictures are `pictures`, in order.
pub(crate) fn paint(p: &Painter, at: Place, d: &Drawing, selected: &[Id], hovered: Option<Id>, thin: bool, pictures: &[TextureId]) {
    let mut pictures = pictures.iter();
    let color = |c: Rgb| if at.paper { Color32::from_rgb(c.0, c.1, c.2) } else { color(c) };
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        let accent = match *owner {
            Some(id) if selected.contains(&id) => Some((pal().accent, pal().select_fill)),
            Some(id) if Some(id) == hovered => Some((pal().hover, pal().hover_fill)),
            _ => None,
        };
        match prim {
            // ponytail: convex rings only (every fill the plans make today); tessellate once they fill holes or concave rings.
            Prim::Fill { rings, color: c } => {
                for r in rings {
                    let fill = accent.map_or(color(*c), |a| a.1);
                    p.add(Shape::convex_polygon(r.iter().map(|q| at.pos(*q)).collect(), fill, Stroke::NONE));
                }
            }
            Prim::Image { at: corner, width, height, .. } => {
                if let Some(texture) = pictures.next() {
                    let rect = Rect::from_min_size(at.pos(*corner), vec2(*width as f32, *height as f32) * at.k);
                    let tint = accent.map_or(Color32::WHITE, |a| a.1);
                    p.image(*texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
                }
            }
            Prim::Line { pts, width, color: c, dash, closed } => {
                let stroke = Stroke::new(if thin { 1.0 } else { *width * at.k }, accent.map_or(color(*c), |a| a.0));
                let mut pts: Vec<Pos2> = pts.iter().map(|q| at.pos(*q)).collect();
                match dash {
                    Some([on, off]) => {
                        // A dashed ring: its closing side too.
                        if *closed {
                            pts.extend(pts.first().copied());
                        }
                        p.extend(Shape::dashed_line(&pts, stroke, *on, *off));
                    }
                    None if *closed => {
                        p.add(Shape::closed_line(pts, stroke));
                    }
                    None => {
                        p.add(Shape::line(pts, stroke));
                    }
                }
            }
        }
    }
}

/// The selected elements' outlines taken by `f` (on screen): where a move, a turn or a mirror
/// puts them.
fn ghost(p: &Painter, origin: Pos2, d: &Drawing, selected: &[Id], f: &dyn Fn(Pos2) -> Pos2) {
    let stroke = Stroke::new(1.0, pal().accent);
    let at = |q: &(f32, f32)| f(pos(origin, *q));
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        if !owner.is_some_and(|id| selected.contains(&id)) {
            continue;
        }
        match prim {
            Prim::Fill { rings, .. } => {
                for r in rings {
                    p.add(Shape::closed_line(r.iter().map(at).collect(), stroke));
                }
            }
            Prim::Line { pts, closed: true, .. } => {
                p.add(Shape::closed_line(pts.iter().map(at).collect(), stroke));
            }
            Prim::Line { pts, .. } => {
                p.add(Shape::line(pts.iter().map(at).collect(), stroke));
            }
            Prim::Image { .. } => {}
        }
    }
}

/// What a preview draws on: the canvas, the model and its drawing, the level and the view.
struct Scene<'a> {
    p: &'a Painter,
    m: &'a Model,
    d: &'a Drawing,
    level: Id,
    v: &'a View,
}

impl Scene<'_> {
    fn screen(&self, pt: P2) -> Pos2 {
        pos(self.p.clip_rect().min, self.v.px(pt))
    }
}

/// Of the lines Align, Mirror and dimensions use on an element (a grid's, a wall's centre line,
/// faces and ends, a door's or a window's centre and jambs), the one nearest `p`.
fn nearest_line(m: &Model, id: Id, p: P2) -> Option<(P2, P2)> {
    m.reference_at(id, p, |_| true).and_then(|(r, _)| m.reference_line(&r).ok())
}

/// Degrees from the ray `centre`→`from` to the ray `centre`→`to`, counter-clockwise, in
/// (-180, 180]; each ray onto the nearest 15° when it's within 3° of one.
fn angle_between(centre: P2, from: P2, to: P2) -> f64 {
    let (a, b) = (snap_angle(from - centre), snap_angle(to - centre));
    let d = (b.1.atan2(b.0) - a.1.atan2(a.0)).to_degrees();
    if d > 180.0 {
        d - 360.0
    } else if d <= -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// A turn of `degrees` counter-clockwise in the model, about `c`, as the screen sees it (its y
/// runs down, so the turn runs the other way).
fn turned(c: Pos2, degrees: f64) -> impl Fn(Pos2) -> Pos2 {
    let (s, k) = (-degrees.to_radians()).sin_cos();
    let (s, k) = (s as f32, k as f32);
    move |q| {
        let d = q - c;
        c + vec2(d.x * k - d.y * s, d.x * s + d.y * k)
    }
}

/// The mirror across the screen line through `a` and `b`.
fn mirrored(a: Pos2, b: Pos2) -> impl Fn(Pos2) -> Pos2 {
    let u = (b - a).normalized();
    move |q| {
        let d = q - a;
        a + u * (2.0 * d.dot(u)) - d
    }
}

/// `p` to the nearest 10 mm.
fn round(p: P2) -> P2 {
    P2((p.0 / 10.0).round() * 10.0, (p.1 / 10.0).round() * 10.0)
}

fn point(p: P2) -> Value {
    json!([p.0, p.1])
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// `d`'s direction, turned onto the nearest multiple of 15° when it's within 3° of one, as
/// Revit's angle snaps do.
fn snap_angle(d: P2) -> P2 {
    let a = d.1.atan2(d.0);
    let near = (a / (PI / 12.0)).round() * (PI / 12.0);
    let a = if (a - near).abs() <= 3f64.to_radians() { near } else { a };
    P2(a.cos(), a.sin())
}

/// The point a tool picked first, while it has one.
fn start_of(tool: &Tool) -> &[P2] {
    match tool {
        Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) => start.as_slice(),
        _ => &[],
    }
}

/// Where a crop region's edges have their grips: west, east, south and north (in a section: its
/// start, its end, its bottom and its top).
fn grips([lo, hi]: [P2; 2]) -> [P2; 4] {
    let mid = (lo + hi) * 0.5;
    [P2(lo.0, mid.1), P2(hi.0, mid.1), P2(mid.0, lo.1), P2(mid.0, hi.1)]
}

/// The region with edge `k` (as [`grips`] orders them) moved to `p`, at least 100 mm across.
fn regrip([lo, hi]: [P2; 2], k: usize, p: P2) -> [P2; 2] {
    match k {
        0 => [P2(p.0.min(hi.0 - 100.0), lo.1), hi],
        1 => [lo, P2(p.0.max(lo.0 + 100.0), hi.1)],
        2 => [P2(lo.0, p.1.min(hi.1 - 100.0)), hi],
        _ => [lo, P2(hi.0, p.1.max(lo.1 + 100.0))],
    }
}

/// Of element `id`'s lines that a dimension of `kind` can take after `first` (for an aligned one,
/// parallel to it; for an angular one, across it), the nearest `p`, if it's within `near` mm: a
/// wall's far end isn't picked by a click on its middle.
fn reference_for(m: &Model, kind: DimKind, first: Option<&Reference>, id: Id, p: P2, near: f64) -> Option<Reference> {
    let first = first.and_then(|r| m.reference_line(r).ok());
    let keep = |l: Line| first.is_none_or(|(a, b)| geom::parallel(a, b, l.0, l.1) == (kind == DimKind::Aligned));
    m.reference_at(id, p, keep).filter(|(_, d)| *d <= near).map(|(r, _)| r)
}

/// The modifier keys held at the last primary-button release (else now): Ctrl adds to the
/// selection, Shift takes away.
pub(crate) fn click_modifiers(ui: &Ui) -> Modifiers {
    ui.input(|i| {
        i.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::PointerButton { button: PointerButton::Primary, pressed: false, modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or(i.modifiers)
    })
}

/// Where a click at `at` lands when picking points (walls, grids, a move's): on a wall's end or
/// a point of the chain within a few px, else on the centre line of the wall under the cursor
/// (so walls meet), else on the nearest 10 mm. `square` keeps the step from the chain's last
/// point horizontal or vertical.
fn snap(m: &Model, level: Id, v: &View, at: (f32, f32), chain: &[P2], square: bool) -> P2 {
    let mut p = v.mm(at);
    if square && let Some(&from) = chain.last() {
        if (p.0 - from.0).abs() > (p.1 - from.1).abs() {
            p.1 = from.1;
        } else {
            p.0 = from.0;
        }
    }
    let walls: Vec<(P2, P2, f64)> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) if m.cut_by(w, level) => Some((w.start, w.end, m.thickness(w))),
            _ => None,
        })
        .collect();
    let near = SNAP / v.scale;
    let ends = walls.iter().flat_map(|(a, b, _)| [*a, *b]).chain(chain.iter().copied());
    if let Some(end) = ends.filter(|e| e.dist(p) <= near).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p))) {
        return end;
    }
    let on = walls
        .iter()
        .map(|(a, b, t)| (*a, *b, geom::dist_to_segment(p, *a, *b) - t / 2.0))
        .filter(|(_, _, d)| *d <= f64::from(PICK) / v.scale)
        .min_by(|x, y| x.2.total_cmp(&y.2));
    match on {
        // Along the wall to the nearest 10 mm from its start.
        Some((a, b, _)) => micron(a + (b - a).unit() * ((geom::param_on_segment(p, a, b) * a.dist(b) / 10.0).round() * 10.0)),
        None => round(p),
    }
}

impl BimApp {
    /// The plan of `level`, filling `ui`.
    pub(crate) fn canvas(&mut self, ui: &mut Ui, level: Id) {
        // Clicks and drags; never the keyboard, which the shortcuts need.
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = std::sync::Arc::clone(&self.session.model);
        let fit = |w, h| render::plan(&model, level, w, h).view;
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, level, fit) else { return };
        let mods = ui.input(|i| i.modifiers);
        let c = &mut self.canvas;
        c.cursor = local.map(|q| view.mm(q));
        // ponytail: redraws the whole plan every frame; cache the drawing per model and view if plans grow slow.
        let d = render::draw(&model, level, view);

        // What's under the cursor; Tab steps through it. (Without the app's input hook, as in
        // tests, egui moved the keyboard to a widget on that Tab: give it back.)
        let under = match (&self.tool, local) {
            (Tool::Modify, Some(q)) => d.pick_all(q, PICK),
            _ => Vec::new(),
        };
        if under != c.under {
            (c.under, c.cycle) = (under, 0);
        }
        let tab = ui.input(|i| i.key_pressed(Key::Tab)) && local.is_some();
        if tab {
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
        if (std::mem::take(&mut self.tab_cycle) || tab) && !c.under.is_empty() {
            c.cycle = (c.cycle + 1) % c.under.len();
        }
        // How near the cursor a dimension's reference line must be, mm.
        let near = 2.0 * f64::from(PICK) / view.scale;
        let hovered = match (&self.tool, local) {
            (Tool::Modify, Some(_)) => c.under.get(c.cycle).copied(),
            (Tool::Door | Tool::Window | Tool::Trim(_) | Tool::Split | Tool::Offset, Some(q)) => {
                d.pick(q, PICK).filter(|id| model.wall(*id).is_some())
            }
            (Tool::Align(_) | Tool::Mirror { draw: false, .. }, Some(q)) => {
                d.pick(q, PICK).filter(|id| !model.reference_lines(*id).is_empty())
            }
            // Something to measure to: with a line parallel to the first one picked, or for an angle,
            // one across it.
            (Tool::Dimension(refs), Some(q)) => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| reference_for(&model, DimKind::Aligned, refs.first(), *id, view.mm(q), near).is_some()),
            (Tool::Angular(refs), Some(q)) if refs.len() < 2 => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| reference_for(&model, DimKind::Angular, refs.first(), *id, view.mm(q), near).is_some()),
            (Tool::Radial(None), Some(q)) => {
                d.pick_all(q, PICK).into_iter().find(|id| model.reference_arc(&Reference { id: *id, face: Face::Centre }).is_ok())
            }
            // What a spot elevation would read.
            (Tool::Spot, Some(q)) => model.spot_host(level, view.mm(q)),
            (Tool::Tag, Some(q)) => {
                d.pick(q, PICK).filter(|id| matches!(model.elements.get(id), Some(Element::Door(_) | Element::Window(_))))
            }
            _ => None,
        };

        let crop = self.crop_grips(ui, &response, rect, &view, level);
        // A drag from an element moves the selection; from empty floor, it draws a selection box.
        if response.drag_started_by(PointerButton::Primary)
            && self.tool == Tool::Modify
            && self.canvas.crop_drag.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let o = (o.x - rect.min.x, o.y - rect.min.y);
            self.canvas.drag = Some(match d.pick(o, PICK) {
                Some(id) => {
                    if !self.selection.contains(&id) {
                        self.selection = vec![id];
                        self.raise_context();
                    }
                    Drag::Move(view.mm(o))
                }
                None => Drag::Box(o),
            });
        }
        let at = ui.input(|i| i.pointer.latest_pos()).map(|p| (p.x - rect.min.x, p.y - rect.min.y));
        if response.drag_stopped()
            && let (Some(drag), Some(q)) = (self.canvas.drag.take(), at)
        {
            match drag {
                Drag::Move(from) => {
                    let by = round(view.mm(q) - from);
                    if by.length() > 0.0 {
                        self.run("move", json!({"ids": self.selection, "by": point(by)}));
                    }
                }
                Drag::Box(from) => self.pick_up(d.in_box(from, q, q.0 < from.0), click_modifiers(ui)),
            }
        }
        // A double click on a section's or an elevation's mark opens its view, as in Revit.
        if response.double_clicked()
            && let Some(id) = hovered.filter(|id| model.section(*id).is_some())
        {
            self.open_sheet(Sheet::Section(id));
            return;
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.click(&model, level, &view, (at.x - rect.min.x, at.y - rect.min.y), hovered, click_modifiers(ui));
        }

        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let mut marked = self.selection.clone();
        if let Tool::Trim(Some((first, _))) = self.tool {
            marked.push(first);
        }
        paint(&p, Place::at(rect.min), &d, &marked, hovered, self.thin, &[]);
        p.extend(crop);
        let rooms = hovered.iter().map(|id| (*id, pal().hover)).chain(self.selection.iter().map(|id| (*id, pal().accent)));
        for (id, accent) in rooms {
            if let Some(Element::Room(r)) = model.elements.get(&id)
                && let Some(g) = model.room_region(r)
            {
                p.add(Shape::closed_line(g.outer.iter().map(|q| pos(rect.min, view.px(*q))).collect(), Stroke::new(2.0, accent)));
            }
        }
        match (self.canvas.drag, local) {
            (Some(Drag::Move(from)), Some(q)) => {
                let (a, b) = (view.px(from), view.px(from + round(view.mm(q) - from)));
                ghost(&p, rect.min, &d, &self.selection, &|q| q + vec2(b.0 - a.0, b.1 - a.1));
            }
            (Some(Drag::Box(from)), Some(q)) => {
                let r = Rect::from_two_pos(pos(rect.min, from), pos(rect.min, q));
                p.rect_filled(r, 0.0, pal().accent.gamma_multiply(0.08));
                // Revit draws a crossing box (right to left) dashed, a window box solid.
                if q.0 < from.0 {
                    let ring = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    p.extend(Shape::dashed_line(&ring, Stroke::new(1.0, pal().accent), 6.0, 4.0));
                } else {
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, pal().accent), StrokeKind::Middle);
                }
            }
            _ => {}
        }
        if let Some(q) = local {
            let scene = Scene { p: &p, m: &model, d: &d, level, v: &view };
            self.preview(&scene, q, hovered, mods.shift);
        }
        self.temporary_dimension(ui, &model, &view, rect);
        self.nav_bar(ui, rect, level);
    }

    /// The navigation bar of view `id`, at its top right as in Revit: fit, zoom in, zoom out.
    pub(crate) fn nav_bar(&mut self, ui: &mut Ui, rect: Rect, id: Id) {
        let bar = Rect::from_min_size(rect.right_top() + vec2(-46.0, 14.0), vec2(32.0, 80.0));
        ui.painter_at(rect).rect(bar, 6.0, pal().surface, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        ui.scope_builder(UiBuilder::new().max_rect(bar.shrink(3.0)).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if icon_button(ui, Icon::Fit, "Zoom to Fit").clicked() {
                self.canvas.fit(id);
            }
            if icon_button(ui, Icon::ZoomIn, "Zoom In").clicked() {
                self.canvas.zoom(id, 1.25);
            }
            if icon_button(ui, Icon::ZoomOut, "Zoom Out").clicked() {
                self.canvas.zoom(id, 0.8);
            }
        });
    }

    /// View `id`'s crop region over it, in `rect` through `view`: its outline, and with Modify, a
    /// grip at the middle of each edge. A drag from a grip moves that edge; on release, `crop`.
    /// What to paint over the view once it's drawn.
    pub(crate) fn crop_grips(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, id: Id) -> Vec<Shape> {
        let Some(region) = render::crop_region(&self.session.model, id) else { return Vec::new() };
        let at = |p: P2| pos(rect.min, view.px(p));
        let modify = self.tool == Tool::Modify;
        if modify
            && response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = grips(region).iter().position(|g| at(*g).distance(o) <= 8.0)
        {
            self.canvas.crop_drag = Some((id, k));
        }
        let held = self.canvas.crop_drag.filter(|(v, _)| *v == id).map(|(_, k)| k);
        let now = ui.input(|i| i.pointer.latest_pos()).map(|q| view.mm((q.x - rect.min.x, q.y - rect.min.y)));
        let shown = match (held, now) {
            (Some(k), Some(p)) => regrip(region, k, round(p)),
            _ => region,
        };
        if response.drag_stopped()
            && let Some(k) = held
        {
            self.canvas.crop_drag = None;
            let moved = shown != region && self.run("crop", json!({"view": id, "region": [point(shown[0]), point(shown[1])]})).is_some();
            // A section's start went along its line: what's on screen stays where it was.
            if moved && k == 0 && self.session.model.section(id).is_some() {
                self.canvas.slide(id, -shown[0].0);
            }
        }
        let [lo, hi] = shown;
        let ring = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(at).to_vec();
        let mut shapes = vec![Shape::closed_line(ring, Stroke::new(1.0, if held.is_some() { pal().accent } else { pal().muted }))];
        if modify {
            for g in grips(shown) {
                let r = Rect::from_center_size(at(g), vec2(9.0, 9.0));
                shapes.push(Shape::rect_filled(r, 1.0, pal().surface));
                shapes.push(Shape::rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle));
            }
        }
        shapes
    }

    /// Add `found` to the selection with Ctrl, take it away with Shift, else select just it.
    pub(crate) fn pick_up(&mut self, found: Vec<Id>, mods: Modifiers) {
        if mods.shift {
            self.selection.retain(|id| !found.contains(id));
        } else if mods.command {
            for id in found {
                if !self.selection.contains(&id) {
                    self.selection.push(id);
                }
            }
        } else {
            self.selection = found;
        }
        if !self.selection.is_empty() {
            self.raise_context();
        }
    }

    fn click(&mut self, m: &Model, level: Id, v: &View, at: (f32, f32), hovered: Option<Id>, mods: Modifiers) {
        let typed = |app: &BimApp, cat: &str, mut p: Value| {
            if let Some(t) = app.types.get(cat) {
                p["type"] = json!(t);
            }
            p
        };
        match &self.tool {
            Tool::Modify => self.pick_up(hovered.into_iter().collect(), mods),
            Tool::Door | Tool::Window => {
                let cat = if matches!(self.tool, Tool::Door) { "door" } else { "window" };
                let p = typed(self, cat, json!({"at": point(round(v.mm(at))), "level": level}));
                self.run(cat, p);
            }
            Tool::Room => {
                self.run("room", json!({"point": point(round(v.mm(at))), "level": level}));
            }
            Tool::Level => {}
            Tool::Elevation => {
                self.run("elevation", json!({"at": point(round(v.mm(at)))}));
            }
            Tool::Wall(_) | Tool::Grid(_) | Tool::Move(_) | Tool::Copy(_) | Tool::Section(_) => {
                let chain: Vec<P2> = match &self.tool {
                    Tool::Wall(chain) => chain.clone(),
                    Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) => start.iter().copied().collect(),
                    _ => Vec::new(),
                };
                let square = mods.shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
                self.place(level, snap(m, level, v, at, &chain, square));
            }
            &Tool::Rotate { centre, start, place } => {
                self.tool = match (place, start) {
                    (true, _) => Tool::Rotate { centre: snap(m, level, v, at, &[], false), start: None, place: false },
                    (false, None) => Tool::Rotate { centre, start: Some(v.mm(at)), place: false },
                    (false, Some(from)) => return self.turn(centre, angle_between(centre, from, v.mm(at))),
                };
            }
            Tool::Mirror { draw: false, .. } => {
                if let Some(axis) = hovered.and_then(|id| nearest_line(m, id, v.mm(at))) {
                    self.reflect(axis);
                }
            }
            &Tool::Mirror { draw: true, start } => {
                let p = snap(m, level, v, at, start.as_slice(), mods.shift);
                match start {
                    None => self.tool = Tool::Mirror { draw: true, start: Some(p) },
                    Some(a) => self.reflect((a, p)),
                }
            }
            &Tool::Align(reference) => match (reference, hovered.and_then(|id| Some((id, nearest_line(m, id, v.mm(at))?)))) {
                (None, Some((_, line))) => self.tool = Tool::Align(Some(line)),
                (Some(r), Some((id, line))) => self.align(r, id, line),
                _ => {}
            },
            &Tool::Trim(first) => match (first, hovered) {
                (None, Some(a)) => self.tool = Tool::Trim(Some((a, v.mm(at)))),
                (Some((a, keep)), Some(b)) if b != a => {
                    self.run("corner", json!({"ids": [a, b], "keep": [point(keep), point(v.mm(at))]}));
                    self.tool = Tool::Trim(None);
                }
                _ => {}
            },
            Tool::Split => {
                if let Some(id) = hovered {
                    self.run("split", json!({"id": id, "at": point(snap(m, level, v, at, &[], false))}));
                }
            }
            Tool::Offset => {
                if let Some(id) = hovered
                    && let Some(by) = self.offset_by(m, id, v.mm(at))
                {
                    self.run(if self.offset_copy { "copy" } else { "move" }, json!({"ids": [id], "by": point(by)}));
                }
            }
            // A reference under the cursor joins the dimension; a click away from them places it.
            Tool::Dimension(refs) => {
                let mut refs = refs.clone();
                match hovered.and_then(|id| reference_for(m, DimKind::Aligned, refs.first(), id, v.mm(at), 2.0 * f64::from(PICK) / v.scale))
                {
                    Some(r) => {
                        if !refs.contains(&r) {
                            refs.push(r);
                        }
                        self.tool = Tool::Dimension(refs);
                    }
                    None if refs.len() >= 2 => {
                        let p = json!({"refs": refs, "at": point(round(v.mm(at))), "level": level});
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Dimension(Vec::new());
                        }
                    }
                    None => {}
                }
            }
            // Two lines that meet, then where the arc goes.
            Tool::Angular(refs) => {
                let mut refs = refs.clone();
                match hovered.and_then(|id| reference_for(m, DimKind::Angular, refs.first(), id, v.mm(at), 2.0 * f64::from(PICK) / v.scale))
                {
                    Some(r) if refs.len() < 2 && !refs.contains(&r) => {
                        refs.push(r);
                        self.tool = Tool::Angular(refs);
                    }
                    _ if refs.len() == 2 => {
                        let p = json!({"kind": "angular", "refs": refs, "at": point(round(v.mm(at))), "level": level});
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Angular(Vec::new());
                        }
                    }
                    _ => {}
                }
            }
            &Tool::Radial(picked) => match (picked, hovered) {
                (None, Some(id)) => self.tool = Tool::Radial(Some(Reference { id, face: Face::Centre })),
                (Some(r), _) => {
                    if self.run("dimension", json!({"kind": "radial", "refs": [r], "at": point(round(v.mm(at))), "level": level})).is_some()
                    {
                        self.tool = Tool::Radial(None);
                    }
                }
                (None, None) => {}
            },
            Tool::Text => self.write_note(level, round(v.mm(at))),
            Tool::Spot => {
                self.run("spot", json!({"at": point(round(v.mm(at))), "view": level}));
            }
            Tool::Tag => {
                if let Some(id) = hovered {
                    self.run("tag", json!({"ids": [id], "leader": self.tag_leader}));
                }
            }
            Tool::Column => match top_params(m, level, self.column_top, &self.column_height) {
                Ok(mut p) => {
                    p["at"] = point(snap(m, level, v, at, &[], false));
                    p["level"] = json!(level);
                    let p = typed(self, "column", p);
                    self.run("column", p);
                }
                Err(e) => self.status = Status { text: tf("column: {}", &[&e]), error: true },
            },
            &Tool::Stair(start) => {
                let p = snap(m, level, v, at, start.as_slice(), mods.shift);
                match (start, m.units.parse(&self.stair_width)) {
                    (None, _) => self.tool = Tool::Stair(Some(p)),
                    (Some(a), Some(width)) if a.dist(p) > TOL => {
                        self.run("stair", json!({"start": point(a), "end": point(p), "level": level, "width": width}));
                        self.tool = Tool::Stair(None);
                    }
                    (Some(_), None) => self.status = Status { text: tf("stair: `{}` is not a width", &[&self.stair_width]), error: true },
                    (Some(_), Some(_)) => {}
                }
            }
            // On the level above, over this plan's walls; once made, selected, as a finished sketch is.
            Tool::Roof => match (number(&self.roof_slope), m.units.parse(&self.roof_overhang)) {
                (Some(slope), Some(overhang)) => {
                    let on = m.level_above(level).unwrap_or(level);
                    let p = typed(self, "roof", json!({"level": on, "slope": slope, "overhang": overhang, "gable": self.roof_gable}));
                    if let Some(made) = self.run("roof", p) {
                        self.selection = made["id"].as_u64().into_iter().collect();
                        self.tool = Tool::Modify;
                    }
                }
                _ => {
                    let text = tf("roof: the slope `{}` and the overhang `{}` must be numbers", &[&self.roof_slope, &self.roof_overhang]);
                    self.status = Status { text, error: true };
                }
            },
        }
    }

    /// The Text tool's click: a note saying what the Options Bar says, its top left at `at` in
    /// view `view` (a plan's level, a section, an elevation or a sheet).
    pub(crate) fn write_note(&mut self, view: Id, at: P2) {
        if self.note_text.trim().is_empty() {
            self.status = Status { text: t("text: type what the note says in the Options Bar first").into(), error: true };
            return;
        }
        // `\n` typed in the bar breaks the line.
        let text = self.note_text.replace("\\n", "\n");
        self.run("text", json!({"text": text, "at": point(at), "view": view}));
    }

    /// Rotate the selection about `centre` by `angle` degrees, counter-clockwise; with Copy, a
    /// turned copy.
    pub(crate) fn turn(&mut self, centre: P2, angle: f64) {
        let p = json!({"ids": self.selection, "about": point(centre), "angle": angle, "copy": self.rotate_copy});
        let made = self.run("rotate", p);
        self.edited(made, self.rotate_copy);
    }

    /// Mirror the selection across the line through `a` and `b`; with Copy (Revit's default), a
    /// mirrored copy.
    fn reflect(&mut self, (a, b): (P2, P2)) {
        if a.dist(b) > TOL {
            let made = self.run("mirror", json!({"ids": self.selection, "axis": [point(a), point(b)], "copy": self.mirror_copy}));
            self.edited(made, self.mirror_copy);
        }
    }

    /// After Rotate or Mirror: back to Modify, with the copies selected when there are copies.
    fn edited(&mut self, made: Option<Value>, copied: bool) {
        let Some(made) = made else { return };
        if copied {
            self.selection = made["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        }
        self.tool = Tool::Modify;
    }

    /// Align's second pick: element `id` moved across so its `line` lies on the reference line.
    fn align(&mut self, reference: (P2, P2), id: Id, line: (P2, P2)) {
        if !geom::parallel(reference.0, reference.1, line.0, line.1) {
            self.status = Status { text: t("align: the lines aren't parallel").into(), error: true };
            return;
        }
        let n = (reference.1 - reference.0).unit().perp();
        let by = micron(n * (reference.0 - line.0).dot(n));
        if by.length() > 1e-6 {
            self.run("move", json!({"ids": [id], "by": point(by)}));
        }
        self.tool = Tool::Align(None);
    }

    /// Where Offset takes wall `id` for the cursor at `p`: across it, toward `p`, by the Options
    /// Bar's distance.
    fn offset_by(&mut self, m: &Model, id: Id, p: P2) -> Option<P2> {
        let w = m.wall(id)?;
        let Some(d) = m.units.parse(&self.offset).filter(|d| *d > 0.0) else {
            self.status = Status { text: tf("offset: `{}` is not a distance", &[&self.offset]), error: true };
            return None;
        };
        let n = (w.end - w.start).unit().perp();
        Some(micron(n * if (p - w.start).dot(n) >= 0.0 { d } else { -d }))
    }

    /// The tool's next point: of a wall chain, a grid's ends, a move's or a copy's base point and
    /// destination.
    pub(crate) fn place(&mut self, level: Id, p: P2) {
        match &mut self.tool {
            Tool::Wall(chain) => match chain.first() {
                Some(&first) if chain.len() >= 3 && first.dist(p) <= TOL => self.finish_wall(level, true),
                _ if chain.last().is_some_and(|l| l.dist(p) <= TOL) => {}
                _ => {
                    chain.push(p);
                    // Without Chain, every second click ends a wall.
                    if !self.chain && chain.len() == 2 {
                        self.finish_wall(level, false);
                    }
                }
            },
            Tool::Grid(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("grid", json!({"start": point(a), "end": point(p)}));
                }
                Some(a) => *start = Some(a),
            },
            Tool::Section(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("section", json!({"start": point(a), "end": point(p)}));
                }
                Some(a) => *start = Some(a),
            },
            Tool::Move(base) | Tool::Copy(base) => match base.take() {
                None => *base = Some(p),
                Some(from) => self.shift_selection(from, p),
            },
            _ => {}
        }
    }

    /// The Move and Copy tools' second point: the selection moved or copied by `to - from`. A copy
    /// is selected afterwards; with Multiple, the tool goes on copying from the same base point.
    fn shift_selection(&mut self, from: P2, to: P2) {
        let copy = matches!(self.tool, Tool::Copy(_));
        let again = if copy { Tool::Copy(Some(from)) } else { Tool::Move(Some(from)) };
        if from.dist(to) <= TOL {
            self.tool = again;
            return;
        }
        let params = json!({"ids": self.selection, "by": point(to - from)});
        match self.run(if copy { "copy" } else { "move" }, params) {
            None => self.tool = again,
            Some(_) if copy && self.multiple => self.tool = again,
            Some(made) => {
                if copy {
                    self.selection = made["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                }
                self.tool = Tool::Modify;
            }
        }
    }

    /// The typed length, from the tool's base point toward the cursor (onto the nearest 15° when
    /// close): Enter while a length is typed.
    pub(crate) fn place_typed(&mut self, level: Id) {
        let typed = std::mem::take(&mut self.typed_length);
        let (Some(base), Some(cursor)) = (self.base(), self.canvas.cursor) else { return };
        match self.session.model.units.parse(&typed) {
            Some(len) if len > 0.0 => self.place(level, micron(base + snap_angle(cursor - base) * len)),
            _ => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
        }
    }

    /// Build the wall chain drawn so far: the tool's Enter.
    pub(crate) fn finish_wall(&mut self, level: Id, closed: bool) {
        let Tool::Wall(chain) = &mut self.tool else { return };
        let chain = std::mem::take(chain);
        if chain.len() < 2 {
            return;
        }
        let mut p = json!({"points": chain.iter().map(|q| point(*q)).collect::<Vec<_>>(), "level": level, "closed": closed});
        if let Some(t) = self.types.get("wall") {
            p["type"] = json!(t);
        }
        // The Options Bar's Height: up to a level, or unconnected at a height.
        match top_params(&self.session.model, level, self.wall_top, &self.wall_height) {
            Ok(Value::Object(top)) => p.as_object_mut().into_iter().for_each(|p| p.extend(top.clone())),
            Ok(_) => {}
            Err(e) => {
                self.status = Status { text: tf("wall: {}", &[&e]), error: true };
                return;
            }
        }
        self.run("wall", p);
    }

    /// The selected wall's length as a temporary dimension beside it, as Revit shows one. A click
    /// on the number types a new length, which moves the wall's end (the walls joined there follow).
    fn temporary_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let &[id] = self.selection.as_slice() else {
            self.dim_edit = None;
            return;
        };
        let Some(w) = m.wall(id).filter(|_| self.tool == Tool::Modify && self.canvas.drag.is_none()) else { return };
        let (a, b) = (pos(rect.min, v.px(w.start)), pos(rect.min, v.px(w.end)));
        let along = b - a;
        if along.length() < 24.0 {
            return;
        }
        let u = along / along.length();
        // On the side away from the middle of the view: outside, for an outside wall.
        let n = vec2(u.y, -u.x);
        let n = if (a + along / 2.0 + n - rect.center()).length() < (a + along / 2.0 - n - rect.center()).length() { -n } else { n };
        let off = (m.thickness(w) / 2.0 * v.scale) as f32 + 18.0;
        let stroke = Stroke::new(1.0, pal().accent);
        let p = ui.painter_at(rect);
        p.line_segment([a + n * off, b + n * off], stroke);
        for e in [a, b] {
            p.line_segment([e + n * (off - 10.0).max(2.0), e + n * (off + 4.0)], stroke);
            p.line_segment([e + n * off - (u + n) * 3.0, e + n * off + (u + n) * 3.0], stroke);
        }
        let text = m.units.length(w.start.dist(w.end));
        let font = FontId::proportional(12.0);
        let size = p.layout_no_wrap(text.clone(), font.clone(), pal().accent).size() + vec2(10.0, 4.0);
        let at = Rect::from_center_size(a + along / 2.0 + n * (off + 11.0), size);
        let field = ui.id().with(("temporary dimension", id));
        let (mut done, mut start) = (None, false);
        match &mut self.dim_edit {
            Some((wall, typed)) if *wall == id => {
                let edit = ui.put(at.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                if edit.lost_focus() {
                    done = Some(ui.input(|i| !i.key_pressed(Key::Escape)).then(|| typed.clone()));
                } else if !edit.has_focus() {
                    edit.request_focus();
                }
            }
            _ => {
                let r = ui.interact(at, field.with("number"), Sense::click());
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Temporary dimension {}", &[&text])));
                p.rect_filled(at, 2.0, pal().surface);
                p.text(at.center(), Align2::CENTER_CENTER, &text, font, if r.hovered() { pal().hover } else { pal().accent });
                start = r.clicked();
            }
        }
        if start {
            self.dim_edit = Some((id, text));
        }
        if let Some(typed) = done {
            self.dim_edit = None;
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if len > 0.0 => {
                    let by = (w.end - w.start).unit() * (len - w.start.dist(w.end));
                    if by.length() > 1e-6 {
                        self.run("move", json!({"ids": [id], "by": point(by), "end": "end"}));
                    }
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// What the tool would do at pixel `q`: a chain's next segment, the room a click would bound,
    /// where a move, turn or mirror puts the selection, the line Align or Offset would use.
    fn preview(&self, sc: &Scene, q: (f32, f32), hovered: Option<Id>, shift: bool) {
        let (p, m, v, level) = (sc.p, sc.m, sc.v, sc.level);
        let origin = p.clip_rect().min;
        let cursor = pos(origin, q);
        let stroke = Stroke::new(2.0, pal().accent);
        let thin = Stroke::new(1.0, pal().accent);
        let label = |at: Pos2, text: String| {
            p.text(at + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, text, FontId::proportional(12.0), pal().accent);
        };
        let dashed = |a: Pos2, b: Pos2| p.extend(Shape::dashed_line(&[a, b], thin, 8.0, 4.0));
        // An element a click would add, drawn as the plan would draw it.
        let sketched = |e: &Element| ghost(p, origin, &render::sketch(m, *v, m.next_id, e), &[m.next_id], &|pt| pt);
        let ghost = |f: &dyn Fn(Pos2) -> Pos2| ghost(p, origin, sc.d, &self.selection, f);
        let chain = match &self.tool {
            Tool::Wall(chain) => chain.as_slice(),
            Tool::Grid(_) | Tool::Move(_) | Tool::Copy(_) | Tool::Section(None) | Tool::Stair(None) => start_of(&self.tool),
            // The section a click would end, and the elevation a click would place.
            &Tool::Section(Some(start)) => {
                let end = snap(m, level, v, q, &[start], shift);
                sketched(&Element::Section(Section { name: m.free_view_name("Section", true), start, end, depth: None, crop: None }));
                return;
            }
            Tool::Elevation => {
                let at = round(v.mm(q));
                if let Some(f) = m.facing_from(at)
                    && let Some((start, end)) = m.elevation_line(f, Some(at))
                {
                    let name = m.free_view_name(f.name(), false);
                    sketched(&Element::Elevation(Section { name, start, end, depth: None, crop: None }));
                }
                return;
            }
            Tool::Room => {
                if let Some(g) = geom::region_at(&m.bounding_lines(level), v.mm(q), TOL) {
                    p.add(Shape::closed_line(g.outer.iter().map(|pt| sc.screen(*pt)).collect(), stroke));
                    let (area, unit) = m.units.area(g.area());
                    label(cursor, format!("{area:.2} {unit}"));
                }
                return;
            }
            &Tool::Rotate { centre, start, place } => {
                let c = if place { cursor } else { sc.screen(centre) };
                p.circle_stroke(c, 5.0, stroke);
                p.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], thin);
                p.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], thin);
                if let (Some(from), false) = (start, place) {
                    let angle = angle_between(centre, from, v.mm(q));
                    p.line_segment([c, sc.screen(from)], thin);
                    p.line_segment([c, cursor], stroke);
                    ghost(&turned(c, angle));
                    label(cursor, format!("{}°", (angle * 100.0).round() / 100.0));
                }
                return;
            }
            &Tool::Mirror { draw, start } => {
                let axis = match (draw, start) {
                    (false, _) => hovered.and_then(|id| nearest_line(m, id, v.mm(q))).map(|(a, b)| (sc.screen(a), sc.screen(b))),
                    (true, Some(a)) => Some((sc.screen(a), sc.screen(snap(m, level, v, q, &[a], shift)))),
                    (true, None) => None,
                };
                if let Some((a, b)) = axis.filter(|(a, b)| a.distance(*b) > 1.0) {
                    dashed(a, b);
                    ghost(&mirrored(a, b));
                }
                return;
            }
            Tool::Align(reference) => {
                if let Some((a, b)) = reference {
                    // The reference, drawn on past its ends.
                    let (a, b) = (sc.screen(*a), sc.screen(*b));
                    let u = (b - a).normalized() * 2000.0;
                    dashed(a - u, b + u);
                }
                return;
            }
            Tool::Split => {
                if hovered.is_some() {
                    let at = sc.screen(snap(m, level, v, q, &[], false));
                    p.line_segment([at - vec2(0.0, 9.0), at + vec2(0.0, 9.0)], stroke);
                    p.line_segment([at - vec2(9.0, 0.0), at + vec2(9.0, 0.0)], stroke);
                }
                return;
            }
            Tool::Offset => {
                let d = m.units.parse(&self.offset).filter(|d| *d > 0.0);
                if let (Some(w), Some(d)) = (hovered.and_then(|id| m.wall(id)), d) {
                    let n = (w.end - w.start).unit().perp();
                    let by = n * if (v.mm(q) - w.start).dot(n) >= 0.0 { d } else { -d };
                    dashed(sc.screen(w.start + by), sc.screen(w.end + by));
                }
                return;
            }
            // The references picked, the one under the cursor, else the dimension a click places.
            Tool::Dimension(refs) => {
                let candidate =
                    hovered.and_then(|id| reference_for(m, DimKind::Aligned, refs.first(), id, v.mm(q), 2.0 * f64::from(PICK) / v.scale));
                for r in refs.iter().chain(&candidate) {
                    if let Ok((a, b)) = m.reference_line(r) {
                        dashed(sc.screen(a), sc.screen(b));
                    }
                }
                if candidate.is_none() && refs.len() >= 2 {
                    sketched(&Element::Dimension(Dimension { level, kind: DimKind::Aligned, refs: refs.clone(), at: round(v.mm(q)) }));
                }
                return;
            }
            Tool::Tag => {
                if let Some(id) = hovered {
                    sketched(&Element::Tag(Tag { host: id, offset: m.tag_offset(id), leader: self.tag_leader }));
                }
                return;
            }
            // The lines picked, the one under the cursor, else the arc a click places.
            Tool::Angular(refs) => {
                let candidate =
                    hovered.and_then(|id| reference_for(m, DimKind::Angular, refs.first(), id, v.mm(q), 2.0 * f64::from(PICK) / v.scale));
                for r in refs.iter().chain(candidate.as_ref().filter(|_| refs.len() < 2)) {
                    if let Ok((a, b)) = m.reference_line(r) {
                        dashed(sc.screen(a), sc.screen(b));
                    }
                }
                if refs.len() == 2 {
                    sketched(&Element::Dimension(Dimension { level, kind: DimKind::Angular, refs: refs.clone(), at: round(v.mm(q)) }));
                }
                return;
            }
            &Tool::Radial(Some(r)) => {
                sketched(&Element::Dimension(Dimension { level, kind: DimKind::Radial, refs: vec![r], at: round(v.mm(q)) }));
                return;
            }
            Tool::Text if !self.note_text.trim().is_empty() => {
                let text = self.note_text.replace("\\n", "\n");
                sketched(&Element::Text(Note { view: level, at: round(v.mm(q)), text, size: NOTE_SIZE, angle: 0.0, leader: None }));
                return;
            }
            Tool::Spot => {
                let at = round(v.mm(q));
                sketched(&Element::Spot(Spot { view: level, at, host: m.spot_host(level, at) }));
                return;
            }
            Tool::Column => {
                let at = snap(m, level, v, q, &[], false);
                let type_name =
                    self.types.get("column").cloned().or_else(|| m.default_type("column").map(String::from)).unwrap_or_default();
                sketched(&Element::Column(Column { type_name, level, at, angle: 0.0, top_level: None, height: 0.0, base_offset: 0.0 }));
                return;
            }
            // The stair a click would make: its risers up to the level above, toward the cursor.
            &Tool::Stair(Some(start)) => {
                let end = snap(m, level, v, q, &[start], shift);
                if let Some(width) = m.units.parse(&self.stair_width).filter(|_| end.dist(start) > TOL) {
                    let angle = (end.1 - start.1).atan2(end.0 - start.0).to_degrees();
                    let top_level = m.level_above(level);
                    sketched(&Element::Stair(Stair {
                        level,
                        top_level,
                        height: DEFAULT_HEIGHT,
                        start,
                        angle,
                        width,
                        riser: RISER,
                        tread: TREAD,
                    }));
                }
                return;
            }
            // The roof a click would make, with its ridge and hips.
            Tool::Roof => {
                let on = m.level_above(level).unwrap_or(level);
                if let (Some(slope), Some(overhang)) = (number(&self.roof_slope), m.units.parse(&self.roof_overhang))
                    && let Ok(boundary) = roof_footprint(m, on, overhang)
                {
                    let type_name =
                        self.types.get("roof").cloned().or_else(|| m.default_type("roof").map(String::from)).unwrap_or_default();
                    sketched(&Element::Roof(Roof { type_name, level: on, boundary, slope, overhang, gable: self.roof_gable }));
                }
                return;
            }
            _ => return,
        };
        let square = shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
        let next = snap(m, level, v, q, chain, square);
        if let Tool::Move(Some(base)) | Tool::Copy(Some(base)) = &self.tool {
            let delta = sc.screen(next) - sc.screen(*base);
            ghost(&|pt| pt + delta);
        }
        let mut pts: Vec<Pos2> = chain.iter().map(|pt| sc.screen(*pt)).collect();
        pts.push(sc.screen(next));
        p.add(Shape::line(pts, stroke));
        p.rect_stroke(Rect::from_center_size(sc.screen(next), vec2(8.0, 8.0)), 0.0, stroke, StrokeKind::Middle);
        // The listening dimension: the length so far, or the one being typed.
        match (chain.last(), self.typed_length.is_empty()) {
            (Some(_), false) => label(sc.screen(next), format!("{} (Enter)", self.typed_length.trim())),
            (Some(last), true) => label(sc.screen(next), m.units.with_unit(last.dist(next))),
            (None, _) => {}
        }
    }
}
