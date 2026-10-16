//! The Properties palette, laid out as Revit's: the Type Selector, the filter with Edit Type,
//! the parameters grouped under Constraints, Dimensions, Identity Data and Other, and Apply. It
//! shows the selected element, else what the tool places, else the view. Every edit is a
//! command (`modify`, or `type` from the Type Properties dialog), so it is checked, rolled back
//! when it breaks a rule, and undone like any other change.

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::element_json;
use bimcraft_core::model::{Element, Function, Id, Layer, Location, Model, Pattern};
use bimcraft_core::units::Units;
use egui::{
    Align, Align2, Button, Checkbox, ComboBox, Context, FontId, Frame, Grid, Id as UiId, Key, Layout, Margin, Modal, Popup, Rect, RichText,
    Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{self, t, tf};
use crate::style::{self, Icon, pal};
use crate::{BimApp, Sheet, Status, Tool};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    /// A number with no unit: an angle, a scale, a size on paper.
    Number,
    /// A length or an elevation, in the project's units.
    Length,
    /// `[x, y]`, lengths in the project's units, typed as `x, y`.
    Point,
    /// `[x, y]` on paper, in mm.
    PaperPoint,
    Flag,
    Level,
    /// A level, shown by name; read only.
    LevelName,
    /// A level, or none for a wall of unconnected height.
    TopLevel,
    /// A wall's height: typed when its top is unconnected, else what its levels make it.
    Height,
    /// A number, or nothing (a plan's top at the level above): typed, or cleared.
    Limit,
    /// Whether a view is cropped: on crops it to what it shows (`crop`), off uncrops it.
    Crop,
    /// Derived from the rest; read only.
    Shown,
    /// A length (or several), read only.
    LengthShown,
    /// A length some elements have and others don't (an extrusion roof's start and end): typed, and
    /// no row without one.
    LengthIf,
    /// An area in m², read only: shown in m² or ft².
    AreaShown,
    /// A length in m, read only.
    MetresShown,
    /// A dimension's values: lengths, or an angle's degrees.
    Values,
    /// A wall's location line: a drop-down.
    LocationLine,
    /// Whether a roof or a floor is there (a wall's attachment); read only.
    Attached,
}

use Kind::*;

/// A parameter: its group, its field as `elements` names it (a field of a field after a dot, as
/// `view_range.cut`), its label, its editor.
type Row = (&'static str, &'static str, &'static str, Kind);

/// Field `key` of element `e`'s JSON, through the dots.
fn field_of<'a>(e: &'a Value, key: &str) -> &'a Value {
    key.split('.').fold(e, |v, k| &v[k])
}

/// `{key: val}` for `modify`, nested through the dots: modify merges into fields that are objects.
fn nested(key: &str, val: Value) -> Value {
    key.rsplit('.').fold(val, |v, k| json!({ k: v }))
}

fn rows(category: &str) -> &'static [Row] {
    match category {
        "wall" => &[
            ("Constraints", "location", "Location Line", LocationLine),
            ("Constraints", "level", "Base Constraint", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "attach_base", "Base is Attached", Attached),
            ("Constraints", "top_level", "Top Constraint", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Constraints", "attach_top", "Top is Attached", Attached),
            ("Dimensions", "length", "Length", LengthShown),
            ("Dimensions", "radius", "Radius", LengthShown),
            ("Other", "start", "Start", Point),
            ("Other", "end", "End", Point),
        ],
        "door" => &[
            ("Constraints", "level", "Level", LevelName),
            ("Identity Data", "mark", "Mark", Text),
            ("Other", "host", "Host Wall", Shown),
            ("Other", "offset", "Offset", Length),
            ("Other", "flip_facing", "Flip Facing", Flag),
            ("Other", "flip_hand", "Flip Hand", Flag),
        ],
        "window" => &[
            ("Constraints", "level", "Level", LevelName),
            ("Constraints", "sill", "Sill Height", Length),
            ("Identity Data", "mark", "Mark", Text),
            ("Other", "host", "Host Wall", Shown),
            ("Other", "offset", "Offset", Length),
            ("Other", "flip_facing", "Flip Facing", Flag),
        ],
        "room" => &[
            ("Constraints", "level", "Level", Level),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "perimeter_m", "Perimeter", MetresShown),
            ("Identity Data", "number", "Number", Text),
            ("Identity Data", "name", "Name", Text),
            ("Other", "point", "Location", Point),
        ],
        "floor" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "offset", "Height Offset From Level", Length),
            ("Dimensions", "slope", "Slope (°)", Number),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "thickness", "Thickness", LengthShown),
        ],
        "ceiling" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "offset", "Height Offset From Level", Length),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "thickness", "Thickness", LengthShown),
        ],
        "shaft" => &[
            ("Constraints", "level", "Base Constraint", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Constraint", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "area_m2", "Area", AreaShown),
        ],
        "level" => &[("Constraints", "elevation", "Elevation", Length), ("Identity Data", "name", "Name", Text)],
        "grid" => &[("Identity Data", "name", "Name", Text), ("Other", "start", "Start", Point), ("Other", "end", "End", Point)],
        "dimension" => {
            &[("Dimensions", "values", "Value", Values), ("Other", "kind", "Kind", Shown), ("Other", "at", "Line Through", Point)]
        }
        "text" => &[
            ("Text", "text", "Text", Text),
            ("Graphics", "size", "Text Size (mm)", Number),
            ("Other", "angle", "Rotation (°)", Number),
            ("Other", "at", "Location", Point),
        ],
        "spot" => {
            &[("Dimensions", "elevation", "Elevation", LengthShown), ("Other", "host", "Host", Shown), ("Other", "at", "Location", Point)]
        }
        "schedule" => &[("Identity Data", "name", "View Name", Text), ("Other", "of", "Category", Shown)],
        "view3d" => &[
            ("Identity Data", "name", "View Name", Text),
            ("Camera", "azimuth", "Azimuth (°)", Number),
            ("Camera", "elevation", "Elevation (°)", Number),
        ],
        "tag" => {
            &[("Graphics", "leader", "Leader Line", Flag), ("Other", "host", "Tagged Element", Shown), ("Other", "offset", "Offset", Point)]
        }
        "sheet" => &[
            ("Identity Data", "number", "Sheet Number", Text),
            ("Identity Data", "name", "Sheet Name", Text),
            ("Other", "size", "Size", Text),
        ],
        "viewport" => &[
            ("Identity Data", "title", "View Name", Shown),
            ("Graphics", "scale", "View Scale (1 : n)", Number),
            ("Other", "at", "Location", PaperPoint),
        ],
        "column" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Other", "at", "Location", Point),
            ("Other", "angle", "Rotation (°)", Number),
        ],
        "stair" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "width", "Actual Run Width", Length),
            ("Dimensions", "riser", "Maximum Riser Height", Length),
            ("Dimensions", "tread", "Actual Tread Depth", Length),
            ("Dimensions", "risers", "Actual Number of Risers", Shown),
            ("Dimensions", "riser_height", "Actual Riser Height", LengthShown),
            ("Other", "start", "Start", Point),
        ],
        "ramp" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "width", "Width", Length),
            ("Dimensions", "slope", "Max Slope (%)", Number),
            ("Dimensions", "length", "Length", LengthShown),
            ("Other", "start", "Start", Point),
        ],
        "railing" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "host", "Host", Shown),
            ("Constraints", "side", "Side", Shown),
            ("Dimensions", "height", "Height", LengthShown),
        ],
        "roof" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "offset", "Base Offset", Length),
            ("Constraints", "extrusion.from", "Extrusion Start", LengthIf),
            ("Constraints", "extrusion.to", "Extrusion End", LengthIf),
            ("Dimensions", "slope", "Slope (°)", Number),
            ("Dimensions", "ridge", "Max Ridge Height", LengthShown),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Other", "gable", "Gable Ends", Flag),
        ],
        "section" | "elevation" => &[
            ("Identity Data", "name", "View Name", Text),
            ("Extents", "crop", "Crop View", Crop),
            ("Extents", "depth", "Far Clip Offset", Length),
            ("Other", "start", "Start", Point),
            ("Other", "end", "End", Point),
        ],
        _ => &[],
    }
}

