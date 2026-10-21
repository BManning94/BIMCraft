//! Reads Autodesk Revit files (`.rvt`, `.rfa`).
//!
//! Revit's format has no public specification, so this crate is fenced: its code is Apache-2.0 only (some of it is
//! ported from rvt-rs), `bimcraft-core` never depends on it, and every module and format finding names its source in
//! `docs/rvt-provenance.md`. It reads a user's file and nothing more: it never bundles or extracts assets (families,
//! templates, icons, help text) for the project.

pub mod atom;
mod container;
pub mod elem_table;
mod error;
#[cfg(any(test, feature = "testing"))]
pub mod fixture;
pub mod gzip;
pub mod info;
pub mod inventory;
pub mod preview;
#[cfg(any(test, feature = "testing"))]
pub mod project_fixture;
pub mod schema;

pub use container::{BASIC_FILE_INFO, Limits, Revit};
pub use error::{Error, Result};

#[cfg(test)]
mod tests;
