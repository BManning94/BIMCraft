//! The command set: everything the CLI, scripts and MCP can do. Parameters are JSON, lengths are
//! millimetres. Commands build elements; [`Model::check`] then validates the whole model, so a
//! command only has to read its parameters carefully.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::engine::{Command, Run, Session};
use crate::geom::{self, P2};
use crate::language::Language;
use crate::model::{
    Column, ColumnType, DEFAULT_HEIGHT, DimKind, Dimension, Element, Face, Facing, Floor, FloorType, Grid, Id, Level, Line, MAX_COORD,
    MAX_POINTS, MAX_TEXT, Model, NOTE_SIZE, Note, Opening, OpeningType, RISER, Reference, Roof, RoofType, Room, STAIR_WIDTH, Schedule,
    Section, Sheet, Spot, Stair, TOL, TREAD, Tag, View3D, Viewport, Wall, WallType, heading, paper,
};
use crate::render::Drawing;
use crate::units::Units;
use crate::{pdf, render, schedule, sheet, view3d};

type R = Result<Value, String>;

const fn edit(id: &'static str, label: &'static str, aliases: &'static [&'static str], params: &'static str, run: Run) -> Command {
    Command { id, label, aliases, params, run, undoable: true }
}

/// A command that is no undo step: it only reads, or replaces the whole project.
const fn other(id: &'static str, label: &'static str, params: &'static str, run: Run) -> Command {
    Command { id, label, aliases: &[], params, run, undoable: false }
}

pub static ALL: &[Command] = &[
    other(
        "new",
        "New",
        "{name?, language?: en|pt|es} an empty project with Level 1 (0) and Level 2 (3000), named in its language (Nível 1, \
         Nível 2 in Portuguese; Nivel 1, Nivel 2 in Spanish)",
        new,
    ),
    other("open", "Open", "{path} a .bimcraft project", open),
    other(
        "save",
        "Save",
        "{path?} .bimcraft saves the project (Save As with a path); .ifc exports IFC4; .svg or .png exports the lowest level's \
         plan; .pdf prints every sheet, a page each",
        save,
    ),
    other("undo", "Undo", "{}", undo),
    other("redo", "Redo", "{}", redo),
    edit("level", "Level", &["LL"], "{elevation, name?} a level at an elevation in mm", level),
    edit("grid", "Grid", &["GR"], "{start: [x,y], end: [x,y], name?} names follow the last grid: 1, 2… or A, B…", grid),
    edit(
        "wall",
        "Wall",
        &["WA"],
        "{points: [[x,y],…], closed?, type?, level?, top_level?, height?, base_offset?} one wall per segment of its centre line; \
         up to the next level, or `height` mm tall (unconnected), or up to `top_level`",
        wall,
    ),
    edit(
        "door",
        "Door",
        &["DR"],
        "{at: [x,y] | wall: id, offset?, type?, level?, mark?, flip_facing?, flip_hand?} in the wall under `at`, \
         or in `wall` centred `offset` mm from its start",
        door,
    ),
    edit(
        "window",
        "Window",
        &["WN"],
        "{at: [x,y] | wall: id, offset?, sill?, type?, level?, mark?, flip_facing?} like door; `sill` defaults to the type's",
        window,
    ),
    edit(
        "floor",
        "Floor",
        &[],
        "{boundary?: [[x,y],…], level?, type?} slab with its top at the level; no boundary = the outline of the level's walls",
        floor,
    ),
    edit("room", "Room", &["RM"], "{point: [x,y], level?, name?, number?} bounded by the walls around the point", room),
    edit(
        "column",
        "Column",
        &["CL"],
        "{at: [x,y], type?, angle?, level?, top_level?, height?, base_offset?} a column, its width along `angle` degrees; up to \
         the next level, or `height` mm tall, or up to `top_level`",
        column,
    ),
    edit(
        "stair",
        "Stair",
        &[],
        "{start: [x,y], end?: [x,y] | angle?, level?, top_level?, height?, width?, riser?, tread?} a straight flight from the \
         middle of its first riser at `start`, climbing toward `end` (or `angle` degrees) up to the next level: as many risers \
         as keep each at most `riser` mm (180), treads `tread` deep (280), `width` wide (1000); the run is as long as that makes it",
        stair,
    ),
    edit(
        "roof",
        "Roof",
        &[],
        "{level?, boundary?: [[x,y],…], slope?, overhang?, gable?, type?} a roof on `level` (by default the one above the \
         lowest), over `boundary` or else the walls of the level below, `overhang` mm (500) past their faces; flat at `slope` 0, \
         else pitched at `slope` degrees (30) over a rectangle: hipped, or with `gable` its ends left open",
        roof,
    ),
    edit(
        "dimension",
        "Aligned Dimension",
        &["DI"],
        "{refs: [ref, …], at: [x, y], level?, kind?: aligned|angular|radial} aligned (the default): the distances between \
         parallel references, along a line through `at`; angular: the angle between two references that meet, on the side \
         of `at`; radial: a round column's radius, toward `at`. A ref is {id, face?}: a wall's centre (the default), left or \
         right face (of its start-to-end direction), start or end; a door's or window's centre, or its jamb toward its wall's \
         start or end; a grid's line. Or a point [x, y] on one of those (for radial, on a round column's edge). It follows \
         what it measures",
        dimension,
    ),
    edit(
        "text",
        "Text",
        &["TX"],
        "{text, at: [x, y], view?, size?, angle?, leader?: [x, y]} a text note, its first line's top left at `at`, in a view \
         by name or id: a level's plan (the lowest by default; `at` in plan mm), a section or an elevation (`at` along its \
         line from its start, and up), or a sheet by number (`at` in paper mm). `size` is its capitals' height on paper, \
         mm (2.5); `angle` degrees counter-clockwise; with `leader`, a line to that point. Lines break at \\n",
        text_note,
    ),
    edit(
        "spot",
        "Spot Elevation",
        &["EL"],
        "{at: [x, y], view?, host?} a spot elevation: in a level's plan (the lowest by default), the height of the top of \
         what stands on the level at `at` (or of `host`), following it; in a section or an elevation, of its point `at`, \
         along and up",
        spot,
    ),
    edit(
        "tag",
        "Tag by Category",
        &["TG"],
        "{ids?: [id,…], level?, offset?: [dx, dy], leader?} tags doors and windows with their marks, `offset` mm from their \
         centres (beside the wall by default); without ids, every door and window on the level not tagged yet",
        tag,
    ),
    edit(
        "section",
        "Section",
        &[],
        "{start: [x,y], end: [x,y], depth?, name?} a section view along the line, looking to its left (drawn west to east, it \
         looks north): it cuts the model at the line and sees `depth` mm past it, or everything",
        section,
    ),
    edit(
        "elevation",
        "Elevation",
        &[],
        "{facing?: north|south|east|west, at?: [x,y], start?, end?, depth?, name?} an elevation view of the building's side \
         `facing` (North shows the north side, looking south), from 3 m out, or through `at`; with `at` alone, of the side it's \
         on; with `start` and `end`, along that line, looking to its left; with none, the four not there yet",
        elevation,
    ),
    edit(
        "sheet",
        "Sheet",
        &[],
        "{number?, name?, size?: A0|A1|A2|A3|A4} a sheet of paper, landscape (A3 by default), with a title block; numbers \
         run A101, A102…",
        new_sheet,
    ),
    edit(
        "place",
        "Place View",
        &[],
        "{sheet: number or id, view: a level's, section's, elevation's, schedule's or 3D view's name or id, at?: [x, y], \
         scale?, azimuth?, elevation?} puts the view on the sheet at 1 : `scale` (100 by default; a schedule keeps its size), \
         its middle `at` mm from the paper's bottom left corner (by default right of the views there, or in the middle); a \
         view goes on one sheet. A schedule (Room Schedule, Door Schedule…) or the 3D view ({3D}, seen from `azimuth` and \
         `elevation` degrees) comes to be as it's first placed",
        place,
    ),
    edit(
        "crop",
        "Crop View",
        &[],
        "{view, region?: [[x, y], [x, y]] | null} crops a level's plan to the region's south-west and north-east corners \
         (plan mm), or a section or an elevation to its bottom left and top right (along its line from its start, and up: \
         its line then runs between them, and the notes in it stay put); null uncrops it; without a region, it crops to \
         what the view shows now",
        crop,
    ),
    edit(
        "type",
        "Edit Type",
        &[],
        "{category: wall|floor|door|window|column|roof, name, thickness? | width?, height?, sill? | width?, depth?, round?} creates \
         the type, or changes it everywhere it's used",
        edit_type,
    ),
    edit(
        "modify",
        "Modify",
        &[],
        "{id | ids: [id,…], …fields} change fields as `elements` shows them, of one element or several; `level` and `top_level` may be names",
        modify,
    ),
    edit(
        "move",
        "Move",
        &["MV"],
        "{ids: [id,…], by: [dx, dy], end?: start|end} moves elements in plan; walls joined to a moved wall stretch to stay \
         joined, and doors and windows slide along their walls. With `end`, only that end of one wall moves",
        move_elements,
    ),
    edit(
        "copy",
        "Copy",
        &["CO"],
        "{ids: [id,…], by: [dx, dy]} copies elements, with the doors and windows of copied walls; returns the copies' ids",
        copy,
    ),
    edit(
        "rotate",
        "Rotate",
        &["RO"],
        "{ids: [id,…], about: [x, y], angle, copy?} turns elements about a point by `angle` degrees, counter-clockwise; walls \
         joined to them stay joined, and doors and windows turn with their walls",
        rotate,
    ),
    edit(
        "mirror",
        "Mirror",
        &["MM", "DM"],
        "{ids: [id,…], axis: [[x, y], [x, y]], copy?} mirrors elements across the line through two points; mirrored doors \
         and windows face the other way",
        mirror,
    ),
    edit(
        "corner",
        "Trim/Extend to Corner",
        &["TR"],
        "{ids: [wall, wall], keep?: [[x, y], [x, y]]} trims or extends two walls to meet where their lines cross; `keep` is a \
         point on the part of each to keep",
        corner,
    ),
    edit("split", "Split", &["SL"], "{id, at: [x, y]} splits a wall in two at `at`; doors and windows go with their part", split),
    edit(
        "delete",
        "Delete",
        &["DE"],
        "{ids: [id,…]} deleting a wall deletes its doors and windows, deleting those their tags; a view takes its notes and \
         spot elevations, and what a spot elevation reads takes it; dimensions lose what's deleted",
        delete,
    ),
    edit(
        "units",
        "Project Units",
        &["UN"],
        "{length?: mm|cm|m|in|ft|ft-in} how drawings, schedules and the desktop show lengths, and read typed ones: \
         millimetres, centimetres, metres, fractional inches, decimal feet, or feet and fractional inches (to 1/16\"); areas \
         in m², or ft² with inches and feet. The model, and every command, keep millimetres",
        units,
    ),
    edit(
        "language",
        "Project Language",
        &[],
        "{code?: en|pt|es} the language the project's drawings speak (a stair's UP, a title block's SCALE, a schedule's \
         headings) and new names come in (levels, sections, elevations, sheets, rooms, schedules): English, Brazilian \
         Portuguese or Spanish. What's already named stays named",
        language,
    ),
    other("inspect", "Inspect", "{} levels, element counts, types, warnings and undo history", inspect),
    other(
        "render",
        "Render",
        "{view?: a level's, section's or elevation's name or id, a sheet's number, or {3D}, width?, height?, path?, azimuth?, \
         elevation?} a level's floor plan, a section, an elevation, a sheet (at 5 px a paper mm), or the 3D view seen from \
         `azimuth` degrees round from east (counter-clockwise) and `elevation` degrees up (by default from the south-east, \
         30° up), as a PNG (pngBase64), or written to a path: .png, .svg for a plan, .pdf for a sheet",
        render_view,
    ),
    other(
        "schedule",
        "Schedule",
        "{category: rooms|doors|windows|walls|floors|levels, path?} columns and rows; a .csv file with a path",
        schedule,
    ),
    other(
        "elements",
        "Elements",
        "{category?, level?, limit?, offset?} elements with ids, fields and derived values (length, height, area_m2…)",
        elements,
    ),
];

