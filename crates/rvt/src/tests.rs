use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The commit of rvt-rs the code was ported from; the headers and NOTICE name it.
const RVT_RS_COMMIT: &str = "b6086d41b9758fd9b92f42f0a47b8345b9c58056";

#[test]
fn only_the_command_line_and_the_desktop_take_the_reader_and_only_through_a_default_feature() {
    for manifest in ["crates/core/Cargo.toml", "crates/update/Cargo.toml", "crates/ui-egui/Cargo.toml"] {
        let text = read(manifest);
        assert!(!text.contains("bimcraft-rvt"), "{manifest} depends on bimcraft-rvt: the core, the updater and the UI never do");
    }
    for manifest in ["apps/bimcraft-cli/Cargo.toml", "apps/bimcraft/Cargo.toml"] {
        let text = read(manifest);
        let dep = text.lines().find(|l| l.starts_with("bimcraft-rvt")).unwrap_or_else(|| panic!("{manifest} has no bimcraft-rvt"));
        assert!(dep.contains("optional = true"), "{manifest}: the reader is an optional dependency, `{dep}`");
        assert!(text.contains("default = [\"rvt\"]"), "{manifest}: the `rvt` feature is on by default");
        assert!(text.contains("rvt = [\"dep:bimcraft-rvt\"]"), "{manifest}: the `rvt` feature turns the reader on and nothing else");
    }
}

#[test]
fn the_crate_is_apache_only_and_carries_the_licence_and_the_notice_apache_asks_for() {
    let manifest = read("crates/rvt/Cargo.toml");
    assert!(manifest.contains("license = \"Apache-2.0\""), "the reader holds Apache-2.0 code, so it is Apache-2.0 only");
    assert!(!manifest.contains("license.workspace"), "the workspace's MIT OR Apache-2.0 does not apply to it");
    assert_eq!(read("crates/rvt/LICENSE"), read("LICENSE-APACHE"), "the Apache-2.0 text");
    let notice = read("crates/rvt/NOTICE");
    assert!(notice.contains("Copyright 2026 Griffin Long"), "rvt-rs's copyright line");
    assert!(notice.contains(RVT_RS_COMMIT), "NOTICE names the commit it was taken from");
    let readme = read("README.md");
    assert!(readme.contains("distributed under Apache-2.0"), "the README says builds with the reader are Apache-2.0");
}

#[test]
fn every_source_file_is_in_the_provenance_document_and_ported_ones_say_where_from() {
    let provenance = read("docs/rvt-provenance.md");
    let dir = root().join("crates/rvt/src");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".rs") || name == "tests.rs" {
            continue;
        }
        assert!(provenance.contains(&format!("`{name}`")), "docs/rvt-provenance.md has no row for {name}");
        let text = std::fs::read_to_string(&path).unwrap();
        // The attribution header is plain `//` comments above the module's `//!` documentation.
        let header: String = text.lines().take_while(|l| l.starts_with("//") && !l.starts_with("//!")).collect::<Vec<_>>().join("\n");
        if header.contains("rvt-rs") {
            assert!(header.contains(RVT_RS_COMMIT), "{name}: a header that names rvt-rs names its commit");
            assert!(header.contains("src/"), "{name}: and the rvt-rs file it was taken from");
            assert!(header.to_lowercase().contains("changed"), "{name}: and says the code was changed");
            assert!(header.contains("Copyright 2026 Griffin Long"), "{name}: and carries rvt-rs's copyright line");
        }
    }
}

#[test]
fn the_fence_is_written_into_the_policy_documents() {
    for doc in ["CLAUDE.md", "ROADMAP.md"] {
        let text = read(doc);
        assert!(text.contains("docs/rvt-provenance.md"), "{doc} points at the fence");
        assert!(text.contains("no public"), "{doc} says the format has no public specification");
    }
    let provenance = read("docs/rvt-provenance.md");
    for rule in ["Autodesk SDK", "ODA", "revit-ifc", "not a lawyer"] {
        assert!(provenance.contains(rule), "the provenance document states `{rule}`");
    }
}
