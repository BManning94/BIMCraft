//! The app driven the way a user drives it: clicks on the plan at model points, the ribbon and
//! the browser by their labels, keys. Each test then reads the model the session ended up with,
//! or what the window shows.

use bimcraft_core::element_json;
use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id};
use bimcraft_core::sample::SAMPLE;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::{BimApp, Sheet, Tool};

const BOX: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#;

fn app(script: &str) -> Harness<'static, BimApp> {
    app_at(script, 0.25)
}

/// The app with frames `step` seconds apart. Each queued event runs a frame, so a double click
/// (two clicks within 300 ms) needs short ones.
fn app_at(script: &str, step: f32) -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(script).unwrap();
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .with_step_dt(step)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h
}

fn press(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// A tool from the Architecture tab: a tool's contextual tab takes the ribbon, as in Revit.
fn tool(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Architecture");
    press(h, label);
}

/// Replace a text field's contents, as a user selecting all and typing does.
fn type_into(h: &mut Harness<BimApp>, label: &str, text: &str) {
    h.get_by_label(label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_label(label).type_text(text);
    h.run();
}

/// Of the widgets named `name`, the one highest up: a view tab over the browser's entry, a
/// ribbon tab over the tool of the same name.
fn topmost(h: &mut Harness<BimApp>, name: &str) {
    let top = h.get_all_by_label(name).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.rect().center());
    let pos = top.unwrap();
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// A view in the Project Browser, opened as screen readers do: it may lie below the browser's fold.
fn browse(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click_accesskit();
    h.run();
}

/// A primary click on the plan at a model point.
fn click(h: &mut Harness<BimApp>, p: P2) {
    click_with(h, p, Modifiers::NONE);
}

/// A click holding modifier keys (Ctrl adds to the selection, Shift takes away).
fn click_with(h: &mut Harness<BimApp>, p: P2, modifiers: Modifiers) {
    let pos = h.state().screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run();
}

/// A primary click at a point on screen.
fn click_at(h: &mut Harness<BimApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// Two primary clicks at a model point, a frame apart: a double click, in a harness with short
/// frames ([`app_at`]).
fn double_click(h: &mut Harness<BimApp>, p: P2) {
    // Let the last clicks age first: a third click within 600 ms of two others is a triple.
    h.run_steps(15);
    let pos = h.state().screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false, true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// A drag with `button` between points on screen, in steps.
fn drag_screen(h: &mut Harness<BimApp>, a: Pos2, b: Pos2, button: PointerButton) {
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button, pressed: false, modifiers: Modifiers::NONE });
    h.run();
}

/// A primary drag on the plan between model points, in steps, as a hand makes it.
fn drag(h: &mut Harness<BimApp>, from: P2, to: P2) {
    let (a, b) = (h.state().screen(from), h.state().screen(to));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
}

/// The id of the element `f` picks out.
fn find(h: &Harness<BimApp>, f: impl Fn(&Element) -> bool) -> Id {
    h.state().session.model.elements.iter().find(|(_, e)| f(e)).map(|(id, _)| *id).unwrap()
}

fn key(h: &mut Harness<BimApp>, k: Key) {
    h.key_press(k);
    h.run();
}

fn ids(h: &Harness<BimApp>, category: &str) -> Vec<Id> {
    h.state().session.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
}

fn walls(h: &Harness<BimApp>) -> Vec<(P2, P2)> {
    h.state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((w.start, w.end)),
            _ => None,
        })
        .collect()
}

fn field(h: &Harness<BimApp>, id: Id, key: &str) -> serde_json::Value {
    element_json(&h.state().session.model, id)[key].clone()
}

#[test]
fn clicking_the_plan_selects_the_element_drawn_nearest_the_cursor() {
    let mut h = app(SAMPLE);
    let door = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    assert!(h.query_by_label("Flip Facing").is_some(), "the palette shows the door's properties");
    assert!(h.query_by_label("Modify | Doors").is_some(), "the selection's contextual tab");
    click(&mut h, P2(3000.0, 1000.0));
    assert!(h.state().selection.is_empty(), "empty floor selects nothing");
}

#[test]
fn the_wall_tool_chains_clicks_and_enter_makes_them_one_undo_step() {
    let mut h = app("");
    tool(&mut h, "Wall");
    for p in [P2(-3000.0, -3000.0), P2(3000.0, -3000.0), P2(3000.0, 2000.0)] {
        click(&mut h, p);
    }
    assert!(walls(&h).is_empty(), "nothing is built before Enter");
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h), [(P2(-3000.0, -3000.0), P2(3000.0, -3000.0)), (P2(3000.0, -3000.0), P2(3000.0, 2000.0))]);
    assert_eq!(h.state().session.history(), ["Wall"]);
    assert!(matches!(&h.state().tool, Tool::Wall(pts) if pts.is_empty()), "the tool stays on for the next chain");
}

#[test]
fn escape_drops_the_chain_and_clicking_its_start_closes_it() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    key(&mut h, Key::Escape);
    assert!(walls(&h).is_empty());
    for p in [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 3000.0), P2(0.0, 3000.0), P2(0.0, 0.0)] {
        click(&mut h, p);
    }
    assert_eq!(walls(&h).len(), 4, "back on its start, the chain closes");
    key(&mut h, Key::Escape);
    assert!(matches!(h.state().tool, Tool::Modify), "Escape with no chain leaves the tool");
}

#[test]
fn doors_and_windows_go_into_the_wall_clicked() {
    let mut h = app(BOX);
    let bottom = ids(&h, "wall")[0];
    tool(&mut h, "Door");
    click(&mut h, P2(2000.0, 40.0));
    let door = ids(&h, "door");
    assert_eq!(door.len(), 1);
    assert_eq!((field(&h, door[0], "host"), field(&h, door[0], "offset")), (json!(bottom), json!(2000.0)));
    tool(&mut h, "Window");
    click(&mut h, P2(4000.0, 3000.0));
    assert!(ids(&h, "window").is_empty());
    assert!(h.state().status.error && h.state().status.text.contains("no wall"), "{:?}", h.state().status.text);
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(ids(&h, "window").len(), 1);
}

#[test]
fn rooms_go_where_clicked_and_their_area_follows_the_walls() {
    let mut h = app(BOX);
    tool(&mut h, "Room");
    click(&mut h, P2(2000.0, 3000.0));
    let room = ids(&h, "room")[0];
    assert_eq!(field(&h, room, "area_m2"), json!(48.0));
    // A partition clicked a little off the walls' centre lines: its ends snap onto them, so it
    // meets them and splits the room.
    tool(&mut h, "Wall");
    click(&mut h, P2(4040.0, 30.0));
    click(&mut h, P2(4040.0, 5970.0));
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h).last(), Some(&(P2(4040.0, 0.0), P2(4040.0, 6000.0))));
    assert_eq!(field(&h, room, "area_m2"), json!(24.24));
}

#[test]
fn properties_edit_through_modify_and_undo_puts_them_back() {
    let mut h = app(SAMPLE);
    click(&mut h, P2(3000.0, 2000.0));
    let room = h.state().selection[0];
    assert_eq!(field(&h, room, "name"), json!("Living"));
    type_into(&mut h, "Name", "Lounge");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, room, "name"), json!("Lounge"));
    assert_eq!(h.state().session.history().last(), Some(&"Modify"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, room, "name"), json!("Living"));
    press(&mut h, "Redo");
    assert_eq!(field(&h, room, "name"), json!("Lounge"));
}