// ---------- parameters ----------

fn bad(cmd: &str, msg: impl std::fmt::Display) -> String {
    format!("{cmd}: {msg}")
}

fn absent(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `[x, y]` in mm, finite and within the model's limits.
fn point(v: &Value) -> Option<P2> {
    let p = match v.as_array()?.as_slice() {
        [x, y] => P2(x.as_f64()?, y.as_f64()?),
        _ => return None,
    };
    (p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD).then_some(p)
}

fn point_opt(c: &str, p: &Value, key: &str) -> Result<Option<P2>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => point(v).map(Some).ok_or_else(|| bad(c, format!("`{key}` must be a point [x, y] in mm, got {v}"))),
        None => Ok(None),
    }
}

fn point_req(c: &str, p: &Value, key: &str) -> Result<P2, String> {
    point_opt(c, p, key)?.ok_or_else(|| bad(c, format!("`{key}` [x, y] is required")))
}

fn points(c: &str, p: &Value, key: &str) -> Result<Vec<P2>, String> {
    let a = p.get(key).and_then(Value::as_array).ok_or_else(|| bad(c, format!("`{key}` [[x, y], …] is required")))?;
    if a.len() > MAX_POINTS {
        return Err(bad(c, format!("`{key}` has more than {MAX_POINTS} points")));
    }
    a.iter().map(|v| point(v).ok_or_else(|| bad(c, format!("`{key}`: {v} is not a point [x, y] in mm")))).collect()
}

fn num(c: &str, p: &Value, key: &str) -> Result<Option<f64>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && x.abs() <= MAX_COORD)
            .map(Some)
            .ok_or_else(|| bad(c, format!("`{key}` must be a number in mm, got {v}"))),
        None => Ok(None),
    }
}

fn flag(c: &str, p: &Value, key: &str) -> Result<bool, String> {
    match p.get(key) {
        v if absent(v) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(v) => Err(bad(c, format!("`{key}` must be true or false, got {v}"))),
        None => Ok(false),
    }
}

fn string(c: &str, p: &Value, key: &str) -> Result<Option<String>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= MAX_TEXT => Ok(Some(s.clone())),
        Some(v) => Err(bad(c, format!("`{key}` must be text of at most {MAX_TEXT} characters, got {v}"))),
        None => Ok(None),
    }
}

/// `ids` [id, …], or one `id`: elements of the model, each once, in the order given.
fn ids_param(c: &str, m: &Model, p: &Value) -> Result<Vec<Id>, String> {
    let as_id = |v: &Value| v.as_u64().ok_or_else(|| bad(c, format!("{v} is not an element id")));
    let ids: Vec<Id> = match (p.get("ids"), p.get("id")) {
        (Some(Value::Array(a)), _) if a.len() <= MAX_POINTS => a.iter().map(as_id).collect::<Result<_, _>>()?,
        (Some(Value::Array(_)), _) => return Err(bad(c, format!("`ids` lists more than {MAX_POINTS} elements"))),
        (v, Some(one)) if absent(v) => vec![as_id(one)?],
        _ => return Err(bad(c, "`ids` [id, …] is required")),
    };
    if let Some(missing) = ids.iter().find(|id| !m.elements.contains_key(id)) {
        return Err(bad(c, format!("no element {missing}")));
    }
    let mut seen = BTreeSet::new();
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

fn id_opt(c: &str, p: &Value, key: &str) -> Result<Option<Id>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| bad(c, format!("`{key}` must be an element id, got {v}"))),
        None => Ok(None),
    }
}

fn count_opt(c: &str, p: &Value, key: &str, default: usize) -> Result<usize, String> {
    match p.get(key) {
        v if absent(v) => Ok(default),
        Some(v) => {
            v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(c, format!("`{key}` must be a whole number, got {v}")))
        }
        None => Ok(default),
    }
}

/// `level` by id or name; the lowest level when not given.
fn level_param(c: &str, m: &Model, p: &Value, key: &str) -> Result<Id, String> {
    let levels = m.levels();
    match p.get(key) {
        v if absent(v) => levels.first().map(|(id, _)| *id).ok_or_else(|| bad(c, "the model has no levels")),
        Some(Value::String(n)) => levels.iter().find(|(_, l)| l.name == *n).map(|(id, _)| *id).ok_or_else(|| {
            let names: Vec<&str> = levels.iter().map(|(_, l)| l.name.as_str()).collect();
            bad(c, format!("no level named `{n}` (there are: {})", names.join(", ")))
        }),
        Some(v) => v.as_u64().filter(|&id| m.level(id).is_some()).ok_or_else(|| bad(c, format!("`{key}`: {v} is not a level"))),
        None => Err(bad(c, "the model has no levels")),
    }
}

/// `view` by name or id: a level's plan, a section, an elevation, a schedule or a 3D view, or with
/// `sheets` a sheet by its number; when not given, the plan of `level` (the lowest by default).
fn view_param(c: &str, m: &Model, p: &Value, sheets: bool) -> Result<Id, String> {
    match p.get("view") {
        v if absent(v) => level_param(c, m, p, "level"),
        Some(Value::String(n)) => m
            .elements
            .iter()
            .find_map(|(id, e)| {
                (m.view_name(*id) == Some(n.as_str()) || sheets && matches!(e, Element::Sheet(sh) if sh.number == *n)).then_some(*id)
            })
            .ok_or_else(|| bad(c, format!("no view named `{n}`"))),
        Some(v) => v.as_u64().filter(|id| m.elements.contains_key(id)).ok_or_else(|| bad(c, format!("`view`: {v} is not a view"))),
        None => level_param(c, m, p, "level"),
    }
}

/// `type`, or the category's default ([`Model::default_type`]) when not given. [`Model::check`]
/// reports a name that doesn't exist, listing the ones that do.
fn type_param(c: &str, p: &Value, m: &Model, category: &str) -> Result<String, String> {
    match string(c, p, "type")? {
        Some(n) => Ok(n),
        None => m.default_type(category).map(String::from).ok_or_else(|| bad(c, "the model has no types for this")),
    }
}

/// `walls` → `wall`: categories in any case, singular or plural.
pub(crate) fn category(s: &str) -> String {
    s.trim().to_ascii_lowercase().trim_end_matches('s').to_string()
}

// ---------- derived values ----------

/// mm² as m², to the hundredth.
pub fn m2(mm2: f64) -> f64 {
    (mm2 / 1e4).round() / 100.0
}

fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn level_name(m: &Model, id: Id) -> String {
    m.level(id).map_or_else(|| format!("level {id}"), |l| l.name.clone())
}

/// How far along a wall's centre line the point nearest `p` lies, in mm.
fn along(w: &Wall, p: P2) -> f64 {
    geom::param_on_segment(p, w.start, w.end) * w.start.dist(w.end)
}

/// The wall a plan of `level` cuts nearest `at`, within its half thickness plus 100 mm, and how
/// far along it `at` lies.
fn wall_at(m: &Model, level: Id, at: P2) -> Option<(Id, f64)> {
    m.elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::Wall(w) if m.cut_by(w, level) => Some((id, w, geom::dist_to_segment(at, w.start, w.end))),
            _ => None,
        })
        .filter(|(_, w, d)| *d <= m.thickness(w) / 2.0 + 100.0)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, w, _)| (id, along(w, at)))
}

/// The outer outline of the walls a plan of `level` cuts (of the largest piece), counter-clockwise.
fn outline(m: &Model, level: Id) -> Option<Vec<P2>> {
    let outer = geom::faces(&m.bounding_lines(level), TOL).into_iter().filter(|f| geom::area(f) < -TOL * TOL);
    outer.min_by(|a, b| geom::area(a).total_cmp(&geom::area(b))).map(|mut f| {
        f.reverse();
        f
    })
}

/// One past the largest number among `taken` ("1" when there is none).
pub(crate) fn next_number<'a>(taken: impl Iterator<Item = &'a str>) -> String {
    taken.filter_map(|t| t.trim().parse::<u64>().ok()).max().unwrap_or(0).saturating_add(1).to_string()
}

/// Revit-style grid naming: the letter after the last grid's letter, else the next number.
fn next_grid_name(m: &Model) -> String {
    let names: Vec<&str> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Grid(g) => Some(g.name.as_str()),
            _ => None,
        })
        .collect();
    let mut last = names.last().map_or("", |n| n).chars();
    let letter = match (last.next(), last.next()) {
        (Some(ch), None) if ch.is_ascii_uppercase() && ch < 'Z' => Some(char::from(ch as u8 + 1).to_string()),
        _ => None,
    };
    match letter {
        Some(l) if !names.contains(&l.as_str()) => l,
        _ => next_number(names.iter().copied()),
    }
}

