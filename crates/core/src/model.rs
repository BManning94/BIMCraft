//! The building model: levels, grids, walls, floors, ceilings, shafts, doors, windows, rooms,
//! columns, stairs, ramps, railings and roofs, the types they use, the dimensions, tags, text notes and spot elevations annotating
//! views, the sections, elevations, schedules and 3D views looking at it, and the sheets its views
//! are placed on. Lengths are millimetres, x east, y north, z up; each level has an elevation. In
//! a section or an elevation, millimetres along its line from its start, and up; on a sheet,
//! millimetres of paper from its bottom left corner.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2, Region};
use crate::language::Language;
use crate::units::Units;
use crate::walls;

pub type Id = u64;

/// A line in plan, through two points.
pub type Line = (P2, P2);

/// Height of a plan view's cut plane above its level, unless its view range says otherwise; rooms
/// find the walls that bound them at this height.
pub const CUT_HEIGHT: f64 = 1200.0;
/// A text note's capitals, on paper, unless told otherwise: mm.
pub const NOTE_SIZE: f64 = 2.5;
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
/// A ramp's steepest slope, percent (1 in 12), and its width, unless told otherwise.
pub const RAMP_SLOPE: f64 = 100.0 / 12.0;
pub const RAMP_WIDTH: f64 = 1200.0;
/// How far a railing on a stair or a ramp runs in from its edge, mm.
pub const RAILING_INSET: f64 = 50.0;
/// Balusters one railing may have.
pub const MAX_BALUSTERS: f64 = 10_000.0;
/// Names, marks and numbers longer than this are refused.
pub const MAX_TEXT: usize = 256;
/// Points one command or floor outline may carry.
pub const MAX_POINTS: usize = 10_000;
/// Height of a reflected ceiling plan's cut plane above its level: it looks up from there.
pub const CEILING_CUT: f64 = 2300.0;
/// Height of a ceiling's underside above its level, unless told otherwise.
pub const CEILING_HEIGHT: f64 = 2600.0;
/// The steepest slope a floor or a roof may have, degrees.
pub const MAX_SLOPE: f64 = 75.0;

/// A level, and its floor plan: what the plan sees (its view range), and the part of it the plan
/// shows (its crop region, corners south-west then north-east; all of it without one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub name: String,
    pub elevation: f64,
    #[serde(default)]
    pub view_range: ViewRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[P2; 2]>,
}

impl Level {
    pub fn new(name: impl Into<String>, elevation: f64) -> Level {
        Level { name: name.into(), elevation, view_range: ViewRange::default(), crop: None }
    }
}

/// A plan's view range, as Revit's: mm above its level. Cut by the `cut` plane, seen from it down
/// to the `bottom` and, beyond, to the view `depth`; above the cut, up to the `top` (the level
/// above when none) for what overhangs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewRange {
    pub top: Option<f64>,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

impl Default for ViewRange {
    fn default() -> ViewRange {
        ViewRange { top: None, cut: CUT_HEIGHT, bottom: 0.0, depth: 0.0 }
    }
}

/// A plan's view range as heights: its top, cut plane, bottom and view depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanRange {
    pub top: f64,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    pub name: String,
    pub start: P2,
    pub end: P2,
}

/// A wall: its location line from `start` to `end`, straight or, turning `arc` degrees, an arc;
/// which line of the wall that is (its `location`); its exterior side to the left of start to end,
/// as Revit's, so walls drawn clockwise face out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wall {
    #[serde(rename = "type")]
    pub type_name: String,
    /// Base level.
    pub level: Id,
    /// Ends of the location line.
    pub start: P2,
    pub end: P2,
    /// Top constraint: a level, or none for an unconnected wall `height` tall.
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
    /// How far it turns from its start to its end, degrees counter-clockwise: 0 straight, 180 a
    /// half circle bulging to its right.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub arc: f64,
    #[serde(default, skip_serializing_if = "is_centre")]
    pub location: Location,
    /// The roof or floor its top follows, from below, instead of its top constraint; and the one its
    /// base stands on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_top: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_base: Option<Id>,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn is_centre(l: &Location) -> bool {
    *l == Location::Centre
}

/// The line of a wall its ends lie on, as Revit's location line: its centre, its core's centre, or
/// a face, of its finish or of its core, on its exterior or its interior side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    #[default]
    #[serde(alias = "center")]
    Centre,
    #[serde(alias = "core-center")]
    CoreCentre,
    ExteriorFinish,
    InteriorFinish,
    ExteriorCore,
    InteriorCore,
}

impl Location {
    pub const ALL: [Location; 6] = [
        Location::Centre,
        Location::CoreCentre,
        Location::ExteriorFinish,
        Location::InteriorFinish,
        Location::ExteriorCore,
        Location::InteriorCore,
    ];

    /// Its name in commands and files.
    pub fn name(self) -> &'static str {
        match self {
            Location::Centre => "centre",
            Location::CoreCentre => "core-centre",
            Location::ExteriorFinish => "exterior-finish",
            Location::InteriorFinish => "interior-finish",
            Location::ExteriorCore => "exterior-core",
            Location::InteriorCore => "interior-core",
        }
    }

    /// As a menu names it.
    pub fn label(self) -> &'static str {
        match self {
            Location::Centre => "Wall Centreline",
            Location::CoreCentre => "Core Centreline",
            Location::ExteriorFinish => "Finish Face: Exterior",
            Location::InteriorFinish => "Finish Face: Interior",
            Location::ExteriorCore => "Core Face: Exterior",
            Location::InteriorCore => "Core Face: Interior",
        }
    }

    pub fn from_name(name: &str) -> Option<Location> {
        let n = name.trim().to_ascii_lowercase().replace("center", "centre");
        Location::ALL.into_iter().find(|l| l.name() == n)
    }
}

/// What a layer of a wall does: carries it (structure, its core), backs another (substrate),
/// insulates (thermal), seals it (a membrane, which may be as thin as nothing), or finishes a face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Function {
    #[default]
    Structure,
    Substrate,
    Thermal,
    Membrane,
    Finish,
}

/// A layer of a compound wall type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[serde(default)]
    pub function: Function,
    #[serde(default)]
    pub material: String,
    pub thickness: f64,
}

/// Layers a wall type may have.
pub const MAX_LAYERS: usize = 32;

