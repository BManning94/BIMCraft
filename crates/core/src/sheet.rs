//! Sheets: paper with a title block, and views placed on it at a scale. A sheet is drawn once,
//! at [`DENSITY`] px per paper mm, as a display list in paper pixels: what the PNG, the PDF and the
//! desktop's sheet view show. Each view on it is drawn as its own view draws it, moved onto the
//! paper and made the viewport's to pick, and titled under it; a section's picture of what lies
//! beyond goes in as an image.

use crate::geom::P2;
use crate::model::{Element, Id, Model, Sheet, Viewport, paper};
use crate::render::{self, Drawing, PAPER, Prim, Rgb, View};
use crate::{font, view3d};

/// Paper px per mm in a sheet's drawing: plan lettering 10 px tall comes out 2 mm on paper.
pub const DENSITY: f64 = 5.0;
/// Room around what a view shows, on paper, for grid bubbles and the like: mm.
const ROOM: f64 = 12.0;
/// The title box in the bottom right corner, inside the border: mm.
const TITLE: P2 = P2(180.0, 40.0);
const INK: Rgb = Rgb(25, 25, 25);

/// Paper mm (from the bottom left corner, y up) to the sheet drawing's pixels.
fn page(paper: P2) -> View {
    View {
        scale: DENSITY,
        cx: paper.0 / 2.0,
        cy: paper.1 / 2.0,
        width: (paper.0 * DENSITY).round() as u32,
        height: (paper.1 * DENSITY).round() as u32,
    }
}

/// View `view` (a level's plan, a section, an elevation) at 1 : `scale` on paper: the frame its
/// drawing gets, `ROOM` mm around what it shows.
fn framing(m: &Model, view: Id, scale: f64) -> Option<View> {
    let k = DENSITY / scale;
    let pts: Vec<P2> = match m.elements.get(&view)? {
        Element::Level(_) => render::plan_extent(m, view),
        Element::Section(_) | Element::Elevation(_) => render::section_extent(m, view, k)?.to_vec(),
        _ => return None,
    };
    let (lo, hi) = match pts.first() {
        Some(&first) => pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))),
        None => (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)),
    };
    let side = |d: f64| ((d * k + 2.0 * ROOM * DENSITY).ceil().clamp(1.0, f64::from(u32::MAX))) as u32;
    Some(View { scale: k, cx: (lo.0 + hi.0) / 2.0, cy: (lo.1 + hi.1) / 2.0, width: side(hi.0 - lo.0), height: side(hi.1 - lo.1) })
}

/// How much paper view `view` takes at 1 : `scale`, mm.
pub fn size(m: &Model, view: Id, scale: f64) -> Option<P2> {
    framing(m, view, scale).map(|v| P2(f64::from(v.width) / DENSITY, f64::from(v.height) / DENSITY))
}

/// Viewport `id`'s view frame, and where that frame's top left corner sits in its sheet's drawing,
/// px: a point `p` of the view is at `frame.px(p)` plus that corner.
pub fn frame(m: &Model, id: Id) -> Option<(View, (f32, f32))> {
    let Some(Element::Viewport(vp)) = m.elements.get(&id) else { return None };
    let view = framing(m, vp.view, vp.scale)?;
    let (x, y) = page(paper(&m.sheet(vp.sheet)?.size)?).px(vp.at);
    Some((view, (x - view.width as f32 / 2.0, y - view.height as f32 / 2.0)))
}

/// Where view `view` at 1 : `scale` goes on sheet `sheet` by default: right of the views on it,
/// else in the middle of the paper above the title box.
pub fn spot(m: &Model, sheet: Id, view: Id, scale: f64) -> P2 {
    let paper = m.sheet(sheet).and_then(|s| paper(&s.size)).unwrap_or(P2(420.0, 297.0));
    let middle = P2((20.0 + paper.0 - 10.0) / 2.0, (10.0 + TITLE.1 + paper.1 - 10.0) / 2.0);
    let width = size(m, view, scale).map_or(0.0, |s| s.0);
    let right = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Viewport(v) if v.sheet == sheet => Some(v.at.0 + size(m, v.view, v.scale)?.0 / 2.0),
            _ => None,
        })
        .fold(None, |r: Option<f64>, x| Some(r.map_or(x, |r| r.max(x))));
    right.map_or(middle, |r| P2(r + 10.0 + width / 2.0, middle.1))
}

/// `text` with its left end at paper point `at` (mm), capitals `height` mm tall.
fn left(d: &mut Drawing, text: &str, at: P2, height: f64) {
    let h = (height * DENSITY) as f32;
    let (x, y) = d.view.px(at);
    d.text(text, (x + font::width(text, h) / 2.0, y), h, INK);
}