/// An element as agents read it: its fields plus `id` and derived values.
pub fn element_json(m: &Model, id: Id) -> Value {
    let Some(e) = m.elements.get(&id) else { return Value::Null };
    let mut extra = Map::new();
    extra.insert("id".into(), json!(id));
    match e {
        Element::Wall(w) => {
            extra.insert("length".into(), json!(w.start.dist(w.end)));
            extra.insert("height".into(), json!(m.top_z(w) - m.base_z(w)));
            extra.insert("thickness".into(), json!(m.thickness(w)));
        }
        Element::Door(o) | Element::Window(o) => {
            if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("height".into(), json!(t.height));
                extra.insert("level".into(), json!(w.level));
                extra.insert("at".into(), json!(m.anchor(id)));
            }
        }
        Element::Dimension(d) => {
            extra.insert("values".into(), json!(m.dimension_values(d).into_iter().map(hundredth).collect::<Vec<_>>()));
        }
        Element::Column(col) => {
            let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
            extra.insert("height".into(), json!(top - bottom));
            if let Some(t) = m.types.column.get(&col.type_name) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("depth".into(), json!(t.depth));
                extra.insert("round".into(), json!(t.round));
            }
        }
        Element::Stair(st) => {
            let (n, each, length) = m.flight(st);
            let (bottom, top) = m.span(st.level, 0.0, st.top_level, st.height);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("risers".into(), json!(n));
            extra.insert("riser_height".into(), json!(hundredth(each)));
            extra.insert("length".into(), json!(length));
            extra.insert("end".into(), json!(st.start + heading(st.angle) * length));
        }
        Element::Roof(r) => {
            let ridge =
                m.roof_pieces(r).map_or(m.elevation(r.level), |(pieces, _)| pieces.iter().flatten().map(|p| p[2]).fold(f64::MIN, f64::max));
            extra.insert("ridge".into(), json!(hundredth(ridge)));
            extra.insert("area_m2".into(), json!(m2(geom::area(&r.boundary).abs())));
        }
        Element::Sheet(sh) => {
            extra.insert("paper".into(), json!(paper(&sh.size)));
            let on: Vec<Id> =
                m.elements.iter().filter(|(_, e)| matches!(e, Element::Viewport(v) if v.sheet == id)).map(|(id, _)| *id).collect();
            extra.insert("viewports".into(), json!(on));
        }
        Element::Viewport(v) => {
            extra.insert("title".into(), json!(m.view_name(v.view)));
            extra.insert("size".into(), json!(sheet::size(m, v.view, v.scale)));
        }
        Element::Section(v) | Element::Elevation(v) => {
            let l = v.looking();
            extra.insert("length".into(), json!(v.start.dist(v.end)));
            // `+ 0.0`: no `-0`.
            extra.insert("looking".into(), json!([l.0 + 0.0, l.1 + 0.0]));
        }
        Element::Tag(t) => {
            extra.insert("text".into(), json!(m.tag_text(t)));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("at".into(), json!(m.anchor(t.host).map(|a| a + t.offset)));
        }
        Element::Floor(f) => {
            extra.insert("area_m2".into(), json!(m2(geom::area(&f.boundary).abs())));
            extra.insert("thickness".into(), json!(m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness)));
        }
        Element::Room(r) => {
            let region = m.room_region(r);
            extra.insert("enclosed".into(), json!(region.is_some()));
            extra.insert("area_m2".into(), json!(region.as_ref().map_or(0.0, |g| m2(g.area()))));
            extra.insert("perimeter_m".into(), json!(region.as_ref().map_or(0.0, |g| (g.perimeter() / 10.0).round() / 100.0)));
        }
        Element::Text(_) => {
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Spot(sp) => {
            extra.insert("elevation".into(), json!(m.spot_value(sp).map(hundredth)));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Level(_) | Element::Grid(_) | Element::Schedule(_) | Element::View3D(_) => {}
    }
    let mut v = serde_json::to_value(e).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.extend(extra);
    }
    v
}

// ---------- files and history ----------

/// Project files this large or larger are refused.
const MAX_FILE: u64 = 256 << 20;

fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn new(s: &mut Session, p: &Value) -> R {
    let c = "new";
    let language = match string(c, p, "language")? {
        Some(code) => language_param(c, &code)?,
        None => Language::English,
    };
    let name = string(c, p, "name")?.unwrap_or_else(|| language.word("Project").into());
    s.reset(Model::new_in(&name, language), None);
    Ok(json!({"name": name, "language": language.code()}))
}

fn language_param(c: &str, code: &str) -> Result<Language, String> {
    Language::from_code(code)
        .ok_or_else(|| bad(c, format!("`{code}` is no language here; there are en (English), pt (Português) and es (Español)")))
}

fn language(s: &mut Session, p: &Value) -> R {
    let c = "language";
    if let Some(code) = string(c, p, "code")? {
        let l = language_param(c, &code)?;
        if l != s.model.language {
            s.model_mut().language = l;
        }
    }
    Ok(json!({"language": s.model.language.code()}))
}

fn open(s: &mut Session, p: &Value) -> R {
    let c = "open";
    let path = PathBuf::from(string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?);
    let size = std::fs::metadata(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?.len();
    if size >= MAX_FILE {
        return Err(bad(c, format!("{} is too large ({size} bytes)", path.display())));
    }
    // IFC comes in as a new project: saving it asks where.
    if ext(&path) == "ifc" {
        let bytes = std::fs::read(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let stem = path.file_stem().map_or_else(|| "Project".into(), |s| s.to_string_lossy().into_owned());
        let i = crate::import::ifc(&bytes, &stem).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in i.model.elements.values() {
            *counts.entry(e.category()).or_default() += 1;
        }
        let reply =
            json!({"path": path, "name": i.model.name, "format": i.schema, "elements": counts, "skipped": i.skipped, "moved": i.moved});
        s.reset(i.model, None);
        return Ok(reply);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    let m: Model = serde_json::from_str(&text).map_err(|e| bad(c, format!("{} is not a BIMCraft project: {e}", path.display())))?;
    m.check().map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    let name = m.name.clone();
    s.reset(m, Some(path.clone()));
    Ok(json!({"path": path, "name": name}))
}

fn save(s: &mut Session, p: &Value) -> R {
    let c = "save";
    let path = match string(c, p, "path")? {
        Some(p) => PathBuf::from(p),
        None => s.path.clone().ok_or_else(|| bad(c, "`path` is required: this project hasn't been saved yet"))?,
    };
    let format = ext(&path);
    if format == "ifc" {
        std::fs::write(&path, crate::ifc::export(&s.model)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "IFC4"}));
    }
    if format == "pdf" {
        let m = &s.model;
        let mut sheets: Vec<(&str, Drawing, P2)> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Sheet(sh) => Some((sh.number.as_str(), sheet::draw(m, *id)?, paper(&sh.size)?)),
                _ => None,
            })
            .collect();
        if sheets.is_empty() {
            return Err(bad(c, "there are no sheets to print; make one with `sheet` and `place` views on it"));
        }
        sheets.sort_by(|a, b| a.0.cmp(b.0));
        let pages: Vec<(&Drawing, P2)> = sheets.iter().map(|(_, d, p)| (d, *p)).collect();
        std::fs::write(&path, pdf::pdf(&pages)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "PDF", "pages": pages.len()}));
    }
    if matches!(format.as_str(), "svg" | "png") {
        let level = level_param(c, &s.model, &json!({}), "view")?;
        write_plan(&render::plan(&s.model, level, 1600, 1000), &path)?;
        return Ok(json!({"path": path, "format": format}));
    }
    let text = serde_json::to_string_pretty(&*s.model).map_err(|e| bad(c, e))?;
    std::fs::write(&path, text).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    s.path = Some(path.clone());
    Ok(json!({"path": path, "format": "bimcraft"}))
}

/// Standard base64, for images in JSON replies.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

fn write_plan(d: &render::Drawing, path: &Path) -> Result<(), String> {
    let bytes = match ext(path).as_str() {
        "svg" => render::svg(d).into_bytes(),
        "png" => render::png(d)?,
        other => return Err(format!("render: write a .png or .svg, not `.{other}`")),
    };
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A picture that only comes as PNG (the 3D view, a section): written to `path`, a .png, when
/// there is one, else returned in base64; `info` says what it is.
fn png_reply(c: &str, p: &Value, png: Vec<u8>, mut info: Value) -> R {
    match string(c, p, "path")? {
        Some(path) if ext(Path::new(&path)) == "png" => {
            std::fs::write(&path, png).map_err(|e| bad(c, format!("{path}: {e}")))?;
            info["path"] = json!(path);
        }
        Some(path) => return Err(bad(c, format!("only plans make SVGs; save this view as .png, not as {path}"))),
        None => info["pngBase64"] = json!(base64(&png)),
    }
    Ok(info)
}

fn render_view(s: &mut Session, p: &Value) -> R {
    let c = "render";
    let m = &s.model;
    let side = |key: &str, default: usize| count_opt(c, p, key, default).map(|n| n.clamp(16, 8192) as u32);
    let (w, h) = (side("width", 1600)?, side("height", 1000)?);
    let (w3, h3) = (w.min(view3d::MAX_SIDE), h.min(view3d::MAX_SIDE));
    let view = p.get("view");
    if matches!(view, Some(Value::String(v)) if v.eq_ignore_ascii_case("{3D}") || v.eq_ignore_ascii_case("3D")) {
        let (azimuth, elevation) = (num(c, p, "azimuth")?.unwrap_or(view3d::AZIMUTH), num(c, p, "elevation")?.unwrap_or(view3d::ELEVATION));
        let scene = view3d::Scene::new(m);
        let png = scene.render(&scene.fit(azimuth, elevation, w3, h3), &[]).png().map_err(|e| bad(c, e))?;
        return png_reply(
            c,
            p,
            png,
            json!({"width": w3, "height": h3, "view": "{3D}", "kind": "3D view", "azimuth": azimuth, "elevation": elevation}),
        );
    }
    // A section or elevation, by name or id.
    let named = |id: &Id, v: &Section| view.and_then(Value::as_str) == Some(v.name.as_str()) || view.and_then(Value::as_u64) == Some(*id);
    let cut = m.elements.iter().find_map(|(id, e)| match e {
        Element::Section(v) | Element::Elevation(v) if named(id, v) => Some((*id, v.name.clone(), e.category())),
        _ => None,
    });
    if let Some((id, name, kind)) = cut {
        let png = render::section_png(m, id, render::section_fit(m, id, w3, h3)).map_err(|e| bad(c, e))?;
        return png_reply(c, p, png, json!({"width": w3, "height": h3, "view": name, "kind": kind}));
    }
    // A sheet, by number, name or id, unless a level is called that.
    let level = match view {
        None => true,
        Some(Value::String(n)) => m.levels().iter().any(|(_, l)| l.name == *n),
        Some(v) => v.as_u64().is_some_and(|id| m.level(id).is_some()),
    };
    let named = |id: &Id, sh: &Sheet| {
        view.and_then(Value::as_str).is_some_and(|n| n == sh.number || n == sh.name) || view.and_then(Value::as_u64) == Some(*id)
    };
    let paper_of = m.elements.iter().find_map(|(id, e)| match e {
        Element::Sheet(sh) if !level && named(id, sh) => Some((*id, sh.number.clone(), paper(&sh.size)?)),
        _ => None,
    });
    if let Some((id, number, size)) = paper_of {
        let d = sheet::draw(m, id).ok_or_else(|| bad(c, format!("sheet {number} can't be drawn")))?;
        let info = json!({"width": d.view.width, "height": d.view.height, "view": number, "kind": "sheet"});
        if let Some(path) = string(c, p, "path")?.filter(|path| ext(Path::new(path)) == "pdf") {
            std::fs::write(&path, pdf::pdf(&[(&d, size)])).map_err(|e| bad(c, format!("{path}: {e}")))?;
            return Ok(json!({"path": path, "view": info["view"], "kind": "sheet"}));
        }
        return png_reply(c, p, render::png(&d)?, info);
    }
    let level = level_param(c, m, p, "view")?;
    let d = render::plan(m, level, w, h);
    match string(c, p, "path")? {
        Some(path) => {
            let path = PathBuf::from(path);
            write_plan(&d, &path)?;
            Ok(json!({"path": path, "width": w, "height": h, "view": level_name(m, level), "kind": "plan"}))
        }
        None => Ok(json!({"pngBase64": base64(&render::png(&d)?), "width": w, "height": h, "view": level_name(m, level), "kind": "plan"})),
    }
}

fn schedule(s: &mut Session, p: &Value) -> R {
    let c = "schedule";
    let cat = string(c, p, "category")?.ok_or_else(|| bad(c, "`category` is required: rooms, doors, windows, walls, floors or levels"))?;
    let t = crate::schedule::table(&s.model, &cat).map_err(|e| bad(c, e))?;
    if let Some(path) = string(c, p, "path")? {
        std::fs::write(&path, crate::schedule::to_csv(&t)).map_err(|e| bad(c, format!("{path}: {e}")))?;
    }
    Ok(json!({"columns": t.columns, "rows": t.rows}))
}

fn undo(s: &mut Session, _: &Value) -> R {
    s.step_back(false)
}

fn redo(s: &mut Session, _: &Value) -> R {
    s.step_back(true)
}

// ---------- modelling ----------

fn level(s: &mut Session, p: &Value) -> R {
    let c = "level";
    let elevation = num(c, p, "elevation")?.ok_or_else(|| bad(c, "`elevation` in mm is required"))?;
    let m = &s.model;
    let taken: BTreeSet<&str> = m.levels().into_iter().map(|(_, l)| l.name.as_str()).collect();
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => {
            let level = m.language.word("Level");
            (taken.len() + 1..).map(|n| format!("{level} {n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
        }
    };
    let id = s.model_mut().insert(Element::Level(Level::new(name.clone(), elevation)));
    Ok(json!({"id": id, "name": name}))
}

fn grid(s: &mut Session, p: &Value) -> R {
    let c = "grid";
    let (start, end) = (point_req(c, p, "start")?, point_req(c, p, "end")?);
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => next_grid_name(&s.model),
    };
    let id = s.model_mut().insert(Element::Grid(Grid { name: name.clone(), start, end }));
    Ok(json!({"id": id, "name": name}))
}

fn wall(s: &mut Session, p: &Value) -> R {
    let c = "wall";
    let mut pts = points(c, p, "points")?;
    if pts.len() < 2 {
        return Err(bad(c, "`points` needs at least 2 points"));
    }
    if flag(c, p, "closed")?
        && let Some(&first) = pts.first()
    {
        pts.push(first);
    }
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "wall")?;
    let height = num(c, p, "height")?;
    let top_level = match p.get("top_level") {
        v if absent(v) => height.is_none().then(|| m.level_above(level)).flatten(),
        _ => Some(level_param(c, m, p, "top_level")?),
    };
    let base_offset = num(c, p, "base_offset")?.unwrap_or(0.0);
    let height = height.unwrap_or(DEFAULT_HEIGHT);
    let m = s.model_mut();
    let ids: Vec<Id> = pts
        .windows(2)
        .filter_map(|seg| match seg {
            [a, b] => Some(m.insert(Element::Wall(Wall {
                type_name: type_name.clone(),
                level,
                start: *a,
                end: *b,
                top_level,
                height,
                base_offset,
            }))),
            _ => None,
        })
        .collect();
    Ok(json!({"ids": ids}))
}