#[test]
fn the_wheel_zooms_about_the_cursor_and_the_middle_button_pans() {
    let mut h = app(SAMPLE);
    let (p, q) = (P2(8000.0, 3500.0), P2(0.0, 0.0));
    let at = h.state().screen(p);
    let span = (h.state().screen(q) - at).length();
    h.hover_at(at);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!((h.state().screen(p) - at).length() < 1.0, "the point under the cursor stays put");
    assert!((h.state().screen(q) - at).length() > span * 1.2, "zoomed in");
    let to = at + vec2(100.0, 50.0);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    h.hover_at(to);
    h.run();
    h.event(Event::PointerButton { pos: to, button: PointerButton::Middle, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert!((h.state().screen(p) - to).length() < 1.0, "the plan followed the drag");
    // Hostile wheels: the view stays usable.
    for dy in [1e30, -1e30] {
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, dy),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
        let s = h.state().screen(p);
        assert!(s.x.is_finite() && s.y.is_finite());
    }
}

#[test]
fn the_browser_opens_level_plans_and_schedules() {
    let mut h = app(SAMPLE);
    for label in ["Views (all)", "Floor Plans", "Schedules/Quantities (all)"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // Its floor plan, over its ceiling plan.
    topmost(&mut h, "Level 2");
    assert_eq!(h.state().sheet, Sheet::Plan(2));
    // Below the browser's fold, past the elevations and sections: reached as screen readers do.
    h.get_by_label("Room Schedule").click_accesskit();
    h.run();
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    assert!(h.query_by_label("Kitchen").is_some(), "a row of the room schedule");
}

#[test]
fn typed_shortcuts_pick_tools_like_revit() {
    let mut h = app(SAMPLE);
    h.event(Event::Text("W".into()));
    h.event(Event::Text("A".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    h.event(Event::Text("dr".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Door));
    h.event(Event::Text("DI".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Dimension(_)));
    h.event(Event::Text("tg".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Tag));
    key(&mut h, Key::Escape);
    assert!(matches!(h.state().tool, Tool::Modify));
}

#[test]
fn the_level_and_grid_tools_add_datums() {
    let mut h = app("");
    tool(&mut h, "Level");
    assert_eq!(h.get_by_label("Elevation").value().as_deref(), Some("6000"), "the Options Bar proposes the next elevation");
    press(&mut h, "Create");
    let levels = h.state().session.model.levels().iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>();
    assert_eq!(levels.last(), Some(&("Level 3".to_string(), 6000.0)));
    topmost(&mut h, "Level 3");
    // Undoing the level leaves its plan open on nothing: the browser falls back to the lowest.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().sheet, Sheet::Plan(1));
    tool(&mut h, "Grid");
    click(&mut h, P2(-3000.0, -3000.0));
    click(&mut h, P2(-3000.0, 3000.0));
    let grid = ids(&h, "grid");
    assert_eq!(grid.len(), 1);
    assert_eq!((field(&h, grid[0], "start"), field(&h, grid[0], "end")), (json!([-3000.0, -3000.0]), json!([-3000.0, 3000.0])));
}

#[test]
fn saving_writes_the_project_and_exports_by_extension() {
    let mut h = app(SAMPLE);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.state_mut().session.execute("level", &json!({"elevation": 6000})).unwrap();
    assert!(h.state().dirty());
    let project = dir.join("house.bimcraft");
    assert!(h.state_mut().save_to(&project));
    assert!(!h.state().dirty() && h.state().session.path.as_deref() == Some(project.as_path()));
    for (name, starts) in [("house.ifc", &b"ISO-10303-21"[..]), ("plan.png", b"\x89PNG"), ("plan.svg", b"<svg")] {
        assert!(h.state_mut().save_to(&dir.join(name)), "{name}: {}", h.state().status.text);
        assert!(std::fs::read(dir.join(name)).unwrap().starts_with(starts), "{name}");
    }
    assert_eq!(h.state().session.path.as_deref(), Some(project.as_path()), "exports leave the project's path alone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_asks_before_dropping_unsaved_changes() {
    let mut h = app("");
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    // Menu items read out their shortcut too: "New Ctrl+N".
    let new = |h: &mut Harness<BimApp>| {
        press(h, "File");
        h.get_by_label_contains("New").click();
        h.run();
    };
    new(&mut h);
    press(&mut h, "Cancel");
    assert_eq!(walls(&h).len(), 1);
    new(&mut h);
    press(&mut h, "Don't Save");
    assert!(walls(&h).is_empty());
}

#[test]
fn the_ribbon_has_revit_tabs_and_a_contextual_tab_for_the_tool_or_selection() {
    let mut h = app(SAMPLE);
    for label in ["File", "Architecture", "View", "Wall"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert_eq!(h.query_all_by_label("Modify").count(), 2, "the Modify tab, and the Modify tool on Architecture");
    tool(&mut h, "Wall");
    assert!(h.query_by_label("Modify | Place Wall").is_some());
    assert!(h.query_by_label("Wall").is_none(), "the contextual tab took the ribbon");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Modify | Place Wall").is_none(), "gone with the tool");
    assert!(h.query_by_label("Wall").is_some(), "back on Architecture");
    let walls_before = ids(&h, "wall").len();
    click(&mut h, P2(10000.0, 2000.0));
    assert!(h.query_by_label("Modify | Walls").is_some());
    press(&mut h, "Delete");
    assert_eq!(ids(&h, "wall").len(), walls_before - 1, "the contextual tab's Delete");
    press(&mut h, "View");
    for label in ["Thin Lines", "Schedules", "Close Inactive", "User Interface"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
}

#[test]
fn the_type_selector_and_edit_type_change_types_as_in_revit() {
    let mut h = app(BOX);
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
    press(&mut h, "Type Selector");
    press(&mut h, "Exterior - 300mm");
    assert_eq!(field(&h, wall, "type"), json!("Exterior - 300mm"));
    // Edit Type changes the type, so every wall of it.
    press(&mut h, "Edit Type");
    type_into(&mut h, "Width", "350");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.wall.get("Exterior - 300mm").map(|t| t.thickness), Some(350.0));
    // Duplicate makes a new type, and OK puts the selected wall on it.
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Party - 250mm");
    press(&mut h, "Create");
    type_into(&mut h, "Width", "250");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.wall.get("Party - 250mm").map(|t| t.thickness), Some(250.0));
    assert_eq!(field(&h, wall, "type"), json!("Party - 250mm"));
    // With a tool on, the Type Selector picks the type of what it places.
    key(&mut h, Key::Escape);
    tool(&mut h, "Door");
    press(&mut h, "Type Selector");
    press(&mut h, "Single 800 x 2100");
    click(&mut h, P2(2000.0, 0.0));
    let door = ids(&h, "door");
    assert_eq!(field(&h, door[0], "type"), json!("Single 800 x 2100"));
}

#[test]
fn the_options_bar_sets_the_wall_height_and_chain() {
    let mut h = app("");
    tool(&mut h, "Wall");
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    // Unconnected, a field for the height comes next to the drop-down.
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Height:").value().as_deref(), Some("3000"));
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("2500");
    h.run();
    press(&mut h, "Chain");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    let wall = ids(&h, "wall");
    assert_eq!(wall.len(), 1, "without Chain, two clicks make a wall");
    assert_eq!((field(&h, wall[0], "top_level"), field(&h, wall[0], "height")), (json!(null), json!(2500.0)));
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(ids(&h, "wall").len(), 2, "and the next two another");
}

#[test]
fn views_open_in_tabs_that_switch_and_close() {
    let mut h = app(SAMPLE);
    topmost(&mut h, "Level 2");
    browse(&mut h, "Room Schedule");
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().sheet, Sheet::Plan(1));
    press(&mut h, "Close Room Schedule");
    assert!(h.query_by_label("Close Room Schedule").is_none());
    press(&mut h, "View");
    press(&mut h, "Close Inactive");
    assert!(h.query_by_label("Close Level 2").is_none(), "only the active view stays open");
    assert!(h.query_by_label("Close Level 1").is_none(), "and the last view can't close");
    assert_eq!(h.state().sheet, Sheet::Plan(1));
}

#[test]
fn ctrl_adds_shift_takes_away_and_tab_reaches_what_is_underneath() {
    let mut h = app(SAMPLE);
    let door = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    let window = find(&h, |e| matches!(e, Element::Window(o) if o.mark == "1"));
    // The south wall: drawn clockwise, it runs west.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(10000.0, 0.0) && w.end == P2(0.0, 0.0)));
    click(&mut h, P2(2000.0, 0.0));
    click_with(&mut h, P2(4300.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection, [door, window]);
    assert!(h.query_by_label("Modify | Multi-Select").is_some(), "a door and a window: two categories");
    click_with(&mut h, P2(2000.0, 0.0), Modifiers::SHIFT);
    assert_eq!(h.state().selection, [window]);
    // Over the door, Tab moves on to the wall it's in, and a click takes that.
    h.hover_at(h.state().screen(P2(2000.0, 0.0)));
    h.run();
    key(&mut h, Key::Tab);
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn dragging_across_empty_floor_selects_by_window_or_crossing() {
    let mut h = app(SAMPLE);
    let window = find(&h, |e| matches!(e, Element::Window(o) if o.type_name == "Fixed 600 x 900"));
    // The east wall: drawn clockwise, it runs south.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(10000.0, 7000.0) && w.end == P2(10000.0, 0.0)));
    let tag = find(&h, |e| matches!(e, Element::Tag(t) if t.host == window));
    // Left to right: what's wholly inside, the window and its tag. (From empty floor, clear of the
    // Bedroom's tag and the dimension at x = 10900.)
    drag(&mut h, P2(9400.0, 4200.0), P2(11400.0, 6000.0));
    assert_eq!(h.state().selection, [window, tag]);
    // Right to left: what it crosses too.
    drag(&mut h, P2(11400.0, 6000.0), P2(9400.0, 4200.0));
    let sel = &h.state().selection;
    assert!(sel.contains(&window) && sel.contains(&wall), "{sel:?}");
}

#[test]
fn dragging_a_wall_moves_it_and_the_walls_and_room_around_follow() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000]}}"));
    let room = ids(&h, "room")[0];
    drag(&mut h, P2(8000.0, 3000.0), P2(9000.0, 3000.0));
    assert_eq!(walls(&h)[1], (P2(9000.0, 0.0), P2(9000.0, 6000.0)));
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(9000.0, 0.0)), "the corner went along");
    assert_eq!(field(&h, room, "area_m2"), json!(54.0));
    assert_eq!(h.state().session.history().last(), Some(&"Move"));
}

