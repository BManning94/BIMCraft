//! Schedules: one row per element of a category, as a schedule view lists them.

use std::cmp::Ordering;

use serde_json::{Value, json};

use crate::commands::{category, element_json};
use crate::model::Model;
use crate::units::Units;

pub struct Table {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<Value>>,
}

/// The schedules there are, as views: each one's category, and its name.
pub const VIEWS: [(&str, &str); 6] = [
    ("room", "Room Schedule"),
    ("door", "Door Schedule"),
    ("window", "Window Schedule"),
    ("wall", "Wall Schedule"),
    ("floor", "Floor Schedule"),
    ("level", "Level Schedule"),
];

/// What a column measures, by its heading: a length (mm), an area (m²) or a perimeter (m).
#[derive(PartialEq)]
enum Measure {
    Length,
    Area,
    Perimeter,
}

fn measure(column: &str) -> Option<Measure> {
    match column {
        "Width" | "Height" | "Sill" | "Length" | "Thickness" | "Elevation" => Some(Measure::Length),
        "Area (m²)" => Some(Measure::Area),
        "Perimeter (m)" => Some(Measure::Perimeter),
        _ => None,
    }
}

/// A column's heading as a schedule shows it in `units`: areas in m² or ft², perimeters in m or ft.
/// (The rows themselves, as `table` gives them, keep mm, m² and m.)
pub fn heading(units: Units, column: &str) -> String {
    match measure(column) {
        Some(Measure::Area) if units.imperial() => "Area (ft²)".into(),
        Some(Measure::Perimeter) if units.imperial() => "Perimeter (ft)".into(),
        _ => column.into(),
    }
}

/// A cell as a schedule shows it in `units`: lengths as drawings show them; areas and perimeters
/// to two places, so the column lines up; other numbers to the hundredth; text as it is.
pub fn shown(units: Units, column: &str, v: &Value) -> String {
    match (v.as_f64(), measure(column)) {
        (Some(x), Some(Measure::Length)) => units.length(x),
        (Some(x), Some(Measure::Area)) => format!("{:.2}", units.area(x * 1e6).0),
        (Some(x), Some(Measure::Perimeter)) => format!("{:.2}", if units.imperial() { x * 1000.0 / 304.8 } else { x }),
        (Some(x), _) => format!("{}", (x * 100.0).round() / 100.0 + 0.0),
        (None, _) => cell(v),
    }
}

/// Each schedule's columns: a heading and the field of the element's JSON it shows (`level` as
/// the level's name). Rows sort by the first column.
const SCHEDULES: &[(&str, &[(&str, &str)])] = &[
    ("room", &[("Number", "number"), ("Name", "name"), ("Level", "level"), ("Area (m²)", "area_m2"), ("Perimeter (m)", "perimeter_m")]),
    ("door", &[("Mark", "mark"), ("Type", "type"), ("Width", "width"), ("Height", "height"), ("Level", "level"), ("Wall", "host")]),
    (
        "window",
        &[
            ("Mark", "mark"),
            ("Type", "type"),
            ("Width", "width"),
            ("Height", "height"),
            ("Sill", "sill"),
            ("Level", "level"),
            ("Wall", "host"),
        ],
    ),
    ("wall", &[("Id", "id"), ("Type", "type"), ("Level", "level"), ("Length", "length"), ("Height", "height"), ("Thickness", "thickness")]),
    ("floor", &[("Id", "id"), ("Type", "type"), ("Level", "level"), ("Area (m²)", "area_m2"), ("Thickness", "thickness")]),
    ("level", &[("Elevation", "elevation"), ("Name", "name")]),
];

pub fn table(m: &Model, category_name: &str) -> Result<Table, String> {
    let cat = category(category_name);
    let cols = SCHEDULES.iter().find(|(c, _)| *c == cat).map(|(_, cols)| *cols).ok_or_else(|| {
        format!("no schedule of `{category_name}`; there are schedules of rooms, doors, windows, walls, floors and levels")
    })?;
    let mut rows: Vec<Vec<Value>> = m
        .elements
        .iter()
        .filter(|(_, e)| e.category() == cat)
        .map(|(&id, _)| {
            let v = element_json(m, id);
            cols.iter()
                .map(|(_, key)| match *key {
                    "level" => v.get("level").and_then(Value::as_u64).and_then(|l| m.level(l)).map_or(Value::Null, |l| json!(l.name)),
                    k => v.get(k).cloned().unwrap_or(Value::Null),
                })
                .collect()
        })
        .collect();
    rows.sort_by(|a, b| natural(a.first(), b.first()));
    Ok(Table { columns: cols.iter().map(|(c, _)| *c).collect(), rows })
}