fn door(s: &mut Session, p: &Value) -> R {
    opening(s, p, true)
}

fn window(s: &mut Session, p: &Value) -> R {
    opening(s, p, false)
}

fn opening(s: &mut Session, p: &Value, is_door: bool) -> R {
    let c = if is_door { "door" } else { "window" };
    let m = &s.model;
    let types = if is_door { &m.types.door } else { &m.types.window };
    let type_name = type_param(c, p, m, c)?;
    let at = point_opt(c, p, "at")?;
    let (host, along_at) = match id_opt(c, p, "wall")? {
        Some(h) => {
            let w = m.wall(h).ok_or_else(|| bad(c, format!("{h} is not a wall")))?;
            (h, at.map_or(w.start.dist(w.end) / 2.0, |a| along(w, a)))
        }
        None => {
            let at = at.ok_or_else(|| bad(c, "give `at` [x, y] on a wall, or `wall` with `offset`"))?;
            let level = level_param(c, m, p, "level")?;
            wall_at(m, level, at).ok_or_else(|| bad(c, format!("no wall at [{}, {}] on {}", at.0, at.1, level_name(m, level))))?
        }
    };
    let offset = num(c, p, "offset")?.unwrap_or(along_at);
    let sill = if is_door { 0.0 } else { num(c, p, "sill")?.unwrap_or_else(|| types.get(&type_name).map_or(0.0, |t| t.sill)) };
    let mark = match string(c, p, "mark")? {
        Some(mk) => mk,
        None => next_number(m.elements.values().filter_map(|e| match e {
            Element::Door(o) if is_door => Some(o.mark.as_str()),
            Element::Window(o) if !is_door => Some(o.mark.as_str()),
            _ => None,
        })),
    };
    let o = Opening {
        type_name,
        host,
        offset,
        sill,
        mark: mark.clone(),
        flip_facing: flag(c, p, "flip_facing")?,
        flip_hand: flag(c, p, "flip_hand")?,
    };
    let id = s.model_mut().insert(if is_door { Element::Door(o) } else { Element::Window(o) });
    Ok(json!({"id": id, "host": host, "offset": offset, "mark": mark}))
}

fn floor(s: &mut Session, p: &Value) -> R {
    let c = "floor";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "floor")?;
    let mut boundary = if absent(p.get("boundary")) {
        outline(m, level).ok_or_else(|| bad(c, format!("no walls enclose an area on {}; give `boundary`", level_name(m, level))))?
    } else {
        points(c, p, "boundary")?
    };
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    let area = m2(geom::area(&boundary));
    let id = s.model_mut().insert(Element::Floor(Floor { type_name, level, boundary }));
    Ok(json!({"id": id, "area_m2": area}))
}

fn room(s: &mut Session, p: &Value) -> R {
    let c = "room";
    let m = &s.model;
    let point = point_req(c, p, "point")?;
    let level = level_param(c, m, p, "level")?;
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word("Room").into());
    let number = match string(c, p, "number")? {
        Some(n) => n,
        None => next_number(m.elements.values().filter_map(|e| match e {
            Element::Room(r) => Some(r.number.as_str()),
            _ => None,
        })),
    };
    let r = Room { level, point, name, number: number.clone() };
    let region = m.room_region(&r).ok_or_else(|| {
        bad(
            c,
            format!(
                "[{}, {}] isn't enclosed by walls on {}; place rooms inside a closed loop of walls",
                point.0,
                point.1,
                level_name(m, level)
            ),
        )
    })?;
    let area = m2(region.area());
    let id = s.model_mut().insert(Element::Room(r));
    Ok(json!({"id": id, "number": number, "area_m2": area}))
}

/// Of the lines a plan of `level` shows that `keep` takes (walls' faces, centre lines and ends,
/// doors' and windows' centres and jambs, grids), the nearest `p`, within 100 mm.
fn reference_near(m: &Model, level: Id, p: P2, keep: impl Fn(Line) -> bool) -> Option<Reference> {
    let cut = |w: Id| m.wall(w).is_some_and(|w| m.cut_by(w, level));
    m.elements
        .iter()
        .filter(|(id, e)| match e {
            Element::Wall(_) => cut(**id),
            Element::Door(o) | Element::Window(o) => cut(o.host),
            Element::Grid(_) => true,
            _ => false,
        })
        .filter_map(|(&id, _)| m.reference_at(id, p, &keep))
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// The round column on `level` whose edge passes nearest `p`, within 100 mm.
fn arc_near(m: &Model, level: Id, p: P2) -> Option<Reference> {
    m.elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Column(col) if col.level == level))
        .filter_map(|(&id, _)| {
            let r = Reference { id, face: Face::Centre };
            let (c, radius) = m.reference_arc(&r).ok()?;
            Some((r, (p.dist(c) - radius).abs()))
        })
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

fn dimension(s: &mut Session, p: &Value) -> R {
    let c = "dimension";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let at = point_req(c, p, "at")?;
    let kind = match string(c, p, "kind")? {
        None => DimKind::Aligned,
        Some(k) => serde_json::from_value(json!(k)).map_err(|_| bad(c, format!("`kind` is aligned, angular or radial, not `{k}`")))?,
    };
    let given = p.get("refs").and_then(Value::as_array).ok_or_else(|| bad(c, "`refs` [ref, …] is required"))?;
    if given.len() > MAX_POINTS {
        return Err(bad(c, format!("`refs` has more than {MAX_POINTS} references")));
    }
    let mut refs: Vec<Reference> = Vec::new();
    for v in given {
        let near = |q: P2, what: &str| bad(c, format!("no {what} at [{}, {}] on {}", q.0, q.1, level_name(m, level)));
        let r = match point(v) {
            Some(q) if kind == DimKind::Radial => arc_near(m, level, q).ok_or_else(|| near(q, "round column's edge"))?,
            Some(q) => {
                // After the first: one parallel to it, or for an angle, one across it.
                let first = refs.first().and_then(|r| m.reference_line(r).ok());
                let keep = |l: Line| first.is_none_or(|(a, b)| geom::parallel(a, b, l.0, l.1) == (kind == DimKind::Aligned));
                reference_near(m, level, q, keep)
                    .ok_or_else(|| near(q, "wall face, wall centre line, wall end, door, window or grid line"))?
            }
            None => serde_json::from_value::<Reference>(v.clone())
                .map_err(|e| bad(c, format!("{v} is not a reference {{id, face?}} or a point [x, y]: {e}")))?,
        };
        refs.push(r);
    }
    let d = Dimension { level, kind, refs, at };
    let values: Vec<f64> = m.dimension_values(&d).into_iter().map(hundredth).collect();
    let id = s.model_mut().insert(Element::Dimension(d));
    Ok(json!({"id": id, "values": values}))
}