/// A floor plan's parameters, read from and written to its level.
const PLAN: &[Row] = &[
    ("Identity Data", "name", "View Name", Text),
    ("Extents", "id", "Associated Level", LevelName),
    ("Extents", "elevation", "Level Elevation", Length),
    ("Extents", "crop", "Crop View", Crop),
    ("View Range", "view_range.top", "Top", Limit),
    ("View Range", "view_range.cut", "Cut Plane", Length),
    ("View Range", "view_range.bottom", "Bottom", Length),
    ("View Range", "view_range.depth", "View Depth", Length),
];

/// A type's parameters, as its Type Properties list them: its field, its label.
fn type_rows(category: &str) -> &'static [(&'static str, &'static str)] {
    match category {
        "wall" => &[("thickness", "Width")],
        "floor" => &[("thickness", "Default Thickness")],
        "door" => &[("width", "Width"), ("height", "Height")],
        "window" => &[("width", "Width"), ("height", "Height"), ("sill", "Default Sill Height")],
        "column" => &[("width", "Width"), ("depth", "Depth")],
        "roof" => &[("thickness", "Thickness")],
        "ceiling" => &[("thickness", "Thickness"), ("grid", "Tile Size")],
        "railing" => &[("height", "Height"), ("spacing", "Baluster Spacing")],
        _ => &[],
    }
}

fn icon(category: &str) -> Icon {
    match category {
        "wall" => Icon::Wall,
        "door" => Icon::Door,
        "window" => Icon::Window,
        "room" => Icon::Room,
        "level" => Icon::Level,
        "grid" => Icon::Grid,
        "dimension" => Icon::Dimension,
        "section" => Icon::Section,
        "sheet" => Icon::Sheet,
        "viewport" => Icon::PlaceView,
        "elevation" => Icon::Elevation,
        "tag" => Icon::Tag,
        "schedule" => Icon::Schedule,
        "3d view" => Icon::View3D,
        "column" => Icon::Column,
        "stair" => Icon::Stair,
        "roof" => Icon::Roof,
        "floor" => Icon::Floor,
        "ceiling" | "ceiling plan" => Icon::Ceiling,
        "shaft" => Icon::Shaft,
        "ramp" => Icon::Ramp,
        "railing" => Icon::Railing,
        "text" => Icon::Text,
        "spot" => Icon::Spot,
        "view3d" => Icon::View3D,
        _ => Icon::Plan,
    }
}

/// A value as the palette and schedules show it: numbers to the hundredth.
pub(crate) fn text(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map_or_else(String::new, |x| format!("{}", (x * 100.0).round() / 100.0 + 0.0)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Bool(b) => (if *b { "Yes" } else { "No" }).into(),
        Value::Null => String::new(),
        Value::Object(_) => v.to_string(),
    }
}