/// The border, 20 mm in on the left (for binding) and 10 mm on the other sides, and the title
/// box in the bottom right corner: the project, the sheet's name, its scale, and its number.
fn title_block(d: &mut Drawing, project: &str, sheet: &Sheet, paper: P2, scale: &str) {
    let rect = |a: P2, b: P2| vec![a, P2(b.0, a.1), b, P2(a.0, b.1)];
    d.line(&rect(P2(20.0, 10.0), P2(paper.0 - 10.0, paper.1 - 10.0)), 2.5, INK, true);
    let width = TITLE.0.min(paper.0 - 30.0);
    let (x0, x1, y0, y1) = (paper.0 - 10.0 - width, paper.0 - 10.0, 10.0, 10.0 + TITLE.1);
    d.line(&rect(P2(x0, y0), P2(x1, y1)), 2.5, INK, true);
    d.line(&[P2(x0, y0 + 26.0), P2(x1, y0 + 26.0)], 1.0, INK, false);
    d.line(&[P2(x1 - 50.0, y0), P2(x1 - 50.0, y0 + 26.0)], 1.0, INK, false);
    left(d, project, P2(x0 + 4.0, y0 + 33.0), 5.0);
    left(d, &sheet.name, P2(x0 + 4.0, y0 + 17.0), 3.5);
    left(d, scale, P2(x0 + 4.0, y0 + 7.0), 2.0);
    let number = (7.0 * DENSITY) as f32;
    let at = d.view.px(P2(x1 - 25.0, y0 + 13.0));
    d.text(&sheet.number, at, number, INK);
}

/// The scale a sheet's title box gives: its views', when they share one.
fn scale_note(views: &[&Viewport]) -> String {
    match views.split_first() {
        None => String::new(),
        Some((first, rest)) if rest.iter().all(|v| v.scale == first.scale) => format!("SCALE 1 : {}", first.scale),
        Some(_) => "SCALE AS INDICATED".into(),
    }
}

/// Sheet `id` on its paper: its title block, then each view on it, at its scale and titled.
pub fn draw(m: &Model, id: Id) -> Option<Drawing> {
    let sheet = m.sheet(id)?;
    let paper = paper(&sheet.size)?;
    let views: Vec<(Id, &Viewport)> = m
        .elements
        .iter()
        .filter_map(|(&vid, e)| match e {
            Element::Viewport(v) if v.sheet == id => Some((vid, v)),
            _ => None,
        })
        .collect();
    let mut d = Drawing::new(page(paper));
    d.own(Some(id));
    title_block(&mut d, &m.name, sheet, paper, &scale_note(&views.iter().map(|(_, v)| *v).collect::<Vec<_>>()));
    let mut scene = None;
    for (vid, vp) in views {
        let Some((view, (ox, oy))) = frame(m, vid) else { continue };
        d.own(Some(vid));
        // Paper under it, its own: a click anywhere over a view picks it.
        let (w, h) = (view.width as f32, view.height as f32);
        d.push(Prim::Fill { rings: vec![vec![(ox, oy), (ox + w, oy), (ox + w, oy + h), (ox, oy + h)]], color: PAPER });
        let inner = match m.elements.get(&vp.view) {
            Some(Element::Level(_)) => render::draw(m, vp.view, view),
            Some(Element::Section(_) | Element::Elevation(_)) => {
                let scene = scene.get_or_insert_with(|| view3d::Scene::new(m));
                if let Some(img) = render::section_image(m, scene, vp.view, view, &[]) {
                    d.push(Prim::Image { at: (ox, oy), width: img.width, height: img.height, rgba: img.rgba });
                }
                render::section(m, vp.view, view)
            }
            _ => continue,
        };
        // Its own title, owned by no element, gives way to the viewport's.
        for (prim, owner) in inner.prims.into_iter().zip(inner.owners) {
            if owner.is_some() {
                d.push(prim.moved(ox, oy));
            }
        }
        // The title, under it: the view's name over a line, its scale under that.
        let name = m.view_name(vp.view).unwrap_or_default();
        let corner = d.view.mm((ox, oy + view.height as f32));
        let x = corner.0 + ROOM;
        left(&mut d, name, P2(x, corner.1 - 5.0), 3.0);
        let line = f64::from(font::width(name, (3.0 * DENSITY) as f32)) / DENSITY + 10.0;
        d.line(&[P2(x, corner.1 - 8.0), P2(x + line, corner.1 - 8.0)], 1.5, INK, false);
        left(&mut d, &format!("1 : {}", vp.scale), P2(x, corner.1 - 11.0), 2.0);
    }
    d.own(None);
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::geom::P2;
    use crate::render::{POCHE, png};

    fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
        let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
        let c = pm.pixel(x as u32, y as u32).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn sheets_draw_a_title_block_and_their_views_at_scale() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               sheet {"name": "Plans"}
               place {"sheet": "A101", "view": 1}"#,
        )
        .unwrap();
        // Walls 3 to 6, the sheet 7, its viewport 8.
        let d = draw(&s.model, 7).unwrap();
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 120 && g < 120 && b < 120
        };
        assert!(dark(d.view.px(P2(20.0, 150.0))), "the border, 20 mm in from the left");
        assert!(dark(d.view.px(P2(415.0 - 5.0, 10.0))) || dark(d.view.px(P2(410.0, 10.0))), "the border's bottom right");
        // The plan at 1 : 100, where the viewport puts it: a wall in poché.
        let (v, (ox, oy)) = frame(&s.model, 8).unwrap();
        let at = |p: P2| {
            let (x, y) = v.px(p);
            (x + ox, y + oy)
        };
        assert_eq!(pixel(&png, at(P2(4000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert!((v.scale - DENSITY / 100.0).abs() < 1e-12, "1 : 100");
        // What a viewport draws is the viewport's to pick; the title block is the sheet's.
        assert_eq!(d.pick(at(P2(4000.0, 0.0)), 2.0), Some(8));
        assert_eq!(d.pick(d.view.px(P2(20.0, 150.0)), 2.0), Some(7));
    }
}