#[test]
fn move_and_copy_take_a_base_point_and_a_destination() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Move");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(3000.0, 0.0));
    assert_eq!(field(&h, door, "at"), json!([3000.0, 0.0]));
    assert_eq!(h.state().selection, [door], "what moved stays selected");
    press(&mut h, "Copy");
    click(&mut h, P2(3000.0, 0.0));
    click(&mut h, P2(6000.0, 0.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 2);
    assert_eq!(field(&h, doors[1], "at"), json!([6000.0, 0.0]));
    assert_eq!(h.state().selection, [doors[1]], "the copy is selected, as in Revit");
}

#[test]
fn typing_a_length_puts_the_next_point_that_far_toward_the_cursor() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    // A cursor a degree off horizontal: the length goes along the horizontal.
    h.hover_at(h.state().screen(P2(1000.0, 20.0)));
    h.run();
    h.event(Event::Text("4500".into()));
    h.run();
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4500.0, 0.0))]);
}

#[test]
fn a_selected_walls_temporary_dimension_sets_its_length() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    h.get_by_label_contains("Temporary dimension").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("9000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(9000.0, 0.0)));
    assert_eq!(walls(&h)[1].0, P2(9000.0, 0.0), "the wall joined at that end followed");
}

#[test]
fn properties_edit_every_selected_element_in_one_step() {
    let mut h = app(SAMPLE);
    let one = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    let two = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "2"));
    click(&mut h, P2(2000.0, 0.0));
    click_with(&mut h, P2(4500.0, 4000.0), Modifiers::COMMAND);
    assert!(h.query_by_label("Doors (2)").is_some());
    let steps = h.state().session.history().len();
    // The row is below the palette's fold in this window; screen readers click it all the same.
    h.get_by_label("Flip Hand").click_accesskit();
    h.run();
    assert_eq!((field(&h, one, "flip_hand"), field(&h, two, "flip_hand")), (json!(true), json!(true)));
    assert_eq!(h.state().session.history().len(), steps + 1);
}

#[test]
fn properties_show_0_where_the_json_leaves_a_number_out_and_blank_where_it_is_none() {
    let mut h = app(&format!("{BOX}\nfloor {{}}\nfloor {{\"offset\": 300}}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let value = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::TextInput, label).value().unwrap_or_default();
    let floors = ids(&h, "floor");
    // A floor on its level, flat: its JSON leaves its offset and its slope out.
    h.state_mut().selection = floors[..1].to_vec();
    h.run();
    assert_eq!((value(&h, "Height Offset From Level"), value(&h, "Slope (°)")), ("0".into(), "0".into()));
    // Both floors: the slopes they share show, their offsets differ.
    h.state_mut().selection = floors;
    h.run();
    assert_eq!((value(&h, "Height Offset From Level"), value(&h, "Slope (°)")), (String::new(), "0".into()));
    // None is not 0: a section with no far clip sees all there is, a plan's top is the level above.
    h.state_mut().selection = ids(&h, "section");
    h.run();
    assert_eq!(value(&h, "Far Clip Offset"), "");
    h.state_mut().selection.clear();
    h.run();
    assert_eq!(value(&h, "Top"), "");
}

#[test]
fn clearing_a_sections_far_clip_offset_lets_it_see_all_there_is() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000], \"depth\": 4000}}\nsection {{\"start\": [4000, -1000], \"end\": [4000, 7000]}}"
    ));
    // What the field holds, and the hint it shows while blank.
    let clip = |h: &Harness<BimApp>| {
        let n = h.get_by_role_and_label(Role::TextInput, "Far Clip Offset");
        (n.value().unwrap_or_default(), n.accesskit_node().placeholder().filter(|p| !p.is_empty()).map(String::from))
    };
    let sections = ids(&h, "section");
    h.state_mut().selection = sections[..1].to_vec();
    h.run();
    h.get_by_label("Far Clip Offset").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, sections[0], "depth"), Value::Null);
    assert_eq!(clip(&h), (String::new(), Some("No Clip".into())), "blank, saying so");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, sections[0], "depth"), json!(4000.0));
    // Two sections, one clipped: blank because they differ, so no hint that neither clips.
    h.state_mut().selection = sections;
    h.run();
    assert_eq!(clip(&h), (String::new(), None));
}

#[test]
fn the_palette_keeps_its_fields_and_apply_inside_it_whatever_their_labels() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(&format!("{BOX}\nfloor {{}}"));
    h.state_mut().selection = ids(&h, "floor");
    h.run();
    let panel = egui::PanelState::load(&h.ctx, egui::Id::new("side")).map(|s| s.outer_rect).unwrap();
    assert_eq!(panel.width(), 290.0, "the palette at its default width");
    // Height Offset From Level is a long label.
    let fields = h.query_all(By::new().predicate(|n| matches!(n.role(), Role::TextInput | Role::ComboBox)));
    for n in fields.filter(|n| n.rect().intersects(panel)).chain([h.get_by_label("Apply")]) {
        assert!(panel.contains_rect(n.rect()), "{:?} at {:?} runs out of the palette, {panel:?}", n.accesskit_node().label(), n.rect());
    }
}

#[test]
fn rotate_turns_the_selection_about_its_middle() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    click(&mut h, P2(3000.0, 0.0));
    press(&mut h, "Rotate");
    // From the middle of the wall: the angle's start along it, its end a quarter turn round.
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(walls(&h), [(P2(2000.0, -2000.0), P2(2000.0, 2000.0))]);
}

#[test]
fn mirror_pick_axis_copies_across_the_line_picked() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\ngrid {\"start\": [6000, -3000], \"end\": [6000, 3000]}");
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Mirror Pick Axis");
    click(&mut h, P2(6000.0, 2000.0));
    // Copy is on, as in Revit; the copy runs back the other way, its exterior mirrored.
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(8000.0, 0.0), P2(12000.0, 0.0))]);
}

#[test]
fn align_lines_a_wall_face_up_with_a_grid() {
    let mut h = app("wall {\"points\": [[0, 500], [4000, 500]]}\ngrid {\"start\": [-1000, 0], \"end\": [5000, 0], \"name\": \"A\"}");
    topmost(&mut h, "Modify");
    press(&mut h, "Align");
    click(&mut h, P2(2000.0, 0.0));
    // The wall, picked by its lower face: that face goes onto the grid.
    click(&mut h, P2(2000.0, 420.0));
    assert_eq!(walls(&h), [(P2(0.0, 100.0), P2(4000.0, 100.0))]);
}

#[test]
fn trim_extend_to_corner_takes_two_picks() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\nwall {\"points\": [[5000, 1000], [5000, 6000]]}");
    topmost(&mut h, "Modify");
    press(&mut h, "Trim/Extend to Corner");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(5000.0, 3000.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(5000.0, 0.0)), (P2(5000.0, 0.0), P2(5000.0, 6000.0))]);
}

#[test]
fn split_element_cuts_the_wall_clicked() {
    let mut h = app(BOX);
    topmost(&mut h, "Modify");
    press(&mut h, "Split Element");
    click(&mut h, P2(4000.0, 0.0));
    let w = walls(&h);
    assert_eq!((w.len(), w[0], w[4]), (5, (P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(4000.0, 0.0), P2(8000.0, 0.0))));
}

