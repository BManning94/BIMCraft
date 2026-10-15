//! The building model: levels, grids, walls, floors, doors, windows, rooms, columns, stairs and
//! roofs, the types they use, the dimensions and tags annotating plans, the sections and
//! elevations looking at it, and the sheets its views are placed on. Lengths are millimetres, x east, y north, z up; each level
//! has an elevation. On a sheet, millimetres of paper from its bottom left corner.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2, Region};

pub type Id = u64;

/// A line in plan, through two points.
pub type Line = (P2, P2);

/// Height of a plan view's cut plane above its level.
pub const CUT_HEIGHT: f64 = 1200.0;
/// Wall ends closer than this join, in mm.
pub const TOL: f64 = 1.0;
/// Coordinates and lengths beyond 1000 km are refused.
pub const MAX_COORD: f64 = 1e9;
/// Height of a wall or room with no level above it.
pub const DEFAULT_HEIGHT: f64 = 3000.0;
/// A stair's tallest riser, its treads' depth and its width, unless told otherwise, in mm.
pub const RISER: f64 = 180.0;
pub const TREAD: f64 = 280.0;
pub const STAIR_WIDTH: f64 = 1000.0;
/// Names, marks and numbers longer than this are refused.
pub const MAX_TEXT: usize = 256;
/// Points one command or floor outline may carry.
pub const MAX_POINTS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub name: String,
    pub elevation: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    pub name: String,
    pub start: P2,
    pub end: P2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wall {
    #[serde(rename = "type")]
    pub type_name: String,
    /// Base level.
    pub level: Id,
    /// Ends of the location line, which is the wall's centre line.
    pub start: P2,
    pub end: P2,
    /// Top constraint: a level, or none for an unconnected wall `height` tall.
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    /// Outline, counter-clockwise. The slab's top is at the level.
    pub boundary: Vec<P2>,
}

/// A door or window in a host wall, centred `offset` mm along it from the wall's start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    #[serde(rename = "type")]
    pub type_name: String,
    pub host: Id,
    pub offset: f64,
    /// Height of its bottom above the wall's base (0 for doors).
    #[serde(default)]
    pub sill: f64,
    #[serde(default)]
    pub mark: String,
    /// Swing (doors) or face (windows) to the wall's other side.
    #[serde(default)]
    pub flip_facing: bool,
    /// Hinge on the other jamb.
    #[serde(default)]
    pub flip_hand: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub level: Id,
    /// A point inside the room. The boundary is found from the walls around it, so it follows
    /// them when they move.
    pub point: P2,
    pub name: String,
    pub number: String,
}

/// Which of an element's lines a dimension measures to: a wall's centre line, or its face on the
/// left or the right of its start-to-end direction. A grid has only its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    #[serde(alias = "center")]
    Centre,
    Left,
    Right,
}

impl Face {
    /// The same face of a mirrored wall: a mirror image swaps left and right.
    pub fn mirrored(self) -> Face {
        match self {
            Face::Centre => Face::Centre,
            Face::Left => Face::Right,
            Face::Right => Face::Left,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Face::Centre => "centre line",
            Face::Left => "left face",
            Face::Right => "right face",
        }
    }
}

/// A line a dimension measures to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: Id,
    #[serde(default)]
    pub face: Face,
}

/// An aligned dimension in a level's plan: the distances between parallel references, along a
/// line through `at` across them. It measures the model as it is, so it follows what it measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub level: Id,
    pub refs: Vec<Reference>,
    pub at: P2,
}

/// A door's or window's tag, showing its mark in the plan of its wall's level, `offset` mm from
/// its centre: it goes where its door or window goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    pub host: Id,
    pub offset: P2,
    /// A line from the tag to what it names.
    #[serde(default)]
    pub leader: bool,
}

/// A column standing on `level` at `at`, its width along `angle` degrees (counter-clockwise from
/// east): up to `top_level`, or `height` mm tall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    pub at: P2,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
}

/// A straight flight of stairs from `level` up to `top_level` (or `height` mm), climbing from the
/// middle of its first riser at `start` toward `angle` degrees (counter-clockwise from east): as
/// many risers as keep each no higher than `riser`, a tread `tread` deep between each two.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stair {
    pub level: Id,
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    pub start: P2,
    pub angle: f64,
    pub width: f64,
    pub riser: f64,
    pub tread: f64,
}

/// A roof by its footprint, on `level`: its eaves along `boundary`, reaching the level `overhang`
/// mm in from them. Flat at `slope` 0; else pitched at `slope` degrees over a rectangle, hipped,
/// or with `gable` its ends left upright and open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roof {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    pub boundary: Vec<P2>,
    #[serde(default)]
    pub slope: f64,
    #[serde(default)]
    pub overhang: f64,
    #[serde(default)]
    pub gable: bool,
}

/// Flat polygons in space, each a list of points `[x, y, z]` in mm.
pub type Faces = Vec<Vec<[f64; 3]>>;

/// The way `angle` degrees (counter-clockwise from east) points.
pub fn heading(angle: f64) -> P2 {
    let a = angle.to_radians();
    P2(a.cos(), a.sin())
}

/// A section or an elevation: a vertical view along the line from `start` to `end`, looking to
/// its left. A section cuts the model at its line; an elevation's line lies outside the building,
/// looking at it. It sees `depth` mm past its line, or everything without one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub name: String,
    pub start: P2,
    pub end: P2,
    #[serde(default)]
    pub depth: Option<f64>,
}

impl Section {
    /// The way it looks: square to its line, to the left.
    pub fn looking(&self) -> P2 {
        (self.end - self.start).unit().perp()
    }
}

/// The side of the building an elevation shows: North shows the north side, looking south.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Facing {
    North,
    South,
    East,
    West,
}

impl Facing {
    pub const ALL: [Facing; 4] = [Facing::North, Facing::South, Facing::East, Facing::West];

    pub fn name(self) -> &'static str {
        match self {
            Facing::North => "North",
            Facing::South => "South",
            Facing::East => "East",
            Facing::West => "West",
        }
    }

    pub fn parse(s: &str) -> Option<Facing> {
        Facing::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(s.trim()))
    }

    /// Out of the building, toward its side.
    fn out(self) -> P2 {
        match self {
            Facing::North => P2(0.0, 1.0),
            Facing::South => P2(0.0, -1.0),
            Facing::East => P2(1.0, 0.0),
            Facing::West => P2(-1.0, 0.0),
        }
    }
}

