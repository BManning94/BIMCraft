//! What the user chose about updates, kept in a file of their own in their configuration folder, not in a project (which
//! travels, and which somebody else opens): whether to look when BIMCraft starts, and the version they asked not to hear about.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use semver::Version;
use serde_json::{Value, json};

/// The file is a few bytes. One over this isn't ours, and is ignored.
const MAX_FILE: u64 = 64 << 10;

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// Look for a newer version when BIMCraft starts.
    pub check: bool,
    /// A version the user chose not to be told about: neither it nor anything older is offered at startup.
    pub skip: Option<Version>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { check: true, skip: None }
    }
}

impl Prefs {
    /// The preferences in the file at `path`. Whatever is missing, damaged or not what's expected, field by field, is the default:
    /// a settings file never stops the app, or turns the check off by being wrong.
    pub fn load(path: &Path) -> Prefs {
        let text =
            std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= MAX_FILE).and_then(|_| std::fs::read_to_string(path).ok());
        let j: Value = text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        Prefs {
            check: j.get("check").and_then(Value::as_bool).unwrap_or(true),
            skip: j.get("skip").and_then(Value::as_str).and_then(|s| Version::parse(s).ok()),
        }
    }

    /// Written to `path`, whole or not at all: its folder made if need be.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&json!({"check": self.check, "skip": self.skip.as_ref().map(Version::to_string)}))
            .map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The systems whose configuration folders differ.
#[derive(Clone, Copy, Debug)]
pub enum Os {
    Windows,
    Mac,
    Other,
}

/// Where the preferences are kept: in the user's own configuration folder, as the favorites are.
pub fn default_path() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Other
    };
    path_in(os, |name| std::env::var_os(name))
}

/// [`default_path`] on `os`, where `var` looks up an environment variable.
pub fn path_in(os: Os, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let folder = match os {
        Os::Windows => get("APPDATA")?.join("BIMCraft"),
        Os::Mac => get("HOME")?.join("Library").join("Application Support").join("BIMCraft"),
        Os::Other => get("XDG_CONFIG_HOME").or_else(|| get("HOME").map(|h| h.join(".config")))?.join("bimcraft"),
    };
    Some(folder.join("update.json"))
}
