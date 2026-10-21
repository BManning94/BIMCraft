//! Revit files (`.rvt`, `.rfa`) on the command line: `info` reads one with `bimcraft-rvt`, which is Apache-2.0 only (see
//! docs/rvt-provenance.md) and so sits behind the `rvt` feature. Nothing here makes a project of one yet.

use serde_json::Value;

/// Whether a path names a Revit file: a project, a family, a template or a family template.
pub fn is_revit(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "rvt" | "rfa" | "rte" | "rft")
}

/// What `run` and `convert` say to a Revit file: its elements are not in a project yet.
pub const NOT_YET: &str = "this is a Revit file: `bimcraft-cli info` shows what is in one, but its elements do not come into a project yet";

#[cfg(not(feature = "rvt"))]
pub fn info(path: &str) -> Result<Value, String> {
    Err(format!("{path}: this build of bimcraft-cli was made without Revit support (`--no-default-features`)"))
}

/// The release, the last save, the levels and the elements by category of a Revit file. The last saver's name and the path
/// it was saved to are in the file and shown here; nothing writes them into a project or an IFC file.
#[cfg(feature = "rvt")]
pub fn info(path: &str) -> Result<Value, String> {
    use bimcraft_rvt::inventory::category_name;
    use bimcraft_rvt::{Limits, Revit};
    use serde_json::json;

    let failed = |e: bimcraft_rvt::Error| format!("{path}: {e}");
    let mut revit = Revit::open(std::path::Path::new(path), Limits::default()).map_err(failed)?;
    let basic = revit.basic_file_info().map_err(failed)?;
    // The Atom entry is a bonus: a file whose entry is damaged still shows the rest.
    let atom = revit.document_atom().ok().flatten();
    let mm = |feet: f64| (feet * 304.8 * 1000.0).round() / 1000.0;
    let elements = match revit.inventory() {
        Ok(inv) => {
            let categories: Vec<Value> = inv
                .counts()
                .iter()
                .map(|c| json!({"category": c.category, "name": category_name(c.category), "elements": c.elements, "placed": c.placed}))
                .collect();
            json!({
                "declared": inv.declared,
                "total": inv.elements.len(),
                "placed": inv.elements.iter().filter(|e| e.placed).count(),
                "categories": categories,
                "levels": inv.levels.iter().map(|l| json!({"id": l.id, "name": l.name, "elevation": mm(l.elevation_feet)})).collect::<Vec<_>>(),
                "unread_levels": inv.unread_levels,
            })
        }
        // A family or a release the reader does not know: say why, and still show the rest.
        Err(e) => json!({"error": e.to_string()}),
    };
    Ok(json!({
        "revit": {
            "release": basic.release,
            "build": basic.build,
            "document": basic.guid,
            "locale": basic.locale,
            "worksharing": basic.worksharing,
            "title": atom.as_ref().and_then(|a| a.title.clone()),
            "saved": atom.as_ref().and_then(|a| a.updated.clone()),
            "omniclass": atom.as_ref().and_then(|a| a.omniclass.clone()),
        },
        "saved_by": basic.username,
        "saved_to": basic.last_save_path,
        "elements": elements,
        "note": "saved_by is the file's Username line, which is set only when worksharing is on; saved_to is the path it was last saved to, on the saving machine. Both are shown here and written nowhere else",
    }))
}