#[test]
fn offset_copies_a_wall_to_the_cursors_side() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    topmost(&mut h, "Modify");
    press(&mut h, "Offset");
    click(&mut h, P2(2000.0, 60.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(0.0, 1000.0), P2(4000.0, 1000.0))]);
}

#[test]
fn aligned_dimensions_pick_parallel_references_then_place_the_line() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Aligned");
    assert!(h.query_by_label("Modify | Place Dimensions").is_some());
    // The face nearest the cursor: the left wall's inner face, then the right wall's; then where the line goes.
    click(&mut h, P2(95.0, 3000.0));
    click(&mut h, P2(7905.0, 3000.0));
    click(&mut h, P2(4000.0, -1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[0], "values")), (1, json!([7800.0])));
    assert_eq!(h.state().tool, Tool::Dimension(Vec::new()), "the tool stays on for the next one");
    // A wall across the first isn't picked; centre lines are, where the cursor is on them.
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(8000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[1], "values")), (2, json!([8000.0])));
    // Esc drops the references picked, then leaves the tool.
    click(&mut h, P2(0.0, 3000.0));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Dimension(Vec::new()));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn tag_by_category_tags_the_door_clicked_and_tag_all_the_rest() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nwindow {{\"at\": [8000, 3000]}}"));
    press(&mut h, "Annotate");
    press(&mut h, "Tag by Category");
    assert!(h.query_by_label("Modify | Tag").is_some());
    click(&mut h, P2(2000.0, 0.0));
    let tags = ids(&h, "tag");
    assert_eq!((tags.len(), field(&h, tags[0], "text")), (1, json!("1")));
    // Tag All, back on the Annotate tab: the window too.
    press(&mut h, "Annotate");
    press(&mut h, "Tag All");
    assert_eq!(ids(&h, "tag").len(), 2);
    // A tag drags like anything else; it sat outside, the door swinging in.
    key(&mut h, Key::Escape);
    drag(&mut h, P2(2000.0, -500.0), P2(3000.0, -500.0));
    assert_eq!(field(&h, tags[0], "at"), json!([3000.0, -500.0]));
}

#[test]
fn the_default_3d_view_opens_from_the_toolbar_and_clicks_pick_what_it_shows() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(h.query_by_label("3D View: {3D}").is_some(), "Properties shows the view");
    assert!(h.query_by_label("3D Views").is_some(), "the browser lists it");
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let door = find(&h, |e| matches!(e, Element::Door(_)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall], "the south wall's face");
    assert!(h.query_by_label("Modify | Walls").is_some());
    let at = h.state().screen3([2000.0, -25.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [door], "the door, through its opening");
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().selection.is_empty(), "the background selects nothing");
}

#[test]
fn in_3d_a_drag_orbits_the_middle_button_pans_and_the_wheel_zooms() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let cam = |h: &Harness<BimApp>| h.state().orbit.camera.unwrap();
    let (start, mid) = (cam(&h), h.state().orbit.rect.center());
    // 100 px to the right: the model turns 40° with it.
    drag_screen(&mut h, mid, mid + vec2(100.0, 0.0), PointerButton::Primary);
    let turned = cam(&h);
    assert!((turned.azimuth - (start.azimuth - 40.0)).abs() < 0.5 && turned.elevation == start.elevation, "{turned:?}");
    drag_screen(&mut h, mid, mid + vec2(0.0, 50.0), PointerButton::Middle);
    let panned = cam(&h);
    assert!(panned.target != turned.target && panned.azimuth == turned.azimuth, "{panned:?}");
    // Shift and the middle button orbit too, as in Revit: dragging down looks from higher up.
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    drag_screen(&mut h, mid, mid + vec2(0.0, 50.0), PointerButton::Middle);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let raised = cam(&h);
    assert!(raised.elevation > panned.elevation + 15.0, "{raised:?}");
    h.hover_at(mid);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let zoomed = cam(&h);
    assert!(zoomed.scale > raised.scale * 1.2, "{zoomed:?}");
    // Zoom to Fit: the whole model again, from where the camera is now.
    press(&mut h, "Zoom to Fit");
    assert!(cam(&h).scale < zoomed.scale && cam(&h).azimuth == zoomed.azimuth);
}

#[test]
fn the_section_tool_draws_a_section_whose_view_opens_from_the_browser_or_a_double_click() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Section");
    assert!(h.query_by_label("Modify | Section").is_some());
    // Drawn west to east across the box: it looks north.
    click(&mut h, P2(-1000.0, 3000.0));
    click(&mut h, P2(9000.0, 3000.0));
    let section = ids(&h, "section");
    assert_eq!((section.len(), field(&h, section[0], "looking")), (1, json!([0.0, 1.0])));
    assert!(h.query_by_label("Sections (Building Section)").is_some());
    browse(&mut h, "Section 1");
    assert_eq!(h.state().sheet, Sheet::Section(section[0]));
    // In the section, points run along its line from its start, and up: the west wall is cut at 1000.
    let west = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 6000.0)));
    click(&mut h, P2(1060.0, 1500.0));
    assert_eq!(h.state().selection, [west]);
    // Back on the plan, a double click on the section's line opens it again.
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().sheet, Sheet::Plan(1));
    double_click(&mut h, P2(-500.0, 3000.0));
    assert_eq!(h.state().sheet, Sheet::Section(section[0]));
}

#[test]
fn the_elevation_tool_looks_at_the_building_from_where_it_is_clicked() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "Elevation");
    click(&mut h, P2(4000.0, 7000.0));
    let made = ids(&h, "elevation");
    assert_eq!((field(&h, made[0], "name"), field(&h, made[0], "looking")), (json!("North"), json!([0.0, -1.0])));
    assert!(h.query_by_label("Elevations (Building Elevation)").is_some());
}

#[test]
fn sheets_take_views_from_the_view_tab_and_viewports_move_and_open() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    let sheets = ids(&h, "sheet");
    assert_eq!((sheets.len(), field(&h, sheets[0], "number")), (1, json!("A101")));
    assert_eq!(h.state().sheet, Sheet::Paper(sheets[0]));
    assert!(h.query_by_label("Sheets (all)").is_some());
    assert_eq!(h.query_all_by_label("A101 - Unnamed").count(), 2, "its entry in the browser, and its tab");
    // Place View offers the views on no sheet yet.
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "Floor Plan: Level 1");
    let vp = ids(&h, "viewport");
    assert_eq!((vp.len(), field(&h, vp[0], "view")), (1, json!(1)));
    // Points on a sheet are paper mm: the viewport, picked anywhere over it, drags 40 mm right.
    let at = field(&h, vp[0], "at");
    let (x, y) = (at[0].as_f64().unwrap(), at[1].as_f64().unwrap());
    drag(&mut h, P2(x, y), P2(x + 40.0, y));
    assert_eq!(field(&h, vp[0], "at"), json!([x + 40.0, y]));
    assert!(h.query_by_label("Modify | Viewports").is_some());
    // A double click on it opens its view.
    double_click(&mut h, P2(x + 40.0, y));
    assert_eq!(h.state().sheet, Sheet::Plan(1));
}

#[test]
fn columns_go_where_clicked_up_to_the_options_bars_height() {
    let mut h = app(BOX);
    tool(&mut h, "Column");
    assert!(h.query_by_label("Modify | Place Column").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let col = ids(&h, "column");
    assert_eq!((col.len(), field(&h, col[0], "at"), field(&h, col[0], "top_level")), (1, json!([4000.0, 3000.0]), json!(2)));
    // Unconnected, at a height typed: its own Height, not the Wall tool's.
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("2500");
    h.run();
    click(&mut h, P2(2000.0, 3000.0));
    let col = ids(&h, "column");
    assert_eq!((field(&h, col[1], "top_level"), field(&h, col[1], "height")), (json!(null), json!(2500.0)));
    tool(&mut h, "Wall");
    assert!(h.query_by_role_and_label(Role::TextInput, "Height:").is_none(), "the Wall tool still goes up to the next level");
    // Selected, its palette.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(h.state().selection, [col[0]]);
    assert!(h.query_by_label("Modify | Columns").is_some() && h.query_by_label("Top Level").is_some());
}

#[test]
fn stairs_start_where_clicked_and_climb_toward_the_second_click() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    assert!(h.query_by_label("Modify | Create Stair").is_some());
    type_into(&mut h, "Width:", "1200");
    click(&mut h, P2(1000.0, 1000.0));
    assert!(ids(&h, "stair").is_empty(), "the first click starts it");
    // Past the end of a run of all its risers: one straight run.
    click(&mut h, P2(1000.0, 6000.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "angle"), field(&h, st[0], "width")), (1, json!(90.0), json!(1200.0)));
    assert_eq!(
        (field(&h, st[0], "risers"), field(&h, st[0], "top_level"), field(&h, st[0], "landings")),
        (json!(17), json!(2), Value::Null),
        "up to Level 2, in risers of 180 at most"
    );
    assert_eq!(ids(&h, "railing").len(), 2, "with its railings, as the Options Bar says");
    assert_eq!(h.state().tool, Tool::Stair(Vec::new()), "ready for the next one");
}