pub(crate) fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// A value of `kind` as the palette shows it, lengths and areas in `units`.
fn shown(kind: Kind, v: &Value, units: Units) -> String {
    let each = |f: &dyn Fn(f64) -> String| match v {
        Value::Array(a) => a.iter().filter_map(Value::as_f64).map(f).collect::<Vec<_>>().join(", "),
        v => v.as_f64().map_or_else(String::new, f),
    };
    match kind {
        Length | LengthIf | Height | Limit | Point | LengthShown | Values => each(&|x| units.length(x)),
        MetresShown => each(&|x| units.length(x * 1000.0)),
        AreaShown => each(&|x| {
            let (area, unit) = units.area(x * 1e6);
            format!("{area:.2} {unit}")
        }),
        _ => text(v),
    }
}

/// What was typed into a field of `kind`, as the value `modify` takes: lengths in `units`.
fn parse(kind: Kind, s: &str, units: Units) -> Result<Value, String> {
    let length = |typed: &str| units.parse(typed).ok_or_else(|| tf("`{}` is not a length", &[typed.trim()]));
    match kind {
        Number => number(s).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a number", &[s])),
        Length | LengthIf | Height => length(s).map(|x| json!(x)),
        Limit if s.trim().is_empty() => Ok(Value::Null),
        Limit => length(s).map(|x| json!(x)),
        // Lengths with feet and inches hold spaces: a comma parts x from y.
        Point => match s.split(',').collect::<Vec<_>>()[..] {
            [x, y] => Ok(json!([length(x)?, length(y)?])),
            _ => Err(tf("`{}` is not a point; type x, y", &[s])),
        },
        PaperPoint => match s.split([',', ' ']).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>()[..] {
            [x, y] if number(x).is_some() && number(y).is_some() => Ok(json!([number(x), number(y)])),
            _ => Err(tf("`{}` is not a point; type x, y in mm", &[s])),
        },
        _ => Ok(json!(s)),
    }
}

/// A drop-down of `options`, named by `label`; the one picked, if it changed.
fn combo(ui: &mut Ui, salt: (Id, &str), label: UiId, current: &str, options: &[String]) -> Option<String> {
    let mut picked = None;
    let c = ComboBox::from_id_salt(salt).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(o == current, o).clicked() && o != current {
                picked = Some(o.clone());
            }
        }
    });
    c.response.labelled_by(label);
    picked
}

/// A parameter group's header: a bar across the palette that folds the group, as Revit's do.
/// Whether it was clicked.
fn group_header(ui: &mut Ui, group: &str, open: bool) -> bool {
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, group));
    let p = ui.painter();
    p.rect_filled(r, 0.0, if response.hovered() { pal().hover_fill } else { pal().bar });
    p.text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, group, FontId::proportional(13.0), pal().text);
    // A chevron on the right, pointing down while the group is open.
    let c = pos2(r.right() - 12.0, r.center().y);
    let chevron =
        if open { [vec2(-4.0, -2.0), vec2(0.0, 2.0), vec2(4.0, -2.0)] } else { [vec2(-2.0, -4.0), vec2(2.0, 0.0), vec2(-2.0, 4.0)] };
    p.add(Shape::line(chevron.iter().map(|d| c + *d).collect(), Stroke::new(1.5, pal().icon)));
    if response.has_focus() {
        p.rect_stroke(r, 0.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    response.clicked()
}

/// What the palette shows.
#[derive(Clone, Copy, PartialEq)]
enum Subject {
    Element(Id),
    /// Several selected elements, all of a category.
    Elements(&'static str),
    /// Several selected elements of different categories: how many.
    Mixed(usize),
    /// The elements a tool places, of a category.
    New(&'static str),
    Plan(Id),
    CeilingPlan,
    View3D,
    Schedule,
}

/// A type's layer as typed: what it does, what it's made of, how thick.
type LayerRow = (Function, String, String);

/// The categories whose types have layers: a wall's from its exterior in, the others' from the top down.
const LAYERED: [&str; 4] = ["wall", "floor", "roof", "ceiling"];

/// Each pattern as the Materials window names it.
const PATTERNS: [(Pattern, &str); 10] = [
    (Pattern::None, "None"),
    (Pattern::Solid, "Solid Fill"),
    (Pattern::Diagonal, "Diagonal"),
    (Pattern::Crosshatch, "Crosshatch"),
    (Pattern::Horizontal, "Horizontal"),
    (Pattern::Vertical, "Vertical"),
    (Pattern::Grid, "Square Grid"),
    (Pattern::Brick, "Brick"),
    (Pattern::Concrete, "Concrete"),
    (Pattern::Insulation, "Insulation"),
];

fn pattern_name(p: Pattern) -> &'static str {
    PATTERNS.iter().find(|(q, _)| *q == p).map_or("None", |(_, n)| n)
}

/// The Materials window: the library's materials, the one picked to edit, its name as typed (a
/// rename once typed in), its colour as picked (applied once the mouse lets go).
pub(crate) struct MaterialsDialog {
    picked: String,
    name: String,
    color: Option<[u8; 3]>,
}

impl MaterialsDialog {
    pub(crate) fn new(m: &Model) -> MaterialsDialog {
        let picked = m.materials.keys().next().cloned().unwrap_or_default();
        MaterialsDialog { name: picked.clone(), picked, color: None }
    }
}

/// What a layer does, as the layer editor names it.
const FUNCTIONS: [(Function, &str); 5] = [
    (Function::Structure, "Structure"),
    (Function::Substrate, "Substrate"),
    (Function::Thermal, "Thermal"),
    (Function::Membrane, "Membrane"),
    (Function::Finish, "Finish"),
];

fn function_name(f: Function) -> &'static str {
    FUNCTIONS.iter().find(|(g, _)| *g == f).map_or("Structure", |(_, n)| n)
}

/// The Type Properties dialog: a type of a category, and its parameters as typed; a layered
/// type's layers, too.
pub(crate) struct TypeDialog {
    category: &'static str,
    name: String,
    /// The elements that were on the type when the dialog opened; OK puts them on the type shown.
    elements: Vec<Id>,
    fields: Vec<(&'static str, &'static str, String)>,
    /// The name being typed for a duplicate.
    duplicate: Option<String>,
    /// A layered type's layers, exterior or top first, as typed, and as they were: changed, they
    /// make it.
    layers: Vec<LayerRow>,
    layers_were: Vec<LayerRow>,
}

impl TypeDialog {
    pub(crate) fn new(m: &Model, category: &'static str, name: String, elements: Vec<Id>) -> TypeDialog {
        let (fields, layers) = (TypeDialog::fields(m, category, &name), TypeDialog::layers(m, category, &name));
        TypeDialog { category, name, elements, fields, duplicate: None, layers_were: layers.clone(), layers }
    }

    /// The dialog showing type `name` as the model has it now.
    fn show(&mut self, m: &Model, name: String) {
        *self = TypeDialog::new(m, self.category, name, std::mem::take(&mut self.elements));
    }

    /// A type's sizes, as typed: lengths in the project's units.
    fn fields(m: &Model, category: &str, name: &str) -> Vec<(&'static str, &'static str, String)> {
        let types = serde_json::to_value(&m.types).unwrap_or_default();
        type_rows(category).iter().map(|(k, l)| (*k, *l, shown(Length, &types[category][name][k], m.units))).collect()
    }

    /// A layered type's layers, as typed: one structure layer as thick as it, without any.
    fn layers(m: &Model, category: &str, name: &str) -> Vec<LayerRow> {
        let Some((.., thickness, layers)) = m.layered_types().find(|(c, n, ..)| *c == category && *n == name) else { return Vec::new() };
        let typed = |l: &Layer| (l.function, l.material.clone(), m.units.length(l.thickness));
        match layers {
            [] => vec![(Function::Structure, String::new(), m.units.length(thickness))],
            layers => layers.iter().map(typed).collect(),
        }
    }

    /// The `type` command's parameters for the type as typed, or why they aren't lengths. Changed
    /// layers make the type, and how thick it is; `copy` sends them anyway, as a duplicate has them.
    fn params(&self, name: &str, units: Units, copy: bool) -> Result<Value, String> {
        let mut p = json!({"category": self.category, "name": name});
        let layered = copy || self.layers != self.layers_were;
        if layered {
            let layers = self.layers.iter().map(|(function, material, typed)| {
                let thickness = units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[t("Thickness"), typed]))?;
                Ok(json!({"function": function, "material": material.trim(), "thickness": thickness}))
            });
            p["layers"] = Value::Array(layers.collect::<Result<_, String>>()?);
        }
        for (key, label, typed) in self.fields.iter().filter(|(key, ..)| !(layered && *key == "thickness")) {
            p[*key] = json!(units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[t(label), typed]))?);
        }
        Ok(p)
    }
}

