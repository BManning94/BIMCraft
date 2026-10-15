//! BIMCraft's desktop front end, in egui: a thin shell over the core's [`Session`]. Panels read
//! the session's model and change it only through [`Session::execute`], so the desktop, the CLI,
//! scripts and agents share one set of commands, rules and undo history. The layout is Revit's:
//! the Quick Access Toolbar over the ribbon's tabs, a contextual Modify tab for the tool or the
//! selection, the Options Bar, Properties over the Project Browser on the left, views in tabs
//! (plans, the 3D view, schedules), and the tool's prompt in the status bar.

mod canvas;
pub mod lang;
mod orbit;
mod paper;
mod properties;
mod section;
mod style;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bimcraft_core::engine::{self, Session};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model, Reference, STAIR_WIDTH, heading};
use bimcraft_core::schedule;
use bimcraft_core::units::Units;
use egui::containers::menu::{MenuButton, SubMenuButton};
use egui::{
    Align, Align2, Area, CentralPanel, CollapsingHeader, Color32, ComboBox, Context, CornerRadius, Event, FontId, Frame, Grid, Id as UiId,
    Key, KeyboardShortcut, Layout, Margin, Modal, Modifiers, Order, Panel, Popup, Pos2, Rect, Response, RichText, ScrollArea, Sense,
    Stroke, StrokeKind, TextEdit, Ui, ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use bimcraft_core::language::Language;
use lang::{t, tf};
use style::{Icon, pal};

/// A view: what the middle of the window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sheet {
    /// A level's floor plan.
    Plan(Id),
    /// The default 3D view, `{3D}`.
    View3D,
    /// A section's or an elevation's view.
    Section(Id),
    /// A sheet: paper with a title block, and views placed on it.
    Paper(Id),
    /// The schedule of a category, as the `schedule` command names it.
    Schedule(&'static str),
}

/// What clicks on the plan do.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// Select the element under the cursor.
    Modify,
    /// The points of the wall chain clicked so far.
    Wall(Vec<P2>),
    Door,
    Window,
    Room,
    /// The Options Bar asks for the new level's elevation.
    Level,
    /// The grid's start, once clicked.
    Grid(Option<P2>),
    /// Move the selection: its base point, once clicked.
    Move(Option<P2>),
    /// Copy the selection: its base point, once clicked.
    Copy(Option<P2>),
    /// Rotate the selection about `centre`: the start of the angle, once clicked; with `place`, the
    /// next click moves the centre.
    Rotate {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Mirror the selection across a line picked, or with `draw`, a line drawn from `start`.
    Mirror {
        draw: bool,
        start: Option<P2>,
    },
    /// Line an element up with a reference line, once picked.
    Align(Option<(P2, P2)>),
    /// Trim or extend two walls to a corner: the first wall, and where it was picked.
    Trim(Option<(Id, P2)>),
    /// Split the wall picked.
    Split,
    /// Copy (or move) the wall picked to the cursor's side, by the Options Bar's distance.
    Offset,
    /// An aligned dimension: the references picked so far. A click away from any places it.
    Dimension(Vec<Reference>),
    /// An angular dimension: the lines picked so far, two that meet; then a click where its arc goes.
    Angular(Vec<Reference>),
    /// A radial dimension: the round column picked, then a click toward where its radius goes.
    Radial(Option<Reference>),
    /// A text note where clicked, saying what the Options Bar says.
    Text,
    /// A spot elevation where clicked.
    Spot,
    /// Tag the door or window clicked.
    Tag,
    /// A section: its start, once clicked.
    Section(Option<P2>),
    /// An elevation, looking at the side of the building clicked beside.
    Elevation,
    /// A column where clicked.
    Column,
    /// A stair: where it starts, once clicked; the next click says which way it climbs.
    Stair(Option<P2>),
    /// A roof over the walls of the plan's level, on the level above: Revit's Roof by Footprint,
    /// with every wall picked at once.
    Roof,
}

/// Why the last command failed, shown in a box over the view.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub text: String,
    pub error: bool,
}

/// The ribbon's tabs. `Context` is the contextual Modify tab of the tool or the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tab {
    Architecture,
    Annotate,
    View,
    Manage,
    Modify,
    Context,
}

/// A step that would drop unsaved changes, waiting on the user.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    New,
    Open,
    Close,
}

/// The schedules: the category `schedule` takes, and the view's name.
pub(crate) const SCHEDULES: [(&str, &str); 6] = schedule::VIEWS;

/// Ribbon buttons that pick tools: icon, label, tooltip, tool.
type Tools<'a> = &'a [(Icon, &'a str, &'a str, Tool)];

/// The Modify panel's small buttons: icon, label, tooltip, the tool (none for Delete), enabled.
type Buttons<'a> = &'a [(Icon, &'a str, &'a str, Option<Tool>, bool)];

/// What an IFC import brought in, and the first few things it left out (the core's reasons, in
/// English).
fn import_note(file: &str, reply: &Value) -> String {
    let counts: Vec<String> = reply["elements"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(cat, n)| match n.as_u64().unwrap_or(0) {
            1 => format!("1 {}", lang::category(cat, false).to_lowercase()),
            n => format!("{n} {}", lang::category(cat, true).to_lowercase()),
        })
        .collect();
    let mut text = tf("Imported {}: {}.", &[file, &counts.join(", ")]);
    let skipped: Vec<&str> = reply["skipped"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !skipped.is_empty() {
        let more = match skipped.len().saturating_sub(3) {
            0 => String::new(),
            n => tf("; and {} more", &[&n.to_string()]),
        };
        text += &tf(" Left out: {}{}.", &[&skipped.iter().take(3).copied().collect::<Vec<_>>().join("; "), &more]);
    }
    if let Some([x, y]) = reply["moved"].as_array().map(Vec::as_slice) {
        text += &tf(" Moved {}, {} mm to bring it near the origin.", &[&x.to_string(), &y.to_string()]);
    }
    text
}

pub struct BimApp {
    pub session: Session,
    /// The active view; `views` holds every open one, as tabs.
    pub sheet: Sheet,
    pub tool: Tool,
    /// The selected elements, in the order they were picked.
    pub selection: Vec<Id>,
    pub status: Status,
    /// The interface's language: English unless told otherwise (the app asks the environment).
    pub lang: Language,
    views: Vec<Sheet>,
    tab: Tab,
    /// Where the ribbon goes back to when the contextual tab goes.
    back: Tab,
    /// The type each placing tool uses, by category; the core's default when missing.
    types: BTreeMap<&'static str, String>,
    /// The Wall tool's top: `None` as the core does it (up to the next level up), `Some(None)`
    /// unconnected at `wall_height`, `Some(Some(level))` up to that level.
    wall_top: Option<Option<Id>>,
    wall_height: String,
    /// The Column tool's top, as `wall_top` and `wall_height` are the Wall tool's.
    column_top: Option<Option<Id>>,
    column_height: String,
    /// The Stair tool's width; the Roof tool's slope, overhang and gable ends.
    stair_width: String,
    roof_slope: String,
    roof_overhang: String,
    roof_gable: bool,
    /// Each click ends a wall and starts the next; off, two clicks make one wall.
    chain: bool,
    /// Thin Lines: every line one pixel wide.
    thin: bool,
    /// Move and Copy keep to horizontal and vertical.
    constrain: bool,
    /// Copy places copy after copy from the same base point.
    multiple: bool,
    /// The Options Bar's Rotate angle, Copy for Rotate and for Mirror, and Offset distance and Copy.
    angle: String,
    rotate_copy: bool,
    mirror_copy: bool,
    offset: String,
    offset_copy: bool,
    /// Tags placed with a leader.
    tag_leader: bool,
    /// What the Text tool writes.
    note_text: String,
    /// The project units the Options Bar's lengths were last written in.
    units: Units,
    /// A length typed while picking points: the listening dimension.
    typed_length: String,
    /// A selected wall's temporary dimension being typed: the wall, the length so far.
    dim_edit: Option<(Id, String)>,
    /// Tab pressed over the plan, taken from egui's focus keys: cycle what's under the cursor.
    tab_cycle: bool,
    show_properties: bool,
    show_browser: bool,
    canvas: canvas::Canvas,
    orbit: orbit::Orbit,
    cut: section::Cut,
    paper: paper::Papers,
    /// The model as last opened or saved: any other is unsaved.
    saved: Arc<Model>,
    /// The palette field being typed in: element, field, text so far.
    edit: Option<(Id, &'static str, String)>,
    /// The Level tool's elevation and name, as typed in the Options Bar.
    new_level: (String, String),
    /// The last letters typed over the plan, for two-letter shortcuts.
    typed: String,
    pending: Option<Pending>,
    type_dialog: Option<properties::TypeDialog>,
    /// The Properties palette's folded groups.
    collapsed: BTreeSet<&'static str>,
    /// Where views show, last frame.
    view_rect: Rect,
    /// Unsaved changes were dealt with: the window may close.
    closing: bool,
    title: String,
    styled: bool,
}

/// The Options Bar's Height: up to a level above the plan's `level` (by default the next one up),
/// or Unconnected, with a field for the height. `top` and `height` are the tool's own.
fn height_options(ui: &mut Ui, m: &Model, level: Id, top: &mut Option<Option<Id>>, height: &mut String, salt: &str) {
    let z = m.elevation(level);
    let above: Vec<(Id, &str)> = m.levels().into_iter().filter(|(_, l)| l.elevation > z).map(|(id, l)| (id, l.name.as_str())).collect();
    let now = top.unwrap_or_else(|| above.first().map(|(id, _)| *id));
    let current = now.and_then(|id| m.level(id)).map_or(t("Unconnected"), |l| l.name.as_str());
    let label = ui.label(t("Height:"));
    let combo = ComboBox::from_id_salt(salt).selected_text(current).width(130.0).show_ui(ui, |ui| {
        for (id, name) in &above {
            if ui.selectable_label(now == Some(*id), *name).clicked() {
                *top = Some(Some(*id));
            }
        }
        if ui.selectable_label(now.is_none(), t("Unconnected")).clicked() {
            *top = Some(None);
        }
    });
    combo.response.labelled_by(label.id);
    if now.is_none() {
        ui.add(TextEdit::singleline(height).desired_width(64.0).hint_text(m.units.name())).labelled_by(label.id);
    }
}

/// The params for the top an Options Bar set from the plan of `level`: up to a level, or
/// unconnected at a height.
fn top_params(m: &Model, level: Id, top: Option<Option<Id>>, height: &str) -> Result<Value, String> {
    match top.unwrap_or_else(|| m.level_above(level)) {
        Some(t) => Ok(json!({"top_level": t})),
        None => m.units.parse(height).map(|h| json!({"height": h})).ok_or_else(|| tf("`{}` is not a height", &[height])),
    }
}

/// A ribbon button: an icon over its label, tinted while its tool is on; as wide as its label.
fn big_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    let (label, tip) = (t(label), t(tip));
    let font = FontId::proportional(12.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size().x + 14.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(58.0), 52.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    // Keyboard focus shows as a ring, whatever else the button shows.
    let stroke = if r.has_focus() { Stroke::new(2.0, pal().accent) } else { stroke };
    let p = ui.painter();
    p.rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
    style::paint(p, icon, Rect::from_center_size(rect.center_top() + vec2(0.0, 19.0), vec2(24.0, 24.0)), ink);
    let text = match (active, enabled) {
        (true, _) => pal().accent,
        (false, true) => pal().text,
        (false, false) => pal().muted.gamma_multiply(0.6),
    };
    p.text(rect.center_bottom() - vec2(0.0, 10.0), Align2::CENTER_CENTER, label, font, text);
    r.on_hover_text(tip)
}

/// A small ribbon button: an icon alone, as in Revit's Modify panel; its name in its tooltip.
fn small_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    let (label, tip) = (t(label), t(tip));
    let (rect, r) = ui.allocate_exact_size(vec2(30.0, 25.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    let stroke = if r.has_focus() { Stroke::new(2.0, pal().accent) } else { stroke };
    ui.painter().rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
    style::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(19.0, 19.0)), ink);
    r.on_hover_text(tip)
}

/// A small icon button, named for screen readers and tests.
fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if r.hovered() {
        ui.painter().rect_filled(rect, 4.0, pal().hover_fill);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    style::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), pal().icon);
    r.on_hover_text(label)
}