#[test]
fn a_stair_run_ends_where_clicked_and_the_next_turns_toward_the_third_click() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    // 7 treads north: 8 risers, 9 to go; then to the right of the landing.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    assert!(ids(&h, "stair").is_empty(), "risers are left: the next run waits");
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "landings")), (1, json!([{"at": 8.0, "turn": -90.0}])));
    assert!(ids(&h, "railing").is_empty(), "Railings off");
}

#[test]
fn ramps_climb_at_the_options_bars_slope_and_turn_at_landings() {
    let mut h = app(BOX);
    tool(&mut h, "Ramp");
    assert!(h.query_by_label("Modify | Create Ramp").is_some());
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("600");
    h.run();
    type_into(&mut h, "Slope:", "10");
    // 360 mm up its first run, 3.6 m at 10%; then back beside it, to its left.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(4600.0, 1000.0));
    assert!(ids(&h, "ramp").is_empty());
    click(&mut h, P2(1000.0, 2200.0));
    let r = ids(&h, "ramp");
    assert_eq!((r.len(), field(&h, r[0], "height"), field(&h, r[0], "length")), (1, json!(600.0), json!(6000.0)));
    assert_eq!(field(&h, r[0], "landings"), json!([{"at": 360.0, "turn": 180.0}]));
    assert_eq!(ids(&h, "railing").len(), 2, "with its railings");
}

#[test]
fn the_railing_tool_follows_a_stair_picked_or_a_path_clicked() {
    let mut h = app(&format!("{BOX}\nstair {{\"start\": [1000, 1000], \"end\": [1000, 2000]}}"));
    let stair = ids(&h, "stair")[0];
    tool(&mut h, "Railing");
    assert!(h.query_by_label("Modify | Create Railing Path").is_some());
    // The stair picked: a railing down each side.
    click(&mut h, P2(1000.0, 3000.0));
    let on: Vec<Id> = ids(&h, "railing");
    assert_eq!(on.len(), 2);
    assert!(on.iter().all(|r| field(&h, *r, "host") == json!(stair)));
    // A path clicked away from it, then Enter: one railing along it, on the plan's level.
    click(&mut h, P2(4000.0, 4000.0));
    click(&mut h, P2(7000.0, 4000.0));
    click(&mut h, P2(7000.0, 5500.0));
    key(&mut h, Key::Enter);
    let path = ids(&h, "railing")[2];
    assert_eq!(field(&h, path, "path"), json!([[4000.0, 4000.0], [7000.0, 4000.0], [7000.0, 5500.0]]));
}

#[test]
fn the_roof_tool_roofs_the_walls_of_the_plan_on_the_level_above() {
    let mut h = app(BOX);
    tool(&mut h, "Roof");
    assert!(h.query_by_label("Modify | Create Roof Footprint").is_some());
    type_into(&mut h, "Slope:", "40");
    press(&mut h, "Gable");
    click(&mut h, P2(4000.0, 3000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    assert_eq!(
        (field(&h, roof[0], "level"), field(&h, roof[0], "slope"), field(&h, roof[0], "gable")),
        (json!(2), json!(40.0), json!(true))
    );
    assert_eq!(field(&h, roof[0], "boundary")[0], json!([-600.0, -600.0]), "500 mm past the walls' faces");
    assert_eq!(h.state().selection, roof, "finished, the roof is selected");
    assert!(h.query_by_label("Modify | Roofs").is_some() && h.query_by_label("Slope (°)").is_some());
}

#[test]
fn the_roof_tool_leaves_a_courtyard_open_inside_a_loop_of_walls_standing_free() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Roof");
    click(&mut h, P2(3000.0, 8000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    let courtyard = json!([[[6600.0, 6600.0], [6600.0, 9400.0], [9400.0, 9400.0], [9400.0, 6600.0]]]);
    assert_eq!(field(&h, roof[0], "courtyards"), courtyard, "500 mm past the courtyard walls' faces");
}

#[test]
fn opening_an_ifc_file_imports_it_and_says_what_came_in() {
    let mut h = app(SAMPLE);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-ifc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ifc = dir.join("house.ifc");
    assert!(h.state_mut().save_to(&ifc));
    assert!(h.state_mut().open_path(&ifc));
    h.run();
    assert_eq!(ids(&h, "wall").len(), 7);
    assert_eq!(h.state().session.path, None, "an import is a new project: Save asks where");
    let note = h.state().status.text.clone();
    assert!(!h.state().status.error && note.contains("7 walls") && note.contains("house.ifc"), "{note}");
    assert!(h.query_by_label("Note").is_some(), "shown over the view until dismissed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dark_mode_turns_the_window_and_the_drawing_dark() {
    use bimcraft_core::render::{PAPER, Rgb};
    let mut h = app(BOX);
    assert!(!crate::style::dark(), "light, as the system's theme is here");
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    assert!(crate::style::dark() && h.ctx.theme() == egui::Theme::Dark);
    // Ink light on a dark ground, the grid still blue.
    let (ground, ink, grid) = (crate::canvas::color(PAPER), crate::canvas::color(Rgb(25, 25, 25)), crate::canvas::color(Rgb(60, 100, 150)));
    assert!(ground.r() < 40 && ink.r() > 200 && grid.b() > grid.r() + 40, "{ground:?} {ink:?} {grid:?}");
    assert_eq!(crate::style::pal().surface, crate::style::DARK.surface);
    press(&mut h, "Dark Mode");
    assert!(!crate::style::dark() && crate::canvas::color(PAPER) == egui::Color32::WHITE);
}

#[test]
fn text_notes_go_where_clicked_in_plans_sections_and_sheets() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nsheet {{}}"), 0.05);
    let (section, sheet) = (ids(&h, "section")[0], ids(&h, "sheet")[0]);
    press(&mut h, "Annotate");
    press(&mut h, "Text");
    assert!(h.query_by_label("Modify | Place Text").is_some());
    // With nothing typed, a click says what's missing.
    click(&mut h, P2(2000.0, 2000.0));
    assert!(ids(&h, "text").is_empty() && h.state().status.error);
    type_into(&mut h, "Text:", "Tiled floor");
    click(&mut h, P2(2000.0, 2000.0));
    let notes = ids(&h, "text");
    assert_eq!(
        (field(&h, notes[0], "text"), field(&h, notes[0], "at"), field(&h, notes[0], "view")),
        (json!("Tiled floor"), json!([2000.0, 2000.0]), json!(1))
    );
    // The tool goes on working in a section, along it and up, and on a sheet, in paper mm.
    browse(&mut h, "Section 1");
    assert_eq!((h.state().sheet, &h.state().tool), (Sheet::Section(section), &Tool::Text));
    click(&mut h, P2(3000.0, 2500.0));
    h.get_by_label("A101 - Unnamed").click_accesskit();
    h.run();
    click(&mut h, P2(50.0, 50.0));
    let notes = ids(&h, "text");
    assert_eq!((field(&h, notes[1], "view"), field(&h, notes[1], "at")), (json!(section), json!([3000.0, 2500.0])));
    assert_eq!((field(&h, notes[2], "view"), field(&h, notes[2], "at")), (json!(sheet), json!([50.0, 50.0])));
}

#[test]
fn angular_and_radial_dimensions_and_spot_elevations_from_the_annotate_tab() {
    // Walls 3 to 6, a slanted one 7, a round column 8, the floor 9.
    let mut h = app(&format!(
        "{BOX}\nwall {{\"points\": [[0, 0], [3000, 3000]], \"type\": \"Interior - 100mm\"}}\ncolumn {{\"at\": [6000, 4000], \"type\": \"Round 300\"}}\nfloor {{}}"
    ));
    press(&mut h, "Annotate");
    press(&mut h, "Angular");
    assert!(h.query_by_label("Modify | Place Dimensions").is_some());
    // The south wall, the slanted one, then where the arc goes.
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(1500.0, 1500.0));
    click(&mut h, P2(2500.0, 1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((field(&h, dims[0], "kind"), field(&h, dims[0], "values")), (json!("angular"), json!([45.0])));
    assert_eq!(h.state().tool, Tool::Angular(Vec::new()), "ready for the next one");
    press(&mut h, "Annotate");
    press(&mut h, "Radial");
    click(&mut h, P2(6150.0, 4000.0));
    click(&mut h, P2(6800.0, 4800.0));
    let dims = ids(&h, "dimension");
    assert_eq!((field(&h, dims[1], "kind"), field(&h, dims[1], "values")), (json!("radial"), json!([150.0])));
    press(&mut h, "Annotate");
    press(&mut h, "Spot Elevation");
    click(&mut h, P2(5000.0, 2000.0));
    let spots = ids(&h, "spot");
    assert_eq!((field(&h, spots[0], "host"), field(&h, spots[0], "elevation")), (json!(9), json!(0.0)));
}

#[test]
fn crop_view_and_its_grips_frame_plans_and_sections() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let section = ids(&h, "section")[0];
    let crop_view = |h: &mut Harness<BimApp>| {
        // The ribbon's button; Properties has a Crop View row too.
        h.get_by_role_and_label(Role::Button, "Crop View").click();
        h.run();
    };
    press(&mut h, "View");
    crop_view(&mut h);
    // All the plan shows (the walls, the section's line), a metre past it.
    assert_eq!(field(&h, 1, "crop"), json!([[-2000.0, -1100.0], [10000.0, 7100.0]]));
    // The east edge, by its grip, dragged west.
    drag(&mut h, P2(10000.0, 3000.0), P2(4000.0, 3000.0));
    assert_eq!(field(&h, 1, "crop"), json!([[-2000.0, -1100.0], [4000.0, 7100.0]]));
    assert!(h.state().selection.is_empty(), "the grip took the drag, not a selection box");
    crop_view(&mut h);
    assert_eq!(field(&h, 1, "crop"), Value::Null);
    // A section's top, dragged down; its start, dragged along its line: what's on screen stays put.
    browse(&mut h, "Section 1");
    drag(&mut h, P2(5000.0, 3600.0), P2(5000.0, 2000.0));
    assert_eq!(field(&h, section, "crop"), json!([-600.0, 2000.0]));
    let before = h.state().screen(P2(1000.0, 700.0));
    drag(&mut h, P2(0.0, 700.0), P2(1000.0, 700.0));
    assert_eq!(field(&h, section, "start"), json!([0.0, 3000.0]));
    assert!((h.state().screen(P2(0.0, 700.0)) - before).length() < 1.0);
}

#[test]
fn a_plans_view_range_is_in_its_properties() {
    let mut h = app(BOX);
    assert!(h.query_by_label("View Range").is_some());
    type_into(&mut h, "Cut Plane", "1500");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 1, "view_range"), json!({"top": null, "cut": 1500.0, "bottom": 0.0, "depth": 0.0}));
    type_into(&mut h, "Top", "2600");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 1, "view_range")["top"], json!(2600.0));
    // Cleared, back to the level above.
    h.get_by_label("Top").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 1, "view_range")["top"], Value::Null);
}

