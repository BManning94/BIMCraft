//! Linked models: Manage > Link Model asks for a project or an IFC and links it; Reload Links reads the files again.

use std::path::Path;

use serde_json::json;

use crate::BimApp;
use crate::lang::t;

impl BimApp {
    /// Link Model (LK): the file the user picks.
    pub(crate) fn link_model_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter(t("BIMCraft project or IFC"), &["bimcraft", "ifc"]).pick_file() {
            self.link_path(&path);
        }
    }

    /// Links the project at `path`, kept relative to this project's folder where it lies under it, so the two move together.
    pub(crate) fn link_path(&mut self, path: &Path) {
        let folder = self.session.path.as_deref().and_then(Path::parent);
        let kept = folder.and_then(|dir| path.strip_prefix(dir).ok()).unwrap_or(path);
        self.run("link", json!({"path": kept.to_string_lossy()}));
    }
}
