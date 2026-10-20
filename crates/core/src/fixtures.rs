//! The fixtures and devices a new project starts with, BIMCraft's own drawings: what the discipline tabs place as components
//! (Plumbing's basins, sinks, showers, drains, fittings and valves, and the other tabs' as they come). Each stands with its
//! back on its x axis and its front toward -y, as the furniture does; a pipe fitting or a valve is centred on its pipe, so
//! placing it at a pipe's height lines them up.

use serde_json::{Value, json};

use crate::language::Language;

/// The starter fixtures, as family definitions by name, in `language`.
pub fn library(language: Language) -> Vec<(String, Value)> {
    let mut out = plumbing(language);
    out.extend(pipework(language));
    out
}

/// A box from `[x0, y0, x1, y1]` up from `z0` to `z1`, each a number or a formula, of material `m`.
fn block([x0, y0, x1, y1]: [Value; 4], z0: Value, z1: Value, m: &str) -> Value {
    json!({"profile": [[x0, y0], [x1.clone(), y0], [x1, y1.clone()], [x0, y1]], "from": z0, "to": z1, "material": m})
}

/// What a form cuts out of the solids listed before it: the same shape, no material.
fn void(mut form: Value) -> Value {
    if let Some(o) = form.as_object_mut() {
        o.remove("material");
        o.insert("void".into(), json!(true));
    }
    form
}

/// A cylinder standing up from `z0` to `z1`, round (`x`, `y`).
fn post(x: Value, y: Value, r: Value, z0: Value, z1: Value, m: &str) -> Value {
    json!({"circle": {"centre": [x, y], "radius": r}, "from": z0, "to": z1, "material": m})
}

fn size(name: &str, value: f64) -> Value {
    json!({"name": name, "value": value})
}

fn material(name: &str, value: &str) -> Value {
    json!({"name": name, "kind": "material", "value": value})
}

fn f(e: impl Into<String>) -> Value {
    Value::String(e.into())
}

/// Sanitary fixtures: a kitchen sink, a shower, a floor drain (the WC, the washbasin and the bathtub are in the furniture's
/// library).
fn plumbing(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height) = (w("Width"), w("Depth"), w("Height"));
    let (metal, porcelain, mat) = (w("Metal"), w("Porcelain"), w("Material"));
    let n = |x: f64| json!(x);
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-{width} / 2"));
    let mut out = Vec::new();

    // A kitchen sink on four legs: its steel top, a bowl cut into it with a drainer beside, and a tap at the back.
    let bowl = |inset: f64| {
        block(
            [
                f(format!("-({width}) / 2 + {}", 60.0 + inset)),
                f(format!("-({depth}) + {}", 60.0 + inset)),
                f(format!("{width} / 2 - 300 - {inset}")),
                n(-60.0 - inset),
            ],
            f(format!("{height} - {}", 200.0 - inset)),
            f(height.clone()),
            &metal,
        )
    };
    let leg = |east: bool, north: bool| {
        let x = if east {
            [f(format!("{width} / 2 - 50")), f(format!("{width} / 2 - 10"))]
        } else {
            [f(format!("-({width}) / 2 + 10")), f(format!("-({width}) / 2 + 50"))]
        };
        let y = if north { [n(-50.0), n(-10.0)] } else { [f(format!("-({depth}) + 10")), f(format!("-({depth}) + 50"))] };
        let [x0, x1] = x;
        let [y0, y1] = y;
        block([x0, y0, x1, y1], n(0.0), f(format!("{height} - 40")), &metal)
    };
    // The bowl's middle, whatever the sink's width: its drainer is 300 and its walls 70 wide.
    let tap_x = -120.0;
    let mut forms = vec![
        block(
            [f(minus_half_w.clone()), f(format!("-{depth}")), f(half_w.clone()), n(0.0)],
            f(format!("{height} - 40")),
            f(height.clone()),
            &metal,
        ),
        // The bowl: its walls and floor, then the cut that opens it.
        block(
            [f(format!("-({width}) / 2 + 50")), f(format!("-({depth}) + 50")), f(format!("{width} / 2 - 290")), n(-50.0)],
            f(format!("{height} - 210")),
            f(format!("{height} - 40")),
            &metal,
        ),
    ];
    forms.push(void(bowl(10.0)));
    forms.extend([(true, true), (true, false), (false, true), (false, false)].map(|(e, nn)| leg(e, nn)));
    forms.push(post(n(tap_x), n(-40.0), n(15.0), f(height.clone()), f(format!("{height} + 250")), &metal));
    forms.push(block(
        [n(tap_x - 15.0), n(-220.0), n(tap_x + 15.0), n(-25.0)],
        f(format!("{height} + 220")),
        f(format!("{height} + 250")),
        &metal,
    ));
    out.push((
        w("Kitchen Sink"),
        json!({
            "category": "plumbing",
            "parameters": [size(&width, 1200.0), size(&depth, 550.0), size(&height, 900.0), material(&mat, &metal)],
            "types": {"1200 x 550": {}, "800 x 500": {&width: 800, &depth: 500}},
            "forms": forms
        }),
    ));

    // A shower: its tray, and the head on an arm from a riser at the wall.
    let hollow = void(block(
        [f(format!("-({width}) / 2 + 60")), f(format!("-({depth}) + 60")), f(format!("{width} / 2 - 60")), n(-60.0)],
        n(25.0),
        n(50.0),
        "",
    ));
    out.push((
        w("Shower"),
        json!({
            "category": "plumbing",
            "parameters": [size(&width, 900.0), size(&depth, 900.0)],
            "types": {"900 x 900": {}, "800 x 800": {&width: 800, &depth: 800}},
            "forms": [
                block([f(minus_half_w), f(format!("-{depth}")), f(half_w), n(0.0)], n(0.0), n(50.0), &porcelain),
                hollow,
                post(n(0.0), n(-60.0), n(12.0), n(50.0), n(2100.0), &metal),
                block([n(-12.0), n(-300.0), n(12.0), n(-48.0)], n(2070.0), n(2100.0), &metal),
                post(n(0.0), n(-300.0), n(90.0), n(2040.0), n(2070.0), &metal)
            ]
        }),
    ));

    // A floor drain: its frame flush with the floor, a round opening, and the slats of its grate.
    let side = &width;
    let opening = void(post(n(0.0), n(0.0), f(format!("({side}) / 2 - 20")), n(-30.0), n(0.0), ""));
    let slat = |at: f64| {
        block([f(format!("-({side}) / 2 + 20")), n(at - 4.0), f(format!("{side} / 2 - 20")), n(at + 4.0)], n(-10.0), n(0.0), &metal)
    };
    out.push((
        w("Floor Drain"),
        json!({
            "category": "drain",
            "parameters": [size(&width, 150.0)],
            "types": {"150 x 150": {}, "100 x 100": {&width: 100}},
            "forms": [
                block([f(format!("-({side}) / 2")), f(format!("-({side}) / 2")), f(format!("{side} / 2")), f(format!("{side} / 2"))], n(-50.0), n(0.0), &metal),
                opening,
                slat(-20.0), slat(0.0), slat(20.0)
            ]
        }),
    ));
    out
}