/// A slab under its top, along `boundary` (counter-clockwise): its top at its level, `offset` mm
/// above it; with a slope arrow, from tail to head in plan, the top there and rising `slope`
/// degrees toward the head (falling, negative). It's its type's thickness across its slope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    pub boundary: Vec<P2>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slope: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<[P2; 2]>,
}

/// A ceiling along `boundary` (counter-clockwise), its underside `offset` mm above its level, its
/// type's thickness over that.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ceiling {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    pub boundary: Vec<P2>,
    pub offset: f64,
}

/// A shaft opening: a hole along `boundary` through the floors, ceilings and roofs it passes
/// through, from `base_offset` above `level` up to `top_level`, or `height` mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shaft {
    pub level: Id,
    pub boundary: Vec<P2>,
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
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

/// Which of an element's lines a dimension measures to: a wall's centre line, its face on the
/// left or the right of its start-to-end direction, or its start or end; a door's or a window's
/// centre, or its jamb toward its wall's start or end. A grid has only its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    #[serde(alias = "center")]
    Centre,
    Left,
    Right,
    Start,
    End,
}

impl Face {
    /// The same line of a mirrored wall, which runs back the other way: its start and end swap
    /// (a door's or a window's jambs too), its exterior and interior faces stay its left and right.
    pub fn mirrored(self) -> Face {
        match self {
            Face::Start => Face::End,
            Face::End => Face::Start,
            other => other,
        }
    }

    /// The same line of a flipped wall, which runs the other way about the same line: its left and
    /// right swap, and its start and end.
    pub fn flipped(self) -> Face {
        match self {
            Face::Left => Face::Right,
            Face::Right => Face::Left,
            other => other.mirrored(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Face::Centre => "centre line",
            Face::Left => "left face",
            Face::Right => "right face",
            Face::Start => "start",
            Face::End => "end",
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

/// A dimension in a level's plan. Aligned: the distances between parallel references, along a
/// line through `at` across them. Angular: the angle between two references that meet, on the
/// side of `at`, its arc through `at`. Radial: a round element's radius, its line toward `at`. It
/// measures the model as it is, so it follows what it measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub level: Id,
    #[serde(default)]
    pub kind: DimKind,
    pub refs: Vec<Reference>,
    pub at: P2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DimKind {
    #[default]
    Aligned,
    Angular,
    Radial,
}

/// A text note in a view: a level's plan, a section, an elevation or a sheet. `at` is the top left
/// of its first line, in the view's own terms; `size` its capitals' height on paper, mm; turned
/// `angle` degrees counter-clockwise; with a leader to `leader`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub view: Id,
    pub at: P2,
    pub text: String,
    #[serde(default = "note_size")]
    pub size: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<P2>,
}

fn note_size() -> f64 {
    NOTE_SIZE
}

/// A spot elevation in a view: in a level's plan, the height of the top of `host` at `at`; in a
/// section or an elevation, of its point `at`, along and up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spot {
    pub view: Id,
    pub at: P2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
}

/// A schedule view: the elements of category `of` in rows, as the `schedule` command lists them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub name: String,
    pub of: String,
}

/// A 3D view, seen from `azimuth` degrees round from east (counter-clockwise) and `elevation`
/// degrees up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View3D {
    pub name: String,
    pub azimuth: f64,
    pub elevation: f64,
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

/// A stair from `level` up to `top_level` (or `height` mm), climbing from the middle of its first
/// riser at `start` toward `angle` degrees (counter-clockwise from east): as many risers as keep
/// each no higher than `riser`, a tread `tread` deep between each two; in one straight run, or in
/// runs with a landing between each two.
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landings: Vec<Landing>,
}

/// A landing between two runs of a stair or a ramp: after `at` risers of a stair, or `at` mm up a
/// ramp; then the next run goes on straight (`turn` 0), turned left (90) or right (-90), or back
/// beside the last, to its left (180) or its right (-180). It's as deep as the runs are wide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landing {
    pub at: f64,
    #[serde(default)]
    pub turn: f64,
}

/// A ramp from `level` up to `top_level` (or `height` mm), from the middle of its foot at `start`
/// toward `angle` degrees: rising at its slope, `slope` percent (rise over run), in one run or in
/// runs with a landing between each two. Solid down to its level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    pub level: Id,
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub height: f64,
    pub start: P2,
    pub angle: f64,
    pub width: f64,
    pub slope: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landings: Vec<Landing>,
}

/// A side of a stair or a ramp, going up it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// A railing: along a side of a stair or a ramp, `host`, climbing with it; or along `path` on its
/// level, `base_offset` mm above it. Its rail is its type's height over what it runs along.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Railing {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Id,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<P2>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub base_offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
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
/// looking at it. It sees `depth` mm past its line, or everything without one; cropped, only from
/// the height `crop[0]` up to `crop[1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub name: String,
    pub start: P2,
    pub end: P2,
    #[serde(default)]
    pub depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f64; 2]>,
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

    /// A side by its name, in any language there is.
    pub fn parse(s: &str) -> Option<Facing> {
        Facing::ALL.into_iter().find(|f| Language::ALL.iter().any(|l| l.find(s, &[f.name()]).is_some()))
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

/// A view on a sheet: a level's plan, a section, an elevation, a schedule or a 3D view, its middle
/// `at` mm from the paper's bottom left corner, at 1 : `scale` (a schedule keeps its own size).
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
    Ceiling(Ceiling),
    Shaft(Shaft),
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
    Ramp(Ramp),
    Railing(Railing),
    Roof(Roof),
    Text(Note),
    Spot(Spot),
    Schedule(Schedule),
    View3D(View3D),
}

impl Element {
    pub fn category(&self) -> &'static str {
        match self {
            Element::Text(_) => "text",
            Element::Spot(_) => "spot",
            Element::Schedule(_) => "schedule",
            Element::View3D(_) => "view3d",
            Element::Level(_) => "level",
            Element::Grid(_) => "grid",
            Element::Wall(_) => "wall",
            Element::Floor(_) => "floor",
            Element::Ceiling(_) => "ceiling",
            Element::Shaft(_) => "shaft",
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
            Element::Ramp(_) => "ramp",
            Element::Railing(_) => "railing",
            Element::Roof(_) => "roof",
        }
    }
}