#[test]
fn place_view_puts_schedules_and_the_3d_view_on_sheets() {
    let mut h = app_at(&format!("{BOX}\nroom {{\"point\": [4000, 3000], \"name\": \"Hall\"}}"), 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    for view in ["Schedule: Room Schedule", "3D View: {3D}"] {
        press(&mut h, "View");
        press(&mut h, "Place View");
        press(&mut h, view);
    }
    let (schedules, views) = (ids(&h, "schedule"), ids(&h, "view3d"));
    assert_eq!((schedules.len(), views.len(), ids(&h, "viewport").len()), (1, 1, 2));
    // Placed, they're no longer offered.
    press(&mut h, "View");
    press(&mut h, "Place View");
    assert!(h.query_by_label("Schedule: Room Schedule").is_none() && h.query_by_label("Schedule: Door Schedule").is_some());
    key(&mut h, Key::Escape);
    // A double click on the schedule opens it.
    let vp = ids(&h, "viewport")[0];
    let at = field(&h, vp, "at");
    double_click(&mut h, P2(at[0].as_f64().unwrap(), at[1].as_f64().unwrap()));
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
}

#[test]
fn project_units_show_and_read_lengths_in_feet_and_inches() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Project Units");
    press(&mut h, "Feet and inches");
    assert_eq!(h.state().session.model.units.name(), "ft-in");
    // The bottom wall, 8000 mm long, in Properties; a base offset typed in feet and inches.
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("26'-2 15/16\"").is_some());
    type_into(&mut h, "Base Offset", "1' 6\"");
    key(&mut h, Key::Enter);
    let wall = h.state().selection[0];
    assert_eq!(field(&h, wall, "base_offset"), json!(457.2));
}

#[test]
fn lengths_typed_while_drawing_read_in_the_project_units() {
    let mut h = app("units {\"length\": \"ft-in\"}");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    h.hover_at(h.state().screen(P2(1000.0, 20.0)));
    h.run();
    // Feet, inches and a fraction: everything after the first digit is the length's, not a shortcut.
    h.event(Event::Text("10' 6 1/2\"".into()));
    h.run();
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(0.0, 0.0)]), "no letter or mark started a tool");
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(3213.1, 0.0))]);
    // A metre, typed with its unit, in a project in feet and inches.
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 2000.0));
    h.hover_at(h.state().screen(P2(1000.0, 2000.0)));
    h.run();
    h.event(Event::Text("1m".into()));
    h.run();
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[1], (P2(0.0, 2000.0), P2(1000.0, 2000.0)));
}

#[test]
fn the_interface_speaks_portuguese() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "User Interface");
    press(&mut h, "Português (Brasil)");
    for label in ["Arquitetura", "Anotar", "Vista", "Gerenciar", "Modificar", "Propriedades", "Plantas de piso", "Intervalo da vista"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    press(&mut h, "Arquitetura");
    press(&mut h, "Parede");
    assert!(h.query_by_label("Modificar | Inserir parede").is_some());
    // A new project, in the interface's language: its levels named in Portuguese.
    key(&mut h, Key::Escape);
    press(&mut h, "Arquivo");
    // Menu items read out their shortcut too: "Novo Ctrl+N".
    h.get_by_label_contains("Novo").click();
    h.run();
    let levels: Vec<String> = h.state().session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert_eq!(levels, ["Nível 1", "Nível 2"]);
    assert!(h.query_by_label("Plantas de piso").is_some());
    assert!(h.query_all_by_label("Nível 1").count() >= 2, "in the browser, and as the view's tab");
}

#[test]
fn the_interface_speaks_spanish() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "User Interface");
    press(&mut h, "Español");
    for label in ["Arquitectura", "Anotar", "Vista", "Gestionar", "Modificar", "Propiedades", "Plantas", "Rango de vista"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    press(&mut h, "Arquitectura");
    press(&mut h, "Muro");
    assert!(h.query_by_label("Modificar | Colocar muro").is_some());
    key(&mut h, Key::Escape);
    press(&mut h, "Archivo");
    h.get_by_label_contains("Nuevo").click();
    h.run();
    let levels: Vec<String> = h.state().session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert_eq!(levels, ["Nivel 1", "Nivel 2"]);
}