/// Pipe fittings and a valve, centred on the pipe they join (its centre line at their z = 0, along x, and a branch toward +y):
/// they are components until fittings have connectors.
pub fn pipework(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (diameter, mat) = (w("Diameter"), w("Material"));
    let (pvc, metal) = (w("PVC"), w("Metal"));
    let r = format!("{diameter} / 2");
    // A hub's length past the pipe's axis: a pipe's diameter and a half, and 30 mm.
    let hub = format!("{diameter} * 1.5 + 30");
    // A pipe along x from `a` to `b`, and one along y; round, in the sketch planes that sweep along them.
    let along_x =
        |a: &str, b: &str, m: &str| json!({"plane": "yz", "circle": {"centre": [0, 0], "radius": r}, "from": a, "to": b, "material": m});
    let along_y =
        |a: &str, b: &str, m: &str| json!({"plane": "xz", "circle": {"centre": [0, 0], "radius": r}, "from": a, "to": b, "material": m});
    let sizes = |names: &[&str]| -> Value {
        names
            .iter()
            .map(|n| (n.to_string(), json!({&diameter: n.parse::<f64>().unwrap_or(50.0)})))
            .collect::<serde_json::Map<String, Value>>()
            .into()
    };
    let neg = |e: &str| format!("-({e})");
    vec![
        (
            w("Elbow 90"),
            json!({
                "category": "pipefitting",
                "parameters": [size(&diameter, 50.0), material(&mat, &pvc)],
                "types": sizes(&["25", "50", "110"]),
                "forms": [along_x(&neg(&hub), &r, &mat), along_y(&neg(&r), &hub, &mat)]
            }),
        ),
        (
            w("Tee"),
            json!({
                "category": "pipefitting",
                "parameters": [size(&diameter, 50.0), material(&mat, &pvc)],
                "types": sizes(&["25", "50", "110"]),
                "forms": [along_x(&neg(&hub), &hub, &mat), along_y("0", &hub, &mat)]
            }),
        ),
        (
            w("Gate Valve"),
            json!({
                "category": "pipeaccessory",
                "parameters": [size(&diameter, 50.0), material(&mat, &metal)],
                "types": sizes(&["25", "50"]),
                "forms": [
                    along_x(&neg(&hub), &hub, &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.7")), f(neg(&r)), f(format!("{diameter} * 0.7")), &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.25")), f(format!("{diameter} * 0.7")), f(format!("{diameter} * 1.6")), &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.8")), f(format!("{diameter} * 1.6")), f(format!("{diameter} * 1.7")), &mat)
                ]
            }),
        ),
    ]
}
