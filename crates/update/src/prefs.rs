//! What the user chose about updates, kept in a file of their own in their configuration folder, not in a project (which
//! travels, and which somebody else opens): whether to look when BIMCraft starts, and the version they asked not to hear about.

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

/// What the preference file is called.
const FILE: &str = "update.json";

/// Where the preferences are kept, given where the favorites are (`favorites::default_path` in the core, which finds the user's own
/// configuration folder): beside them, in a file of their own. The desktop passes this to [`crate::Updater::desktop`], since this
/// crate does not depend on the core.
pub fn beside(favorites: &Path) -> PathBuf {
    favorites.with_file_name(FILE)
}