#[test]
fn nothing_a_translated_interface_shows_is_left_in_english() {
    use bimcraft_core::language::Language;
    use egui_kittest::kittest::{By, NodeT};
    // A label in English that the table translates is a string the window forgot to translate.
    let english = |h: &Harness<BimApp>, l: Language| -> Vec<String> {
        h.query_all(By::new().predicate(move |n| n.label().is_some_and(|s| crate::lang::translated(l, &s).is_some_and(|x| x != s))))
            .filter_map(|n| n.accesskit_node().label())
            .collect()
    };
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nroof {{}}"));
    // The probe finds what it looks for: in English, the same labels are all English.
    assert!(english(&h, Language::Spanish).contains(&"Architecture".to_string()));
    for l in [Language::Portuguese, Language::Spanish] {
        h.state_mut().lang = l;
        h.run();
        let tr = |en: &'static str| crate::lang::translated(l, en).unwrap_or(en);
        let mut missed = Vec::new();
        for tab in ["Architecture", "Annotate", "View", "Manage", "Modify"] {
            press(&mut h, tr(tab));
            missed.extend(english(&h, l));
        }
        // A wall's Properties and its contextual tab; a door's; a roof's; the tools' Options Bars.
        let tools = ["Wall", "Roof", "Roof by Extrusion", "Floor", "Ceiling", "Shaft", "Dormer", "Stair", "Ramp", "Railing"]
            .map(|t| (P2(0.0, 0.0), Some(t)));
        for (at, tool) in [(P2(5000.0, 0.0), None), (P2(2000.0, 0.0), None), (P2(-600.0, 3000.0), None)].into_iter().chain(tools) {
            key(&mut h, Key::Escape);
            if let Some(tool) = tool {
                press(&mut h, tr("Architecture"));
                press(&mut h, tr(tool));
            } else {
                click(&mut h, at);
            }
            missed.extend(english(&h, l));
        }
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?}: left in English: {missed:?}");
    }
}

#[test]
fn the_wall_tool_draws_arcs_and_walls_on_the_location_line_picked() {
    let mut h = app("");
    tool(&mut h, "Wall");
    // Revit's Draw panel: a Start-End-Radius Arc takes its start, its end, then a point it passes through.
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(2000.0, -2000.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0))]);
    let arc = ids(&h, "wall")[0];
    assert!((field(&h, arc, "arc").as_f64().unwrap() - 180.0).abs() < 1e-6, "{}", field(&h, arc, "arc"));
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(4000.0, 0.0)]), "chained, the next one starts where it ends");
    key(&mut h, Key::Escape);
    // Straight again, on the exterior finish face: the Options Bar's Location Line.
    press(&mut h, "Line");
    h.get_by_role_and_label(Role::ComboBox, "Location Line:").click();
    h.run();
    press(&mut h, "Finish Face: Exterior");
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    key(&mut h, Key::Enter);
    let line = ids(&h, "wall")[1];
    assert_eq!((field(&h, line, "location"), field(&h, line, "arc")), (json!("exterior-finish"), Value::Null));
}

#[test]
fn a_selected_wall_attaches_flips_and_takes_another_location_line() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{BOX}\nroof {{}}"));
    click(&mut h, P2(4000.0, 0.0));
    let wall = h.state().selection[0];
    // Put on its interior face (going east, its exterior is to its left, north), it stays where it is.
    h.get_by_role_and_label(Role::ComboBox, "Location Line").click();
    h.run();
    press(&mut h, "Finish Face: Interior");
    assert_eq!((field(&h, wall, "location"), field(&h, wall, "start")), (json!("interior-finish"), json!([0.0, -100.0])));
    // Attach Top/Base, then the roof, by its edge seen overhead: the wall's top follows it.
    press(&mut h, "Attach Top/Base");
    let roof = ids(&h, "roof")[0];
    click(&mut h, P2(4000.0, -600.0));
    assert_eq!(field(&h, wall, "attach_top"), json!(roof));
    assert_eq!(h.state().selection, [wall], "the wall stays selected");
    let attached = h.get_by_role_and_label(Role::CheckBox, "Top is Attached").accesskit_node().toggled();
    assert_eq!(attached, Some(egui::accesskit::Toggled::True));
    // Flipped from the same tab, it runs the other way; detached, its top is its own again.
    press(&mut h, "Flip");
    assert_eq!(walls(&h)[0], (P2(8000.0, -100.0), P2(0.0, -100.0)));
    press(&mut h, "Detach Top/Base");
    assert_eq!(field(&h, wall, "attach_top"), Value::Null);
}

#[test]
fn type_properties_build_a_wall_types_layers() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Edit Type");
    // One structure layer as thick as the type; a finish inserted outside it, a brick 100 thick.
    press(&mut h, "Insert Layer");
    let nth = |h: &Harness<BimApp>, role: Role, label: &'static str, n: usize| {
        h.query_all(By::new().role(role).label(label)).nth(n).map(|n| n.accesskit_node().id()).unwrap()
    };
    // Its material from the library.
    let pick = nth(&h, Role::ComboBox, "Material", 0);
    h.get_by(|n| n.id() == pick).click();
    h.run();
    press(&mut h, "Brick");
    let thick = nth(&h, Role::TextInput, "Thickness", 0);
    h.get_by(|n| n.id() == thick).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == thick).type_text("100");
    h.run();
    press(&mut h, "OK");
    let t = &h.state().session.model.types.wall["Generic - 200mm"];
    assert_eq!((t.thickness, t.layers.len()), (300.0, 2));
    assert_eq!((t.layers[0].material.as_str(), t.layers[0].thickness, t.layers[1].thickness), ("Brick", 100.0, 200.0));
}

#[test]
fn duplicate_copies_a_types_layers() {
    let mut h = app(r#"wall {"points": [[0, 0], [8000, 0]], "type": "Exterior - 300mm"}"#);
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Exterior copy");
    press(&mut h, "Create");
    press(&mut h, "OK");
    let types = &h.state().session.model.types.wall;
    assert_eq!(types["Exterior copy"].layers, types["Exterior - 300mm"].layers);
}

#[test]
fn floors_go_under_the_walls_and_a_slope_arrow_slopes_them() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    assert!(h.query_by_label("Modify | Create Floor").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let floor = ids(&h, "floor");
    assert_eq!((floor.len(), field(&h, floor[0], "area_m2")), (1, json!(48.0)));
    assert_eq!(h.state().selection, floor, "finished, the floor is selected");
    assert!(h.query_by_label("Modify | Floors").is_some() && h.query_by_label("Height Offset From Level").is_some());
    // Its slope arrow, from Shape Editing: its tail, then its head, at the Options Bar's slope.
    press(&mut h, "Slope Arrow");
    type_into(&mut h, "Slope:", "3");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(7000.0, 3000.0));
    assert_eq!((field(&h, floor[0], "slope"), field(&h, floor[0], "arrow")), (json!(3.0), json!([[1000.0, 3000.0], [7000.0, 3000.0]])));
    assert_eq!((h.state().selection.as_slice(), &h.state().tool), (floor.as_slice(), &Tool::Modify), "the floor stays selected");
}

#[test]
fn the_floor_tool_leaves_a_courtyard_open_inside_a_loop_of_walls_standing_free() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Floor");
    click(&mut h, P2(3000.0, 8000.0));
    let floor = ids(&h, "floor");
    assert_eq!(floor.len(), 1);
    let courtyard = json!([[[6000.0, 6000.0], [6000.0, 10000.0], [10000.0, 10000.0], [10000.0, 6000.0]]]);
    assert_eq!((field(&h, floor[0], "openings"), field(&h, floor[0], "area_m2")), (courtyard, json!(240.0)), "the loop's centre lines");
}

#[test]
fn ceilings_go_in_the_room_clicked_and_show_in_its_ceiling_plan() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(BOX);
    tool(&mut h, "Ceiling");
    assert!(h.query_by_label("Modify | Place Ceiling").is_some());
    press(&mut h, "Type Selector");
    press(&mut h, "Tile 600 x 600");
    type_into(&mut h, "Height Offset:", "2400");
    click(&mut h, P2(4000.0, 3000.0));
    let c = ids(&h, "ceiling");
    // Out to the walls' faces, 100 mm in from their centre lines.
    assert_eq!((c.len(), field(&h, c[0], "offset"), field(&h, c[0], "area_m2")), (1, json!(2400.0), json!(45.24)));
    assert_eq!(field(&h, c[0], "type"), json!("Tile 600 x 600"));
    assert_eq!(h.state().tool, Tool::Ceiling, "ready for the next room");
    // The browser's Ceiling Plans open the level's reflected ceiling plan: its tiles pick the ceiling.
    key(&mut h, Key::Escape);
    let lowest = h.get_all_by_label("Level 1").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.accesskit_node().id());
    h.get_by(|n| Some(n.id()) == lowest).click_accesskit();
    h.run();
    assert_eq!(h.state().sheet, Sheet::Ceiling(1));
    assert_eq!(h.get_all_by_label("Ceiling Plan: Level 1").count(), 2, "its tab and the palette name the view");
    click(&mut h, P2(4000.0, 1000.0));
    assert_eq!(h.state().selection, c, "a tile line through the ceiling's middle");
    assert!(h.query_by_label("Modify | Ceilings").is_some());
}