/// A wall type: how thick its walls are and, a compound one, its layers from the exterior side in,
/// which make up that thickness. Its core runs from its first structure layer to its last (all of
/// it, without one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorType {
    pub thickness: f64,
}

/// A railing type: how high its rail stands, and how far apart its balusters are (0: none).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailingType {
    pub height: f64,
    #[serde(default)]
    pub spacing: f64,
}

/// A ceiling type: how thick, and with `grid`, square tiles that wide, which its reflected ceiling
/// plan draws.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CeilingType {
    pub thickness: f64,
    #[serde(default)]
    pub grid: f64,
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
    #[serde(default)]
    pub ceiling: BTreeMap<String, CeilingType>,
    #[serde(default)]
    pub railing: BTreeMap<String, RailingType>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub name: String,
    /// Seeds the IFC GlobalIds, so every export of this model keeps them.
    pub uid: u64,
    /// How lengths and areas are shown and typed; the model itself is in millimetres.
    #[serde(default)]
    pub units: Units,
    /// What its drawings say, and the names commands give.
    #[serde(default)]
    pub language: Language,
    pub next_id: Id,
    pub types: Types,
    pub elements: BTreeMap<Id, Element>,
}

impl Model {
    /// An empty project with two levels and a few types to start from.
    pub fn new(name: &str) -> Model {
        Model::new_in(name, Language::English)
    }

    /// An empty project in `language`: its two levels named in it.
    pub fn new_in(name: &str, language: Language) -> Model {
        let mut m = Model {
            name: name.into(),
            uid: fresh_uid(),
            units: Units::default(),
            language,
            next_id: 1,
            types: Types::default(),
            elements: BTreeMap::new(),
        };
        for (n, t) in [("Generic - 200mm", 200.0), ("Interior - 100mm", 100.0)] {
            m.types.wall.insert(n.into(), WallType { thickness: t, layers: Vec::new() });
        }
        // An exterior wall's layers, outside in: brick, insulation, a block core, plaster.
        let layer = |function, material: &str, thickness| Layer { function, material: language.word(material).into(), thickness };
        let layers = vec![
            layer(Function::Finish, "Brick", 100.0),
            layer(Function::Thermal, "Insulation", 50.0),
            layer(Function::Structure, "Concrete block", 140.0),
            layer(Function::Finish, "Plaster", 10.0),
        ];
        m.types.wall.insert("Exterior - 300mm".into(), WallType { thickness: 300.0, layers });
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
        m.types.ceiling.insert("Generic - 20mm".into(), CeilingType { thickness: 20.0, grid: 0.0 });
        m.types.ceiling.insert("Tile 600 x 600".into(), CeilingType { thickness: 20.0, grid: 600.0 });
        m.types.railing.insert("Railing - 1000mm".into(), RailingType { height: 1000.0, spacing: 120.0 });
        let level = language.word("Level");
        m.insert(Element::Level(Level::new(format!("{level} 1"), 0.0)));
        m.insert(Element::Level(Level::new(format!("{level} 2"), 3000.0)));
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

    /// Its base by its level and offset, whatever it's attached to: where its doors' and windows'
    /// sills are measured from.
    pub fn own_base(&self, w: &Wall) -> f64 {
        self.elevation(w.level) + w.base_offset
    }

    /// Its top by its top constraint, whatever it's attached to.
    pub fn own_top(&self, w: &Wall) -> f64 {
        match w.top_level {
            Some(t) => self.elevation(t),
            None => self.own_base(w) + w.height,
        }
    }

    /// Its lowest base: on what its base is attached to, or by its level and offset.
    pub fn base_z(&self, w: &Wall) -> f64 {
        crate::walls::heights(self, w).0
    }

    /// Its highest top: under what its top is attached to, or by its top constraint.
    pub fn top_z(&self, w: &Wall) -> f64 {
        crate::walls::heights(self, w).1
    }

    /// Whether a plan of `level` cuts the wall: the wall spans the plan's cut plane.
    pub fn cut_by(&self, w: &Wall, level: Id) -> bool {
        self.spans(w, self.plan_range(level).cut)
    }

    /// Whether the wall bounds the rooms of `level`: it spans the height rooms are found at,
    /// [`CUT_HEIGHT`] above the level, whatever the level's plan cuts at.
    pub fn bounding(&self, w: &Wall, level: Id) -> bool {
        self.spans(w, self.elevation(level) + CUT_HEIGHT)
    }

    /// Whether the wall spans height `z`.
    pub(crate) fn spans(&self, w: &Wall, z: f64) -> bool {
        self.base_z(w) <= z && z < self.top_z(w)
    }

    /// The heights of the view range of `level`'s plan. A top that isn't set is the level above
    /// (none above: no limit), and never below the cut plane.
    pub fn plan_range(&self, level: Id) -> PlanRange {
        let z = self.elevation(level);
        let r = self.level(level).map_or_else(ViewRange::default, |l| l.view_range);
        let above = || self.level_above(level).map_or(f64::INFINITY, |a| self.elevation(a));
        PlanRange { top: r.top.map_or_else(above, |t| z + t).max(z + r.cut), cut: z + r.cut, bottom: z + r.bottom, depth: z + r.depth }
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
            "ceiling" => names(&self.types.ceiling),
            "railing" => names(&self.types.railing),
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
            "ceiling" => "Generic - 20mm",
            "railing" => "Railing - 1000mm",
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
            Element::Ceiling(c) => Some(c.level),
            Element::Shaft(sh) => Some(sh.level),
            Element::Room(r) => Some(r.level),
            Element::Dimension(d) => Some(d.level),
            Element::Column(c) => Some(c.level),
            Element::Stair(st) => Some(st.level),
            Element::Ramp(r) => Some(r.level),
            // On a stair or a ramp, its host's.
            Element::Railing(r) => r.host.and_then(|h| self.elements.get(&h)).and_then(|e| self.level_of(e)).or(Some(r.level)),
            Element::Roof(r) => Some(r.level),
            Element::Door(o) | Element::Window(o) => wall_level(o),
            Element::Tag(t) => match self.elements.get(&t.host) {
                Some(Element::Door(o) | Element::Window(o)) => wall_level(o),
                _ => None,
            },
            // Notes and spot elevations in a level's plan.
            Element::Text(Note { view, .. }) | Element::Spot(Spot { view, .. }) => self.level(*view).map(|_| *view),
            Element::Level(_)
            | Element::Grid(_)
            | Element::Section(_)
            | Element::Elevation(_)
            | Element::Sheet(_)
            | Element::Viewport(_)
            | Element::Schedule(_)
            | Element::View3D(_) => None,
        }
    }

