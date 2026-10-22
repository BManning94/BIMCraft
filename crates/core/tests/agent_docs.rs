//! The agent docs stay layered: the root `CLAUDE.md` under its word budget, and every file the docs point at there, so a
//! feature's write-up goes to `docs/areas/` or a crate's `CLAUDE.md` and the root does not grow back (issue 30).

use std::fs;
use std::path::{Path, PathBuf};

/// The most words the root `CLAUDE.md` holds: its rules, checks, conventions and map. Everything else is moved, not cut.
const BUDGET: usize = 3000;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The files that carry agent guidance: the root, each crate's and the apps', and the write-ups by area.
fn guides() -> Result<Vec<PathBuf>, String> {
    let repo = repo();
    let mut found = vec![repo.join("CLAUDE.md"), repo.join("apps").join("CLAUDE.md")];
    for dir in ["crates", "docs/areas"] {
        let entries = fs::read_dir(repo.join(dir)).map_err(|e| format!("{dir}: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.push(path.join("CLAUDE.md"));
            } else if path.extension().is_some_and(|e| e == "md") {
                found.push(path);
            }
        }
    }
    Ok(found)
}

/// The targets of the markdown links in `text` that point at a file of the repository.
fn local_links(text: &str) -> Vec<String> {
    text.split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .map(|target| target.split('#').next().unwrap_or_default().to_string())
        .filter(|target| !target.is_empty() && !target.contains("://") && !target.starts_with("mailto:"))
        .collect()
}

#[test]
fn the_root_claude_md_stays_under_its_word_budget() -> Result<(), String> {
    let words = read(&repo().join("CLAUDE.md"))?.split_whitespace().count();
    if words > BUDGET {
        return Err(format!(
            "CLAUDE.md has {words} words, over the budget of {BUDGET}: move the detail of a feature to docs/areas/<area>.md or the \
             nearest crate's CLAUDE.md, and keep the rules, the checks, the conventions and the map here"
        ));
    }
    Ok(())
}

#[test]
fn every_file_the_agent_docs_link_to_exists() -> Result<(), String> {
    let mut missing = Vec::new();
    for guide in guides()? {
        let text = read(&guide)?;
        let dir = guide.parent().map(Path::to_path_buf).unwrap_or_default();
        for target in local_links(&text) {
            if !dir.join(&target).exists() {
                missing.push(format!("{} links to {target}", guide.strip_prefix(repo()).unwrap_or(&guide).display()));
            }
        }
    }
    if missing.is_empty() { Ok(()) } else { Err(missing.join("\n")) }
}

#[test]
fn the_root_claude_md_reaches_every_crate_note_and_area_write_up() -> Result<(), String> {
    let root = read(&repo().join("CLAUDE.md"))?;
    let mut unreached = Vec::new();
    for guide in
        guides()?.into_iter().filter(|g| g.file_name().is_some_and(|n| n != "CLAUDE.md" || g.parent().is_some_and(|p| p != repo())))
    {
        let relative = guide.strip_prefix(repo()).unwrap_or(&guide).to_string_lossy().replace('\\', "/");
        if !root.contains(&relative) {
            unreached.push(relative);
        }
    }
    if unreached.is_empty() { Ok(()) } else { Err(format!("CLAUDE.md does not link to: {}", unreached.join(", "))) }
}