/// Numbers (and numeric text such as marks "2" and "10") by value first, then other text.
fn natural(a: Option<&Value>, b: Option<&Value>) -> Ordering {
    let key = |v: Option<&Value>| {
        let s = v.map_or_else(String::new, cell);
        (s.trim().parse::<f64>().ok(), s)
    };
    match (key(a), key(b)) {
        ((Some(x), _), (Some(y), _)) => x.total_cmp(&y),
        ((Some(_), _), (None, _)) => Ordering::Less,
        ((None, _), (Some(_), _)) => Ordering::Greater,
        ((None, x), (None, y)) => x.cmp(&y),
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

pub fn to_csv(t: &Table) -> String {
    let quote = |s: String| if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s };
    let mut out = t.columns.iter().map(|c| quote(c.to_string())).collect::<Vec<_>>().join(",");
    out.push('\n');
    for row in &t.rows {
        out.push_str(&row.iter().map(|v| quote(cell(v))).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               wall {"points": [[5000,0],[5000,6000]], "type": "Interior - 100mm"}
               room {"point": [6500, 3000], "name": "Kitchen"}
               room {"point": [2000, 3000], "name": "Living", "number": "10"}
               door {"at": [6000, 0], "mark": "D2"}
               door {"at": [2000, 0]}"#,
        )
        .unwrap();
        s
    }

    #[test]
    fn room_schedules_list_number_name_level_area_and_perimeter_in_number_order() {
        let t = table(&house().model, "rooms").unwrap();
        assert_eq!(t.columns, ["Number", "Name", "Level", "Area (m²)", "Perimeter (m)"]);
        assert_eq!(
            t.rows,
            [
                vec![json!("1"), json!("Kitchen"), json!("Level 1"), json!(18.0), json!(18.0)],
                vec![json!("10"), json!("Living"), json!("Level 1"), json!(30.0), json!(22.0)],
            ]
        );
    }

    #[test]
    fn door_schedules_read_sizes_from_the_type() {
        let t = table(&house().model, "Door").unwrap();
        assert_eq!(t.columns, ["Mark", "Type", "Width", "Height", "Level", "Wall"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][0], "1", "numbers before other marks");
        assert_eq!((&t.rows[1][0], &t.rows[1][2], &t.rows[1][3]), (&json!("D2"), &json!(900.0), &json!(2100.0)));
    }

    #[test]
    fn unknown_categories_name_the_schedules_there_are() {
        let e = table(&house().model, "stairs").err().unwrap();
        assert!(e.contains("rooms") && e.contains("doors"), "{e}");
    }

    #[test]
    fn schedules_show_lengths_and_areas_in_the_project_units() {
        let (ft, m) = (Units::FeetInches, Units::Metres);
        assert_eq!((heading(ft, "Area (m²)"), shown(ft, "Area (m²)", &json!(18.0))), ("Area (ft²)".into(), "193.75".into()));
        assert_eq!((heading(m, "Area (m²)"), shown(m, "Area (m²)", &json!(18.0))), ("Area (m²)".into(), "18.00".into()));
        assert_eq!((heading(ft, "Perimeter (m)"), shown(ft, "Perimeter (m)", &json!(18.0))), ("Perimeter (ft)".into(), "59.06".into()));
        assert_eq!([shown(m, "Width", &json!(900.0)), shown(ft, "Width", &json!(900.0))], ["0.90", "2'-11 7/16\""]);
        assert_eq!([shown(ft, "Mark", &json!("D2")), shown(ft, "Wall", &json!(4))], ["D2", "4"]);
    }

    #[test]
    fn csv_quotes_the_cells_that_need_it() {
        let t =
            Table { columns: vec!["Name", "Note"], rows: vec![vec![json!("A, B"), json!("say \"hi\"")], vec![json!(1.5), Value::Null]] };
        assert_eq!(to_csv(&t), "Name,Note\n\"A, B\",\"say \"\"hi\"\"\"\n1.5,\n");
    }
}