    /// From a base level and offset up to a top level, or `height` above the base: bottom and top.
    pub fn span(&self, level: Id, base_offset: f64, top_level: Option<Id>, height: f64) -> (f64, f64) {
        let base = self.elevation(level) + base_offset;
        (base, top_level.map_or(base + height, |t| self.elevation(t)))
    }

    /// A stair's flight: how many risers, how high each is, and how long its runs are together
    /// (a tread less each, the last riser of each climbing onto a landing or the floor above).
    pub fn flight(&self, st: &Stair) -> (u32, f64, f64) {
        let (base, top) = self.span(st.level, 0.0, st.top_level, st.height);
        let rise = top - base;
        // Not a number, or less than one, is none.
        let n = (rise / st.riser - 1e-9).ceil().clamp(0.0, 1e6) as u32;
        let each = if n > 0 { rise / f64::from(n) } else { 0.0 };
        let runs = u32::try_from(st.landings.len()).unwrap_or(u32::MAX).saturating_add(1);
        (n, each, f64::from(n.saturating_sub(runs)) * st.tread)
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

    /// A floor's top: level, at its level and offset; or through there at its slope arrow's tail,
    /// rising `slope` degrees toward its head.
    pub fn floor_top(&self, f: &Floor) -> walls::Plane {
        let z = self.elevation(f.level) + f.offset;
        match f.arrow {
            Some([tail, head]) if f.slope != 0.0 && tail.dist(head) > 0.0 => {
                let g = (head - tail).unit() * f.slope.to_radians().tan();
                walls::Plane { z: z - g.dot(tail), g }
            }
            _ => walls::Plane::flat(z),
        }
    }

    /// How far a floor's underside lies below its top, plumb: its type's thickness across its slope.
    pub fn floor_depth(&self, f: &Floor) -> f64 {
        self.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness) / f.slope.to_radians().cos()
    }

    /// A floor's slab: its underside, a flat polygon in space, counter-clockwise; and how far its
    /// top lies above that.
    pub fn floor_slab(&self, f: &Floor) -> (Vec<[f64; 3]>, f64) {
        let (top, depth) = (self.floor_top(f), self.floor_depth(f));
        (ccw(&f.boundary).iter().map(|q| [q.0, q.1, top.at(*q) - depth]).collect(), depth)
    }

    /// A ceiling's slab: its underside, counter-clockwise, and its thickness.
    pub fn ceiling_slab(&self, c: &Ceiling) -> (Vec<[f64; 3]>, f64) {
        let z = self.elevation(c.level) + c.offset;
        let t = self.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
        (ccw(&c.boundary).iter().map(|q| [q.0, q.1, z]).collect(), t)
    }