/// What the last frame painted as text: labels, and what the canvas writes beside the cursor.
fn painted(h: &Harness<BimApp>) -> Vec<String> {
    fn texts(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| texts(&c.shape, &mut out));
    out
}

#[test]
fn the_ceiling_tool_stops_at_the_faces_of_a_loop_of_walls_in_the_room() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Ceiling");
    // Before the click, the ring the ceiling would fill and its area, 15.8² less 4.2².
    h.hover_at(h.state().screen(P2(3000.0, 3000.0)));
    h.run();
    assert!(painted(&h).iter().any(|t| t == "232.00 m²"), "{:?}", painted(&h));
    click(&mut h, P2(3000.0, 3000.0));
    let c = ids(&h, "ceiling");
    assert_eq!(c.len(), 1);
    let island = json!([[[5900.0, 5900.0], [5900.0, 10100.0], [10100.0, 10100.0], [10100.0, 5900.0]]]);
    assert_eq!((field(&h, c[0], "openings"), field(&h, c[0], "area_m2")), (island, json!(232.0)), "out to the loop's faces");
}

#[test]
fn shaft_openings_take_two_corners_and_cut_the_floors_they_pass_through() {
    let mut h = app(&format!("{BOX}\nfloor {{\"level\": 2, \"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}}"));
    tool(&mut h, "Shaft");
    assert!(h.query_by_label("Modify | Create Shaft Opening").is_some());
    click(&mut h, P2(3000.0, 2000.0));
    assert!(ids(&h, "shaft").is_empty(), "the first click is a corner");
    click(&mut h, P2(4000.0, 4000.0));
    let shaft = ids(&h, "shaft");
    assert_eq!((shaft.len(), field(&h, shaft[0], "area_m2"), field(&h, shaft[0], "top_level")), (1, json!(2.0), json!(2)));
    assert_eq!(field(&h, ids(&h, "floor")[0], "area_m2"), json!(46.0), "the floor above, less the hole");
    assert_eq!(h.state().tool, Tool::Shaft(None), "ready for the next one");
    // Selected by its cross, its palette.
    key(&mut h, Key::Escape);
    click(&mut h, P2(3500.0, 3000.0));
    assert_eq!(h.state().selection, shaft);
    assert!(h.query_by_label("Modify | Shaft Openings").is_some() && h.query_by_label("Top Constraint").is_some());
}

#[test]
fn roofs_by_extrusion_take_a_line_then_how_far_across() {
    let mut h = app(BOX);
    tool(&mut h, "Roof by Extrusion");
    assert!(h.query_by_label("Modify | Create Extrusion Roof").is_some());
    // Along the south wall, then across to the north one: a gable rising 30° to a ridge over the middle.
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(8000.0, 0.0));
    assert!(ids(&h, "roof").is_empty(), "two clicks are its line");
    click(&mut h, P2(4000.0, 6000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    let x = field(&h, roof[0], "extrusion");
    assert_eq!((&x["line"], &x["from"], &x["to"]), (&json!([[0.0, 0.0], [8000.0, 0.0]]), &json!(0.0), &json!(6000.0)));
    let ridge = field(&h, roof[0], "ridge").as_f64().unwrap();
    assert!((ridge - (3000.0 + 4000.0 * 30f64.to_radians().tan())).abs() < 0.01, "on Level 2: {ridge}");
    assert_eq!(h.state().selection, roof, "finished, the roof is selected");
    assert!(h.query_by_label("Extrusion End").is_some());
    // A shed rises along the whole line; swept the other way, it lies to the line's right.
    tool(&mut h, "Roof by Extrusion");
    press(&mut h, "Shed");
    for p in [P2(0.0, 6000.0), P2(8000.0, 6000.0), P2(4000.0, 0.0)] {
        click(&mut h, p);
    }
    let shed = ids(&h, "roof")[1];
    let x = field(&h, shed, "extrusion");
    assert_eq!((x["profile"].as_array().map(Vec::len), &x["from"], &x["to"]), (Some(2), &json!(-6000.0), &json!(0.0)));
}

#[test]
fn dormers_go_where_a_roof_slope_is_clicked() {
    let mut h = app(&format!("{BOX}\nroof {{\"slope\": 45}}"));
    tool(&mut h, "Dormer");
    assert!(h.query_by_label("Modify | Dormer").is_some());
    type_into(&mut h, "Height:", "1000");
    click(&mut h, P2(4000.0, 800.0));
    let roofs = ids(&h, "roof");
    assert_eq!(roofs.len(), 2, "{}", h.state().status.text);
    assert_eq!(field(&h, roofs[1], "join"), json!(roofs[0]), "its roof runs into the one it stands on");
    assert_eq!((ids(&h, "wall").len(), ids(&h, "window").len()), (7, 1), "its front and sides, a window in its front");
    assert_eq!(h.state().tool, Tool::Dormer, "ready for the next one");
}

#[test]
fn gable_edge_stands_a_roof_edge_upright_and_slopes_it_again() {
    let mut h = app(&format!("{BOX}\nroof {{}}"));
    // The roof, by its edge overhead in Level 1's plan, then Shape Editing's Gable Edge.
    click(&mut h, P2(-600.0, 3000.0));
    let roof = ids(&h, "roof")[0];
    assert_eq!(h.state().selection, vec![roof]);
    press(&mut h, "Gable Edge");
    // Its east end and its west end, upright; the east one again, sloping.
    click(&mut h, P2(8600.0, 3000.0));
    click(&mut h, P2(-600.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([1, 3]));
    click(&mut h, P2(8600.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([3]));
    assert_eq!((h.state().selection.as_slice(), &h.state().tool), (&[roof][..], &Tool::GableEdge), "the roof stays selected");
    // A courtyard's edges are the roof's too, numbered on after the outline's.
    let court = r#"roof {"boundary": [[20000, 0], [30000, 0], [30000, 8000], [20000, 8000]], "courtyards": [[[23000, 3000], [27000, 3000], [27000, 5000], [23000, 5000]]]}"#;
    let mut h = app(&format!("{BOX}\n{court}"));
    click(&mut h, P2(20000.0, 4000.0));
    let roof = ids(&h, "roof")[0];
    press(&mut h, "Gable Edge");
    click(&mut h, P2(25000.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([4]));
}

#[test]
fn the_materials_window_edits_the_projects_library() {
    use bimcraft_core::model::Pattern;
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Materials");
    // A new one, picked: its cut pattern, then its name.
    press(&mut h, "New");
    assert!(h.state().session.model.materials.contains_key("Material 1"));
    h.get_by_role_and_label(Role::ComboBox, "Cut Pattern").click();
    h.run();
    press(&mut h, "Diagonal");
    assert_eq!(h.state().session.model.materials["Material 1"].cut, Pattern::Diagonal);
    type_into(&mut h, "Name", "Oak");
    key(&mut h, Key::Enter);
    let m = &h.state().session.model.materials;
    assert!(m.contains_key("Oak") && !m.contains_key("Material 1"), "{:?}", m.keys().collect::<Vec<_>>());
    press(&mut h, "Delete");
    assert!(!h.state().session.model.materials.contains_key("Oak"));
    press(&mut h, "Close");
    assert!(h.query_by_label("Cut Pattern").is_none());
}

#[test]
fn a_floor_types_layers_go_from_its_top_down() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    click(&mut h, P2(4000.0, 3000.0));
    press(&mut h, "Edit Type");
    // A layer on top of its structure: tiles, 10 mm.
    press(&mut h, "Insert Layer");
    let first = |h: &Harness<BimApp>, role: Role, label: &'static str| {
        h.query_all(By::new().role(role).label(label)).next().map(|n| n.accesskit_node().id()).unwrap()
    };
    let pick = first(&h, Role::ComboBox, "Material");
    h.get_by(|n| n.id() == pick).click();
    h.run();
    press(&mut h, "Ceramic tile");
    let thick = first(&h, Role::TextInput, "Thickness");
    h.get_by(|n| n.id() == thick).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == thick).type_text("10");
    h.run();
    assert!(h.get_all_by_label("Down").count() > 0 && h.query_by_label("In").is_none(), "a floor's layers move up and down");
    press(&mut h, "OK");
    let t = &h.state().session.model.types.floor["Generic - 200mm"];
    assert_eq!((t.thickness, t.layers.len()), (210.0, 2));
    assert_eq!((t.layers[0].material.as_str(), t.layers[0].thickness), ("Ceramic tile", 10.0));
}