/// A tab, of the ribbon or of a view. Contextual tabs are green, as Revit's are.
fn tab_button(ui: &mut Ui, label: &str, active: bool, contextual: bool) -> Response {
    let label = t(label);
    let font = FontId::proportional(13.0);
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(size + vec2(24.0, 10.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    let (fill, ink) = match (contextual, active, r.hovered()) {
        (true, true, _) => (pal().surface, pal().context),
        (true, false, _) => (pal().context_fill, pal().context),
        (false, true, _) => (pal().surface, pal().text),
        (false, false, true) => (pal().hover_fill, pal().text),
        (false, false, false) => (Color32::TRANSPARENT, pal().muted),
    };
    let p = ui.painter();
    let radius = CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 };
    p.rect(rect, radius, fill, if active { Stroke::new(1.0, pal().border) } else { Stroke::NONE }, StrokeKind::Inside);
    if contextual {
        p.line_segment([rect.left_top() + vec2(2.0, 1.0), rect.right_top() + vec2(-2.0, 1.0)], Stroke::new(3.0, pal().context));
    }
    if r.has_focus() {
        p.rect_stroke(rect, radius, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, ink);
    r
}

/// A palette's title bar, across its width.
fn title_bar(ui: &mut Ui, title: &str) {
    let title = t(title);
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, title));
    let p = ui.painter();
    p.rect_filled(r, 0.0, pal().bar);
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal().border));
    p.text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(13.0), pal().text);
}

/// A ribbon panel: its buttons over its name, as Revit groups tools.
fn panel(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let title = t(title);
    ui.vertical(|ui| {
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add(ui);
        });
        let font = FontId::proportional(11.0);
        let width = ui.painter().layout_no_wrap(title.to_string(), font.clone(), pal().muted).size().x + 16.0;
        let (r, _) = ui.allocate_exact_size(vec2(row.response.rect.width().max(width), 16.0), Sense::hover());
        ui.painter().rect_filled(r, 2.0, pal().bar);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, title, font, pal().muted);
    });
    ui.separator();
}

impl BimApp {
    pub fn new(session: Session) -> BimApp {
        let first = Sheet::Plan(session.model.levels().first().map_or(0, |(id, _)| *id));
        let units = session.model.units;
        BimApp {
            saved: Arc::clone(&session.model),
            session,
            sheet: first,
            tool: Tool::Modify,
            selection: Vec::new(),
            status: Status::default(),
            views: vec![first],
            tab: Tab::Architecture,
            back: Tab::Architecture,
            types: BTreeMap::new(),
            wall_top: None,
            wall_height: units.length(3000.0),
            column_top: None,
            column_height: units.length(3000.0),
            stair_width: units.length(STAIR_WIDTH),
            roof_slope: "30".into(),
            roof_overhang: units.length(500.0),
            roof_gable: false,
            chain: true,
            thin: false,
            constrain: false,
            multiple: false,
            angle: String::new(),
            rotate_copy: false,
            mirror_copy: true,
            offset: units.length(1000.0),
            offset_copy: true,
            tag_leader: false,
            note_text: String::new(),
            units,
            lang: Language::English,
            typed_length: String::new(),
            dim_edit: None,
            tab_cycle: false,
            show_properties: true,
            show_browser: true,
            canvas: canvas::Canvas::default(),
            orbit: orbit::Orbit::default(),
            cut: section::Cut::default(),
            paper: paper::Papers::default(),
            edit: None,
            new_level: (String::new(), String::new()),
            typed: String::new(),
            pending: None,
            type_dialog: None,
            collapsed: BTreeSet::new(),
            view_rect: Rect::ZERO,
            closing: false,
            title: String::new(),
            styled: false,
        }
    }

    /// Whether the model changed since it was last opened or saved.
    pub fn dirty(&self) -> bool {
        !Arc::ptr_eq(&self.saved, &self.session.model)
    }

    /// Where model point `p` shows on screen, on the plan drawn last.
    pub fn screen(&self, p: P2) -> Pos2 {
        self.canvas.screen(p)
    }

