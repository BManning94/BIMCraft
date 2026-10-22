//! Revit files on the desktop. The reader, `bimcraft-rvt`, is Apache-2.0 only (docs/rvt-provenance.md), so this crate never depends
//! on it: the desktop's `main` attaches one as an [`Importer`], and a window without one (every test, a build with
//! `--no-default-features`) offers no `.rvt` and says why when it is given one. A file is read on a thread of its own while a box
//! says so and holds the window still; what comes back replaces the project as a new unsaved one, and a refusal, a panic or a
//! lost thread is said in the status box.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::import::Imported;
use egui::{Context, Id, Modal, RichText};

use crate::lang::{t, tf};
use crate::{BimApp, Status};

/// Reads a Revit project into a model, or says why not. It runs on a thread of its own, and may take minutes on a big file.
pub type Importer = Arc<dyn Fn(&Path) -> Result<Imported, String> + Send + Sync>;

/// The extensions File > Open offers: `.revcraft` is a project saved before the name changed, and `.rvt` is there only with a reader.
pub fn open_extensions(revit: bool) -> Vec<&'static str> {
    let mut extensions = vec!["bimcraft", "revcraft", "ifc"];
    if revit {
        extensions.push("rvt");
    }
    extensions
}

/// Whether `path` names a Revit project. Families (`.rfa`) and templates are not read here.
pub fn is_revit(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("rvt"))
}

/// An import under way: the file it reads, and where its answer comes from.
pub(crate) struct Job {
    file: String,
    answer: Receiver<Result<Imported, String>>,
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// How many elements a note of an import says were left out: `N walls left out, why (ids …)` is N. Notes that say something
/// else (an element that came in, narrowed or moved) count none.
pub(crate) fn count_left_out(note: &str) -> Option<usize> {
    let (head, _) = note.split_once(", ")?;
    let (n, what) = head.split_once(' ')?;
    what.ends_with(" left out").then(|| n.parse().ok())?
}

/// What a reader that panicked says: the payload's text when it is one.
fn stopped(panic: &(dyn Any + Send)) -> String {
    let why = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
    match why {
        Some(why) => format!("the Revit reader stopped on this file ({why})"),
        None => "the Revit reader stopped on this file".to_string(),
    }
}

impl BimApp {
    /// A file to open once the window is up, as the command line names one: read like any the system asks for, so the same
    /// questions are asked first.
    pub fn open_later(&mut self, path: PathBuf) {
        self.opens.push(path);
    }

    /// Whether a Revit file is being read.
    pub fn importing(&self) -> bool {
        self.revit_job.is_some()
    }

    /// Starts reading a Revit file on a thread of its own, when this window has a reader and is not reading one already. Whether
    /// it started: the project is replaced later, by [`poll_revit`](Self::poll_revit), and only if the file comes through.
    pub(crate) fn open_revit(&mut self, path: &Path) -> bool {
        let file = file_name(path);
        let Some(reader) = self.revit.clone() else {
            let template = "{}: this build of BIMCraft was made without Revit support (`--no-default-features`); build with the default `rvt` feature to read Revit files";
            self.status = Status { text: tf(template, &[&file]), error: true };
            return false;
        };
        if self.revit_job.is_some() {
            return false;
        }
        let (send, answer) = channel();
        let path = path.to_path_buf();
        // The reader is hostile-input code: whatever it does, the window gets an answer.
        let spawned = std::thread::Builder::new().name("revit-import".into()).spawn(move || {
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader(&path))).unwrap_or_else(|p| Err(stopped(&*p)));
            let _ = send.send(read);
        });
        if let Err(e) = spawned {
            self.status = Status { text: tf("Couldn't open {}: {}", &[&file, &e.to_string()]), error: true };
            return false;
        }
        self.revit_job = Some(Job { file, answer });
        true
    }

    /// Takes the answer of the import, when it has come: the project replaces this one, or the status box says why not.
    pub(crate) fn poll_revit(&mut self, ctx: &Context) {
        let Some(job) = &self.revit_job else { return };
        let answer = match job.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the Revit reader stopped without an answer".to_string()),
        };
        let Some(Job { file, .. }) = self.revit_job.take() else { return };
        match answer {
            Ok(imported) => self.install_revit(&file, imported),
            Err(e) => self.status = Status { text: tf("Couldn't open {}: {}", &[&file, &e]), error: true },
        }
    }

    /// A Revit project that came in is the project now: new, unsaved (saving asks for a `.bimcraft` name) and in no list of recent files.
    fn install_revit(&mut self, _file: &str, imported: Imported) {
        let elements = imported.model.elements.len();
        let left = imported.skipped.iter().filter_map(|n| count_left_out(n)).sum::<usize>();
        let schema = imported.schema;
        self.session.reset(imported.model, None);
        self.reopened();
        let count = if elements == 1 { t("1 element").to_string() } else { tf("{} elements", &[&elements.to_string()]) };
        self.status = Status { text: tf("Opened from {}: {}, {} left out.", &[&schema, &count, &left.to_string()]), error: false };
    }

    /// The box over the window while a Revit file is read: nothing under it can be touched, because the project that comes in
    /// replaces this one.
    pub(crate) fn importing_box(&self, ctx: &Context) {
        let Some(job) = &self.revit_job else { return };
        Modal::new(Id::new("revit-import")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tf("Opening a Revit file: {}", &[&job.file])).strong());
            });
        });
    }
}