fn tag(s: &mut Session, p: &Value) -> R {
    let c = "tag";
    let m = &s.model;
    let offset = point_opt(c, p, "offset")?;
    let leader = flag(c, p, "leader")?;
    let hosts = if absent(p.get("ids")) && absent(p.get("id")) {
        // Tag All: the level's doors and windows that have no tag.
        let level = level_param(c, m, p, "level")?;
        let tagged: BTreeSet<Id> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Tag(t) => Some(t.host),
                _ => None,
            })
            .collect();
        let open: Vec<Id> = m
            .elements
            .iter()
            .filter(|(id, e)| matches!(e, Element::Door(_) | Element::Window(_)) && !tagged.contains(id) && m.level_of(e) == Some(level))
            .map(|(id, _)| *id)
            .collect();
        if open.is_empty() {
            return Err(bad(c, format!("nothing to tag: {} has no door or window without a tag", level_name(m, level))));
        }
        open
    } else {
        ids_param(c, m, p)?
    };
    let tags: Vec<Tag> = hosts.iter().map(|&host| Tag { host, offset: offset.unwrap_or_else(|| m.tag_offset(host)), leader }).collect();
    let m = s.model_mut();
    let ids: Vec<Id> = tags.into_iter().map(|t| m.insert(Element::Tag(t))).collect();
    Ok(json!({"ids": ids}))
}

fn text_note(s: &mut Session, p: &Value) -> R {
    let c = "text";
    let m = &s.model;
    let text = string(c, p, "text")?.ok_or_else(|| bad(c, "`text` is required"))?;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, true)?;
    let n = Note {
        view,
        at,
        text,
        size: num(c, p, "size")?.unwrap_or(NOTE_SIZE),
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        leader: point_opt(c, p, "leader")?,
    };
    let id = s.model_mut().insert(Element::Text(n));
    Ok(json!({"id": id}))
}

fn spot(s: &mut Session, p: &Value) -> R {
    let c = "spot";
    let m = &s.model;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, false)?;
    let host = match (id_opt(c, p, "host")?, m.level(view)) {
        (Some(h), _) => Some(h),
        (None, Some(_)) => Some(m.spot_host(view, at).ok_or_else(|| {
            let place = format!("[{}, {}] on {}", at.0, at.1, level_name(m, view));
            bad(c, format!("nothing to read at {place}; place it on a floor, a roof, a stair, a wall or a column"))
        })?),
        (None, None) => None,
    };
    let sp = Spot { view, at, host };
    let elevation = m.spot_value(&sp).map(hundredth);
    let id = s.model_mut().insert(Element::Spot(sp));
    Ok(json!({"id": id, "elevation": elevation, "host": host}))
}

/// The top of something standing on `level`: up to the next level, or `height` mm tall, or up to
/// `top_level`; and that height.
fn top_param(c: &str, m: &Model, p: &Value, level: Id) -> Result<(Option<Id>, f64), String> {
    let height = num(c, p, "height")?;
    let top_level = match p.get("top_level") {
        v if absent(v) => height.is_none().then(|| m.level_above(level)).flatten(),
        _ => Some(level_param(c, m, p, "top_level")?),
    };
    Ok((top_level, height.unwrap_or(DEFAULT_HEIGHT)))
}

/// Degrees to a billionth, so a quarter turn reads 90.
fn degrees(a: f64) -> f64 {
    (a * 1e9).round() / 1e9
}

fn column(s: &mut Session, p: &Value) -> R {
    let c = "column";
    let m = &s.model;
    let at = point_req(c, p, "at")?;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "column")?;
    let (top_level, height) = top_param(c, m, p, level)?;
    let col = Column {
        type_name,
        level,
        at,
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        top_level,
        height,
        base_offset: num(c, p, "base_offset")?.unwrap_or(0.0),
    };
    let id = s.model_mut().insert(Element::Column(col));
    Ok(json!({"id": id}))
}

fn stair(s: &mut Session, p: &Value) -> R {
    let c = "stair";
    let m = &s.model;
    let start = point_req(c, p, "start")?;
    let angle = match (point_opt(c, p, "end")?, num(c, p, "angle")?) {
        (Some(end), _) if end.dist(start) > TOL => degrees((end.1 - start.1).atan2(end.0 - start.0).to_degrees()),
        (Some(_), _) => return Err(bad(c, "`end` must be away from `start`: it says which way the stair climbs")),
        (None, a) => a.unwrap_or(0.0),
    };
    let level = level_param(c, m, p, "level")?;
    let (top_level, height) = top_param(c, m, p, level)?;
    let dim = |key: &str, default: f64| num(c, p, key).map(|v| v.unwrap_or(default));
    let st = Stair {
        level,
        top_level,
        height,
        start,
        angle,
        width: dim("width", STAIR_WIDTH)?,
        riser: dim("riser", RISER)?,
        tread: dim("tread", TREAD)?,
    };
    let (n, _, length) = m.flight(&st);
    let id = s.model_mut().insert(Element::Stair(st));
    Ok(json!({"id": id, "risers": n, "length": length}))
}

fn roof(s: &mut Session, p: &Value) -> R {
    let c = "roof";
    let m = &s.model;
    let levels = m.levels();
    let level = if absent(p.get("level")) {
        levels.get(1).or(levels.first()).map(|(id, _)| *id).ok_or_else(|| bad(c, "the model has no levels"))?
    } else {
        level_param(c, m, p, "level")?
    };
    let type_name = type_param(c, p, m, "roof")?;
    let given = !absent(p.get("boundary"));
    let overhang = num(c, p, "overhang")?.unwrap_or(if given { 0.0 } else { 500.0 });
    let mut boundary = if given { points(c, p, "boundary")? } else { roof_footprint(m, level, overhang).map_err(|e| bad(c, e))? };
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    let r = Roof { type_name, level, boundary, slope: num(c, p, "slope")?.unwrap_or(30.0), overhang, gable: flag(c, p, "gable")? };
    let id = s.model_mut().insert(Element::Roof(r));
    Ok(json!({"id": id}))
}

/// The footprint `roof` gives a roof on `level` by default: the outline of the walls of the level
/// below, `overhang` mm past their outer faces, counter-clockwise from its south-west corner.
pub fn roof_footprint(m: &Model, level: Id, overhang: f64) -> Result<Vec<P2>, String> {
    let below = m.levels().into_iter().rev().find(|(_, l)| l.elevation < m.elevation(level) - TOL).map(|(id, _)| id);
    let below = below.ok_or_else(|| format!("{} has no level below to roof over; give `boundary`", level_name(m, level)))?;
    let ring = outline(m, below).ok_or_else(|| format!("no walls enclose an area on {}; give `boundary`", level_name(m, below)))?;
    // ponytail: the thickest wall on the level sets where the faces are; per edge if outlines mix wall types.
    let face = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) if m.cut_by(w, below) => Some(m.thickness(w) / 2.0),
            _ => None,
        })
        .fold(0.0, f64::max);
    let mut ring: Vec<P2> = geom::corners(&geom::grow(&ring, face + overhang)).into_iter().map(micron).collect();
    let south_west =
        ring.iter().enumerate().min_by(|(_, a), (_, b)| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap_or(std::cmp::Ordering::Equal));
    let first = south_west.map_or(0, |(i, _)| i);
    ring.rotate_left(first);
    Ok(ring)
}

fn section(s: &mut Session, p: &Value) -> R {
    let c = "section";
    let (start, end, depth) = (point_req(c, p, "start")?, point_req(c, p, "end")?, num(c, p, "depth")?);
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => s.model.free_view_name(s.model.language.word("Section"), true),
    };
    let id = s.model_mut().insert(Element::Section(Section { name: name.clone(), start, end, depth, crop: None }));
    Ok(json!({"id": id, "name": name}))
}

fn elevation(s: &mut Session, p: &Value) -> R {
    let c = "elevation";
    let m = &s.model;
    let (depth, name, at) = (num(c, p, "depth")?, string(c, p, "name")?, point_opt(c, p, "at")?);
    let facing = match string(c, p, "facing")? {
        Some(f) => Some(Facing::parse(&f).ok_or_else(|| bad(c, format!("`facing` is north, south, east or west, not `{f}`")))?),
        None => None,
    };
    let nothing = || bad(c, "there are no walls or floors to look at; draw some, or give `start` and `end`");
    let lines: Vec<(String, P2, P2)> = match (point_opt(c, p, "start")?, point_opt(c, p, "end")?) {
        (Some(a), Some(b)) => vec![(name.unwrap_or_else(|| m.free_view_name(m.language.word("Elevation"), true)), a, b)],
        (None, None) => {
            let sides = match (facing, at) {
                (Some(f), _) => vec![f],
                (None, Some(q)) => vec![m.facing_from(q).ok_or_else(nothing)?],
                (None, None) => {
                    let there =
                        |f: &Facing| m.elements.values().any(|e| matches!(e, Element::Elevation(v) if v.name == m.language.word(f.name())));
                    let missing: Vec<Facing> = Facing::ALL.into_iter().filter(|f| !there(f)).collect();
                    if missing.is_empty() {
                        return Err(bad(c, "the four building elevations are there already"));
                    }
                    missing
                }
            };
            let mut lines = Vec::new();
            for f in sides {
                let (a, b) = m.elevation_line(f, at).ok_or_else(nothing)?;
                lines.push((name.clone().unwrap_or_else(|| m.free_view_name(m.language.word(f.name()), false)), a, b));
            }
            lines
        }
        _ => return Err(bad(c, "give both `start` and `end`, or neither")),
    };
    let m = s.model_mut();
    let made: Vec<(Id, String)> = lines
        .into_iter()
        .map(|(name, start, end)| (m.insert(Element::Elevation(Section { name: name.clone(), start, end, depth, crop: None })), name))
        .collect();
    match made.as_slice() {
        [(id, name)] => Ok(json!({"id": id, "name": name})),
        _ => Ok(json!({"ids": made.iter().map(|(id, _)| *id).collect::<Vec<_>>()})),
    }
}