/// A sheet: paper of an ISO size (A0 to A4, landscape), with a title block, and the views placed
/// on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub number: String,
    pub name: String,
    pub size: String,
}

/// The paper an ISO size names, landscape: width and height in mm.
pub fn paper(size: &str) -> Option<P2> {
    match size.trim().to_ascii_uppercase().as_str() {
        "A0" => Some(P2(1189.0, 841.0)),
        "A1" => Some(P2(841.0, 594.0)),
        "A2" => Some(P2(594.0, 420.0)),
        "A3" => Some(P2(420.0, 297.0)),
        "A4" => Some(P2(297.0, 210.0)),
        _ => None,
    }
}

/// A view on a sheet: a level's plan, a section or an elevation, its middle `at` mm from the
/// paper's bottom left corner, at 1 : `scale`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub sheet: Id,
    pub view: Id,
    pub at: P2,
    pub scale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "lowercase")]
pub enum Element {
    Level(Level),
    Grid(Grid),
    Wall(Wall),
    Floor(Floor),
    Door(Opening),
    Window(Opening),
    Room(Room),
    Dimension(Dimension),
    Tag(Tag),
    Section(Section),
    Elevation(Section),
    Sheet(Sheet),
    Viewport(Viewport),
    Column(Column),
    Stair(Stair),
    Roof(Roof),
}