impl BimApp {
    fn subject(&self) -> Subject {
        let m = &self.session.model;
        match (self.selection.as_slice(), &self.tool, self.sheet) {
            (&[id], _, _) => Subject::Element(id),
            ([_, _, ..], _, _) => {
                let categories: BTreeSet<&'static str> =
                    self.selection.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
                match categories.first() {
                    Some(c) if categories.len() == 1 => Subject::Elements(c),
                    _ => Subject::Mixed(self.selection.len()),
                }
            }
            ([], Tool::Wall(_), _) => Subject::New("wall"),
            ([], Tool::Door, _) => Subject::New("door"),
            ([], Tool::Window, _) => Subject::New("window"),
            ([], Tool::Room, _) => Subject::New("room"),
            ([], Tool::Level, _) => Subject::New("level"),
            ([], Tool::Grid(_), _) => Subject::New("grid"),
            ([], Tool::Dimension(_) | Tool::Angular(_) | Tool::Radial(_), _) => Subject::New("dimension"),
            ([], Tool::Text, _) => Subject::New("text"),
            ([], Tool::Spot, _) => Subject::New("spot"),
            ([], Tool::Tag, _) => Subject::New("tag"),
            ([], Tool::Section(_), _) => Subject::New("section"),
            ([], Tool::Elevation, _) => Subject::New("elevation"),
            ([], Tool::Column, _) => Subject::New("column"),
            ([], Tool::Stair(_), _) => Subject::New("stair"),
            ([], Tool::Ramp(_), _) => Subject::New("ramp"),
            ([], Tool::Railing(_), _) => Subject::New("railing"),
            ([], Tool::Roof | Tool::ExtrusionRoof(_), _) => Subject::New("roof"),
            ([], Tool::Dormer, _) => Subject::New("wall"),
            ([], Tool::Floor, _) => Subject::New("floor"),
            ([], Tool::Ceiling, _) => Subject::New("ceiling"),
            ([], Tool::Shaft(_), _) => Subject::New("shaft"),
            // A section's or an elevation's view shows its own parameters.
            ([], _, Sheet::Section(id) | Sheet::Paper(id)) => Subject::Element(id),
            // Modify, and the tools that pick walls rather than place anything: the view.
            ([], _, Sheet::Plan(level)) => Subject::Plan(level),
            ([], _, Sheet::Ceiling(_)) => Subject::CeilingPlan,
            ([], _, Sheet::View3D) => Subject::View3D,
            ([], _, Sheet::Schedule(_)) => Subject::Schedule,
        }
    }

    /// The type the selected elements share, if they do.
    fn shared_type(&self, m: &Model) -> Option<String> {
        let names: BTreeSet<String> =
            self.selection.iter().filter_map(|id| element_json(m, *id)["type"].as_str().map(String::from)).collect();
        if names.len() == 1 { names.into_iter().next() } else { None }
    }

