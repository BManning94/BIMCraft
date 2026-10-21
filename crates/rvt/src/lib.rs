//! Reads Autodesk Revit files (`.rvt`, `.rfa`).
//!
//! Revit's format has no public specification, so this crate is fenced: its code is Apache-2.0 only (some of it is
//! ported from rvt-rs), `bimcraft-core` never depends on it, and every module and format finding names its source in
//! `docs/rvt-provenance.md`. It reads a user's file and nothing more: it never bundles or extracts assets (families,
//! templates, icons, help text) for the project.

#[cfg(test)]
mod tests;