impl Element {
    pub fn category(&self) -> &'static str {
        match self {
            Element::Level(_) => "level",
            Element::Grid(_) => "grid",
            Element::Wall(_) => "wall",
            Element::Floor(_) => "floor",
            Element::Door(_) => "door",
            Element::Window(_) => "window",
            Element::Room(_) => "room",
            Element::Dimension(_) => "dimension",
            Element::Tag(_) => "tag",
            Element::Section(_) => "section",
            Element::Elevation(_) => "elevation",
            Element::Sheet(_) => "sheet",
            Element::Viewport(_) => "viewport",
            Element::Column(_) => "column",
            Element::Stair(_) => "stair",
            Element::Roof(_) => "roof",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallType {
    pub thickness: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorType {
    pub thickness: f64,
}

/// A column type: `width` by `depth`, or round, `width` across.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnType {
    pub width: f64,
    pub depth: f64,
    #[serde(default)]
    pub round: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoofType {
    pub thickness: f64,
}

/// A door or window type. `sill` is a window's default sill height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningType {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub sill: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Types {
    pub wall: BTreeMap<String, WallType>,
    pub floor: BTreeMap<String, FloorType>,
    pub door: BTreeMap<String, OpeningType>,
    pub window: BTreeMap<String, OpeningType>,
    #[serde(default)]
    pub column: BTreeMap<String, ColumnType>,
    #[serde(default)]
    pub roof: BTreeMap<String, RoofType>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub name: String,
    /// Seeds the IFC GlobalIds, so every export of this model keeps them.
    pub uid: u64,
    pub next_id: Id,
    pub types: Types,
    pub elements: BTreeMap<Id, Element>,
}

impl Model {
    /// An empty project with two levels and a few types to start from.
    pub fn new(name: &str) -> Model {
        let mut m = Model { name: name.into(), uid: fresh_uid(), next_id: 1, types: Types::default(), elements: BTreeMap::new() };
        for (n, t) in [("Generic - 200mm", 200.0), ("Interior - 100mm", 100.0), ("Exterior - 300mm", 300.0)] {
            m.types.wall.insert(n.into(), WallType { thickness: t });
        }
        m.types.floor.insert("Generic - 200mm".into(), FloorType { thickness: 200.0 });
        for (n, w) in [("Single 900 x 2100", 900.0), ("Single 800 x 2100", 800.0)] {
            m.types.door.insert(n.into(), OpeningType { width: w, height: 2100.0, sill: 0.0 });
        }
        for (n, w, h, s) in [("Fixed 1200 x 1500", 1200.0, 1500.0, 900.0), ("Fixed 600 x 900", 600.0, 900.0, 1100.0)] {
            m.types.window.insert(n.into(), OpeningType { width: w, height: h, sill: s });
        }
        m.types.column.insert("Rectangular 400 x 400".into(), ColumnType { width: 400.0, depth: 400.0, round: false });
        m.types.column.insert("Round 300".into(), ColumnType { width: 300.0, depth: 300.0, round: true });
        m.types.roof.insert("Generic - 300mm".into(), RoofType { thickness: 300.0 });
        m.insert(Element::Level(Level { name: "Level 1".into(), elevation: 0.0 }));
        m.insert(Element::Level(Level { name: "Level 2".into(), elevation: 3000.0 }));
        m
    }

    /// Adds an element unchecked; the engine runs [`Model::check`] after every command.
    pub fn insert(&mut self, e: Element) -> Id {
        // A hand-edited file can carry a stale `next_id`; never overwrite an element.
        let id = self.next_id.max(self.elements.keys().next_back().map_or(1, |k| k + 1));
        self.next_id = id + 1;
        self.elements.insert(id, e);
        id
    }

    pub fn level(&self, id: Id) -> Option<&Level> {
        match self.elements.get(&id) {
            Some(Element::Level(l)) => Some(l),
            _ => None,
        }
    }

    pub fn wall(&self, id: Id) -> Option<&Wall> {
        match self.elements.get(&id) {
            Some(Element::Wall(w)) => Some(w),
            _ => None,
        }
    }

    /// Levels from lowest to highest.
    pub fn levels(&self) -> Vec<(Id, &Level)> {
        let mut v: Vec<(Id, &Level)> = self
            .elements
            .iter()
            .filter_map(|(&id, e)| match e {
                Element::Level(l) => Some((id, l)),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.1.elevation.total_cmp(&b.1.elevation));
        v
    }

    pub fn elevation(&self, level: Id) -> f64 {
        self.level(level).map_or(0.0, |l| l.elevation)
    }

    /// The next level up from `level`.
    pub fn level_above(&self, level: Id) -> Option<Id> {
        let z = self.elevation(level);
        self.levels().into_iter().find(|(_, l)| l.elevation > z + TOL).map(|(id, _)| id)
    }

    pub fn thickness(&self, w: &Wall) -> f64 {
        self.types.wall.get(&w.type_name).map_or(0.0, |t| t.thickness)
    }

    pub fn base_z(&self, w: &Wall) -> f64 {
        self.elevation(w.level) + w.base_offset
    }

    pub fn top_z(&self, w: &Wall) -> f64 {
        match w.top_level {
            Some(t) => self.elevation(t),
            None => self.base_z(w) + w.height,
        }
    }

    /// Whether a plan of `level` cuts the wall: the wall spans the level's cut plane.
    pub fn cut_by(&self, w: &Wall, level: Id) -> bool {
        let z = self.elevation(level) + CUT_HEIGHT;
        self.base_z(w) <= z && z < self.top_z(w)
    }

    /// How far wall `id` reaches past its end at `p` so a corner closes: the half thickness of the
    /// thickest other wall ending there that it overlaps in height. Joined walls overlap at the
    /// corner; plans draw their union. A wall in line with it (a split one) needs no reach.
    pub fn join_reach(&self, id: Id, p: P2) -> f64 {
        let Some(w) = self.wall(id) else { return 0.0 };
        let (lo, hi) = (self.base_z(w), self.top_z(w));
        // ponytail: scans every wall per end, O(n²) per view; index wall ends when models grow large.
        self.elements
            .iter()
            .filter_map(|(&other, e)| match e {
                Element::Wall(o)
                    if other != id
                        && (o.start.dist(p) <= TOL || o.end.dist(p) <= TOL)
                        && self.base_z(o) < hi
                        && lo < self.top_z(o)
                        && (o.end - o.start).unit().cross((w.end - w.start).unit()).abs() > 1e-9 =>
                {
                    Some(self.thickness(o) / 2.0)
                }
                _ => None,
            })
            .fold(0.0, f64::max)
    }

    /// The names of a category's types (wall, floor, door or window, in any case, singular or
    /// plural); none for the others.
    pub fn type_names(&self, category: &str) -> Vec<&str> {
        fn names<T>(m: &BTreeMap<String, T>) -> Vec<&str> {
            m.keys().map(String::as_str).collect()
        }
        match crate::commands::category(category).as_str() {
            "wall" => names(&self.types.wall),
            "floor" => names(&self.types.floor),
            "door" => names(&self.types.door),
            "window" => names(&self.types.window),
            "column" => names(&self.types.column),
            "roof" => names(&self.types.roof),
            _ => Vec::new(),
        }
    }

    /// The type a new element of `category` gets when none is named: the one a new model starts
    /// with, else the first by name.
    pub fn default_type(&self, category: &str) -> Option<&str> {
        let preferred = match crate::commands::category(category).as_str() {
            "wall" | "floor" => "Generic - 200mm",
            "door" => "Single 900 x 2100",
            "column" => "Rectangular 400 x 400",
            "roof" => "Generic - 300mm",
            _ => "Fixed 1200 x 1500",
        };
        let names = self.type_names(category);
        names.iter().find(|n| **n == preferred).or(names.first()).copied()
    }

    /// The type of a door or window.
    pub fn opening_type(&self, e: &Element) -> Option<&OpeningType> {
        match e {
            Element::Door(o) => self.types.door.get(&o.type_name),
            Element::Window(o) => self.types.window.get(&o.type_name),
            _ => None,
        }
    }

    /// The level an element stands on: a door's or window's is its wall's, a tag's its door's or
    /// window's. Levels and grids stand on none.
    pub fn level_of(&self, e: &Element) -> Option<Id> {
        let wall_level = |o: &Opening| self.wall(o.host).map(|w| w.level);
        match e {
            Element::Wall(w) => Some(w.level),
            Element::Floor(f) => Some(f.level),
            Element::Room(r) => Some(r.level),
            Element::Dimension(d) => Some(d.level),
            Element::Column(c) => Some(c.level),
            Element::Stair(st) => Some(st.level),
            Element::Roof(r) => Some(r.level),
            Element::Door(o) | Element::Window(o) => wall_level(o),
            Element::Tag(t) => match self.elements.get(&t.host) {
                Some(Element::Door(o) | Element::Window(o)) => wall_level(o),
                _ => None,
            },
            Element::Level(_)
            | Element::Grid(_)
            | Element::Section(_)
            | Element::Elevation(_)
            | Element::Sheet(_)
            | Element::Viewport(_) => None,
        }
    }

    /// From a base level and offset up to a top level, or `height` above the base: bottom and top.
    pub fn span(&self, level: Id, base_offset: f64, top_level: Option<Id>, height: f64) -> (f64, f64) {
        let base = self.elevation(level) + base_offset;
        (base, top_level.map_or(base + height, |t| self.elevation(t)))
    }

    /// A stair's flight: how many risers, how high each is, and how long its run is.
    pub fn flight(&self, st: &Stair) -> (u32, f64, f64) {
        let (base, top) = self.span(st.level, 0.0, st.top_level, st.height);
        let rise = top - base;
        // Not a number, or less than one, is none.
        let n = (rise / st.riser - 1e-9).ceil().clamp(0.0, 1e6) as u32;
        let each = if n > 0 { rise / f64::from(n) } else { 0.0 };
        (n, each, f64::from(n.saturating_sub(1)) * st.tread)
    }

    /// A roof's pieces: each slope's underside (a flat polygon, x, y, z), and the roof's thickness
    /// measured upright, how far each rises. A flat roof is its outline at its level; a pitched one
    /// needs a rectangle: its two long slopes, and hipped, its two ends.
    pub fn roof_pieces(&self, r: &Roof) -> Result<(Faces, f64), String> {
        let t = self.types.roof.get(&r.type_name).map_or(0.0, |t| t.thickness);
        let base = self.elevation(r.level);
        let at = |p: P2, z: f64| [p.0, p.1, z];
        let mut ring: Vec<P2> = r.boundary.clone();
        if geom::area(&ring) < 0.0 {
            ring.reverse();
        }
        if r.slope == 0.0 {
            return Ok((vec![ring.iter().map(|p| at(*p, base)).collect()], t));
        }
        // The corners, counter-clockwise, without the points on straight edges.
        let corners = geom::corners(&ring);
        let square = |a: P2, b: P2, c: P2| (b - a).unit().dot((c - b).unit()).abs() < 1e-6;
        let (a, b, c, d) = match corners.as_slice() {
            &[a, b, c, d] if square(a, b, c) && square(b, c, d) && square(c, d, a) => (a, b, c, d),
            _ => return Err("a pitched roof needs a rectangular footprint, four corners square; give `slope: 0` for a flat one".into()),
        };
        // The long sides first.
        let (a, b, c, d) = if a.dist(b) >= b.dist(c) { (a, b, c, d) } else { (b, c, d, a) };
        let (long, short) = (a.dist(b), b.dist(c));
        let (u, inward) = ((b - a).unit(), (b - a).unit().perp());
        let tan = r.slope.to_radians().tan();
        let (eave, ridge) = (base - r.overhang * tan, base + (short / 2.0 - r.overhang) * tan);
        let mut pieces = if r.gable {
            let (g1, g2) = (a + inward * (short / 2.0), b + inward * (short / 2.0));
            vec![vec![at(a, eave), at(b, eave), at(g2, ridge), at(g1, ridge)], vec![at(c, eave), at(d, eave), at(g1, ridge), at(g2, ridge)]]
        } else {
            let half = short / 2.0;
            let (r1, r2) = (a + u * half + inward * half, a + u * (long - half) + inward * half);
            vec![
                vec![at(a, eave), at(b, eave), at(r2, ridge), at(r1, ridge)],
                vec![at(c, eave), at(d, eave), at(r1, ridge), at(r2, ridge)],
                vec![at(b, eave), at(c, eave), at(r2, ridge)],
                vec![at(d, eave), at(a, eave), at(r1, ridge)],
            ]
        };
        // Over a square, the ridge is a point: each long slope a triangle.
        for piece in &mut pieces {
            piece.dedup_by(|p, q| P2(p[0], p[1]).dist(P2(q[0], q[1])) < TOL);
        }
        Ok((pieces, t / r.slope.to_radians().cos()))
    }

    /// A column's outline in plan: its rectangle turned to its angle, or a round one's 24-sided
    /// polygon. Nothing for an unknown type.
    pub fn column_outline(&self, col: &Column) -> Vec<P2> {
        let Some(t) = self.types.column.get(&col.type_name) else { return Vec::new() };
        if t.round {
            return (0..24).map(|i| col.at + heading(f64::from(i) * 15.0) * (t.width / 2.0)).collect();
        }
        let (u, w, d) = (heading(col.angle), t.width / 2.0, t.depth / 2.0);
        [(-w, -d), (w, -d), (w, d), (-w, d)].into_iter().map(|(a, c)| col.at + u * a + u.perp() * c).collect()
    }

    /// Sheet `id`.
    pub fn sheet(&self, id: Id) -> Option<&Sheet> {
        match self.elements.get(&id) {
            Some(Element::Sheet(s)) => Some(s),
            _ => None,
        }
    }

    /// The name of view `id`, a level's plan, a section or an elevation; none for another element.
    pub fn view_name(&self, id: Id) -> Option<&str> {
        match self.elements.get(&id) {
            Some(Element::Level(l)) => Some(&l.name),
            Some(Element::Section(v) | Element::Elevation(v)) => Some(&v.name),
            _ => None,
        }
    }

    /// Section or elevation `id`.
    pub fn section(&self, id: Id) -> Option<&Section> {
        match self.elements.get(&id) {
            Some(Element::Section(v) | Element::Elevation(v)) => Some(v),
            _ => None,
        }
    }

    /// The first of `base`, `base 2`, `base 3`… (or with `numbered`, `base 1`, `base 2`…) that no
    /// view has: no level, section or elevation.
    pub fn free_view_name(&self, base: &str, numbered: bool) -> String {
        let taken: BTreeSet<&str> = self
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Level(l) => Some(l.name.as_str()),
                Element::Section(v) | Element::Elevation(v) => Some(v.name.as_str()),
                _ => None,
            })
            .collect();
        (1..)
            .map(|n| if n == 1 && !numbered { base.to_string() } else { format!("{base} {n}") })
            .find(|n| !taken.contains(n.as_str()))
            .unwrap_or_default()
    }

    /// The box around the walls (faces included) and floors in plan.
    pub fn footprint(&self) -> Option<(P2, P2)> {
        let mut pts = self.elements.values().flat_map(|e| match e {
            Element::Wall(w) => {
                let t = self.thickness(w) / 2.0;
                vec![w.start - P2(t, t), w.start + P2(t, t), w.end - P2(t, t), w.end + P2(t, t)]
            }
            Element::Floor(f) => f.boundary.clone(),
            _ => Vec::new(),
        });
        let first = pts.next()?;
        Some(pts.fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))))
    }

    /// The side of the building `at` is on: the way it lies farther from the building's middle.
    pub fn facing_from(&self, at: P2) -> Option<Facing> {
        let (lo, hi) = self.footprint()?;
        let d = at - (lo + hi) * 0.5;
        Some(match (d.0.abs() > d.1.abs(), d.0 > 0.0, d.1 > 0.0) {
            (true, true, _) => Facing::East,
            (true, false, _) => Facing::West,
            (false, _, true) => Facing::North,
            (false, _, false) => Facing::South,
        })
    }

    /// The line of an elevation of the building's `facing` side: 3 m out from it, or through
    /// `through`, and 1 m past it each way; it looks back at the building.
    pub fn elevation_line(&self, facing: Facing, through: Option<P2>) -> Option<(P2, P2)> {
        let (lo, hi) = self.footprint()?;
        let (middle, out) = ((lo + hi) * 0.5, facing.out());
        let u = out.perp();
        let reach = through.map_or((hi - lo).dot(out).abs() / 2.0 + 3000.0, |p| (p - middle).dot(out));
        let half = (hi - lo).dot(u).abs() / 2.0 + 1000.0;
        let base = middle + out * reach;
        Some((base - u * half, base + u * half))
    }

    /// A door's or window's centre, on its wall's centre line.
    pub fn anchor(&self, id: Id) -> Option<P2> {
        match self.elements.get(&id) {
            Some(Element::Door(o) | Element::Window(o)) => self.wall(o.host).map(|w| w.start + (w.end - w.start).unit() * o.offset),
            _ => None,
        }
    }

    /// Where a new tag of door or window `host` goes from its centre: beside the wall, 400 mm
    /// past its face, on the side the door doesn't swing to (the window doesn't face).
    pub fn tag_offset(&self, host: Id) -> P2 {
        let (Some(Element::Door(o) | Element::Window(o)), Some(w)) = (self.elements.get(&host), self.wall_of(host)) else {
            return P2(0.0, 0.0);
        };
        let away = if o.flip_facing { 1.0 } else { -1.0 };
        let p = (w.end - w.start).unit().perp() * (away * (self.thickness(w) / 2.0 + 400.0));
        // `+ 0.0`: no `-0` in files.
        P2(p.0 + 0.0, p.1 + 0.0)
    }

    fn wall_of(&self, opening: Id) -> Option<&Wall> {
        match self.elements.get(&opening) {
            Some(Element::Door(o) | Element::Window(o)) => self.wall(o.host),
            _ => None,
        }
    }

    /// What a tag shows: its door's or window's mark, `?` while that has none.
    pub fn tag_text(&self, t: &Tag) -> String {
        match self.elements.get(&t.host) {
            Some(Element::Door(o) | Element::Window(o)) if !o.mark.trim().is_empty() => o.mark.clone(),
            _ => "?".into(),
        }
    }

    /// The line `r` measures to, or why there is none. A wall's lines reach as far as its plan
    /// outline does, past the ends it shares.
    pub fn reference_line(&self, r: &Reference) -> Result<Line, String> {
        match (self.elements.get(&r.id), r.face) {
            (Some(Element::Grid(g)), Face::Centre) => Ok((g.start, g.end)),
            (Some(Element::Grid(_)), _) => Err(format!("grid {} has no faces; measure to its line", r.id)),
            (Some(Element::Wall(w)), face) => {
                let u = (w.end - w.start).unit();
                let side = match face {
                    Face::Centre => 0.0,
                    Face::Left => 0.5,
                    Face::Right => -0.5,
                };
                let n = u.perp() * (side * self.thickness(w));
                Ok((w.start - u * self.join_reach(r.id, w.start) + n, w.end + u * self.join_reach(r.id, w.end) + n))
            }
            (Some(e), _) => Err(format!("{} {} isn't a wall or a grid", e.category(), r.id)),
            (None, _) => Err(format!("element {} doesn't exist", r.id)),
        }
    }

    /// The lines dimensions measure to on element `id`: a wall's centre line and faces, a grid's.
    pub fn reference_lines(&self, id: Id) -> Vec<(Reference, Line)> {
        let faces: &[Face] = match self.elements.get(&id) {
            Some(Element::Wall(_)) => &[Face::Centre, Face::Left, Face::Right],
            Some(Element::Grid(_)) => &[Face::Centre],
            _ => &[],
        };
        faces
            .iter()
            .filter_map(|&face| {
                let r = Reference { id, face };
                Some((r, self.reference_line(&r).ok()?))
            })
            .collect()
    }

    /// Of element `id`'s reference lines, the one nearest `p`, and how far it is.
    pub fn reference_at(&self, id: Id, p: P2) -> Option<(Reference, f64)> {
        self.reference_lines(id).into_iter().map(|(r, (a, b))| (r, geom::dist_to_segment(p, a, b))).min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// Which way a dimension's line runs, square to its first reference: eastward, or northward
    /// when it runs north and south. Then where its references cross it, in order that way: how
    /// far from `at`, and the reference's line.
    pub fn crossings(&self, d: &Dimension) -> (P2, Vec<(f64, Line)>) {
        let lines: Vec<Line> = d.refs.iter().filter_map(|r| self.reference_line(r).ok()).collect();
        let Some(&(a, b)) = lines.first() else { return (P2(0.0, 0.0), Vec::new()) };
        let n = (b - a).unit().perp();
        let n = if n.0 < -1e-9 || (n.0.abs() <= 1e-9 && n.1 < 0.0) { n * -1.0 } else { n };
        let mut v: Vec<(f64, Line)> = lines
            .into_iter()
            .map(|(a, b)| {
                let c = geom::line_intersection(d.at, d.at + n, a, b).unwrap_or(a);
                ((c - d.at).dot(n), (a, b))
            })
            .collect();
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
        (n, v)
    }

    /// The lengths a dimension shows, segment by segment: west to east, or south to north.
    pub fn dimension_values(&self, d: &Dimension) -> Vec<f64> {
        self.crossings(d)
            .1
            .windows(2)
            .filter_map(|w| match w {
                [x, y] => Some(y.0 - x.0),
                _ => None,
            })
            .collect()
    }

    /// Centre lines of the walls a plan of `level` cuts: what bounds its rooms.
    pub fn bounding_lines(&self, level: Id) -> Vec<(P2, P2)> {
        self.elements
            .values()
            .filter_map(|e| match e {
                Element::Wall(w) if self.cut_by(w, level) => Some((w.start, w.end)),
                _ => None,
            })
            .collect()
    }

    /// A room's boundary, from the walls around its point. `None` when it isn't enclosed.
    /// Areas are measured at the wall centre lines (Revit's "At wall centerline" setting).
    pub fn room_region(&self, r: &Room) -> Option<Region> {
        geom::region_at(&self.bounding_lines(r.level), r.point, TOL)
    }

    /// Everything that must hold between commands: references resolve, geometry is finite and
    /// not degenerate, doors and windows fit their walls and don't overlap.
    pub fn check(&self) -> Result<(), String> {
        for (cat, map) in [("door", &self.types.door), ("window", &self.types.window)] {
            for (n, t) in map {
                size(&format!("{cat} type `{n}` width"), t.width)?;
                size(&format!("{cat} type `{n}` height"), t.height)?;
                coord(&format!("{cat} type `{n}` sill"), t.sill)?;
            }
        }
        for (n, t) in &self.types.wall {
            size(&format!("wall type `{n}` thickness"), t.thickness)?;
        }
        for (n, t) in &self.types.floor {
            size(&format!("floor type `{n}` thickness"), t.thickness)?;
        }
        for (n, t) in &self.types.column {
            size(&format!("column type `{n}` width"), t.width)?;
            size(&format!("column type `{n}` depth"), t.depth)?;
        }
        for (n, t) in &self.types.roof {
            size(&format!("roof type `{n}` thickness"), t.thickness)?;
        }
        let mut level_names = BTreeSet::new();
        let mut grid_names = BTreeSet::new();
        let mut view_names = BTreeSet::new();
        let mut sheet_numbers = BTreeSet::new();
        // Each view on a sheet: the sheet.
        let mut placed: BTreeMap<Id, Id> = BTreeMap::new();
        // Openings by host wall: (start, end, id) along it.
        let mut spans: BTreeMap<Id, Vec<(f64, f64, Id)>> = BTreeMap::new();
        for (&id, e) in &self.elements {
            let who = format!("{} {id}", e.category());
            let level_ok = |l: Id| self.level(l).map(|_| ()).ok_or(format!("{who}: level {l} doesn't exist"));
            match e {
                Element::Level(l) => {
                    text(&who, "name", &l.name)?;
                    coord(&format!("{who} elevation"), l.elevation)?;
                    if !level_names.insert(l.name.as_str()) {
                        return Err(format!("{who}: another level is already named `{}`", l.name));
                    }
                }
                Element::Grid(g) => {
                    text(&who, "name", &g.name)?;
                    line(&who, g.start, g.end)?;
                    if !grid_names.insert(g.name.as_str()) {
                        return Err(format!("{who}: another grid is already named `{}`", g.name));
                    }
                }
                Element::Wall(w) => {
                    known(&who, &w.type_name, &self.types.wall)?;
                    level_ok(w.level)?;
                    line(&who, w.start, w.end)?;
                    coord(&format!("{who} base_offset"), w.base_offset)?;
                    match w.top_level {
                        Some(t) => {
                            level_ok(t)?;
                            if self.top_z(w) <= self.base_z(w) + TOL {
                                return Err(format!("{who}: its top level must be above its base"));
                            }
                        }
                        None => size(&format!("{who} height"), w.height)?,
                    }
                }
                Element::Floor(f) => {
                    known(&who, &f.type_name, &self.types.floor)?;
                    level_ok(f.level)?;
                    if f.boundary.len() < 3 || f.boundary.len() > MAX_POINTS {
                        return Err(format!("{who}: a boundary needs 3 to {MAX_POINTS} points"));
                    }
                    for p in &f.boundary {
                        point(&who, *p)?;
                    }
                    if geom::area(&f.boundary).abs() <= TOL * TOL {
                        return Err(format!("{who}: its boundary encloses no area"));
                    }
                }
                Element::Door(o) | Element::Window(o) => {
                    let types = if matches!(e, Element::Door(_)) { &self.types.door } else { &self.types.window };
                    let t = known(&who, &o.type_name, types)?;
                    let w = self.wall(o.host).ok_or(format!("{who}: host wall {} doesn't exist", o.host))?;
                    coord(&format!("{who} offset"), o.offset)?;
                    coord(&format!("{who} sill"), o.sill)?;
                    text(&who, "mark", &o.mark)?;
                    let (a, b) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
                    if a < -TOL || b > w.start.dist(w.end) + TOL {
                        return Err(format!(
                            "{who}: {} mm wide at offset {} doesn't fit wall {}, {} mm long",
                            t.width,
                            o.offset,
                            o.host,
                            w.start.dist(w.end)
                        ));
                    }
                    if o.sill < 0.0 || o.sill + t.height > self.top_z(w) - self.base_z(w) + TOL {
                        return Err(format!("{who}: sill {} + height {} doesn't fit wall {}'s height", o.sill, t.height, o.host));
                    }
                    spans.entry(o.host).or_default().push((a, b, id));
                }
                Element::Room(r) => {
                    level_ok(r.level)?;
                    point(&who, r.point)?;
                    text(&who, "name", &r.name)?;
                    text(&who, "number", &r.number)?;
                }
                Element::Dimension(d) => {
                    level_ok(d.level)?;
                    point(&who, d.at)?;
                    if d.refs.len() < 2 || d.refs.len() > MAX_POINTS {
                        return Err(format!("{who}: a dimension measures between 2 to {MAX_POINTS} references"));
                    }
                    let mut seen = BTreeSet::new();
                    let mut first: Option<(Reference, Line)> = None;
                    for r in &d.refs {
                        let line = self.reference_line(r).map_err(|e| format!("{who}: {e}"))?;
                        let what = format!("{} {}", self.cat(r.id), r.id);
                        if !seen.insert((r.id, r.face)) {
                            return Err(format!("{who}: it measures to {what}'s {} twice", r.face.name()));
                        }
                        match first {
                            None => first = Some((*r, line)),
                            Some((f, (a, b))) if !geom::parallel(a, b, line.0, line.1) => {
                                return Err(format!(
                                    "{who}: {what} isn't parallel to {} {}; a dimension measures between parallel walls and grids",
                                    self.cat(f.id),
                                    f.id
                                ));
                            }
                            Some(_) => {}
                        }
                    }
                }
                Element::Section(v) | Element::Elevation(v) => {
                    text(&who, "name", &v.name)?;
                    if v.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    line(&who, v.start, v.end)?;
                    if let Some(d) = v.depth {
                        size(&format!("{who} depth"), d)?;
                    }
                    if !view_names.insert(v.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", v.name));
                    }
                }
                Element::Column(col) => {
                    known(&who, &col.type_name, &self.types.column)?;
                    level_ok(col.level)?;
                    point(&who, col.at)?;
                    coord(&format!("{who} angle"), col.angle)?;
                    coord(&format!("{who} base_offset"), col.base_offset)?;
                    match col.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), col.height)?,
                    }
                    let (bottom, top) = self.span(col.level, col.base_offset, col.top_level, col.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                }
                Element::Stair(st) => {
                    level_ok(st.level)?;
                    point(&who, st.start)?;
                    coord(&format!("{who} angle"), st.angle)?;
                    match st.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), st.height)?,
                    }
                    for (what, v) in [("width", st.width), ("riser", st.riser), ("tread", st.tread)] {
                        size(&format!("{who} {what}"), v)?;
                    }
                    let (n, _, _) = self.flight(st);
                    if !(2..=200).contains(&n) {
                        return Err(format!(
                            "{who}: {n} risers; a stair takes 2 to 200, so check its height and its riser ({} mm)",
                            st.riser
                        ));
                    }
                }
                Element::Roof(r) => {
                    known(&who, &r.type_name, &self.types.roof)?;
                    level_ok(r.level)?;
                    if r.boundary.len() < 3 || r.boundary.len() > MAX_POINTS {
                        return Err(format!("{who}: a footprint needs 3 to {MAX_POINTS} points"));
                    }
                    for p in &r.boundary {
                        point(&who, *p)?;
                    }
                    if geom::area(&r.boundary).abs() <= TOL * TOL {
                        return Err(format!("{who}: its footprint encloses no area"));
                    }
                    coord(&format!("{who} overhang"), r.overhang)?;
                    if !(0.0..=75.0).contains(&r.slope) {
                        return Err(format!("{who}: slope {}° isn't from 0° (flat) to 75°", r.slope));
                    }
                    self.roof_pieces(r).map_err(|e| format!("{who}: {e}"))?;
                }
                Element::Sheet(sh) => {
                    text(&who, "number", &sh.number)?;
                    text(&who, "name", &sh.name)?;
                    if sh.number.trim().is_empty() {
                        return Err(format!("{who}: a sheet needs a number"));
                    }
                    if !sheet_numbers.insert(sh.number.as_str()) {
                        return Err(format!("{who}: another sheet is already numbered `{}`", sh.number));
                    }
                    if paper(&sh.size).is_none() {
                        return Err(format!("{who}: size `{}` isn't A0, A1, A2, A3 or A4", sh.size));
                    }
                }
                Element::Viewport(v) => {
                    if self.sheet(v.sheet).is_none() {
                        return Err(format!("{who}: {} {} isn't a sheet", self.cat(v.sheet), v.sheet));
                    }
                    let name = self.view_name(v.view).ok_or(format!(
                        "{who}: {} {} isn't a plan, a section or an elevation",
                        self.cat(v.view),
                        v.view
                    ))?;
                    point(&who, v.at)?;
                    if !(v.scale.is_finite() && (1.0..=100_000.0).contains(&v.scale)) {
                        return Err(format!("{who}: scale 1 : {} isn't between 1 : 1 and 1 : 100000", v.scale));
                    }
                    if let Some(other) = placed.insert(v.view, v.sheet) {
                        let number = self.sheet(other).map_or("", |s| s.number.as_str());
                        return Err(format!("{who}: {name} is on sheet {number} already; a view goes on one sheet"));
                    }
                }
                Element::Tag(t) => {
                    match self.elements.get(&t.host) {
                        Some(Element::Door(_) | Element::Window(_)) => {}
                        Some(h) => return Err(format!("{who}: tags name doors and windows, not {} {}", h.category(), t.host)),
                        None => return Err(format!("{who}: element {} doesn't exist", t.host)),
                    }
                    point(&format!("{who} offset"), t.offset)?;
                }
            }
        }
        if let Some(n) = view_names.intersection(&level_names).next() {
            return Err(format!("a section or elevation is named `{n}`, as a level is; name it otherwise"));
        }
        for (host, mut v) in spans {
            v.sort_by(|x, y| x.0.total_cmp(&y.0));
            for pair in v.windows(2) {
                if let [(_, end, a), (start, _, b)] = pair
                    && *end > start + TOL
                {
                    return Err(format!("{} {a} and {} {b} overlap in wall {host}", self.cat(*a), self.cat(*b)));
                }
            }
        }
        Ok(())
    }

    fn cat(&self, id: Id) -> &'static str {
        self.elements.get(&id).map_or("element", Element::category)
    }
}