    /// The type Edit Type opens: its category, its name, and the element on it.
    /// The type Edit Type opens: its category, its name, and the selected elements on it.
    pub(crate) fn type_subject(&self) -> Option<(&'static str, String, Vec<Id>)> {
        let m = &self.session.model;
        let category = match self.subject() {
            Subject::Element(id) => m.elements.get(&id)?.category(),
            Subject::Elements(category) | Subject::New(category) => category,
            Subject::Mixed(_) | Subject::Plan(_) | Subject::CeilingPlan | Subject::View3D | Subject::Schedule => return None,
        };
        let name = match self.selection.is_empty() {
            false => self.shared_type(m),
            true => self.types.get(category).cloned().or_else(|| m.default_type(category).map(String::from)),
        };
        Some((category, name?, self.selection.clone()))
    }

    pub(crate) fn edit_type(&mut self) {
        if let Some((category, name, elements)) = self.type_subject() {
            self.type_dialog = Some(TypeDialog::new(&self.session.model, category, name, elements));
        }
    }

    pub(crate) fn properties(&mut self, ui: &mut Ui) {
        let model = Arc::clone(&self.session.model);
        let subject = self.subject();
        self.type_selector(ui, &model, subject);
        let (filter, ids, rows) = match subject {
            Subject::Element(id) => match model.elements.get(&id) {
                Some(e) => (format!("{} (1)", lang::category(e.category(), true)), vec![id], rows(e.category())),
                None => return,
            },
            Subject::Elements(category) => {
                (format!("{} ({})", lang::category(category, true), self.selection.len()), self.selection.clone(), rows(category))
            }
            Subject::Mixed(n) => (tf("Multiple Categories ({})", &[&n.to_string()]), Vec::new(), &[][..]),
            Subject::New(category) => (tf("New {}", &[&lang::category(category, true)]), Vec::new(), &[][..]),
            Subject::Plan(level) => (self.view_title(Sheet::Plan(level)), vec![level], PLAN),
            Subject::CeilingPlan | Subject::View3D | Subject::Schedule => (self.view_title(self.sheet), Vec::new(), &[][..]),
        };
        ui.horizontal(|ui| {
            ui.label(filter);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(self.type_subject().is_some(), Button::new(t("Edit Type"))).clicked() {
                    self.edit_type();
                }
            });
        });
        ui.add_space(4.0);
        if !ids.is_empty() {
            self.parameters(ui, &model, &ids, rows);
        }
        // A 3D view on a sheet: where it looks from, too.
        if let Subject::Element(id) = subject
            && let Some(bimcraft_core::model::Element::Viewport(vp)) = model.elements.get(&id)
            && let Some(bimcraft_core::model::Element::View3D(_)) = model.elements.get(&vp.view)
        {
            self.parameters(ui, &model, &[vp.view], self::rows("view3d"));
        }
        ui.add_space(8.0);
        // Fields commit on Enter or when they lose the keyboard, which a click here does.
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let _ = ui.button(t("Apply"));
        });
    }

    /// The Type Selector: the category and the type of what the palette shows, a drop-down of
    /// the category's types.
    fn type_selector(&mut self, ui: &mut Ui, model: &Model, subject: Subject) {
        let (category, current) = match subject {
            Subject::Element(id) => match model.elements.get(&id) {
                Some(e) => (e.category(), element_json(model, id)["type"].as_str().map(String::from)),
                None => return,
            },
            Subject::Elements(category) => (category, self.shared_type(model)),
            Subject::Mixed(_) => ("multiple categories", None),
            Subject::New(category) => {
                (category, self.types.get(category).cloned().or_else(|| model.default_type(category).map(String::from)))
            }
            Subject::Plan(_) => ("floor plan", None),
            Subject::CeilingPlan => ("ceiling plan", None),
            Subject::View3D => ("3d view", None),
            Subject::Schedule => ("schedule", None),
        };
        let names: Vec<String> = model.type_names(category).into_iter().map(String::from).collect();
        let enabled = !names.is_empty();
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), if enabled { Sense::click() } else { Sense::hover() });
        r.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, enabled, t("Type Selector")));
        let p = ui.painter();
        p.rect(
            rect,
            3.0,
            pal().surface,
            Stroke::new(1.0, if r.hovered() && enabled { pal().hover } else { pal().border }),
            StrokeKind::Inside,
        );
        let swatch = Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(40.0, 40.0));
        p.rect_filled(swatch, 2.0, pal().bar);
        style::paint(p, icon(category), swatch.shrink(8.0), pal().icon);
        let x = swatch.right() + 10.0;
        let family = match subject {
            Subject::Plan(_) => t("Floor Plan").to_string(),
            Subject::CeilingPlan => t("Ceiling Plan").to_string(),
            Subject::View3D => t("3D View").to_string(),
            Subject::Mixed(_) => t("Multiple Categories").to_string(),
            _ => lang::category(category, false),
        };
        let first = if current.is_some() { rect.top() + 16.0 } else { rect.center().y };
        p.text(egui::pos2(x, first), Align2::LEFT_CENTER, family, FontId::proportional(13.0), pal().text);
        if let Some(t) = &current {
            p.text(egui::pos2(x, rect.top() + 34.0), Align2::LEFT_CENTER, t, FontId::proportional(12.5), pal().muted);
        }
        if enabled {
            let c = egui::pos2(rect.right() - 14.0, rect.center().y);
            p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], pal().icon, Stroke::NONE));
        }
        if r.has_focus() {
            p.rect_stroke(rect, 3.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
        }
        let mut picked = None;
        Popup::menu(&r).width(rect.width()).show(|ui| {
            for n in &names {
                if ui.selectable_label(current.as_ref() == Some(n), n).clicked() {
                    picked = Some(n.clone());
                }
            }
        });
        match (picked, subject) {
            (Some(n), Subject::Element(_) | Subject::Elements(_)) if current.as_ref() != Some(&n) => {
                self.run("modify", json!({"ids": self.selection, "type": n}));
            }
            (Some(n), Subject::New(category)) => {
                self.types.insert(category, n);
            }
            _ => {}
        }
    }

    /// The element's parameters, group by group.
    /// The elements' parameters, group by group: what they share shows, what varies is blank.
    /// An edit changes all of them, in one step.
    fn parameters(&mut self, ui: &mut Ui, model: &Model, ids: &[Id], rows: &[Row]) {
        let Some(&id) = ids.first() else { return };
        let all: Vec<Value> = ids.iter().map(|id| element_json(model, *id)).collect();
        let first = element_json(model, id);
        let v: serde_json::Map<String, Value> = rows
            .iter()
            .map(|r| r.1)
            .chain(["top_level", "enclosed"])
            .map(|k| {
                let shared = all.iter().all(|e| field_of(e, k) == field_of(&first, k));
                (k.to_string(), if shared { field_of(&first, k).clone() } else { Value::Null })
            })
            .collect();
        let v = Value::Object(v);
        let levels: Vec<String> = model.levels().iter().map(|(_, l)| l.name.clone()).collect();
        let level_name = |v: &Value| v.as_u64().and_then(|l| model.level(l)).map_or_else(String::new, |l| l.name.clone());
        let mut groups: Vec<&'static str> = Vec::new();
        for (g, ..) in rows {
            if !groups.contains(g) {
                groups.push(g);
            }
        }
        let mut change: Option<(&str, Result<Value, String>)> = None;
        let mut crop = None;
        let units = model.units;
        // A note on a sheet sits in paper mm.
        let paper = matches!(model.elements.get(&id), Some(Element::Text(n)) if model.sheet(n.view).is_some());
        for group in groups {
            let open = !self.collapsed.contains(group);
            if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
                self.collapsed.insert(group);
            }
            if !open {
                continue;
            }
            Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
                Grid::new(("properties", id, group)).num_columns(2).striped(true).min_col_width(124.0).show(ui, |ui| {
                    for &(_, key, label, kind) in rows.iter().filter(|r| r.0 == group) {
                        let val = &v[key];
                        // A length one element hasn't got (a straight wall's radius): no row.
                        if ids.len() == 1 && matches!(kind, LengthShown | LengthIf) && val.is_null() {
                            continue;
                        }
                        let kind = match kind {
                            Height if !v["top_level"].is_null() => LengthShown,
                            Point if paper => PaperPoint,
                            kind => kind,
                        };
                        let label = t(label);
                        let name = ui.label(label);
                        match kind {
                            Shown => {
                                ui.label(RichText::new(text(val)).color(pal().muted));
                            }
                            // An angle's degrees; else lengths.
                            Values if v["kind"] == "angular" => {
                                ui.label(RichText::new(format!("{}°", text(val))).color(pal().muted));
                            }
                            LengthShown | AreaShown | MetresShown | Values => {
                                ui.label(RichText::new(shown(kind, val, units)).color(pal().muted));
                            }
                            LevelName => {
                                ui.label(RichText::new(level_name(val)).color(pal().muted));
                            }
                            Crop => {
                                let mut on = !val.is_null();
                                let r = ui.checkbox(&mut on, "");
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
                                if r.labelled_by(name.id).changed() {
                                    crop = Some(on);
                                }
                            }
                            Flag => {
                                let mut on = val.as_bool().unwrap_or(false);
                                let r = ui.checkbox(&mut on, "");
                                // A checkbox without text names itself "" for screen readers: give it its row's name.
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
                                if r.labelled_by(name.id).changed() {
                                    change = Some((key, Ok(json!(on))));
                                }
                            }
                            Level => {
                                if let Some(l) = combo(ui, (id, key), name.id, &level_name(val), &levels) {
                                    change = Some((key, Ok(json!(l))));
                                }
                            }
                            TopLevel => {
                                let none = t("Unconnected");
                                let options: Vec<String> = std::iter::once(none.to_string()).chain(levels.iter().cloned()).collect();
                                let current = if val.is_null() { none.to_string() } else { level_name(val) };
                                if let Some(l) = combo(ui, (id, key), name.id, &current, &options) {
                                    change = Some((key, Ok(if l == none { Value::Null } else { json!(l) })));
                                }
                            }
                            // Shown by its label; a centre line by default, which the wall's JSON leaves out.
                            LocationLine => {
                                let now = val.as_str().and_then(Location::from_name).unwrap_or_default();
                                let options: Vec<String> = Location::ALL.iter().map(|l| t(l.label()).to_string()).collect();
                                if let Some(l) = combo(ui, (id, key), name.id, t(now.label()), &options)
                                    && let Some(loc) = Location::ALL.iter().find(|loc| t(loc.label()) == l)
                                {
                                    change = Some((key, Ok(json!(loc.name()))));
                                }
                            }
                            // Revit's greyed checkbox: Attach Top/Base and Detach Top/Base change it.
                            Attached => {
                                let mut on = !val.is_null();
                                let r = ui.add_enabled(false, Checkbox::new(&mut on, ""));
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, false, on, label));
                                r.labelled_by(name.id);
                            }
                            Text | Number | Length | LengthIf | Point | PaperPoint | Height | Limit => {
                                let hint = if kind == Limit { t("Level Above") } else { "" };
                                if let Some(typed) = self.field(ui, id, key, shown(kind, val, units), name.id, hint) {
                                    change = Some((key, parse(kind, &typed, units)));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        }
        if ids.len() == 1 && v["enclosed"] == json!(false) {
            ui.label(RichText::new(t("Not enclosed: the walls around it don't close.")).color(pal().error));
        }
        // A view's crop: on to all it shows, or off.
        if let (Some(on), &[view]) = (crop, ids) {
            self.run("crop", if on { json!({"view": view}) } else { json!({"view": view, "region": null}) });
        }
        match change {
            Some((key, Ok(val))) => {
                let mut p = nested(key, val);
                p["ids"] = json!(ids);
                self.run("modify", p);
            }
            Some((key, Err(e))) => self.status = Status { text: format!("{key}: {e}"), error: true },
            None => {}
        }
    }

    /// A text field that edits as the user types and commits once, when they press Enter or
    /// click away (Escape drops the edit): the text typed, if it changed.
    fn field(&mut self, ui: &mut Ui, id: Id, key: &'static str, current: String, label: UiId, hint: &str) -> Option<String> {
        let wid = ui.make_persistent_id(("field", id, key));
        let focused = ui.memory(|m| m.has_focus(wid));
        let mut typed = match &self.edit {
            Some((i, k, t)) if focused && *i == id && *k == key => t.clone(),
            _ => current.clone(),
        };
        let r = ui.add(TextEdit::singleline(&mut typed).id(wid).desired_width(f32::INFINITY).hint_text(hint)).labelled_by(label);
        if r.has_focus() {
            self.edit = Some((id, key, typed));
            return None;
        }
        if r.lost_focus() {
            self.edit = None;
            if typed != current && !ui.input(|i| i.key_pressed(Key::Escape)) {
                return Some(typed);
            }
        }
        None
    }

    /// The Type Properties dialog, while it's open: the type's parameters, Duplicate, and OK,
    /// Cancel and Apply. Changing a type changes every element of it.
    pub(crate) fn type_properties(&mut self, ctx: &Context) {
        let Some(mut d) = self.type_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let names: Vec<String> = model.type_names(d.category).into_iter().map(String::from).collect();
        let (mut ok, mut cancel, mut apply, mut create, mut switch) = (false, false, false, false, None);
        let r = Modal::new(UiId::new("type-properties")).show(ctx, |ui| {
            // Wider for a type's layers.
            ui.set_width(if LAYERED.contains(&d.category) { 560.0 } else { 400.0 });
            ui.label(RichText::new(t("Type Properties")).heading());
            ui.add_space(6.0);
            Grid::new("type-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(t("Family:"));
                ui.label(lang::category(d.category, false));
                ui.end_row();
                let l = ui.label(t("Type:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("type-name").selected_text(&d.name).width(200.0).show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(*n == d.name, n).clicked() {
                                switch = Some(n.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if d.duplicate.is_none() && ui.button(t("Duplicate…")).clicked() {
                        d.duplicate = Some(format!("{} 2", d.name));
                    }
                });
                ui.end_row();
                if let Some(new) = &mut d.duplicate {
                    let l = ui.label(t("New type"));
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(new).desired_width(200.0)).labelled_by(l.id);
                        create = ui.button(t("Create")).clicked();
                    });
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(t("Type Parameters")).strong());
            Grid::new("type-parameters").num_columns(2).striped(true).min_col_width(150.0).show(ui, |ui| {
                ui.label(RichText::new(t("Parameter")).color(pal().muted));
                ui.label(RichText::new(t("Value")).color(pal().muted));
                ui.end_row();
                for (_, label, typed) in &mut d.fields {
                    let l = ui.label(t(label));
                    ui.add(TextEdit::singleline(typed).desired_width(160.0)).labelled_by(l.id);
                    ui.end_row();
                }
            });
            if LAYERED.contains(&d.category) {
                ui.add_space(6.0);
                let materials: Vec<String> = model.materials.keys().cloned().collect();
                let (out, inward) = if d.category == "wall" { ("Out", "In") } else { ("Up", "Down") };
                layer_editor(ui, &mut d.layers, model.units, &materials, (out, inward));
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        if let Some(n) = switch {
            d.show(&model, n);
        }
        if create && let Some(new) = d.duplicate.clone() {
            let layered = !model.type_layers(d.category, &d.name).is_empty();
            let made = match d.params(new.trim(), model.units, layered) {
                Ok(p) => self.run("type", p).is_some(),
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            if made {
                d.show(&self.session.model, new.trim().to_string());
            }
        }
        if apply || ok {
            let applied = match d.params(&d.name, model.units, false) {
                Ok(p) => self.run("type", p).is_some(),
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            ok &= applied;
            if applied {
                let name = d.name.clone();
                d.show(&Arc::clone(&self.session.model), name);
            }
        }
        if ok {
            let m = &self.session.model;
            if d.elements.is_empty() {
                self.types.insert(d.category, d.name.clone());
            } else if d.elements.iter().any(|id| element_json(m, *id)["type"].as_str() != Some(d.name.as_str())) {
                self.run("modify", json!({"ids": d.elements, "type": d.name}));
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.type_dialog = Some(d);
        }
    }
}

impl BimApp {
    /// The Materials window: Manage > Materials, Revit's Material Browser. Each change is a
    /// `material` command, undone like any other.
    pub(crate) fn materials_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.materials_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut close, mut run, mut picked): (bool, Option<Value>, Option<String>) = (false, None, None);
        let released = ctx.input(|i| !i.pointer.any_down());
        let r = Modal::new(UiId::new("materials")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(t("Materials")).heading());
            ui.add_space(6.0);
            Grid::new("material-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let l = ui.label(t("Material:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("material-name").selected_text(&d.picked).width(200.0).show_ui(ui, |ui| {
                        for name in model.materials.keys() {
                            if ui.selectable_label(*name == d.picked, name).clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if ui.button(t("New")).clicked() {
                        let fresh =
                            (1..).map(|n| format!("{} {n}", t("Material"))).find(|n| !model.materials.contains_key(n)).unwrap_or_default();
                        run = Some(json!({"name": fresh}));
                        picked = Some(fresh);
                    }
                    if ui.add_enabled(model.materials.contains_key(&d.picked), Button::new(t("Delete"))).clicked() {
                        run = Some(json!({"name": d.picked, "delete": true}));
                        picked = model.materials.keys().find(|n| **n != d.picked).cloned();
                    }
                });
                ui.end_row();
            });
            if let Some(m) = model.materials.get(&d.picked) {
                ui.add_space(6.0);
                Grid::new("material").num_columns(2).striped(true).spacing(vec2(12.0, 6.0)).min_col_width(130.0).show(ui, |ui| {
                    let l = ui.label(t("Name"));
                    let r = ui.add(TextEdit::singleline(&mut d.name).desired_width(200.0)).labelled_by(l.id);
                    if r.lost_focus() && !d.name.trim().is_empty() && d.name.trim() != d.picked {
                        run = Some(json!({"name": d.picked, "rename": d.name.trim()}));
                        picked = Some(d.name.trim().to_string());
                    }
                    ui.end_row();
                    let l = ui.label(t("Color"));
                    let colour = d.color.get_or_insert(m.color);
                    ui.color_edit_button_srgb(colour).labelled_by(l.id);
                    if released && *colour != m.color {
                        run = Some(json!({"name": d.picked, "color": *colour}));
                    }
                    ui.end_row();
                    for (key, label, now) in [("cut", "Cut Pattern", m.cut), ("surface", "Surface Pattern", m.surface)] {
                        let l = ui.label(t(label));
                        let c = ComboBox::from_id_salt(("material-pattern", key)).selected_text(t(pattern_name(now))).width(160.0).show_ui(
                            ui,
                            |ui| {
                                for (p, name) in PATTERNS {
                                    if ui.selectable_label(p == now, t(name)).clicked() {
                                        run = Some(json!({"name": d.picked, key: p}));
                                    }
                                }
                            },
                        );
                        c.response.labelled_by(l.id);
                        ui.end_row();
                    }
                });
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if let Some(p) = run {
            self.run("material", p);
        }
        if let Some(n) = picked {
            (d.name, d.picked, d.color) = (n.clone(), n, None);
        } else if !self.session.model.materials.contains_key(&d.picked) {
            d = MaterialsDialog::new(&self.session.model);
        } else if d.color.is_some_and(|c| self.session.model.materials.get(&d.picked).is_some_and(|m| m.color == c)) {
            d.color = None;
        }
        if !(close || r.should_close()) {
            self.materials_dialog = Some(d);
        }
    }
}

/// A type's layers, exterior or top first: what each does, what it's made of (from the library),
/// how thick; a new one on the outside, or on top, and each moved, or taken away.
fn layer_editor(ui: &mut Ui, layers: &mut Vec<LayerRow>, units: Units, materials: &[String], (out, inward): (&str, &str)) {
    ui.label(RichText::new(t("Layers")).strong());
    let mut action: Option<(usize, i8)> = None;
    Grid::new("type-layers").num_columns(4).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        let function = ui.label(RichText::new(t("Function")).color(pal().muted));
        let material = ui.label(RichText::new(t("Material")).color(pal().muted));
        let thickness = ui.label(RichText::new(t("Thickness")).color(pal().muted));
        ui.label("");
        ui.end_row();
        let last = layers.len().saturating_sub(1);
        for (k, (f, mat, typed)) in layers.iter_mut().enumerate() {
            let c = ComboBox::from_id_salt(("layer-function", k)).selected_text(t(function_name(*f))).width(110.0).show_ui(ui, |ui| {
                for (g, n) in FUNCTIONS {
                    if ui.selectable_label(*f == g, t(n)).clicked() {
                        *f = g;
                    }
                }
            });
            c.response.labelled_by(function.id);
            let none = t("(none)");
            let shown = if mat.is_empty() { none } else { mat.as_str() };
            let c = ComboBox::from_id_salt(("layer-material", k)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(mat.is_empty(), none).clicked() {
                    mat.clear();
                }
                for n in materials {
                    if ui.selectable_label(mat == n, n).clicked() {
                        mat.clone_from(n);
                    }
                }
            });
            c.response.labelled_by(material.id);
            ui.add_sized([70.0, 20.0], TextEdit::singleline(typed).hint_text(units.name())).labelled_by(thickness.id);
            ui.horizontal(|ui| {
                if ui.add_enabled(k > 0, Button::new(t(out)).small()).clicked() {
                    action = Some((k, -1));
                }
                if ui.add_enabled(k < last, Button::new(t(inward)).small()).clicked() {
                    action = Some((k, 1));
                }
                if ui.add_enabled(last > 0, Button::new(t("Delete")).small()).clicked() {
                    action = Some((k, 0));
                }
            });
            ui.end_row();
        }
    });
    match action {
        Some((k, 0)) if k < layers.len() => {
            layers.remove(k);
        }
        Some((k, -1)) if k > 0 => layers.swap(k, k - 1),
        Some((k, _)) if k + 1 < layers.len() => layers.swap(k, k + 1),
        _ => {}
    }
    // A finish 10 mm thick, on the exterior side.
    if ui.button(t("Insert Layer")).clicked() {
        layers.insert(0, (Function::Finish, String::new(), units.length(10.0)));
    }
}