fn crop(s: &mut Session, p: &Value) -> R {
    let c = "crop";
    let m = &s.model;
    let view = view_param(c, m, p, true)?;
    // Some(corners), or Some(None) to uncrop; none: to what the view shows now.
    let region = match p.get("region") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => match points(c, p, "region")?.as_slice() {
            [a, b] => Some(Some([*a, *b])),
            _ => return Err(bad(c, "`region` is two corners, [[x, y], [x, y]]")),
        },
    };
    match m.elements.get(&view) {
        Some(Element::Level(l)) => {
            let crop = match region {
                Some(r) => r,
                None => {
                    let pts = render::plan_shows(m, view);
                    let first = *pts.first().ok_or_else(|| bad(c, format!("{} shows nothing to crop to", l.name)))?;
                    let (lo, hi) =
                        pts.iter().fold((first, first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1))));
                    Some([micron(lo - P2(1000.0, 1000.0)), micron(hi + P2(1000.0, 1000.0))])
                }
            };
            if let Some(Element::Level(l)) = s.model_mut().elements.get_mut(&view) {
                l.crop = crop;
            }
            Ok(json!({"view": view, "crop": crop}))
        }
        Some(Element::Section(v) | Element::Elevation(v)) => {
            let mut v = v.clone();
            // How far along the line its start moves.
            let mut shift = 0.0;
            match region {
                Some(Some([a, b])) => {
                    if !(a.0 < b.0 - TOL && a.1 < b.1 - TOL) {
                        return Err(bad(
                            c,
                            "a crop region runs from its bottom left corner to its top right one (in a plan, south-west to north-east)",
                        ));
                    }
                    // Its line runs between the region's sides.
                    let u = (v.end - v.start).unit();
                    (v.start, v.end, v.crop, shift) = (micron(v.start + u * a.0), micron(v.start + u * b.0), Some([a.1, b.1]), a.0);
                }
                Some(None) => v.crop = None,
                None => v.crop = render::crop_region(m, view).map(|[lo, hi]| [lo.1, hi.1]),
            }
            let crop = v.crop;
            let m = s.model_mut();
            if let Some(Element::Section(old) | Element::Elevation(old)) = m.elements.get_mut(&view) {
                *old = v;
            }
            // What's noted in the view keeps its spot over the model.
            for e in m.elements.values_mut() {
                match e {
                    Element::Text(n) if n.view == view => {
                        n.at.0 -= shift;
                        if let Some(l) = &mut n.leader {
                            l.0 -= shift;
                        }
                    }
                    Element::Spot(sp) if sp.view == view => sp.at.0 -= shift,
                    _ => {}
                }
            }
            Ok(json!({"view": view, "crop": crop}))
        }
        _ => Err(bad(c, format!("{} isn't a plan, a section or an elevation; only those crop", view_title(m, view)))),
    }
}

/// What to call element `id` in a message: a view's or a sheet's name, else its category and id.
fn view_title(m: &Model, id: Id) -> String {
    match (m.view_name(id), m.sheet(id)) {
        (Some(n), _) => n.to_string(),
        (_, Some(sh)) => format!("sheet {}", sh.number),
        _ => format!("{} {id}", m.elements.get(&id).map_or("element", Element::category)),
    }
}

fn new_sheet(s: &mut Session, p: &Value) -> R {
    let c = "sheet";
    let m = &s.model;
    let number = match string(c, p, "number")? {
        Some(n) => n,
        None => {
            let taken: BTreeSet<&str> = m
                .elements
                .values()
                .filter_map(|e| match e {
                    Element::Sheet(sh) => Some(sh.number.as_str()),
                    _ => None,
                })
                .collect();
            (101..).map(|n| format!("A{n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
        }
    };
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word("Unnamed").into());
    let size = string(c, p, "size")?.map_or_else(|| "A3".into(), |z| z.trim().to_ascii_uppercase());
    let id = s.model_mut().insert(Element::Sheet(Sheet { number: number.clone(), name, size }));
    Ok(json!({"id": id, "number": number}))
}

fn place(s: &mut Session, p: &Value) -> R {
    let c = "place";
    let m = &s.model;
    let sheet = match p.get("sheet") {
        Some(Value::String(n)) => m
            .elements
            .iter()
            .find_map(|(id, e)| matches!(e, Element::Sheet(sh) if sh.number == *n || sh.name == *n).then_some(*id))
            .ok_or_else(|| bad(c, format!("no sheet numbered `{n}`")))?,
        Some(v) => v.as_u64().filter(|id| m.sheet(*id).is_some()).ok_or_else(|| bad(c, format!("`sheet`: {v} is not a sheet")))?,
        None => return Err(bad(c, "`sheet`, a sheet's number or id, is required")),
    };
    let scale = num(c, p, "scale")?.unwrap_or(100.0);
    let at = point_opt(c, p, "at")?;
    let view = match p.get("view") {
        Some(Value::String(n)) => match m.elements.keys().find(|id| m.view_name(**id) == Some(n.as_str())) {
            Some(id) => *id,
            // A schedule, or the 3D view, comes to be as it's first placed: a schedule by its name in
            // English or in the project's language, named in the latter.
            None => {
                let names: Vec<&'static str> = schedule::VIEWS.iter().map(|(_, name)| *name).collect();
                let builtin = m.language.find(n, &names).and_then(|name| schedule::VIEWS.iter().find(|(_, v)| *v == name));
                let existing = builtin.and_then(|(of, _)| {
                    m.elements.iter().find_map(|(id, e)| matches!(e, Element::Schedule(sc) if sc.of == *of).then_some(*id))
                });
                match (builtin, existing) {
                    (_, Some(id)) => id,
                    (Some((of, name)), None) => {
                        let name = m.language.word(name).to_string();
                        s.model_mut().insert(Element::Schedule(Schedule { name, of: of.to_string() }))
                    }
                    (None, _) if n == "{3D}" => s.model_mut().insert(Element::View3D(View3D {
                        name: n.clone(),
                        azimuth: num(c, p, "azimuth")?.unwrap_or(view3d::AZIMUTH),
                        elevation: num(c, p, "elevation")?.unwrap_or(view3d::ELEVATION),
                    })),
                    (None, _) => return Err(bad(c, format!("no level, section, elevation, schedule or 3D view named `{n}`"))),
                }
            }
        },
        Some(v) => v
            .as_u64()
            .filter(|id| m.view_name(*id).is_some())
            .ok_or_else(|| bad(c, format!("`view`: {v} is not a plan, a section, an elevation, a schedule or a 3D view")))?,
        None => return Err(bad(c, "`view`, a level's, section's, elevation's, schedule's or 3D view's name or id, is required")),
    };
    let at = at.unwrap_or_else(|| sheet::spot(&s.model, sheet, view, scale));
    let id = s.model_mut().insert(Element::Viewport(Viewport { sheet, view, at, scale }));
    Ok(json!({"id": id, "at": at}))
}

fn edit_type(s: &mut Session, p: &Value) -> R {
    let c = "type";
    let cat = category(&string(c, p, "category")?.ok_or_else(|| bad(c, "`category` is required: wall, floor, door or window"))?);
    let name = string(c, p, "name")?.filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    let (thickness, width, height, sill) = (num(c, p, "thickness")?, num(c, p, "width")?, num(c, p, "height")?, num(c, p, "sill")?);
    let need = |what: &str| bad(c, format!("a new {cat} type needs `{what}`"));
    let m = s.model_mut();
    let t = match cat.as_str() {
        "wall" => {
            let t = WallType { thickness: thickness.or(m.types.wall.get(&name).map(|t| t.thickness)).ok_or_else(|| need("thickness"))? };
            m.types.wall.insert(name.clone(), t.clone());
            json!(t)
        }
        "floor" => {
            let t = FloorType { thickness: thickness.or(m.types.floor.get(&name).map(|t| t.thickness)).ok_or_else(|| need("thickness"))? };
            m.types.floor.insert(name.clone(), t.clone());
            json!(t)
        }
        "door" | "window" => {
            let map = if cat == "door" { &mut m.types.door } else { &mut m.types.window };
            let old = map.get(&name).cloned();
            let t = OpeningType {
                width: width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?,
                height: height.or(old.as_ref().map(|t| t.height)).ok_or_else(|| need("height"))?,
                sill: sill.or(old.as_ref().map(|t| t.sill)).unwrap_or(if cat == "window" { 900.0 } else { 0.0 }),
            };
            map.insert(name.clone(), t.clone());
            json!(t)
        }
        "column" => {
            let old = m.types.column.get(&name).cloned();
            let width = width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?;
            let round = match p.get("round") {
                v if absent(v) => old.as_ref().is_some_and(|t| t.round),
                _ => flag(c, p, "round")?,
            };
            let depth = if round { width } else { num(c, p, "depth")?.or(old.as_ref().map(|t| t.depth)).unwrap_or(width) };
            let t = ColumnType { width, depth, round };
            m.types.column.insert(name.clone(), t.clone());
            json!(t)
        }
        "roof" => {
            let t = RoofType { thickness: thickness.or(m.types.roof.get(&name).map(|t| t.thickness)).ok_or_else(|| need("thickness"))? };
            m.types.roof.insert(name.clone(), t.clone());
            json!(t)
        }
        other => return Err(bad(c, format!("`{other}` has no types; use wall, floor, door, window, column or roof"))),
    };
    Ok(json!({"category": cat, "name": name, "type": t}))
}

fn modify(s: &mut Session, p: &Value) -> R {
    let c = "modify";
    let fields = p.as_object().ok_or_else(|| bad(c, "params must be {id | ids, …fields}"))?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let mut edited = Vec::new();
    for &id in &ids {
        let e = m.elements.get(&id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        let mut v = serde_json::to_value(e).map_err(|e| bad(c, e))?;
        let obj = v.as_object_mut().ok_or_else(|| bad(c, format!("element {id} can't be edited")))?;
        for (k, val) in fields {
            let val = match k.as_str() {
                "id" | "ids" => continue,
                "category" if obj.get("category") == Some(val) => continue,
                "category" => return Err(bad(c, "`category` can't change; delete the element and create another")),
                "level" | "top_level" if val.is_string() => json!(level_param(c, m, p, k)?),
                _ => val.clone(),
            };
            match obj.get_mut(k) {
                // Into a field that is an object (a plan's view range), the new one merges.
                Some(old) if old.is_object() && val.is_object() => merge(old, &val),
                _ => {
                    obj.insert(k.clone(), val);
                }
            }
        }
        edited.push((id, serde_json::from_value::<Element>(v).map_err(|e| bad(c, format!("element {id}: {e}")))?));
    }
    let m = s.model_mut();
    for (id, e) in edited {
        m.elements.insert(id, e);
    }
    match (p.get("ids"), ids.as_slice()) {
        (None, [one]) => Ok(element_json(&s.model, *one)),
        _ => Ok(json!({"modified": ids})),
    }
}

/// `patch` merged into `into`, as JSON Merge Patch (RFC 7386) does: objects merge key by key, a
/// null removes its key, anything else replaces what was there.
fn merge(into: &mut Value, patch: &Value) {
    match (into, patch) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                if v.is_null() {
                    a.remove(k);
                } else {
                    merge(a.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (into, patch) => *into = patch.clone(),
    }
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
pub(crate) fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// How a command takes elements: through `map`, a translation, a turn or a mirror. A translation
/// slides a door or window on its own along its wall by `slide`; a mirror flips the way doors and
/// windows face, as a mirror image does.
struct Motion<'a> {
    map: &'a dyn Fn(P2) -> P2,
    slide: Option<P2>,
    mirror: bool,
}

/// The element taken by `mo`. Levels have no place in plan; a door or window on its own goes
/// only along its wall, by a translation, else only with its wall.
fn transformed(c: &str, m: &Model, id: Id, mo: &Motion, with: &BTreeSet<Id>) -> Result<Element, String> {
    let f = mo.map;
    let mut e = m.elements.get(&id).cloned().ok_or_else(|| bad(c, format!("no element {id}")))?;
    let category = e.category();
    match &mut e {
        Element::Level(_) => return Err(bad(c, format!("level {id} has no place in plan; change its elevation, or add a level"))),
        Element::Sheet(_) => return Err(bad(c, format!("sheet {id} has no place in plan; move the views on it"))),
        Element::Schedule(_) | Element::View3D(_) => {
            return Err(bad(c, format!("{category} {id} has no place in plan; move its viewport on its sheet")));
        }
        Element::Text(n) => {
            let (at, angle) = turned(f, n.at, n.angle);
            // A mirrored note still reads the right way round.
            (n.at, n.angle) = (at, if mo.mirror { n.angle } else { angle });
            n.leader = n.leader.map(f);
        }
        Element::Spot(sp) => sp.at = f(sp.at),
        Element::Viewport(v) => v.at = f(v.at),
        Element::Column(col) => (col.at, col.angle) = turned(f, col.at, col.angle),
        Element::Stair(st) => (st.start, st.angle) = turned(f, st.start, st.angle),
        Element::Roof(r) => {
            r.boundary.iter_mut().for_each(|q| *q = f(*q));
            if mo.mirror {
                r.boundary.reverse();
            }
        }
        Element::Wall(w) => (w.start, w.end) = (f(w.start), f(w.end)),
        Element::Grid(g) => (g.start, g.end) = (f(g.start), f(g.end)),
        Element::Floor(fl) => {
            fl.boundary.iter_mut().for_each(|q| *q = f(*q));
            // A mirror turns the outline clockwise; floors keep theirs counter-clockwise.
            if mo.mirror {
                fl.boundary.reverse();
            }
        }
        Element::Room(r) => r.point = f(r.point),
        Element::Door(o) | Element::Window(o) if !with.contains(&o.host) => match (mo.slide, m.wall(o.host)) {
            (Some(by), Some(w)) => o.offset += by.dot((w.end - w.start).unit()),
            _ => return Err(bad(c, format!("{category} {id} turns and mirrors only with its wall; select the wall too"))),
        },
        Element::Door(_) | Element::Window(_) => {}
        Element::Dimension(d) => d.at = f(d.at),
        // A tag's place hangs on its door or window's: see `relocate` and `duplicate`.
        Element::Tag(_) => {}
        Element::Section(v) | Element::Elevation(v) => {
            (v.start, v.end) = (f(v.start), f(v.end));
            // A mirror image of it looks the mirrored way round: to the left of its line again.
            if mo.mirror {
                std::mem::swap(&mut v.start, &mut v.end);
            }
        }
    }
    Ok(e)
}

/// Where `f` takes a point and the way `angle` degrees points from it.
fn turned(f: &dyn Fn(P2) -> P2, at: P2, angle: f64) -> (P2, f64) {
    let (a, b) = (f(at), f(at + heading(angle) * 1000.0));
    (a, degrees((b.1 - a.1).atan2(b.0 - a.0).to_degrees()))
}

/// Where `mo` takes the heads of the tags among `ids`.
fn tag_heads(m: &Model, ids: &[Id], mo: &Motion) -> Vec<(Id, P2)> {
    ids.iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Tag(t)) => Some((*id, (mo.map)(m.anchor(t.host)? + t.offset))),
            _ => None,
        })
        .collect()
}