/// The element a [`Model::check`] error is about: its messages about an element start with the
/// element's category and id, `door 12: …`.
pub fn culprit(error: &str) -> Option<Id> {
    let mut words = error.split_whitespace();
    words.next()?;
    words.next()?.trim_end_matches(':').parse().ok()
}

fn coord(what: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v.abs() <= MAX_COORD { Ok(()) } else { Err(format!("{what}: {v} is not a usable number")) }
}

fn size(what: &str, v: f64) -> Result<(), String> {
    coord(what, v)?;
    if v > 0.0 { Ok(()) } else { Err(format!("{what} must be above zero, got {v}")) }
}

fn point(who: &str, p: P2) -> Result<(), String> {
    coord(&format!("{who} x"), p.0)?;
    coord(&format!("{who} y"), p.1)
}

fn line(who: &str, a: P2, b: P2) -> Result<(), String> {
    point(who, a)?;
    point(who, b)?;
    if a.dist(b) > TOL { Ok(()) } else { Err(format!("{who}: too short ({} mm)", a.dist(b))) }
}

fn text(who: &str, what: &str, s: &str) -> Result<(), String> {
    if s.chars().count() <= MAX_TEXT { Ok(()) } else { Err(format!("{who}: {what} is longer than {MAX_TEXT} characters")) }
}

