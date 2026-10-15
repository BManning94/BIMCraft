//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

mod commands;
pub mod engine;
mod font;
pub mod geom;
pub mod ifc;
pub mod import;
pub mod language;
pub mod model;
pub mod pdf;
pub mod render;
pub mod sample;
pub mod schedule;
pub mod sheet;
pub mod step;
pub mod units;
pub mod view3d;
pub mod walls;

pub use commands::{element_json, roof_footprint};

#[cfg(test)]
mod tests;