/// Tag `id`, its head put at `head` whatever its door or window did.
fn place_tag(m: &mut Model, id: Id, head: P2) {
    let Some(Element::Tag(t)) = m.elements.get(&id) else { return };
    let Some(a) = m.anchor(t.host) else { return };
    if let Some(Element::Tag(t)) = m.elements.get_mut(&id) {
        t.offset = micron(head - a);
    }
}

/// Whether two walls overlap in height, so they can join (see [`Model::join_reach`]).
fn stacked(m: &Model, a: &Wall, b: &Wall) -> bool {
    m.base_z(a) < m.top_z(b) && m.base_z(b) < m.top_z(a)
}

/// Take elements by `mo`, keeping walls joined: walls joined to a taken wall stretch to stay
/// joined, and doors and windows in walls that changed keep their place or go with their wall.
/// With `end` (`Some(true)` the start, `Some(false)` the end), only that end of one wall goes.
fn relocate(c: &str, s: &mut Session, ids: &[Id], mo: &Motion, end: Option<bool>) -> Result<(), String> {
    let m = &s.model;
    let f = mo.map;
    let moving: BTreeSet<Id> = ids.iter().copied().collect();
    let heads = tag_heads(m, ids, mo);
    // Doors and windows of walls taken whole ride along.
    let carried = |host: &Id| moving.contains(host) && end.is_none();
    let mut walls: BTreeMap<Id, Wall> = BTreeMap::new();
    let mut others: Vec<(Id, Element)> = Vec::new();
    for &id in ids {
        match transformed(c, m, id, mo, &moving)? {
            Element::Wall(mut w) => {
                match (end, m.wall(id)) {
                    (Some(true), Some(old)) => w.end = old.end,
                    (Some(false), Some(old)) => w.start = old.start,
                    _ => {}
                }
                walls.insert(id, w);
            }
            Element::Door(o) | Element::Window(o) if moving.contains(&o.host) => {}
            e => others.push((id, e)),
        }
    }
    let old = |id: &Id| m.wall(*id);
    // A taken wall's end that was on a wall staying put slides back onto it, along the taken wall.
    // (A wall's end taken on its own goes where it's told.)
    for (id, w) in walls.iter_mut().filter(|_| end.is_none()) {
        let Some(before) = old(id) else { continue };
        let line = (w.start, w.end);
        for (end, was) in [(&mut w.start, before.start), (&mut w.end, before.end)] {
            let host = m.elements.iter().find_map(|(other, e)| match e {
                Element::Wall(o)
                    if !moving.contains(other) && stacked(m, o, before) && geom::dist_to_segment(was, o.start, o.end) <= TOL =>
                {
                    Some((o.start, o.end))
                }
                _ => None,
            });
            if let Some((a, b)) = host {
                *end = geom::line_intersection(line.0, line.1, a, b).map_or(*end, micron);
            }
        }
    }
    // Walls staying put stretch to keep their joins with the taken ones: an end on a taken wall's
    // end goes with it; an end on a taken wall's span slides along its own wall onto it.
    let moved: Vec<(Wall, Wall)> = walls.iter().filter_map(|(id, w)| Some((old(id)?.clone(), w.clone()))).collect();
    for (&id, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        if moving.contains(&id) {
            continue;
        }
        let mut n = w.clone();
        for (end, at) in [(&mut n.start, w.start), (&mut n.end, w.end)] {
            for (before, after) in moved.iter().filter(|(before, _)| stacked(m, w, before)) {
                if at.dist(before.start) <= TOL || at.dist(before.end) <= TOL {
                    *end = if at.dist(before.start) <= TOL { after.start } else { after.end };
                    break;
                }
                if geom::dist_to_segment(at, before.start, before.end) <= TOL {
                    *end = geom::line_intersection(w.start, w.end, after.start, after.end).map_or_else(|| f(at), micron);
                    break;
                }
            }
        }
        if n != *w {
            walls.insert(id, n);
        }
    }
    // Doors and windows in walls that changed keep their place, or go with their wall.
    for (&id, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let (Some(before), Some(after)) = (old(&o.host), walls.get(&o.host)) else { continue };
        let along = carried(&o.host) || moving.contains(&id);
        let spot = before.start + (before.end - before.start).unit() * o.offset;
        let at = if along { f(spot) } else { spot };
        let mut o = o.clone();
        o.offset = (at - after.start).dot((after.end - after.start).unit());
        if mo.mirror && carried(&o.host) {
            o.flip_facing = !o.flip_facing;
        }
        others.retain(|(other, _)| *other != id);
        others.push((id, if matches!(e, Element::Door(_)) { Element::Door(o) } else { Element::Window(o) }));
    }
    let m = s.model_mut();
    for (id, w) in walls {
        m.elements.insert(id, Element::Wall(w));
    }
    for (id, e) in others {
        m.elements.insert(id, e);
    }
    // Dimensions keep to the same faces of mirrored walls, whose left and right a mirror swaps.
    if mo.mirror && end.is_none() {
        for e in m.elements.values_mut() {
            if let Element::Dimension(d) = e {
                d.refs.iter_mut().filter(|r| moving.contains(&r.id)).for_each(|r| r.face = r.face.mirrored());
            }
        }
    }
    for (id, head) in heads {
        place_tag(m, id, head);
    }
    Ok(())
}

/// Copies of elements taken by `mo`, with the doors and windows of copied walls; the copies' ids.
/// Copied dimensions and tags go onto the copies of what they measure and name, where there are.
fn duplicate(c: &str, s: &mut Session, ids: &[Id], mo: &Motion) -> R {
    let m = &s.model;
    let copying: BTreeSet<Id> = ids.iter().copied().collect();
    let heads: BTreeMap<Id, P2> = tag_heads(m, ids, mo).into_iter().collect();
    let mut copies = Vec::new();
    // Dimensions and tags, copied last: they may refer to copies.
    let mut notes = Vec::new();
    for &id in ids {
        match transformed(c, m, id, mo, &copying)? {
            // A copied wall brings its doors and windows; see below.
            Element::Door(o) | Element::Window(o) if copying.contains(&o.host) => {}
            e @ (Element::Dimension(_) | Element::Tag(_) | Element::Spot(_)) => notes.push((id, e)),
            e => copies.push((id, e)),
        }
    }
    let hosted: Vec<(Id, Element)> = m
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Door(o) | Element::Window(o) if copying.contains(&o.host)))
        .map(|(&id, e)| {
            let mut e = e.clone();
            if let Element::Door(o) | Element::Window(o) = &mut e
                && mo.mirror
            {
                o.flip_facing = !o.flip_facing;
            }
            (id, e)
        })
        .collect();
    let m = s.model_mut();
    let mut made = Vec::new();
    // Each copied element's copy.
    let mut hosts = BTreeMap::new();
    for (from, mut e) in copies.into_iter().chain(hosted) {
        let door = matches!(e, Element::Door(_));
        match &mut e {
            Element::Grid(g) => g.name = next_grid_name(m),
            Element::Section(v) => v.name = m.free_view_name(m.language.word("Section"), true),
            Element::Elevation(v) => v.name = m.free_view_name(m.language.word("Elevation"), true),
            Element::Room(r) => {
                r.number = next_number(m.elements.values().filter_map(|e| match e {
                    Element::Room(r) => Some(r.number.as_str()),
                    _ => None,
                }))
            }
            Element::Door(o) | Element::Window(o) => {
                o.host = hosts.get(&o.host).copied().unwrap_or(o.host);
                o.mark = next_number(m.elements.values().filter_map(|e| match e {
                    Element::Door(o) if door => Some(o.mark.as_str()),
                    Element::Window(o) if !door => Some(o.mark.as_str()),
                    _ => None,
                }));
            }
            _ => {}
        }
        let id = m.insert(e);
        hosts.insert(from, id);
        made.push(id);
    }
    for (from, mut e) in notes {
        match &mut e {
            Element::Dimension(d) => {
                for r in &mut d.refs {
                    if let Some(&to) = hosts.get(&r.id) {
                        r.id = to;
                        if mo.mirror {
                            r.face = r.face.mirrored();
                        }
                    }
                }
            }
            Element::Tag(t) => t.host = hosts.get(&t.host).copied().unwrap_or(t.host),
            Element::Spot(sp) => sp.host = sp.host.map(|h| hosts.get(&h).copied().unwrap_or(h)),
            _ => {}
        }
        let id = m.insert(e);
        if let Some(&head) = heads.get(&from) {
            place_tag(m, id, head);
        }
        made.push(id);
    }
    Ok(json!({"ids": made}))
}

