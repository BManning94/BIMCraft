//! Opening Revit files: the reader the desktop app attaches, what a window with and without it does with a `.rvt`, and what
//! comes of an import. No test reads a real Revit file: the reader here is a closure that answers as the test says.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use bimcraft_core::import::Imported;

use super::*;
use crate::revit::{Importer, open_extensions};
use crate::{OsEvent, Status};

/// What a Revit import brings: a project named Survey with one wall, and `notes` for what it left out.
fn survey(notes: &[&str]) -> Imported {
    let mut s = Session::new();
    s.script("wall {\"points\": [[0, 0], [3000, 0]]}").unwrap();
    let mut model = (*s.model).clone();
    model.name = "Survey".into();
    Imported { model, schema: "Revit 2024".into(), skipped: notes.iter().map(|n| n.to_string()).collect(), moved: None }
}

type Inbox = Rc<RefCell<Vec<OsEvent>>>;

struct Rig {
    h: Harness<'static, BimApp>,
    /// The system's requests, which the window reads once a frame.
    inbox: Inbox,
    /// The files the reader was asked for.
    asked: Arc<Mutex<Vec<PathBuf>>>,
}

/// The app on a project of four walls, with a reader that answers `answer()` whatever it is given.
fn rig(answer: impl Fn() -> Result<Imported, String> + Send + Sync + 'static) -> Rig {
    let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = Arc::clone(&asked);
    let reader: Importer = Arc::new(move |path: &Path| {
        log.lock().unwrap().push(path.to_path_buf());
        answer()
    });
    let mut h = app(BOX);
    h.state_mut().revit = Some(reader);
    let inbox: Inbox = Rc::default();
    let queue = Rc::clone(&inbox);
    h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
    Rig { h, inbox, asked }
}

impl Rig {
    /// The system asks to open `path`, as Finder or Open With does, and the frame that hears it.
    fn open(&mut self, path: &str) {
        self.inbox.borrow_mut().push(OsEvent::Open(path.into()));
        self.h.step();
    }

    /// Frames a few milliseconds apart until the import is over: the reader runs on a thread of its own.
    fn done(&mut self) {
        for _ in 0..2000 {
            if !self.h.state().importing() {
                return;
            }
            self.h.step();
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("gave up waiting for the import");
    }

    fn status(&self) -> Status {
        self.h.state().status.clone()
    }

    fn project(&self) -> String {
        self.h.state().session.model.name.clone()
    }
}

#[test]
fn a_build_without_the_reader_offers_no_revit_files_and_says_why_when_given_one() {
    assert!(!open_extensions(false).contains(&"rvt") && open_extensions(false).contains(&"ifc"));
    assert!(open_extensions(true).contains(&"rvt") && open_extensions(true).contains(&"bimcraft"));
    let mut h = app(BOX);
    assert!(h.state().revit.is_none(), "a window starts without one: the desktop's main attaches it");
    let model = Arc::clone(&h.state().session.model);
    assert!(!h.state_mut().open_path(Path::new("/nowhere/house.rvt")));
    let status = h.state().status.clone();
    assert!(status.error && status.text.starts_with("house.rvt: this build of BIMCraft was made without Revit support"), "{status:?}");
    assert!(Arc::ptr_eq(&model, &h.state().session.model), "the project is as it was");
}

#[test]
fn a_revit_file_the_system_opens_replaces_the_project_with_a_new_unsaved_one() {
    let mut r = rig(|| Ok(survey(&[])));
    let recent = std::env::temp_dir().join(format!("bimcraft-ui-revit-{}-opens.json", std::process::id()));
    r.h.state_mut().keep_recent_in(recent.clone());
    assert_eq!(walls(&r.h).len(), 4);
    r.open("/projects/house.rvt");
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(walls(&r.h).len(), 1, "the project that came in");
    assert!(r.h.state().session.path.is_none(), "saving it asks for a .bimcraft name: nothing is written back to the .rvt");
    assert!(!r.h.state().dirty() && r.h.state().session.history().is_empty());
    assert_eq!(*r.asked.lock().unwrap(), [PathBuf::from("/projects/house.rvt")]);
    assert!(r.h.state().recent.paths().is_empty() && !recent.exists(), "a Revit file does not go into File > Open Recent");
    let status = r.status();
    assert!(!status.error && status.text.starts_with("Opened from Revit 2024: "), "{status:?}");
}

#[test]
fn a_revit_file_named_on_the_command_line_is_opened_once_the_window_is_up() {
    let mut r = rig(|| Ok(survey(&[])));
    r.h.state_mut().open_later(PathBuf::from("/projects/house.rvt"));
    assert!(r.asked.lock().unwrap().is_empty(), "nothing is read before the first frame: the window comes first");
    r.h.step();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(r.asked.lock().unwrap().len(), 1);
    r.h.step();
    r.h.step();
    assert_eq!(r.asked.lock().unwrap().len(), 1, "and only once");
}

#[test]
fn unsaved_changes_are_asked_about_before_a_revit_file_replaces_the_project() {
    let mut r = rig(|| Ok(survey(&[])));
    r.h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
    assert!(r.h.state().dirty());
    r.open("/projects/house.rvt");
    r.h.step();
    assert!(r.h.query_by_label_contains("Save changes to").is_some(), "asked, not dropped");
    assert!(r.asked.lock().unwrap().is_empty(), "and nothing is read before the answer");
    press(&mut r.h, "Cancel");
    assert!(r.asked.lock().unwrap().is_empty() && r.project() != "Survey", "Cancel leaves the project and reads nothing");
    r.open("/projects/house.rvt");
    r.h.step();
    r.h.get_by_label("Don't Save").click();
    r.h.step();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(r.asked.lock().unwrap().len(), 1);
}

#[test]
fn a_file_the_reader_refuses_is_said_in_the_window_and_the_project_stays() {
    let mut r = rig(|| Err("not a Revit file: its container is damaged".into()));
    let model = Arc::clone(&r.h.state().session.model);
    r.open("/projects/bad.rvt");
    r.done();
    let status = r.status();
    assert!(status.error && status.text == "Couldn't open bad.rvt: not a Revit file: its container is damaged", "{status:?}");
    assert!(Arc::ptr_eq(&model, &r.h.state().session.model) && !r.h.state().dirty());
}

#[test]
fn the_reader_stays_attached_after_a_project_comes_in() {
    let mut r = rig(|| Ok(survey(&[])));
    r.open("/projects/a.rvt");
    r.done();
    r.open("/projects/b.rvt");
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 2, "a new window state would have lost it");
    // And New, which starts over too, keeps it.
    press(&mut r.h, "File");
    r.h.get_by_label_contains("New").click();
    r.h.run();
    r.open("/projects/c.rvt");
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 3);
}