/// The type named `name`, or an error listing the types there are.
fn known<'a, T>(who: &str, name: &str, types: &'a BTreeMap<String, T>) -> Result<&'a T, String> {
    types.get(name).ok_or_else(|| {
        let names: Vec<&str> = types.keys().map(String::as_str).collect();
        format!("{who}: type `{name}` doesn't exist (there are: {})", names.join(", "))
    })
}

/// SplitMix64: spreads a seed over all 64 bits.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fresh_uid() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    mix(nanos ^ u64::from(std::process::id()).rotate_left(32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(level: Id, a: P2, b: P2) -> Element {
        Element::Wall(Wall {
            type_name: "Generic - 200mm".into(),
            level,
            start: a,
            end: b,
            top_level: None,
            height: 3000.0,
            base_offset: 0.0,
        })
    }

    /// A 6 x 4 m box of walls on Level 1 (id 1), returning the bottom wall's id.
    fn boxed(m: &mut Model) -> Id {
        let (a, b, c, d) = (P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 4000.0), P2(0.0, 4000.0));
        let bottom = m.insert(wall(1, a, b));
        for (s, e) in [(b, c), (c, d), (d, a)] {
            m.insert(wall(1, s, e));
        }
        bottom
    }

    fn door(host: Id, offset: f64) -> Element {
        Element::Door(Opening {
            type_name: "Single 900 x 2100".into(),
            host,
            offset,
            sill: 0.0,
            mark: String::new(),
            flip_facing: false,
            flip_hand: false,
        })
    }

    #[test]
    fn a_new_model_has_two_levels_and_checks_clean() {
        let m = Model::new("House");
        let names: Vec<&str> = m.levels().iter().map(|(_, l)| l.name.as_str()).collect();
        assert_eq!(names, ["Level 1", "Level 2"]);
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn walls_constrained_to_a_level_take_its_height() {
        let mut m = Model::new("t");
        let id = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(&id) {
            w.top_level = Some(2);
            w.base_offset = 100.0;
        }
        let w = m.wall(id).unwrap();
        assert_eq!((m.base_z(w), m.top_z(w)), (100.0, 3000.0));
    }

    #[test]
    fn plans_cut_the_walls_that_span_their_cut_plane() {
        let mut m = Model::new("t");
        let one = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        let tall = m.insert(wall(1, P2(0.0, 1000.0), P2(5000.0, 1000.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(&tall) {
            w.height = 6000.0;
        }
        let (one, tall) = (m.wall(one).unwrap(), m.wall(tall).unwrap());
        assert!(m.cut_by(one, 1) && !m.cut_by(one, 2));
        assert!(m.cut_by(tall, 1) && m.cut_by(tall, 2));
    }

    #[test]
    fn walls_reach_past_ends_they_share_to_close_the_corner() {
        let mut m = Model::new("t");
        let bottom = boxed(&mut m);
        // A partition ending mid-span on the bottom wall: a T, not a corner.
        let t = m.insert(wall(1, P2(3000.0, 0.0), P2(3000.0, 4000.0)));
        assert_eq!((m.join_reach(bottom, P2(0.0, 0.0)), m.join_reach(bottom, P2(6000.0, 0.0))), (100.0, 100.0));
        assert_eq!(m.join_reach(t, P2(3000.0, 0.0)), 0.0);
        // A wall upstairs meeting the same corner doesn't join walls downstairs.
        let up = m.insert(wall(2, P2(0.0, 0.0), P2(0.0, -3000.0)));
        assert_eq!(m.join_reach(up, P2(0.0, 0.0)), 0.0);
    }

    #[test]
    fn rooms_are_bounded_by_the_walls_their_level_cuts() {
        let mut m = Model::new("t");
        boxed(&mut m);
        let room = Room { level: 1, point: P2(3000.0, 2000.0), name: "Hall".into(), number: "1".into() };
        assert_eq!(m.room_region(&room).map(|r| r.area()), Some(24e6));
        let upstairs = Room { level: 2, ..room };
        assert_eq!(m.room_region(&upstairs), None, "Level 2 has no walls");
    }

    #[test]
    fn check_names_unknown_types_and_lists_the_known_ones() {
        let mut m = Model::new("t");
        let id = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(&id) {
            w.type_name = "Brick".into();
        }
        let e = m.check().unwrap_err();
        assert!(e.contains(&format!("wall {id}")) && e.contains("Brick") && e.contains("Generic - 200mm"), "{e}");
    }

    #[test]
    fn check_refuses_doors_that_do_not_fit_their_wall() {
        let mut m = Model::new("t");
        let host = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        m.insert(door(host, 300.0));
        let e = m.check().unwrap_err();
        assert!(e.contains("fit"), "{e}");
    }

    #[test]
    fn check_refuses_overlapping_openings() {
        let mut m = Model::new("t");
        let host = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        m.insert(door(host, 1000.0));
        m.insert(door(host, 2000.0));
        assert_eq!(m.check(), Ok(()));
        m.insert(door(host, 2600.0));
        let e = m.check().unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }

    #[test]
    fn check_refuses_dangling_references_and_bad_numbers() {
        let mut m = Model::new("t");
        m.insert(wall(99, P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert!(m.check().unwrap_err().contains("level 99"));
        let mut m = Model::new("t");
        m.insert(wall(1, P2(0.0, 0.0), P2(f64::NAN, 0.0)));
        assert!(m.check().is_err());
        let mut m = Model::new("t");
        m.insert(wall(1, P2(0.0, 0.0), P2(0.5, 0.0)));
        assert!(m.check().unwrap_err().contains("short"));
    }

    #[test]
    fn element_json_rejects_unknown_fields() {
        let ok = r#"{"category":"level","name":"Roof","elevation":6000}"#;
        assert!(serde_json::from_str::<Element>(ok).is_ok());
        let typo = r#"{"category":"level","name":"Roof","elevaton":6000}"#;
        let e = serde_json::from_str::<Element>(typo).unwrap_err().to_string();
        assert!(e.contains("elevaton"), "{e}");
    }

    #[test]
    fn new_elements_default_to_the_types_a_new_model_starts_with() {
        let mut m = Model::new("t");
        let defaults = ["wall", "floor", "door", "window"].map(|c| m.default_type(c));
        assert_eq!(defaults, [Some("Generic - 200mm"), Some("Generic - 200mm"), Some("Single 900 x 2100"), Some("Fixed 1200 x 1500")]);
        assert_eq!(m.type_names("doors"), ["Single 800 x 2100", "Single 900 x 2100"], "any case, singular or plural");
        m.types.wall.remove("Generic - 200mm");
        assert_eq!(m.default_type("wall"), Some("Exterior - 300mm"), "else the first by name");
        assert_eq!(m.default_type("room"), None, "rooms have no types");
    }

    #[test]
    fn insert_never_reuses_an_id_from_a_stale_counter() {
        let mut m = Model::new("t");
        m.next_id = 1;
        let id = m.insert(wall(1, P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert_eq!(id, 3);
    }
}