fn move_elements(s: &mut Session, p: &Value) -> R {
    let c = "move";
    let ids = ids_param(c, &s.model, p)?;
    let by = point_req(c, p, "by")?;
    // Moving one end: `Some(true)` the start, `Some(false)` the end.
    let end = match string(c, p, "end")?.as_deref() {
        None => None,
        Some("start") => Some(true),
        Some("end") => Some(false),
        Some(other) => return Err(bad(c, format!("`end` is `start` or `end`, not `{other}`"))),
    };
    if end.is_some() && !matches!(ids.as_slice(), [one] if s.model.wall(*one).is_some()) {
        return Err(bad(c, "`end` moves an end of one wall; give `ids` one wall"));
    }
    relocate(c, s, &ids, &Motion { map: &|q| q + by, slide: Some(by), mirror: false }, end)?;
    Ok(json!({"moved": ids}))
}

fn copy(s: &mut Session, p: &Value) -> R {
    let c = "copy";
    let ids = ids_param(c, &s.model, p)?;
    let by = point_req(c, p, "by")?;
    duplicate(c, s, &ids, &Motion { map: &|q| q + by, slide: Some(by), mirror: false })
}

fn rotate(s: &mut Session, p: &Value) -> R {
    let c = "rotate";
    let ids = ids_param(c, &s.model, p)?;
    let about = point_req(c, p, "about")?;
    let angle = num(c, p, "angle")?.ok_or_else(|| bad(c, "`angle` in degrees, counter-clockwise, is required"))?;
    let (sin, cos) = angle.to_radians().sin_cos();
    let turn = |q: P2| {
        let d = q - about;
        micron(about + P2(d.0 * cos - d.1 * sin, d.0 * sin + d.1 * cos))
    };
    let mo = Motion { map: &turn, slide: None, mirror: false };
    if flag(c, p, "copy")? {
        return duplicate(c, s, &ids, &mo);
    }
    relocate(c, s, &ids, &mo, None)?;
    Ok(json!({"rotated": ids}))
}

fn mirror(s: &mut Session, p: &Value) -> R {
    let c = "mirror";
    let ids = ids_param(c, &s.model, p)?;
    let axis = points(c, p, "axis")?;
    let (a, b) = match axis.as_slice() {
        [a, b] if a.dist(*b) > TOL => (*a, *b),
        _ => return Err(bad(c, "`axis` is two different points [[x, y], [x, y]]")),
    };
    let u = (b - a).unit();
    let reflect = |q: P2| {
        let d = q - a;
        micron(a + u * (2.0 * d.dot(u)) - d)
    };
    let mo = Motion { map: &reflect, slide: None, mirror: true };
    if flag(c, p, "copy")? {
        return duplicate(c, s, &ids, &mo);
    }
    relocate(c, s, &ids, &mo, None)?;
    Ok(json!({"mirrored": ids}))
}

fn corner(s: &mut Session, p: &Value) -> R {
    let c = "corner";
    let ids = ids_param(c, &s.model, p)?;
    let m = &s.model;
    let [a, b] = ids.as_slice() else { return Err(bad(c, "`ids` takes two walls")) };
    let (Some(wa), Some(wb)) = (m.wall(*a), m.wall(*b)) else { return Err(bad(c, "`ids` takes two walls")) };
    let keep = if absent(p.get("keep")) { Vec::new() } else { points(c, p, "keep")? };
    if !keep.is_empty() && keep.len() != 2 {
        return Err(bad(c, "`keep` is a point on each wall, [[x, y], [x, y]]"));
    }
    let hit = micron(
        geom::line_intersection(wa.start, wa.end, wb.start, wb.end).ok_or_else(|| bad(c, "the walls are parallel: they never meet"))?,
    );
    for (i, &id) in [*a, *b].iter().enumerate() {
        let Some(w) = s.model.wall(id).cloned() else { continue };
        let u = (w.end - w.start).unit();
        let at = |q: P2| (q - w.start).dot(u);
        // The end that goes to the corner: away from the part kept, else the one nearer to it.
        let start = match keep.get(i) {
            Some(k) => at(*k) > at(hit),
            None => hit.dist(w.start) < hit.dist(w.end),
        };
        let by = hit - if start { w.start } else { w.end };
        if by.length() > 1e-6 {
            relocate(c, s, &[id], &Motion { map: &|q| q + by, slide: Some(by), mirror: false }, Some(start))?;
        }
    }
    Ok(json!({"corner": [hit.0, hit.1]}))
}

fn split(s: &mut Session, p: &Value) -> R {
    let c = "split";
    let m = &s.model;
    let id = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id`, a wall, is required"))?;
    let w = m.wall(id).cloned().ok_or_else(|| bad(c, format!("{id} is not a wall")))?;
    let at = point_req(c, p, "at")?;
    if geom::dist_to_segment(at, w.start, w.end) > m.thickness(&w) / 2.0 + 100.0 {
        return Err(bad(c, format!("[{}, {}] isn't on wall {id}", at.0, at.1)));
    }
    let len = w.start.dist(w.end);
    let along = (geom::param_on_segment(at, w.start, w.end) * len * 1e3).round() / 1e3;
    if along <= TOL || along >= len - TOL {
        return Err(bad(c, "split between the wall's ends"));
    }
    let cut = micron(w.start + (w.end - w.start).unit() * along);
    // Doors and windows go with the part they're in; one across the cut is in the way.
    let mut later = Vec::new();
    for (&oid, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        if o.host != id {
            continue;
        }
        let half = m.opening_type(e).map_or(0.0, |t| t.width / 2.0);
        if o.offset + half <= along + TOL {
            continue;
        }
        if o.offset - half < along - TOL {
            return Err(bad(c, format!("{} {oid} is in the way; split elsewhere", e.category())));
        }
        later.push(oid);
    }
    let (mut first, mut second) = (w.clone(), w);
    (first.end, second.start) = (cut, cut);
    let m = s.model_mut();
    m.elements.insert(id, Element::Wall(first));
    let new = m.insert(Element::Wall(second));
    for oid in later {
        if let Some(Element::Door(o) | Element::Window(o)) = m.elements.get_mut(&oid) {
            o.host = new;
            o.offset -= along;
        }
    }
    Ok(json!({"ids": [id, new]}))
}

fn delete(s: &mut Session, p: &Value) -> R {
    let c = "delete";
    let m = &s.model;
    let mut gone: BTreeSet<Id> = ids_param(c, m, p)?.into_iter().collect();
    // What hangs on what goes, goes: a wall's doors and windows, and their tags.
    loop {
        let hosted: Vec<Id> = m
            .elements
            .iter()
            .filter(|(id, e)| {
                !gone.contains(id)
                    && match e {
                        Element::Door(o) | Element::Window(o) => gone.contains(&o.host),
                        Element::Tag(t) => gone.contains(&t.host),
                        Element::Viewport(v) => gone.contains(&v.sheet) || gone.contains(&v.view),
                        // Notes and spot elevations go with their view, spots with what they read.
                        Element::Text(n) => gone.contains(&n.view),
                        Element::Spot(sp) => gone.contains(&sp.view) || sp.host.is_some_and(|h| gone.contains(&h)),
                        _ => false,
                    }
            })
            .map(|(id, _)| *id)
            .collect();
        if hosted.is_empty() {
            break;
        }
        gone.extend(hosted);
    }
    // A dimension loses the references that go; left with fewer than two, it goes too.
    let mut kept = Vec::new();
    for (&id, e) in &m.elements {
        if let Element::Dimension(d) = e
            && !gone.contains(&id)
            && d.refs.iter().any(|r| gone.contains(&r.id))
        {
            let mut d = d.clone();
            d.refs.retain(|r| !gone.contains(&r.id));
            if d.refs.len() < 2 {
                gone.insert(id);
            } else {
                kept.push((id, d));
            }
        }
    }
    let m = s.model_mut();
    for id in &gone {
        m.elements.remove(id);
    }
    for (id, d) in kept {
        m.elements.insert(id, Element::Dimension(d));
    }
    Ok(json!({"deleted": gone}))
}

// ---------- reading ----------

fn units(s: &mut Session, p: &Value) -> R {
    let c = "units";
    if let Some(name) = string(c, p, "length")? {
        let u = Units::from_name(&name).ok_or_else(|| bad(c, format!("`length` is mm, cm, m, in, ft or ft-in, not `{name}`")))?;
        if u != s.model.units {
            s.model_mut().units = u;
        }
    }
    Ok(json!({"length": s.model.units.name()}))
}

fn inspect(s: &mut Session, _: &Value) -> R {
    let m = &s.model;
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for e in m.elements.values() {
        *counts.entry(e.category()).or_default() += 1;
    }
    let levels: Vec<Value> = m.levels().iter().map(|(id, l)| json!({"id": id, "name": l.name, "elevation": l.elevation})).collect();
    let warnings: Vec<String> = m
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Room(r) if m.room_region(r).is_none() => {
                Some(format!("room {id} ({} {}) is not enclosed: the walls around it no longer close", r.number, r.name))
            }
            _ => None,
        })
        .collect();
    Ok(json!({
        "name": m.name, "units": "mm", "project_units": m.units.name(), "language": m.language.code(), "path": s.path, "levels": levels,
        "counts": counts,
        "types": m.types, "warnings": warnings, "history": s.history(),
    }))
}

fn elements(s: &mut Session, p: &Value) -> R {
    let c = "elements";
    let m = &s.model;
    let cat = string(c, p, "category")?.map(|k| category(&k));
    let level = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let (limit, offset) = (count_opt(c, p, "limit", 500)?, count_opt(c, p, "offset", 0)?);
    let hits: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| cat.as_deref().is_none_or(|k| e.category() == k))
        .filter(|(_, e)| level.is_none_or(|l| m.level_of(e) == Some(l)))
        .map(|(id, _)| *id)
        .collect();
    let page: Vec<Value> = hits.iter().skip(offset).take(limit).map(|id| element_json(m, *id)).collect();
    Ok(json!({"count": hits.len(), "returned": page.len(), "elements": page}))
}