    /// Run a command; when it fails, say why.
    fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(v) => {
                self.status = Status::default();
                Some(v)
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// The contextual tab's name while a tool is on or something is selected.
    fn context(&self) -> Option<String> {
        let place = match self.tool {
            Tool::Modify
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Rotate { .. }
            | Tool::Mirror { .. }
            | Tool::Align(_)
            | Tool::Trim(_)
            | Tool::Split
            | Tool::Offset => None,
            Tool::Wall(_) => Some("Place Wall"),
            Tool::Door => Some("Place Door"),
            Tool::Window => Some("Place Window"),
            Tool::Room => Some("Place Room"),
            Tool::Level => Some("Place Level"),
            Tool::Grid(_) => Some("Place Grid"),
            Tool::Dimension(_) | Tool::Angular(_) | Tool::Radial(_) => Some("Place Dimensions"),
            Tool::Text => Some("Place Text"),
            Tool::Spot => Some("Spot Elevation"),
            Tool::Tag => Some("Tag"),
            Tool::Section(_) => Some("Section"),
            Tool::Elevation => Some("Elevation"),
            Tool::Column => Some("Place Column"),
            Tool::Stair(_) => Some("Create Stair"),
            Tool::Roof => Some("Create Roof Footprint"),
        };
        if let Some(what) = place {
            return Some(format!("{} | {}", t("Modify"), t(what)));
        }
        let m = &self.session.model;
        let categories: BTreeSet<&str> = self.selection.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
        match categories.len() {
            0 => None,
            1 => categories.first().map(|c| format!("{} | {}", t("Modify"), lang::category(c, true))),
            _ => Some(format!("{} | {}", t("Modify"), t("Multi-Select"))),
        }
    }

    /// Bring the contextual tab up, as Revit does for a tool or a selection.
    fn raise_context(&mut self) {
        if self.tab != Tab::Context {
            self.back = self.tab;
            self.tab = Tab::Context;
        }
    }

    fn view_name(&self, sheet: Sheet) -> String {
        match sheet {
            Sheet::Plan(level) => self.session.model.level(level).map_or_else(|| "Floor Plan".into(), |l| l.name.clone()),
            Sheet::View3D => "{3D}".into(),
            Sheet::Section(id) => self.session.model.section(id).map_or_else(|| "Section".into(), |v| v.name.clone()),
            Sheet::Paper(id) => self.session.model.sheet(id).map_or_else(|| "Sheet".into(), |s| format!("{} - {}", s.number, s.name)),
            Sheet::Schedule(cat) => t(SCHEDULES.iter().find(|(c, _)| *c == cat).map_or(cat, |(_, name)| name)).to_string(),
        }
    }

    /// The view as the title bar names it: `Floor Plan: Level 1`.
    fn view_title(&self, sheet: Sheet) -> String {
        let kind = match sheet {
            Sheet::Plan(_) => "Floor Plan",
            Sheet::View3D => "3D View",
            Sheet::Section(id) if matches!(self.session.model.elements.get(&id), Some(Element::Elevation(_))) => "Elevation",
            Sheet::Section(_) => "Section",
            Sheet::Paper(_) => "Sheet",
            Sheet::Schedule(_) => "Schedule",
        };
        format!("{}: {}", t(kind), self.view_name(sheet))
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.styled {
            style::apply(&ctx);
            self.styled = true;
        }
        style::follow(&ctx);
        lang::set(self.lang);
        self.tidy();
        self.keys(&ctx);
        let view = self.view_title(self.sheet);
        let title = format!("{}{} · {view} · BIMCraft", self.session.model.name, if self.dirty() { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
        }
        let bar = Frame::new().fill(pal().bar).inner_margin(vec2(8.0, 4.0)).stroke(Stroke::new(1.0, pal().border));
        Panel::top("quick-access").frame(bar).show(ui, |ui| self.quick_access(ui, &view));
        Panel::top("ribbon").frame(Frame::new().fill(pal().surface)).show(ui, |ui| self.ribbon(ui));
        Panel::top("options").frame(bar).show(ui, |ui| self.options(ui));
        Panel::bottom("status").frame(bar).show(ui, |ui| self.status_bar(ui));
        if self.show_properties || self.show_browser {
            let palette = Frame::new().fill(pal().panel).stroke(Stroke::new(1.0, pal().border));
            Panel::left("side").frame(palette).resizable(true).default_size(290.0).show(ui, |ui| {
                if self.show_properties && self.show_browser {
                    Panel::top("properties").frame(palette).resizable(true).default_size(ui.available_height() * 0.55).show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        self.properties_palette(ui);
                    });
                    self.browser_palette(ui);
                } else if self.show_properties {
                    self.properties_palette(ui);
                } else {
                    self.browser_palette(ui);
                }
            });
        }
        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            Panel::top("view-tabs").frame(Frame::new().fill(pal().bar).inner_margin(Margin { left: 6, right: 6, top: 4, bottom: 0 })).show(
                ui,
                |ui| {
                    self.view_tabs(ui);
                },
            );
            self.view_rect = ui.available_rect_before_wrap();
            // The plan sets the cursor each frame it shows.
            self.canvas.cursor = None;
            match self.sheet {
                Sheet::Plan(level) if self.session.model.level(level).is_some() => self.canvas(ui, level),
                Sheet::Plan(_) => {
                    ui.centered_and_justified(|ui| ui.label(t("This project has no levels. Add one with the Level tool.")));
                }
                Sheet::View3D => self.view3d(ui),
                Sheet::Section(id) => self.section_view(ui, id),
                Sheet::Paper(id) => self.paper_view(ui, id),
                Sheet::Schedule(cat) => self.schedule(ui, cat),
            }
        });
        self.warning(&ctx);
        self.dialogs(&ctx);
    }

    /// Follow what the last command removed: the selection, views of levels that are gone, and
    /// the contextual tab once its tool or selection is.
    fn tidy(&mut self) {
        // The Options Bar's lengths, rewritten in the project's units when they change.
        let (from, to) = (self.units, self.session.model.units);
        if from != to {
            for field in [&mut self.wall_height, &mut self.column_height, &mut self.stair_width, &mut self.roof_overhang, &mut self.offset]
            {
                if let Some(mm) = from.parse(field) {
                    *field = to.length(mm);
                }
            }
            self.units = to;
        }
        let m = &self.session.model;
        self.selection.retain(|id| m.elements.contains_key(id));
        let exists = |s: &Sheet| match s {
            Sheet::Plan(level) => m.level(*level).is_some(),
            Sheet::Section(id) => m.section(*id).is_some(),
            Sheet::Paper(id) => m.sheet(*id).is_some(),
            Sheet::View3D | Sheet::Schedule(_) => true,
        };
        self.views.retain(exists);
        if !exists(&self.sheet) {
            self.sheet = self.views.last().copied().unwrap_or(Sheet::Plan(m.levels().first().map_or(0, |(id, _)| *id)));
        }
        if !self.views.contains(&self.sheet) {
            self.views.push(self.sheet);
        }
        if self.tab == Tab::Context && self.context().is_none() {
            self.tab = self.back;
        }
    }

    fn keys(&mut self, ctx: &Context) {
        // A dialog takes the keys while it's open.
        if self.type_dialog.is_some() || self.pending.is_some() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save_as();
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.save();
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.guard(Pending::Open, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.guard(Pending::New, ctx);
        }
        // The rest belongs to a text field while one has the keyboard.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.run("redo", json!({}));
        } else if pressed(Modifiers::COMMAND, Key::Z) {
            self.run("undo", json!({}));
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.escape();
        }
        // Enter places a typed length, else builds the wall chain.
        if pressed(Modifiers::NONE, Key::Enter)
            && let Sheet::Plan(level) = self.sheet
        {
            if self.typed_length.is_empty() { self.finish_wall(level, false) } else { self.place_typed(level) }
        }
        if !self.typed_length.is_empty() && pressed(Modifiers::NONE, Key::Backspace) {
            self.typed_length.pop();
        }
        if pressed(Modifiers::NONE, Key::Delete) {
            self.delete();
        }
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        });
        // A digit starts a length while a tool is picking its next point, and everything typed
        // after it (feet, inches, fractions, a unit) is the length's until Enter; letters before
        // one are shortcuts.
        let mut shortcut = String::new();
        for ch in typed.chars() {
            if self.base().is_some() && (ch.is_ascii_digit() || ch == '.' || !self.typed_length.is_empty()) {
                self.typed_length.push(ch);
            } else {
                shortcut.push(ch);
            }
        }
        for ch in shortcut.chars().filter(char::is_ascii_alphabetic) {
            self.typed.push(ch.to_ascii_uppercase());
            if self.typed.len() > 2 {
                self.typed.remove(0);
            }
            if self.typed.len() == 2 && self.shortcut() {
                self.typed.clear();
            }
        }
    }

    /// Revit's two-letter shortcuts: the commands' own (WA, DR, WN, RM, LL, GR, DE), plus MD
    /// for Modify, ZF to zoom to fit and TL for Thin Lines.
    fn shortcut(&mut self) -> bool {
        let tool = match self.typed.as_str() {
            "MD" => Tool::Modify,
            "MM" => Tool::Mirror { draw: false, start: None },
            "DM" => Tool::Mirror { draw: true, start: None },
            "AL" => Tool::Align(None),
            "OF" => Tool::Offset,
            "ZF" | "ZE" => {
                self.fit();
                return true;
            }
            "TL" => {
                self.thin = !self.thin;
                return true;
            }
            code => match engine::find(code).map(|c| c.id) {
                Some("wall") => Tool::Wall(Vec::new()),
                Some("door") => Tool::Door,
                Some("window") => Tool::Window,
                Some("room") => Tool::Room,
                Some("level") => Tool::Level,
                Some("grid") => Tool::Grid(None),
                Some("move") => Tool::Move(None),
                Some("copy") => Tool::Copy(None),
                Some("rotate") => Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false },
                Some("corner") => Tool::Trim(None),
                Some("split") => Tool::Split,
                Some("dimension") => Tool::Dimension(Vec::new()),
                Some("text") => Tool::Text,
                Some("spot") => Tool::Spot,
                Some("tag") => Tool::Tag,
                Some("column") => Tool::Column,
                Some("delete") => {
                    self.delete();
                    return true;
                }
                _ => return false,
            },
        };
        self.set_tool(tool);
        true
    }

    fn set_tool(&mut self, mut tool: Tool) {
        // Move, Copy, Rotate and Mirror take the selection; Align, Trim, Split and Offset pick walls.
        let edits = matches!(tool, Tool::Move(_) | Tool::Copy(_) | Tool::Rotate { .. } | Tool::Mirror { .. });
        let picks = matches!(tool, Tool::Align(_) | Tool::Trim(_) | Tool::Split | Tool::Offset);
        if edits && self.selection.is_empty() {
            self.status = Status { text: t("select what to move, copy, rotate or mirror first").into(), error: true };
            return;
        }
        if let Tool::Rotate { centre, .. } = &mut tool
            && let Some(middle) = self.middle()
        {
            *centre = middle;
        }
        if tool == Tool::Level {
            let top = self.session.model.levels().last().map_or(0.0, |(_, l)| l.elevation);
            self.new_level = (self.session.model.units.length(top + 3000.0), String::new());
        }
        if tool != Tool::Modify && !edits {
            self.selection.clear();
        }
        if tool != Tool::Modify && !picks {
            self.raise_context();
        }
        self.typed.clear();
        self.typed_length.clear();
        self.tool = tool;
    }

    /// The middle of what's selected: where Rotate turns it about until told otherwise.
    fn middle(&self) -> Option<P2> {
        let m = &self.session.model;
        let mut pts = Vec::new();
        for id in &self.selection {
            match m.elements.get(id) {
                Some(Element::Wall(w)) => pts.extend([w.start, w.end]),
                Some(Element::Grid(g)) => pts.extend([g.start, g.end]),
                Some(Element::Floor(f)) => pts.extend(&f.boundary),
                Some(Element::Room(r)) => pts.push(r.point),
                Some(Element::Door(_) | Element::Window(_)) => pts.extend(m.anchor(*id)),
                Some(Element::Dimension(d)) => pts.push(d.at),
                Some(Element::Section(v) | Element::Elevation(v)) => pts.extend([v.start, v.end]),
                Some(Element::Tag(t)) => pts.extend(m.anchor(t.host).map(|a| a + t.offset)),
                Some(Element::Column(c)) => pts.push(c.at),
                Some(Element::Stair(st)) => pts.extend([st.start, st.start + heading(st.angle) * m.flight(st).2]),
                Some(Element::Roof(r)) => pts.extend(&r.boundary),
                _ => {}
            }
        }
        let first = *pts.first()?;
        let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1))));
        Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
    }

    /// The point the tool measures its next one from, while it has one.
    fn base(&self) -> Option<P2> {
        match &self.tool {
            Tool::Wall(chain) => chain.last().copied(),
            Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) => *start,
            _ => None,
        }
    }

    /// Escape: dismiss the error, drop the chain being drawn, else leave the tool, else clear the
    /// selection.
    fn escape(&mut self) {
        self.typed.clear();
        self.typed_length.clear();
        self.status = Status::default();
        self.tool = match std::mem::replace(&mut self.tool, Tool::Modify) {
            Tool::Wall(chain) if !chain.is_empty() => Tool::Wall(Vec::new()),
            Tool::Grid(Some(_)) => Tool::Grid(None),
            Tool::Mirror { draw, start: Some(_) } => Tool::Mirror { draw, start: None },
            Tool::Align(Some(_)) => Tool::Align(None),
            Tool::Trim(Some(_)) => Tool::Trim(None),
            Tool::Dimension(refs) if !refs.is_empty() => Tool::Dimension(Vec::new()),
            Tool::Angular(refs) if !refs.is_empty() => Tool::Angular(Vec::new()),
            Tool::Radial(Some(_)) => Tool::Radial(None),
            Tool::Section(Some(_)) => Tool::Section(None),
            Tool::Stair(Some(_)) => Tool::Stair(None),
            Tool::Modify => {
                self.selection.clear();
                Tool::Modify
            }
            _ => Tool::Modify,
        };
    }

    fn delete(&mut self) {
        if !self.selection.is_empty() {
            self.run("delete", json!({"ids": self.selection}));
        }
    }

    fn fit(&mut self) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.fit(id),
            Sheet::View3D => self.fit3d(),
            Sheet::Schedule(_) => {}
        }
    }

    /// Open a view in a tab, or bring its tab forward.
    fn open_sheet(&mut self, sheet: Sheet) {
        if !self.views.contains(&sheet) {
            self.views.push(sheet);
        }
        self.sheet = sheet;
        self.selection.clear();
        self.typed_length.clear();
        // The tools that pick points work in plans (Text in sections and on sheets too, Spot
        // Elevation in sections); any other view takes clicks to select.
        let works = match sheet {
            Sheet::Plan(_) => true,
            Sheet::Section(_) => matches!(self.tool, Tool::Text | Tool::Spot),
            Sheet::Paper(_) => self.tool == Tool::Text,
            _ => false,
        };
        if !works && self.tool != Tool::Level {
            self.tool = Tool::Modify;
        }
        match &mut self.tool {
            Tool::Wall(chain) => chain.clear(),
            Tool::Grid(start) => *start = None,
            Tool::Dimension(refs) | Tool::Angular(refs) => refs.clear(),
            Tool::Radial(r) => *r = None,
            Tool::Section(start) | Tool::Stair(start) => *start = None,
            Tool::Move(_) | Tool::Copy(_) => self.tool = Tool::Modify,
            _ => {}
        }
    }

    /// The views on no sheet yet, as Place View lists them: plans, sections and elevations, the
    /// 3D view and the schedules; each as `place` takes it, by id, or by name while it isn't an
    /// element yet.
    fn unplaced_views(&self) -> Vec<(Value, String)> {
        let m = &self.session.model;
        let placed: BTreeSet<&str> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Viewport(v) => m.view_name(v.view),
                _ => None,
            })
            .collect();
        let plans = m.levels().into_iter().map(|(id, l)| (json!(id), tf("Floor Plan: {}", &[&l.name]), l.name.as_str()));
        let cuts = m.elements.iter().filter_map(|(id, e)| match e {
            Element::Section(v) => Some((json!(id), tf("Section: {}", &[&v.name]), v.name.as_str())),
            Element::Elevation(v) => Some((json!(id), tf("Elevation: {}", &[&v.name]), v.name.as_str())),
            _ => None,
        });
        let others = std::iter::once((json!("{3D}"), t("3D View: {3D}").to_string(), "{3D}"))
            .chain(SCHEDULES.iter().map(|(_, name)| (json!(name), tf("Schedule: {}", &[t(name)]), *name)));
        plans.chain(cuts).chain(others).filter(|(_, _, name)| !placed.contains(name)).map(|(v, label, _)| (v, label)).collect()
    }

    /// Close a view's tab; the last one stays open.
    fn close_view(&mut self, sheet: Sheet) {
        if self.views.len() < 2 {
            return;
        }
        self.views.retain(|v| *v != sheet);
        if self.sheet == sheet
            && let Some(&last) = self.views.last()
        {
            self.open_sheet(last);
        }
    }

    // ---------- files ----------

    /// Save the project (`.bimcraft`, or any other name) or export by extension: `.ifc`, or the
    /// view on screen as `.png` (a plan or the 3D view) or `.svg` (a plan). Whether it worked; the
    /// warning box says why not.
    pub fn save_to(&mut self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = path.to_string_lossy();
        if matches!(ext.as_str(), "png" | "svg") {
            let mut p = json!({"path": file, "width": 1600, "height": 1000});
            match (self.sheet, self.orbit.camera) {
                (Sheet::Plan(id) | Sheet::Section(id) | Sheet::Paper(id), _) => p["view"] = json!(id),
                (Sheet::View3D, camera) => {
                    p["view"] = json!("{3D}");
                    if let Some(c) = camera {
                        (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                    }
                }
                (Sheet::Schedule(_), _) => {}
            }
            return self.run("render", p).is_some();
        }
        let saved = self.run("save", json!({"path": file})).is_some();
        // Exports (IFC, the sheets' PDF) leave the project unsaved.
        if saved && !matches!(ext.as_str(), "ifc" | "pdf") {
            self.saved = Arc::clone(&self.session.model);
        }
        saved
    }

    fn save(&mut self) -> bool {
        match self.session.path.clone() {
            Some(p) => self.save_to(&p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        self.ask_path("BIMCraft project", "bimcraft").is_some_and(|p| self.save_to(&p))
    }

    fn export(&mut self, what: &str, ext: &str) {
        if let Some(p) = self.ask_path(what, ext) {
            self.save_to(&p);
        }
    }

    /// Where to save, from the system's file dialog, ending in `.ext`.
    fn ask_path(&self, what: &str, ext: &str) -> Option<PathBuf> {
        let name = format!("{}.{ext}", self.session.model.name.replace(['/', '\\'], "-"));
        let p = rfd::FileDialog::new().add_filter(t(what), &[ext]).set_file_name(name).save_file()?;
        Some(if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) { p } else { p.with_extension(ext) })
    }

    /// Open a project, or import an IFC file and say what came in and what was left out.
    pub(crate) fn open_path(&mut self, path: &Path) -> bool {
        let Some(reply) = self.run("open", json!({"path": path.to_string_lossy()})) else { return false };
        self.reopened();
        if reply["format"].is_string() {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.status = Status { text: import_note(&name, &reply), error: false };
        }
        true
    }

    /// Start over on the model the session holds now (after New and Open), keeping how the
    /// window is set up.
    fn reopened(&mut self) {
        let mut fresh = BimApp::new(std::mem::take(&mut self.session));
        fresh.status = std::mem::take(&mut self.status);
        (fresh.thin, fresh.show_properties, fresh.show_browser, fresh.lang) =
            (self.thin, self.show_properties, self.show_browser, self.lang);
        *self = fresh;
    }

    /// Do `p` now, or first ask about unsaved changes.
    fn guard(&mut self, p: Pending, ctx: &Context) {
        if self.dirty() { self.pending = Some(p) } else { self.proceed(p, ctx) }
    }

    fn proceed(&mut self, p: Pending, ctx: &Context) {
        match p {
            Pending::New => {
                // In the interface's language: its levels named in it.
                if self.run("new", json!({"language": self.lang.code()})).is_some() {
                    self.reopened();
                }
            }
            Pending::Open => {
                // `.revcraft`: projects saved before the name changed.
                let projects = ["bimcraft", "revcraft", "ifc"];
                if let Some(path) = rfd::FileDialog::new().add_filter("BIMCraft project or IFC", &projects).pick_file() {
                    self.open_path(&path);
                }
            }
            Pending::Close => {
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    // ---------- the top: Quick Access Toolbar, ribbon, Options Bar ----------

    fn quick_access(&mut self, ui: &mut Ui, view: &str) {
        let ctx = ui.ctx().clone();
        let row = ui
            .horizontal(|ui| {
                if icon_button(ui, Icon::Open, "Open").clicked() {
                    self.guard(Pending::Open, &ctx);
                }
                if icon_button(ui, Icon::Save, "Save").clicked() {
                    self.save();
                }
                ui.separator();
                if icon_button(ui, Icon::Undo, "Undo").clicked() {
                    self.run("undo", json!({}));
                }
                if icon_button(ui, Icon::Redo, "Redo").clicked() {
                    self.run("redo", json!({}));
                }
                ui.separator();
                if icon_button(ui, Icon::View3D, "Default 3D View").clicked() {
                    self.open_sheet(Sheet::View3D);
                }
            })
            .response
            .rect;
        // The project and the view, centred, where Revit's title bar has them.
        let at = pos2(ui.max_rect().center().x, row.center().y);
        ui.painter().text(
            at,
            Align2::CENTER_CENTER,
            format!("{} · {view}", self.session.model.name),
            FontId::proportional(12.5),
            pal().muted,
        );
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        Frame::new().fill(pal().bar).inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let file = egui::Button::new(RichText::new(t("File")).color(Color32::WHITE).strong())
                    .fill(pal().accent)
                    .min_size(vec2(58.0, 26.0));
                MenuButton::from_button(file).ui(ui, |ui| self.file_menu(ui, &ctx));
                for (tab, label) in [
                    (Tab::Architecture, "Architecture"),
                    (Tab::Annotate, "Annotate"),
                    (Tab::View, "View"),
                    (Tab::Manage, "Manage"),
                    (Tab::Modify, "Modify"),
                ] {
                    if tab_button(ui, label, self.tab == tab, false).clicked() {
                        self.tab = tab;
                    }
                }
                if let Some(name) = self.context()
                    && tab_button(ui, &name, self.tab == Tab::Context, true).clicked()
                {
                    self.tab = Tab::Context;
                }
            });
        });
        ui.painter().line_segment([ui.min_rect().left_bottom(), ui.min_rect().right_bottom()], Stroke::new(1.0, pal().border));
        Frame::new().inner_margin(vec2(8.0, 6.0)).show(ui, |ui| {
            ui.horizontal(|ui| match self.tab {
                Tab::Architecture => self.architecture_tab(ui),
                Tab::Annotate => self.annotate_tab(ui),
                Tab::View => self.view_tab(ui),
                Tab::Manage => self.manage_tab(ui),
                Tab::Modify | Tab::Context => self.modify_tab(ui),
            });
        });
    }

    /// The application menu, under the File tab.
    fn file_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        let item = |ui: &mut Ui, label: &str, keys: Option<KeyboardShortcut>| {
            let b = egui::Button::new(t(label));
            ui.add(match keys {
                Some(k) => b.shortcut_text(ctx.format_shortcut(&k)),
                None => b,
            })
            .clicked()
        };
        let cmd = |k: Key| Some(KeyboardShortcut::new(Modifiers::COMMAND, k));
        if item(ui, "New", cmd(Key::N)) {
            self.guard(Pending::New, ctx);
        }
        if item(ui, "Open…", cmd(Key::O)) {
            self.guard(Pending::Open, ctx);
        }
        if item(ui, "Save", cmd(Key::S)) {
            self.save();
        }
        if item(ui, "Save As…", Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S))) {
            self.save_as();
        }
        SubMenuButton::new(t("Export")).ui(ui, |ui| {
            if ui.button(t("IFC…")).clicked() {
                self.export("IFC4", "ifc");
            }
            if ui.button(t("View as PNG…")).clicked() {
                self.export("PNG image", "png");
            }
            if ui.button(t("Plan as SVG…")).clicked() {
                self.export("SVG drawing", "svg");
            }
            if ui.button(t("Sheets as PDF…")).clicked() {
                self.export("PDF", "pdf");
            }
        });
        ui.separator();
        if item(ui, "Exit", None) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Ribbon buttons that pick tools, under the panel's name.
    fn tools(&mut self, ui: &mut Ui, name: &str, tools: Tools) {
        panel(ui, name, |ui| {
            for (icon, label, tip, tool) in tools {
                let active = std::mem::discriminant(&self.tool) == std::mem::discriminant(tool);
                if big_button(ui, *icon, label, active, tip).clicked() {
                    self.set_tool(tool.clone());
                }
            }
        });
    }

    fn architecture_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        self.tools(
            ui,
            "Build",
            &[
                (Icon::Wall, "Wall", "Wall (WA): click points; Enter finishes, Esc cancels", Tool::Wall(Vec::new())),
                (Icon::Door, "Door", "Door (DR): click a wall", Tool::Door),
                (Icon::Window, "Window", "Window (WN): click a wall", Tool::Window),
                (Icon::Column, "Column", "Column (CL): click where it stands", Tool::Column),
                (Icon::Roof, "Roof", "Roof by Footprint: click inside the walls to roof them, on the level above", Tool::Roof),
            ],
        );
        self.tools(
            ui,
            "Circulation",
            &[(Icon::Stair, "Stair", "Stair: click where it starts, then which way it climbs", Tool::Stair(None))],
        );
        self.tools(ui, "Room & Area", &[(Icon::Room, "Room", "Room (RM): click inside walls", Tool::Room)]);
        self.tools(
            ui,
            "Datum",
            &[
                (Icon::Level, "Level", "Level (LL): a new level at an elevation", Tool::Level),
                (Icon::Grid, "Grid", "Grid (GR): click its two ends", Tool::Grid(None)),
            ],
        );
    }

    fn annotate_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        self.tools(
            ui,
            "Dimension",
            &[
                (
                    Icon::Dimension,
                    "Aligned",
                    "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, then click where the \
                     dimension line goes",
                    Tool::Dimension(Vec::new()),
                ),
                (
                    Icon::Angular,
                    "Angular",
                    "Angular Dimension: pick two lines that meet, then click where the arc goes",
                    Tool::Angular(Vec::new()),
                ),
                (Icon::Radial, "Radial", "Radial Dimension: pick a round column, then click where the radius goes", Tool::Radial(None)),
                (Icon::Spot, "Spot Elevation", "Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point", Tool::Spot),
            ],
        );
        self.tools(
            ui,
            "Text",
            &[(Icon::Text, "Text", "Text (TX): type the note in the Options Bar, then click where it goes", Tool::Text)],
        );
        panel(ui, "Tag", |ui| {
            let tip = "Tag by Category (TG): click a door or window to tag it with its mark";
            if big_button(ui, Icon::Tag, "Tag by Category", self.tool == Tool::Tag, tip).clicked() {
                self.set_tool(Tool::Tag);
            }
            if big_button(ui, Icon::TagAll, "Tag All", false, "Tag every door and window in this view that has no tag").clicked() {
                self.tag_all();
            }
        });
    }

    /// Tag All: the doors and windows of the plan's level that have no tag yet.
    fn tag_all(&mut self) {
        if let Sheet::Plan(level) = self.sheet {
            self.run("tag", json!({"level": level, "leader": self.tag_leader}));
        }
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Graphics", |ui| {
            if big_button(ui, Icon::ThinLines, "Thin Lines", self.thin, "Thin Lines (TL): every line one pixel wide").clicked() {
                self.thin = !self.thin;
            }
            let dark = style::dark();
            let tip = "Dark Mode: the window and the views light on dark; sheets stay paper. It starts as your system's theme";
            if big_button(ui, Icon::Theme, "Dark Mode", dark, tip).clicked() {
                ui.ctx().set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
            }
            // A plan's or a section's crop region: on to what it shows, or off.
            let view = match self.sheet {
                Sheet::Plan(id) | Sheet::Section(id) => Some(id),
                _ => None,
            };
            let cropped = view.is_some_and(|id| match self.session.model.elements.get(&id) {
                Some(Element::Level(l)) => l.crop.is_some(),
                Some(Element::Section(v) | Element::Elevation(v)) => v.crop.is_some(),
                _ => false,
            });
            let tip = "Crop View: show only the crop region; drag its edges by their grips";
            if ui.add_enabled_ui(view.is_some(), |ui| big_button(ui, Icon::Crop, "Crop View", cropped, tip)).inner.clicked()
                && let Some(id) = view
            {
                self.run("crop", if cropped { json!({"view": id, "region": null}) } else { json!({"view": id}) });
            }
        });
        panel(ui, "Create", |ui| {
            if big_button(ui, Icon::View3D, "3D View", self.sheet == Sheet::View3D, "Open the default 3D view").clicked() {
                self.open_sheet(Sheet::View3D);
            }
            let tip = "Section: click its start, then its end; it looks to the left of the line";
            if big_button(ui, Icon::Section, "Section", matches!(self.tool, Tool::Section(_)), tip).clicked() {
                self.set_tool(Tool::Section(None));
            }
            let tip = "Elevation: click beside the building, on the side to look at";
            if big_button(ui, Icon::Elevation, "Elevation", self.tool == Tool::Elevation, tip).clicked() {
                self.set_tool(Tool::Elevation);
            }
            let r = big_button(ui, Icon::Schedule, "Schedules", false, "Open a schedule");
            Popup::menu(&r).show(|ui| {
                for (cat, name) in SCHEDULES {
                    if ui.button(name).clicked() {
                        self.open_sheet(Sheet::Schedule(cat));
                    }
                }
            });
        });
        panel(ui, "Sheet Composition", |ui| {
            if big_button(ui, Icon::Sheet, "Sheet", false, "A new sheet, with a title block").clicked()
                && let Some(id) = self.run("sheet", json!({})).and_then(|made| made["id"].as_u64())
            {
                self.open_sheet(Sheet::Paper(id));
            }
            let sheet = match self.sheet {
                Sheet::Paper(id) => Some(id),
                _ => None,
            };
            let tip = "Put a view on this sheet: one not on a sheet yet";
            let r = ui.add_enabled_ui(sheet.is_some(), |ui| big_button(ui, Icon::PlaceView, "Place View", false, tip)).inner;
            if let Some(sheet) = sheet {
                let views = self.unplaced_views();
                Popup::menu(&r).show(|ui| {
                    if views.is_empty() {
                        ui.label(t("Every view is on a sheet."));
                    }
                    for (view, label) in views {
                        if ui.button(label).clicked() {
                            let mut p = json!({"sheet": sheet, "view": view});
                            // The 3D view as it's turned on screen.
                            if let (Some("{3D}"), Some(c)) = (p["view"].as_str(), self.orbit.camera) {
                                (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                            }
                            self.run("place", p);
                        }
                    }
                });
            }
        });
        panel(ui, "Windows", |ui| {
            if big_button(ui, Icon::CloseInactive, "Close Inactive", false, "Close every view but this one").clicked() {
                let active = self.sheet;
                self.views.retain(|v| *v == active);
            }
            let tip = "Show or hide the palettes, and pick the interface's language";
            let r = big_button(ui, Icon::Interface, "User Interface", false, tip);
            Popup::menu(&r).show(|ui| {
                ui.checkbox(&mut self.show_properties, t("Properties"));
                ui.checkbox(&mut self.show_browser, t("Project Browser"));
                ui.separator();
                ui.label(RichText::new(t("Language")).color(pal().muted));
                for l in Language::ALL {
                    if ui.selectable_label(self.lang == l, l.label()).clicked() {
                        self.lang = l;
                    }
                }
            });
        });
    }

    /// Manage: the project's settings, as Revit's tab has them.
    fn manage_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Settings", |ui| {
            let now = self.session.model.units;
            let tip = tf("Project Units (UN): how lengths show and read; now {}", &[&t(now.label()).to_lowercase()]);
            let r = big_button(ui, Icon::Units, "Project Units", false, &tip);
            Popup::menu(&r).show(|ui| {
                for u in Units::ALL {
                    if ui.selectable_label(u == now, t(u.label())).clicked() {
                        self.run("units", json!({"length": u.name()}));
                    }
                }
            });
            // What the drawings say, and how new things are named.
            let now = self.session.model.language;
            let tip = tf("Project Language: what the drawings say and how new things are named; now {}", &[now.label()]);
            let r = big_button(ui, Icon::Text, "Project Language", false, &tip);
            Popup::menu(&r).show(|ui| {
                for l in Language::ALL {
                    if ui.selectable_label(l == now, l.label()).clicked() {
                        self.run("language", json!({"code": l.code()}));
                    }
                }
            });
        });
    }

    fn modify_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, "Properties", |ui| {
            if big_button(ui, Icon::Properties, "Properties", self.show_properties, "Show or hide the Properties palette").clicked() {
                self.show_properties = !self.show_properties;
            }
            let typed = self.type_subject().is_some();
            if ui
                .add_enabled_ui(typed, |ui| big_button(ui, Icon::TypeProperties, "Type Properties", false, "Edit the type"))
                .inner
                .clicked()
            {
                self.edit_type();
            }
        });
        // Revit's Modify panel: small buttons in rows. Those taking the selection need one.
        let some = !self.selection.is_empty();
        // The button lit while its tool is on (the two Mirrors told apart by how they take the axis).
        let on = |t: &Tool, other: &Tool| match (t, other) {
            (Tool::Mirror { draw: a, .. }, Tool::Mirror { draw: b, .. }) => a == b,
            _ => std::mem::discriminant(t) == std::mem::discriminant(other),
        };
        let rows: [Buttons; 2] = [
            &[
                (Icon::Align, "Align", "Align (AL): pick a reference line, then what to line up with it", Some(Tool::Align(None)), true),
                (Icon::Offset, "Offset", "Offset (OF): pick a wall; a copy goes to the cursor's side", Some(Tool::Offset), true),
                (
                    Icon::MirrorPick,
                    "Mirror Pick Axis",
                    "Mirror, Pick Axis (MM): pick a wall or grid to mirror across",
                    Some(Tool::Mirror { draw: false, start: None }),
                    some,
                ),
                (
                    Icon::MirrorDraw,
                    "Mirror Draw Axis",
                    "Mirror, Draw Axis (DM): click the axis's two ends",
                    Some(Tool::Mirror { draw: true, start: None }),
                    some,
                ),
                (Icon::Split, "Split Element", "Split Element (SL): click a wall where to split it", Some(Tool::Split), true),
            ],
            &[
                (Icon::Move, "Move", "Move (MV): click a base point, then where it goes", Some(Tool::Move(None)), some),
                (Icon::Copy, "Copy", "Copy (CO): click a base point, then where the copy goes", Some(Tool::Copy(None)), some),
                (
                    Icon::Rotate,
                    "Rotate",
                    "Rotate (RO): click the angle's start, then its end",
                    Some(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
                    some,
                ),
                (
                    Icon::Trim,
                    "Trim/Extend to Corner",
                    "Trim/Extend to Corner (TR): pick two walls, on the parts to keep",
                    Some(Tool::Trim(None)),
                    true,
                ),
                (Icon::Delete, "Delete", "Delete (DE) the selection", None, some),
            ],
        ];
        panel(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in rows {
                    ui.horizontal(|ui| {
                        for (icon, label, tip, tool, enabled) in row {
                            let active = tool.as_ref().is_some_and(|t| on(t, &self.tool) && *t != Tool::Modify);
                            if ui.add_enabled_ui(*enabled, |ui| small_button(ui, *icon, label, active, tip)).inner.clicked() {
                                match tool {
                                    Some(t) => self.set_tool(t.clone()),
                                    None => self.delete(),
                                }
                            }
                        }
                    });
                }
            });
        });
    }

    /// The Options Bar: the contextual tab's name, then the tool's options.
    fn options(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            if let Some(name) = self.context() {
                // Painted: the tab already names it for screen readers.
                let font = FontId::proportional(13.0);
                let size = ui.painter().layout_no_wrap(name.clone(), font.clone(), pal().text).size();
                let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font, pal().text);
                ui.separator();
            }
            match self.tool {
                Tool::Wall(_) => self.wall_options(ui),
                Tool::Column => {
                    if let Sheet::Plan(level) = self.sheet {
                        height_options(ui, &self.session.model, level, &mut self.column_top, &mut self.column_height, "column-top");
                    }
                }
                Tool::Stair(_) => {
                    let l = ui.label(t("Width:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.stair_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                }
                Tool::Roof => {
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                    let l = ui.label(t("Overhang:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.roof_overhang).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    ui.checkbox(&mut self.roof_gable, t("Gable"));
                }
                Tool::Level => self.level_options(ui),
                Tool::Move(_) => {
                    ui.checkbox(&mut self.constrain, t("Constrain"));
                }
                Tool::Copy(_) => {
                    ui.checkbox(&mut self.constrain, t("Constrain"));
                    ui.checkbox(&mut self.multiple, t("Multiple"));
                }
                Tool::Rotate { .. } => self.rotate_options(ui),
                Tool::Mirror { .. } => {
                    ui.checkbox(&mut self.mirror_copy, t("Copy"));
                }
                Tool::Tag => {
                    ui.checkbox(&mut self.tag_leader, t("Leader"));
                }
                Tool::Text => {
                    let l = ui.label(t("Text:"));
                    ui.add(TextEdit::singleline(&mut self.note_text).desired_width(240.0).hint_text(t("what the note says")))
                        .labelled_by(l.id);
                }
                Tool::Offset => {
                    let l = ui.label(t("Offset:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.offset).desired_width(70.0).hint_text(hint)).labelled_by(l.id);
                    ui.checkbox(&mut self.offset_copy, t("Copy"));
                }
                _ => {}
            }
        });
    }

    fn wall_options(&mut self, ui: &mut Ui) {
        let Sheet::Plan(level) = self.sheet else { return };
        height_options(ui, &self.session.model, level, &mut self.wall_top, &mut self.wall_height, "wall-top");
        ui.checkbox(&mut self.chain, t("Chain"));
    }

    /// Rotate's Options Bar: an angle to type (Enter turns by it), Place Centre, and Copy.
    fn rotate_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Angle:"));
        let r = ui.add(TextEdit::singleline(&mut self.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match (self.angle.trim().parse::<f64>(), &self.tool) {
                (Ok(a), &Tool::Rotate { centre, .. }) if a.is_finite() => self.turn(centre, a),
                _ => self.status = Status { text: tf("rotate: `{}` is not an angle in degrees", &[&self.angle]), error: true },
            }
        }
        if ui.button(t("Place Centre")).clicked()
            && let Tool::Rotate { place, .. } = &mut self.tool
        {
            *place = true;
        }
        ui.checkbox(&mut self.rotate_copy, t("Copy"));
    }

    fn level_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Elevation"));
        ui.add(TextEdit::singleline(&mut self.new_level.0).desired_width(80.0)).labelled_by(l.id);
        let l = ui.label(t("Name"));
        ui.add(TextEdit::singleline(&mut self.new_level.1).hint_text(t("the next free name")).desired_width(140.0)).labelled_by(l.id);
        if ui.button(t("Create")).clicked() {
            match self.session.model.units.parse(&self.new_level.0) {
                Some(elevation) => {
                    let mut p = json!({"elevation": elevation});
                    if !self.new_level.1.trim().is_empty() {
                        p["name"] = json!(self.new_level.1.trim());
                    }
                    if self.run("level", p).is_some() {
                        self.tool = Tool::Modify;
                    }
                }
                None => self.status = Status { text: tf("level: `{}` is not an elevation", &[&self.new_level.0]), error: true },
            }
        }
    }

    // ---------- the bottom: status bar, warnings ----------

    /// What the tool wants next, in the status bar as Revit puts it.
    fn prompt(&self) -> &'static str {
        match (self.sheet, &self.tool) {
            (Sheet::View3D, Tool::Modify) => {
                return "Click to select. Drag to orbit (or Shift and the middle button), the middle button pans, the wheel zooms.";
            }
            (Sheet::Paper(_), Tool::Modify) => {
                return "Click a view on the sheet to select it, drag it to move it, double-click it to open it.";
            }
            (Sheet::Section(_), Tool::Modify) => {
                return "Click to select what the view cuts or sees; the middle button pans, the wheel zooms.";
            }
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), Tool::Level) => {
                return "Type the new level's elevation in the Options Bar, then Create.";
            }
            (Sheet::Section(_) | Sheet::Paper(_), Tool::Text) => return "Type the note in the Options Bar, then click where it goes.",
            (Sheet::Section(_), Tool::Spot) => return "Click a point to mark its height.",
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), _) => {
                return "This tool works in plan views: open one from the Project Browser.";
            }
            _ => {}
        }
        match &self.tool {
            Tool::Modify => "Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move.",
            Tool::Wall(chain) if chain.is_empty() => "Click to enter the wall's start point.",
            Tool::Wall(_) => "Click to enter the next point. Enter finishes, Esc cancels, Shift keeps it square.",
            Tool::Door => "Click a wall to place the door.",
            Tool::Window => "Click a wall to place the window.",
            Tool::Room => "Click inside walls to place a room; the outline shows the room you'd get.",
            Tool::Level => "Type the new level's elevation in the Options Bar, then Create.",
            Tool::Grid(None) => "Click to enter the grid's start point.",
            Tool::Grid(Some(_)) => "Click to enter the grid's end point.",
            Tool::Move(None) | Tool::Copy(None) => "Click a base point to measure from.",
            Tool::Move(Some(_)) => "Click where the base point goes, or type a distance and press Enter. Esc cancels.",
            Tool::Copy(Some(_)) => "Click where the copy's base point goes, or type a distance and press Enter. Esc cancels.",
            Tool::Rotate { place: true, .. } => "Click the centre of rotation.",
            Tool::Rotate { start: None, .. } => "Click the start of the angle, or type an angle in the Options Bar.",
            Tool::Rotate { start: Some(_), .. } => "Click the end of the angle. Esc cancels.",
            Tool::Mirror { draw: false, .. } => "Pick a wall or grid to mirror across.",
            Tool::Mirror { draw: true, start: None } => "Click the start of the mirror axis.",
            Tool::Mirror { draw: true, start: Some(_) } => "Click the end of the mirror axis.",
            Tool::Align(None) => "Pick a reference line: a grid, or a wall's face or centre line.",
            Tool::Align(Some(_)) => "Pick the wall or grid to line up with the reference.",
            Tool::Trim(None) => "Pick the first wall, on the part to keep.",
            Tool::Trim(Some(_)) => "Pick the second wall, on the part to keep.",
            Tool::Split => "Click a wall where to split it.",
            Tool::Offset => "Point to a wall: the offset goes to the cursor's side. Click to place it.",
            Tool::Dimension(refs) if refs.is_empty() => "Pick a wall face, centre line or end, a door, a window or a grid to measure from.",
            Tool::Dimension(refs) if refs.len() == 1 => "Pick the next reference, parallel to the first.",
            Tool::Dimension(_) => "Pick more references, or click away from them where the dimension line goes. Esc cancels.",
            Tool::Tag => "Click a door or window to tag it with its mark.",
            Tool::Angular(refs) if refs.len() < 2 => "Pick a wall, a grid or another line; then one that meets it.",
            Tool::Angular(_) => "Click where the arc goes: the angle on that side. Esc cancels.",
            Tool::Radial(None) => "Pick a round column.",
            Tool::Radial(Some(_)) => "Click where the radius goes. Esc cancels.",
            Tool::Text => "Type the note in the Options Bar, then click where it goes: its top left corner.",
            Tool::Spot => "Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there.",
            Tool::Section(None) => "Click the section's start: it looks to the left of the line you draw.",
            Tool::Section(Some(_)) => "Click the section's end.",
            Tool::Elevation => "Click beside the building: the elevation looks at the side you click by.",
            Tool::Column => "Click to place a column, as high as the Options Bar says.",
            Tool::Stair(None) => "Click where the stair starts: the middle of its first riser.",
            Tool::Stair(Some(_)) => "Click which way it climbs: its risers take it up to the level above. Esc cancels.",
            Tool::Roof => "Click inside the walls to roof them, on the level above: the overhang runs past their faces.",
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t(self.prompt())).color(pal().muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // How many elements are selected, by the selection filter, as Revit shows it.
                ui.label(format!(":{}", self.selection.len()));
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                style::paint(ui.painter(), Icon::Filter, r, pal().icon);
                if let Some(p) = self.canvas.cursor {
                    let u = self.session.model.units;
                    ui.monospace(format!("x {}  y {}", u.with_unit(p.0), u.with_unit(p.1)));
                }
                if self.dirty() {
                    ui.label(RichText::new(t("Unsaved changes")).color(pal().muted));
                }
            });
        });
    }

    /// Why the last command failed, in a box over the view's corner until the next one works.
    fn warning(&mut self, ctx: &Context) {
        if self.status.text.is_empty() {
            return;
        }
        let (title, color) = if self.status.error { (t("Error"), pal().error) } else { (t("Note"), pal().accent) };
        let at = self.view_rect.right_bottom() - vec2(16.0, 16.0);
        Area::new(UiId::new("warning")).order(Order::Foreground).fixed_pos(at).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).stroke(Stroke::new(1.0, color)).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong().color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Dismiss").clicked() {
                            self.status = Status::default();
                        }
                    });
                });
                ui.label(&self.status.text);
            });
        });
    }

    // ---------- the left: Properties, Project Browser ----------

    fn properties_palette(&mut self, ui: &mut Ui) {
        title_bar(ui, "Properties");
        ScrollArea::vertical().id_salt("properties").show(ui, |ui| Frame::new().inner_margin(8.0).show(ui, |ui| self.properties(ui)));
    }

    fn browser_palette(&mut self, ui: &mut Ui) {
        title_bar(ui, &tf("Project Browser · {}", &[&self.session.model.name]));
        ScrollArea::vertical().id_salt("browser").show(ui, |ui| Frame::new().inner_margin(8.0).show(ui, |ui| self.browser(ui)));
    }

    /// The Project Browser: views by kind, as Revit's tree has them.
    fn browser(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let levels: Vec<(Id, String)> = m.levels().iter().map(|(id, l)| (*id, l.name.clone())).collect();
        // Sections and elevations, by name.
        let views = |category: &str| {
            let mut v: Vec<(Id, String)> = m
                .elements
                .iter()
                .filter(|(_, e)| e.category() == category)
                .filter_map(|(id, _)| Some((*id, m.section(*id)?.name.clone())))
                .collect();
            v.sort_by(|a, b| a.1.cmp(&b.1));
            v
        };
        let (elevations, sections) = (views("elevation"), views("section"));
        // Sheets, by number, as "A101 - Name".
        let mut sheets: Vec<(Id, String)> =
            m.elements.keys().filter_map(|id| m.sheet(*id).map(|s| (*id, format!("{} - {}", s.number, s.name)))).collect();
        sheets.sort_by(|a, b| a.1.cmp(&b.1));
        let mut open = None;
        let active = self.sheet;
        let item = |ui: &mut Ui, name: &str, sheet: Sheet| ui.selectable_label(active == sheet, name).clicked().then_some(sheet);
        CollapsingHeader::new(t("Views (all)")).default_open(true).show(ui, |ui| {
            CollapsingHeader::new(t("Floor Plans")).default_open(true).show(ui, |ui| {
                for (id, name) in &levels {
                    open = item(ui, name, Sheet::Plan(*id)).or(open);
                }
            });
            CollapsingHeader::new(t("3D Views")).default_open(true).show(ui, |ui| {
                open = item(ui, "{3D}", Sheet::View3D).or(open);
            });
            for (group, views) in [("Elevations (Building Elevation)", &elevations), ("Sections (Building Section)", &sections)] {
                if !views.is_empty() {
                    CollapsingHeader::new(t(group)).default_open(true).show(ui, |ui| {
                        for (id, name) in views {
                            open = item(ui, name, Sheet::Section(*id)).or(open);
                        }
                    });
                }
            }
        });
        CollapsingHeader::new(t("Schedules/Quantities (all)")).default_open(true).show(ui, |ui| {
            for (cat, name) in SCHEDULES {
                open = item(ui, t(name), Sheet::Schedule(cat)).or(open);
            }
        });
        if !sheets.is_empty() {
            CollapsingHeader::new(t("Sheets (all)")).default_open(true).show(ui, |ui| {
                for (id, name) in &sheets {
                    open = item(ui, name, Sheet::Paper(*id)).or(open);
                }
            });
        }
        if let Some(sheet) = open {
            self.open_sheet(sheet);
        }
    }

    // ---------- the middle: view tabs, schedules ----------

    fn view_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for sheet in self.views.clone() {
                let name = self.view_name(sheet);
                if tab_button(ui, &name, sheet == self.sheet, false).clicked() {
                    self.open_sheet(sheet);
                }
                if self.views.len() > 1 && icon_button(ui, Icon::Close, &tf("Close {}", &[&name])).clicked() {
                    self.close_view(sheet);
                }
                ui.add_space(6.0);
            }
        });
    }

    fn schedule(&mut self, ui: &mut Ui, cat: &'static str) {
        Frame::new().fill(pal().surface).inner_margin(16.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let name = self.view_name(Sheet::Schedule(cat));
            ui.label(RichText::new(&name).heading());
            ui.add_space(8.0);
            match schedule::table(&self.session.model, cat) {
                Err(e) => {
                    ui.label(e);
                }
                Ok(t) if t.rows.is_empty() => {
                    ui.label(RichText::new(tf("No rows yet: the {} is empty.", &[&name.to_lowercase()])).color(pal().muted));
                }
                Ok(t) => {
                    ScrollArea::both().show(ui, |ui| {
                        Grid::new(("schedule", cat)).striped(true).spacing(vec2(28.0, 6.0)).show(ui, |ui| {
                            let units = self.session.model.units;
                            for c in &t.columns {
                                ui.label(RichText::new(lang::t(&schedule::heading(units, c))).strong());
                            }
                            ui.end_row();
                            for row in &t.rows {
                                for (v, c) in row.iter().zip(&t.columns) {
                                    let text = schedule::shown(units, c, v);
                                    if v.is_number() {
                                        ui.monospace(text)
                                    } else {
                                        ui.label(text)
                                    };
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
            }
        });
    }

    fn dialogs(&mut self, ctx: &Context) {
        self.type_properties(ctx);
        let Some(pending) = self.pending else { return };
        #[derive(PartialEq)]
        enum Choice {
            Save,
            Discard,
            Cancel,
        }
        let mut choice = None;
        let name = self.session.model.name.clone();
        let r = Modal::new(UiId::new("unsaved")).show(ctx, |ui| {
            ui.label(RichText::new(tf("Save changes to {}?", &[&name])).strong());
            ui.label(t("If you don't, the changes since it was last saved are lost."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, c) in [("Save", Choice::Save), ("Don't Save", Choice::Discard), ("Cancel", Choice::Cancel)] {
                    if ui.button(t(label)).clicked() {
                        choice = Some(c);
                    }
                }
            });
        });
        if choice.is_some() || r.should_close() {
            self.pending = None;
        }
        match choice {
            Some(Choice::Save) if self.save() => self.proceed(pending, ctx),
            Some(Choice::Discard) => self.proceed(pending, ctx),
            _ => {}
        }
    }
}

impl eframe::App for BimApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    /// Tab over the plan cycles through what's under the cursor, as in Revit, rather than moving
    /// the keyboard between widgets.
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if self.canvas.hovering() && !ctx.egui_wants_keyboard_input() {
            self.tab_cycle |= raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));
            raw.events.retain(|e| !matches!(e, Event::Key { key: Key::Tab, .. }));
        }
    }
}