    /// A floor's, a ceiling's or a roof's slab: `face`, a flat polygon in space, counter-clockwise,
    /// and what lies up to `rise` over it. In plan, what's left of it where shafts pass through it,
    /// and those shafts: a shaft cuts what its span reaches into over its outline, all the way
    /// through.
    pub fn less_shafts(&self, face: &[[f64; 3]], rise: f64) -> (Vec<Vec<P2>>, Vec<Id>) {
        let ring: Vec<P2> = face.iter().map(|p| P2(p[0], p[1])).collect();
        let Some(plane) = walls::Plane::of(face) else { return (vec![ring], Vec::new()) };
        let area = |rings: &[Vec<P2>]| rings.iter().map(|r| geom::area(r).abs()).sum::<f64>();
        let (mut pieces, mut cut) = (vec![ring], Vec::new());
        for (&id, e) in &self.elements {
            let Element::Shaft(sh) = e else { continue };
            let (bottom, top) = self.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            let zs = sh.boundary.iter().map(|q| plane.at(*q));
            let (lo, hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
            if !(lo < top - TOL && hi + rise > bottom + TOL) {
                continue;
            }
            let left = walls::minus(&pieces, &sh.boundary);
            if area(&left) < area(&pieces) - 1.0 {
                pieces = left;
                cut.push(id);
            }
        }
        (pieces, cut)
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

    /// The name of view `id`: a level's plan, a section, an elevation, a schedule or a 3D view;
    /// none for another element.
    pub fn view_name(&self, id: Id) -> Option<&str> {
        match self.elements.get(&id) {
            Some(Element::Level(l)) => Some(&l.name),
            Some(Element::Section(v) | Element::Elevation(v)) => Some(&v.name),
            Some(Element::Schedule(sc)) => Some(&sc.name),
            Some(Element::View3D(v)) => Some(&v.name),
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
    /// view has.
    pub fn free_view_name(&self, base: &str, numbered: bool) -> String {
        let taken: BTreeSet<&str> = self.elements.keys().filter_map(|id| self.view_name(*id)).collect();
        (1..)
            .map(|n| if n == 1 && !numbered { base.to_string() } else { format!("{base} {n}") })
            .find(|n| !taken.contains(n.as_str()))
            .unwrap_or_default()
    }

    /// The box around the walls (faces included) and floors in plan.
    pub fn footprint(&self) -> Option<(P2, P2)> {
        let mut pts = self.elements.iter().flat_map(|(&id, e)| match e {
            Element::Wall(_) => walls::outline(self, id),
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
            Some(Element::Door(o) | Element::Window(o)) => self.wall(o.host).map(|w| walls::Axis::of(w).at(o.offset, 0.0)),
            _ => None,
        }
    }

    /// Where a new tag of door or window `host` goes from its centre: beside the wall, 400 mm
    /// past its face, on the side the door doesn't swing to, or the window faces (its wall's
    /// exterior, unless flipped).
    pub fn tag_offset(&self, host: Id) -> P2 {
        let (Some(e @ (Element::Door(o) | Element::Window(o))), Some(w)) = (self.elements.get(&host), self.wall_of(host)) else {
            return P2(0.0, 0.0);
        };
        let (ext, int) = walls::faces(self, w);
        let outside = o.flip_facing == matches!(e, Element::Door(_));
        let p = walls::Axis::of(w).tangent(o.offset).perp() * if outside { ext + 400.0 } else { int - 400.0 };
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

    /// The line `r` measures to, or why there is none. A straight wall's lines reach as far as its
    /// plan outline does, past the ends it shares; its ends, a curved wall's too, and a door's or a
    /// window's centre and jambs, run across it from face to face. Its centre is its centre line,
    /// wherever its location line is.
    pub fn reference_line(&self, r: &Reference) -> Result<Line, String> {
        match (self.elements.get(&r.id), r.face) {
            (Some(Element::Grid(g)), Face::Centre) => Ok((g.start, g.end)),
            (Some(Element::Grid(_)), _) => Err(format!("grid {} has no faces; measure to its line", r.id)),
            (Some(Element::Wall(w)), face) => {
                let axis = walls::Axis::of(w);
                let (ext, int) = walls::faces(self, w);
                let (a, b) = match axis.radius() {
                    Some(_) if !matches!(face, Face::Start | Face::End) => {
                        return Err(format!("wall {}'s faces are arcs; measure to its start or its end, or its radius", r.id));
                    }
                    Some(_) => (0.0, axis.length()),
                    None => {
                        let along: Vec<f64> = walls::outline(self, r.id).iter().map(|p| (*p - w.start).dot(axis.tangent(0.0))).collect();
                        (along.iter().copied().fold(0.0, f64::min), along.iter().copied().fold(axis.length(), f64::max))
                    }
                };
                Ok(match face {
                    Face::Centre => (axis.at(a, (ext + int) / 2.0), axis.at(b, (ext + int) / 2.0)),
                    Face::Left => (axis.at(a, ext), axis.at(b, ext)),
                    Face::Right => (axis.at(a, int), axis.at(b, int)),
                    Face::Start => (axis.at(a, int), axis.at(a, ext)),
                    Face::End => (axis.at(b, int), axis.at(b, ext)),
                })
            }
            (Some(e @ (Element::Door(o) | Element::Window(o))), face) => {
                let what = format!("{} {}", e.category(), r.id);
                let (Some(w), Some(t)) = (self.wall(o.host), self.opening_type(e)) else {
                    return Err(format!("{what} has no wall or no type to measure"));
                };
                let along = match face {
                    Face::Centre => o.offset,
                    Face::Start => o.offset - t.width / 2.0,
                    Face::End => o.offset + t.width / 2.0,
                    Face::Left | Face::Right => {
                        return Err(format!("{what} has a centre and two jambs, start and end; measure to one of those"));
                    }
                };
                let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                Ok((axis.at(along, int), axis.at(along, ext)))
            }
            (Some(e), _) => Err(format!("{} {} isn't a wall, a door, a window or a grid", e.category(), r.id)),
            (None, _) => Err(format!("element {} doesn't exist", r.id)),
        }
    }

    /// The lines dimensions measure to on element `id`: a wall's centre line, faces and ends, a
    /// door's or a window's centre and jambs, a grid's line.
    pub fn reference_lines(&self, id: Id) -> Vec<(Reference, Line)> {
        let faces: &[Face] = match self.elements.get(&id) {
            Some(Element::Wall(_)) => &[Face::Centre, Face::Left, Face::Right, Face::Start, Face::End],
            Some(Element::Door(_) | Element::Window(_)) => &[Face::Centre, Face::Start, Face::End],
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

    /// Of element `id`'s reference lines that `keep` takes, the one nearest `p`, and how far it is.
    pub fn reference_at(&self, id: Id, p: P2, keep: impl Fn(Line) -> bool) -> Option<(Reference, f64)> {
        self.reference_lines(id)
            .into_iter()
            .filter(|(_, l)| keep(*l))
            .map(|(r, (a, b))| (r, geom::dist_to_segment(p, a, b)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// The circle a radial dimension measures on `r`: a round column's centre and radius, or a
    /// curved wall's, of its centre line or one of its faces.
    pub fn reference_arc(&self, r: &Reference) -> Result<(P2, f64), String> {
        let what = "a radial dimension measures round columns and curved walls";
        match self.elements.get(&r.id) {
            Some(Element::Column(c)) => match self.types.column.get(&c.type_name) {
                Some(t) if t.round => Ok((c.at, t.width / 2.0)),
                _ => Err(format!("column {} isn't round; {what}", r.id)),
            },
            Some(Element::Wall(w)) => {
                let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                let off = match r.face {
                    Face::Left => ext,
                    Face::Right => int,
                    _ => (ext + int) / 2.0,
                };
                let c = axis.centre().ok_or_else(|| format!("wall {} is straight; {what}", r.id))?;
                Ok((c, axis.at(0.0, off).dist(c)))
            }
            Some(e) => Err(format!("{} {} isn't round; {what}", e.category(), r.id)),
            None => Err(format!("element {} doesn't exist", r.id)),
        }
    }

    /// An angular dimension's arc: where its two lines meet, the angle (radians, counter-clockwise
    /// from east) of the ray it starts on, and how far it sweeps counter-clockwise to the other:
    /// the one of the four angles the lines make that holds `at`.
    pub fn angle_sector(&self, d: &Dimension) -> Option<(P2, f64, f64)> {
        let [r1, r2] = d.refs.as_slice() else { return None };
        let ((a, b), (c, e)) = (self.reference_line(r1).ok()?, self.reference_line(r2).ok()?);
        let o = geom::line_intersection(a, b, c, e)?;
        let at = d.at - o;
        let theta = at.1.atan2(at.0);
        let rays = |p: P2, q: P2| {
            let u = q - p;
            [u.1.atan2(u.0), (-u.1).atan2(-u.0)]
        };
        for s in rays(a, b) {
            for t in rays(c, e) {
                for (from, to) in [(s, t), (t, s)] {
                    let sweep = (to - from).rem_euclid(TAU);
                    if sweep < PI && (theta - from).rem_euclid(TAU) <= sweep {
                        return Some((o, from, sweep));
                    }
                }
            }
        }
        None
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

    /// What a dimension shows: an aligned one's lengths, segment by segment (west to east, or
    /// south to north); an angular one's angle, in degrees; a radial one's radius.
    pub fn dimension_values(&self, d: &Dimension) -> Vec<f64> {
        match d.kind {
            DimKind::Aligned => self
                .crossings(d)
                .1
                .windows(2)
                .filter_map(|w| match w {
                    [x, y] => Some(y.0 - x.0),
                    _ => None,
                })
                .collect(),
            DimKind::Angular => self.angle_sector(d).map(|(_, _, sweep)| sweep.to_degrees()).into_iter().collect(),
            DimKind::Radial => d.refs.first().and_then(|r| self.reference_arc(r).ok()).map(|(_, r)| r).into_iter().collect(),
        }
    }

    /// What a spot elevation placed in the plan of `level` at `p` reads: the highest top there of
    /// what stands on the level.
    pub fn spot_host(&self, level: Id, p: P2) -> Option<Id> {
        // A floor plan looks down, under the ceilings.
        let on = |id: &Id| self.elements.get(id).is_some_and(|e| self.level_of(e) == Some(level) && !matches!(e, Element::Ceiling(_)));
        crate::view3d::tops_at(self, p).into_iter().filter(|(id, _)| on(id)).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// What a spot elevation reads: in a plan, the top of its host at its point (none while its
    /// host isn't there); in a section or an elevation, its point's height.
    pub fn spot_value(&self, sp: &Spot) -> Option<f64> {
        // ponytail: builds every solid per spot, per drawing; share one set of solids if plans carry many spots.
        match self.elements.get(&sp.view)? {
            Element::Level(_) => crate::view3d::tops_at(self, sp.at).get(&sp.host?).copied(),
            Element::Section(_) | Element::Elevation(_) => Some(sp.at.1),
            _ => None,
        }
    }

    /// Centre lines of the walls that bound the rooms of `level`, arcs as chords.
    pub fn bounding_lines(&self, level: Id) -> Vec<(P2, P2)> {
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(w) if self.bounding(w, level)))
            .flat_map(|(&id, _)| {
                let line = walls::centre_line(self, id);
                line.windows(2).filter_map(|s| s.first().zip(s.get(1)).map(|(a, b)| (*a, *b))).collect::<Vec<_>>()
            })
            .collect()
    }

    /// The faces of the walls that bound the rooms of `level`: the edges of their outlines.
    pub fn face_lines(&self, level: Id) -> Vec<(P2, P2)> {
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(w) if self.bounding(w, level)))
            .flat_map(|(&id, _)| geom::ring_edges(&walls::outline(self, id)).collect::<Vec<_>>())
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
            if t.layers.len() > MAX_LAYERS {
                return Err(format!("wall type `{n}`: more than {MAX_LAYERS} layers"));
            }
            for (i, l) in t.layers.iter().enumerate() {
                let what = format!("wall type `{n}` layer {}", i + 1);
                text(&what, "material", &l.material)?;
                match l.function {
                    Function::Membrane if l.thickness == 0.0 => {}
                    _ => size(&format!("{what} thickness"), l.thickness)?,
                }
            }
            let sum: f64 = t.layers.iter().map(|l| l.thickness).sum();
            if !t.layers.is_empty() && (sum - t.thickness).abs() > 1e-6 {
                return Err(format!("wall type `{n}`: its layers add up to {sum} mm, not its thickness, {} mm", t.thickness));
            }
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
        for (n, t) in &self.types.railing {
            size(&format!("railing type `{n}` height"), t.height)?;
            if !(t.spacing == 0.0 || (50.0..=MAX_COORD).contains(&t.spacing)) {
                return Err(format!("railing type `{n}`: spacing {} mm; balusters stand 50 mm apart or more, or 0 for none", t.spacing));
            }
        }
        for (n, t) in &self.types.ceiling {
            size(&format!("ceiling type `{n}` thickness"), t.thickness)?;
            // A finer grid would draw a line every few pixels, and millions over a large ceiling.
            if !(t.grid == 0.0 || (100.0..=MAX_COORD).contains(&t.grid)) {
                return Err(format!("ceiling type `{n}`: grid {} mm; its tiles are 100 mm or wider, or 0 for none", t.grid));
            }
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
            // A view annotation is drawn in: a plan, a section or an elevation, or with `sheets`, a sheet.
            let view_ok = |v: Id, sheets: bool| match self.elements.get(&v) {
                Some(Element::Level(_) | Element::Section(_) | Element::Elevation(_)) => Ok(()),
                Some(Element::Sheet(_)) if sheets => Ok(()),
                Some(other) => Err(format!(
                    "{who}: {} {v} isn't a plan, a section, an elevation{}",
                    other.category(),
                    if sheets { " or a sheet" } else { "" }
                )),
                None => Err(format!("{who}: view {v} doesn't exist")),
            };
            match e {
                Element::Level(l) => {
                    text(&who, "name", &l.name)?;
                    coord(&format!("{who} elevation"), l.elevation)?;
                    if !level_names.insert(l.name.as_str()) {
                        return Err(format!("{who}: another level is already named `{}`", l.name));
                    }
                    let r = &l.view_range;
                    for (what, v) in [("cut", r.cut), ("bottom", r.bottom), ("depth", r.depth)].into_iter().chain(r.top.map(|t| ("top", t)))
                    {
                        coord(&format!("{who} view range {what}"), v)?;
                    }
                    if !(r.depth <= r.bottom && r.bottom <= r.cut && r.top.is_none_or(|t| r.cut <= t)) {
                        return Err(format!(
                            "{who}: its plan's view range runs top ≥ cut plane ≥ bottom ≥ view depth, not top {}, cut {}, bottom {}, \
                             depth {}",
                            r.top.map_or_else(|| "at the level above".into(), |t| t.to_string()),
                            r.cut,
                            r.bottom,
                            r.depth
                        ));
                    }
                    if let Some([a, b]) = l.crop {
                        crop(&who, a, b)?;
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
                    // An arc within reach: all of its circle inside the model's limits.
                    let axis = walls::Axis::of(w);
                    let reach = axis.centre().zip(axis.radius()).is_none_or(|(c, r)| c.0.abs().max(c.1.abs()) + r <= MAX_COORD);
                    if !(w.arc.is_finite() && w.arc.abs() < 360.0 && reach) {
                        return Err(format!("{who}: arc {}° isn't between -360° and 360°, reaching no farther than 1000 km", w.arc));
                    }
                    for (what, at) in [("attach_top", w.attach_top), ("attach_base", w.attach_base)] {
                        if let Some(a) = at
                            && !matches!(self.elements.get(&a), Some(Element::Roof(_) | Element::Floor(_)))
                        {
                            return Err(format!("{who}: {what} {a} isn't a roof or a floor"));
                        }
                    }
                    match w.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), w.height)?,
                    }
                    if self.top_z(w) <= self.base_z(w) + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                }
                Element::Floor(f) => {
                    known(&who, &f.type_name, &self.types.floor)?;
                    level_ok(f.level)?;
                    boundary(&who, &f.boundary)?;
                    coord(&format!("{who} offset"), f.offset)?;
                    if !(f.slope.is_finite() && f.slope.abs() <= MAX_SLOPE) {
                        return Err(format!("{who}: slope {}° isn't between -{MAX_SLOPE}° and {MAX_SLOPE}°", f.slope));
                    }
                    match f.arrow {
                        Some([tail, head]) => line(&format!("{who} arrow"), tail, head)?,
                        None if f.slope != 0.0 => {
                            return Err(format!(
                                "{who}: a sloped floor needs a slope arrow, from where its top is at its offset toward where it rises"
                            ));
                        }
                        None => {}
                    }
                }
                Element::Ceiling(c) => {
                    known(&who, &c.type_name, &self.types.ceiling)?;
                    level_ok(c.level)?;
                    boundary(&who, &c.boundary)?;
                    coord(&format!("{who} offset"), c.offset)?;
                }
                Element::Shaft(sh) => {
                    level_ok(sh.level)?;
                    boundary(&who, &sh.boundary)?;
                    coord(&format!("{who} base_offset"), sh.base_offset)?;
                    match sh.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), sh.height)?,
                    }
                    let (bottom, top) = self.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
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
                    let length = crate::walls::Axis::of(w).length();
                    if a < -TOL || b > length + TOL {
                        return Err(format!(
                            "{who}: {} mm wide at offset {} doesn't fit wall {}, {length} mm long",
                            t.width, o.offset, o.host
                        ));
                    }
                    // Up to the wall's top over it, which a roof it's attached to may raise.
                    let top = [a, o.offset, b].map(|s| crate::walls::top_at(self, w, s)).into_iter().fold(f64::INFINITY, f64::min);
                    if o.sill < 0.0 || self.own_base(w) + o.sill + t.height > top + TOL {
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
                    match (d.kind, d.refs.len()) {
                        (DimKind::Aligned, 2..=MAX_POINTS) | (DimKind::Angular, 2) | (DimKind::Radial, 1) => {}
                        (DimKind::Aligned, _) => return Err(format!("{who}: a dimension measures between 2 to {MAX_POINTS} references")),
                        (DimKind::Angular, _) => return Err(format!("{who}: an angular dimension measures between two lines")),
                        (DimKind::Radial, _) => return Err(format!("{who}: a radial dimension measures one round element")),
                    }
                    let mut seen = BTreeSet::new();
                    let mut first: Option<(Reference, Line)> = None;
                    for r in &d.refs {
                        if d.kind == DimKind::Radial {
                            self.reference_arc(r).map_err(|e| format!("{who}: {e}"))?;
                            continue;
                        }
                        let line = self.reference_line(r).map_err(|e| format!("{who}: {e}"))?;
                        let what = format!("{} {}", self.cat(r.id), r.id);
                        if !seen.insert((r.id, r.face)) {
                            return Err(format!("{who}: it measures to {what}'s {} twice", r.face.name()));
                        }
                        match first {
                            None => first = Some((*r, line)),
                            Some((f, (a, b))) => {
                                let parallel = geom::parallel(a, b, line.0, line.1);
                                let other = format!("{} {}", self.cat(f.id), f.id);
                                if d.kind == DimKind::Aligned && !parallel {
                                    return Err(format!(
                                        "{who}: {what} isn't parallel to {other}; a dimension measures between parallel walls and grids"
                                    ));
                                }
                                if d.kind == DimKind::Angular && parallel {
                                    return Err(format!("{who}: {what} is parallel to {other}; an angle is between lines that meet"));
                                }
                            }
                        }
                    }
                }
                Element::Text(n) => {
                    view_ok(n.view, true)?;
                    point(&who, n.at)?;
                    text(&who, "text", &n.text)?;
                    if n.text.trim().is_empty() {
                        return Err(format!("{who}: a text note needs text"));
                    }
                    if !(n.size > 0.0 && n.size <= 100.0) {
                        return Err(format!("{who}: size {} mm isn't above 0 and at most 100", n.size));
                    }
                    coord(&format!("{who} angle"), n.angle)?;
                    if let Some(l) = n.leader {
                        point(&format!("{who} leader"), l)?;
                    }
                }
                Element::Spot(sp) => {
                    view_ok(sp.view, false)?;
                    point(&who, sp.at)?;
                    match (self.level(sp.view), sp.host) {
                        (Some(_), None) => return Err(format!("{who}: a spot elevation in a plan reads an element's top; give `host`")),
                        (_, Some(h)) if !self.elements.contains_key(&h) => return Err(format!("{who}: element {h} doesn't exist")),
                        _ => {}
                    }
                }
                Element::Schedule(sc) => {
                    text(&who, "name", &sc.name)?;
                    if sc.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    if !crate::schedule::VIEWS.iter().any(|(c, _)| *c == crate::commands::category(&sc.of)) {
                        let names: Vec<&str> = crate::schedule::VIEWS.iter().map(|(c, _)| *c).collect();
                        return Err(format!("{who}: there's no schedule of `{}`; there are: {}", sc.of, names.join(", ")));
                    }
                    if !view_names.insert(sc.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", sc.name));
                    }
                }
                Element::View3D(v) => {
                    text(&who, "name", &v.name)?;
                    if v.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    coord(&format!("{who} azimuth"), v.azimuth)?;
                    if !(-89.0..=89.0).contains(&v.elevation) {
                        return Err(format!("{who}: elevation {}° isn't between -89° and 89°", v.elevation));
                    }
                    if !view_names.insert(v.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", v.name));
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
                    if let Some([bottom, top]) = v.crop {
                        coord(&format!("{who} crop"), bottom)?;
                        coord(&format!("{who} crop"), top)?;
                        if bottom >= top - TOL {
                            return Err(format!("{who}: its crop runs from a bottom up to a top above it, not from {bottom} to {top}"));
                        }
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
                    landings(&who, &st.landings, Some(f64::from(n)))?;
                }
                Element::Ramp(r) => {
                    level_ok(r.level)?;
                    point(&who, r.start)?;
                    coord(&format!("{who} angle"), r.angle)?;
                    match r.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), r.height)?,
                    }
                    size(&format!("{who} width"), r.width)?;
                    if !(1.0..=50.0).contains(&r.slope) {
                        return Err(format!("{who}: slope {}% isn't from 1% to 50%", r.slope));
                    }
                    let (bottom, top) = self.span(r.level, 0.0, r.top_level, r.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                    coord(&format!("{who} length"), (top - bottom) / r.slope * 100.0)?;
                    landings(&who, &r.landings, None)?;
                    if let Some(l) = r.landings.last().filter(|l| l.at >= top - bottom - TOL) {
                        return Err(format!("{who}: a landing {} mm up isn't below its top, {} mm up", l.at, top - bottom));
                    }
                }
                Element::Railing(rl) => {
                    let t = known(&who, &rl.type_name, &self.types.railing)?;
                    level_ok(rl.level)?;
                    coord(&format!("{who} base_offset"), rl.base_offset)?;
                    match (rl.host, rl.side) {
                        (Some(h), Some(_)) => {
                            if !matches!(self.elements.get(&h), Some(Element::Stair(_) | Element::Ramp(_))) {
                                return Err(format!("{who}: host {h} isn't a stair or a ramp"));
                            }
                        }
                        (Some(_), None) => {
                            return Err(format!("{who}: a railing on a stair or a ramp runs along its left or its right side"));
                        }
                        (None, _) => {
                            if rl.path.len() < 2 || rl.path.len() > MAX_POINTS {
                                return Err(format!("{who}: a path needs 2 to {MAX_POINTS} points"));
                            }
                            for p in &rl.path {
                                point(&who, *p)?;
                            }
                        }
                    }
                    let line = crate::stairs::railing(self, rl);
                    let length: f64 = line.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.0.dist(b.0))).sum();
                    if length <= TOL {
                        return Err(format!("{who}: its path has no length"));
                    }
                    if t.spacing > 0.0 && length / t.spacing > MAX_BALUSTERS {
                        return Err(format!("{who}: {:.0} balusters, more than {MAX_BALUSTERS}; space them wider", length / t.spacing));
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
                    if !(0.0..=MAX_SLOPE).contains(&r.slope) {
                        return Err(format!("{who}: slope {}° isn't from 0° (flat) to {MAX_SLOPE}°", r.slope));
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
                        "{who}: {} {} isn't a plan, a section, an elevation, a schedule or a 3D view",
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
            return Err(format!("a view is named `{n}`, as a level is; name it otherwise"));
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

/// A crop region from its corner `a` to `b`: finite, and `a` south-west of `b` (in a section, below
/// and before it).
fn crop(who: &str, a: P2, b: P2) -> Result<(), String> {
    point(&format!("{who} crop"), a)?;
    point(&format!("{who} crop"), b)?;
    if a.0 < b.0 - TOL && a.1 < b.1 - TOL {
        Ok(())
    } else {
        Err(format!("{who}: a crop region runs from its south-west corner to its north-east one (in a section, bottom left to top right)"))
    }
}

/// Landings between runs, going up: each a run of two risers or more from the last and to the
/// next (on a stair of `risers`), after a whole number of them; or on a ramp, up from its foot;
/// each turning square or back.
fn landings(who: &str, landings: &[Landing], risers: Option<f64>) -> Result<(), String> {
    if landings.len() > 100 {
        return Err(format!("{who}: more than 100 landings"));
    }
    let mut last = 0.0;
    for l in landings {
        coord(&format!("{who} landing"), l.at)?;
        if !matches!(l.turn, 0.0 | 90.0 | -90.0 | 180.0 | -180.0) {
            return Err(format!(
                "{who}: a landing turns 0 (straight on), 90 or -90 (left or right), or 180 or -180 (back beside the run, to its left or \
                 its right), not {}",
                l.turn
            ));
        }
        match risers {
            Some(_) if l.at.fract() != 0.0 => return Err(format!("{who}: a landing goes after a whole number of risers, not {}", l.at)),
            Some(n) if l.at < last + 2.0 || l.at > n - 2.0 => {
                return Err(format!(
                    "{who}: a landing after {} risers leaves a run of fewer than 2 risers; each run needs 2 or more",
                    l.at
                ));
            }
            None if l.at <= last + TOL => {
                return Err(format!("{who}: a landing {} mm up isn't above its foot or the landing before it", l.at));
            }
            _ => {}
        }
        last = l.at;
    }
    Ok(())
}

/// `ring` counter-clockwise.
fn ccw(ring: &[P2]) -> Vec<P2> {
    if geom::area(ring) < 0.0 { ring.iter().rev().copied().collect() } else { ring.to_vec() }
}

/// An outline: 3 to [`MAX_POINTS`] points around some area.
fn boundary(who: &str, ring: &[P2]) -> Result<(), String> {
    if ring.len() < 3 || ring.len() > MAX_POINTS {
        return Err(format!("{who}: a boundary needs 3 to {MAX_POINTS} points"));
    }
    for p in ring {
        point(who, *p)?;
    }
    if geom::area(ring).abs() <= TOL * TOL {
        return Err(format!("{who}: its boundary encloses no area"));
    }
    Ok(())
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
            arc: 0.0,
            location: Location::Centre,
            attach_top: None,
            attach_base: None,
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
    fn walls_join_where_they_meet_but_not_across_levels() {
        let mut m = Model::new("t");
        let bottom = boxed(&mut m);
        // Mitred at the corners: the outer corner where the outer faces cross.
        assert!(walls::cap(&m, bottom, false).contains(&P2(-100.0, -100.0)));
        // A partition ending mid-span on the bottom wall stops at its face.
        let t = m.insert(wall(1, P2(3000.0, 0.0), P2(3000.0, 4000.0)));
        assert_eq!(walls::cap(&m, t, false), [P2(3100.0, 100.0), P2(2900.0, 100.0)]);
        // A wall upstairs meeting the same corner doesn't join walls downstairs.
        let up = m.insert(wall(2, P2(0.0, 0.0), P2(0.0, -3000.0)));
        assert_eq!(walls::cap(&m, up, false), [P2(-100.0, 0.0), P2(100.0, 0.0)]);
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
