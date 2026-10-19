//! Updating the macOS app in place. BIMCraft.app comes out of the disk image into a hidden copy beside the installed one, which has to
//! verify and to run before it takes the installed one's place, by two renames that can be undone. macOS lets a running app's files go,
//! so nothing waits for this window to close, and no helper copy is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::Duration;

use crate::detect::plist_string;
use crate::exec::{run, says_version};

/// The app, in the disk image and where it is installed.
const APP: &str = "BIMCraft.app";

/// How long each program gets. The new app runs once, and macOS looks a program over the first time it runs, which can take a while.
const EACH: Duration = Duration::from_secs(60);

/// The macOS programs an update runs, so that a test can stand in for each.
pub struct Tools {
    pub hdiutil: PathBuf,
    pub ditto: PathBuf,
    pub codesign: PathBuf,
    pub open: PathBuf,
}

impl Tools {
    /// By absolute path: nothing here is found through `PATH`, which a launcher can set to anything.
    pub fn system() -> Tools {
        let at = |name: &str| Path::new("/usr/bin").join(name);
        Tools { hdiutil: at("hdiutil"), ditto: at("ditto"), codesign: at("codesign"), open: at("open") }
    }
}

/// Replaces the app at `bundle` with the one in the disk image `dmg`, a download that passed its checks, and starts it with `project`
/// open. `work` is a folder of the update's own, where the image is mounted. The new app is copied beside the installed one, and must
/// verify and say it is `version` before it takes its place: if it doesn't, the installed one is as it was. The caller closes this
/// window afterwards.
pub fn replace(dmg: &Path, bundle: &Path, work: &Path, version: &str, project: Option<&Path>) -> Result<(), String> {
    replace_with(dmg, bundle, work, version, project, &Tools::system(), EACH)
}

/// [`replace`], with other programs and a limit on each.
pub fn replace_with(
    dmg: &Path,
    bundle: &Path,
    work: &Path,
    version: &str,
    project: Option<&Path>,
    tools: &Tools,
    limit: Duration,
) -> Result<(), String> {
    let folder = bundle.parent().ok_or("the app has no folder")?;
    let (staged, old) =
        (folder.join(format!(".BIMCraft-update-{}.app", process::id())), folder.join(format!(".BIMCraft-old-{}.app", process::id())));
    let ready = stage(dmg, work, &staged, tools, limit).and_then(|()| check(&staged, version, tools, limit));
    if let Err(e) = ready {
        let _ = fs::remove_dir_all(&staged);
        return Err(e);
    }
    swap(bundle, &staged, &old)?;
    let mut open = Command::new(&tools.open);
    open.arg("-n").arg(bundle);
    if let Some(project) = project {
        open.arg("--args").arg(project);
    }
    run(&mut open, limit).map(drop).map_err(|e| format!("the new version is installed, but starting it failed: {e}"))
}

/// Mounts `dmg` under `work` and copies the app out of it to `staged`, a folder made now: a name that someone else put there, as a
/// link to a folder of yours, is no place to copy to. The image is let go either way.
fn stage(dmg: &Path, work: &Path, staged: &Path, tools: &Tools, limit: Duration) -> Result<(), String> {
    let mount = work.join("mount");
    fs::create_dir_all(&mount).map_err(|e| format!("{}: {e}", mount.display()))?;
    let attach = ["attach", "-nobrowse", "-readonly", "-noverify", "-noautoopen", "-mountpoint"];
    run(Command::new(&tools.hdiutil).args(attach).arg(&mount).arg(dmg), limit)?;
    let app = mount.join(APP);
    let copied = if app.is_dir() {
        fs::create_dir(staged)
            .map_err(|e| format!("{}: {e}", staged.display()))
            .and_then(|()| run(Command::new(&tools.ditto).arg(&app).arg(staged), limit).map(drop))
    } else {
        Err(format!("the disk image holds no {APP}"))
    };
    // Forced: nothing of ours is open on it any more, and a volume that Spotlight is looking at can say "busy" for a while.
    let _ = run(Command::new(&tools.hdiutil).args(["detach", "-force"]).arg(&mount), limit);
    copied
}

/// The staged app is intact, names a program that runs, and says it is `version`.
fn check(staged: &Path, version: &str, tools: &Tools, limit: Duration) -> Result<(), String> {
    run(Command::new(&tools.codesign).args(["--verify", "--strict", "--deep"]).arg(staged), limit)?;
    let contents = staged.join("Contents");
    let plist = fs::read_to_string(contents.join("Info.plist")).map_err(|e| format!("Info.plist: {e}"))?;
    // One name, not a path: where the program stands is the bundle's to say, and it stands in the bundle.
    let program = plist_string(&plist, "CFBundleExecutable").filter(|name| Path::new(name).file_name().is_some_and(|f| f == *name));
    let program = program.ok_or("the app's Info.plist names no program")?;
    says_version(&run(Command::new(contents.join("MacOS").join(program)).arg("--version"), limit)?, version)
}

/// `staged` in the place of `bundle`, by two renames, and `staged` is spent: it is put in, or removed once the old app has been put
/// back, or, if the old app can't be put back, kept, since between them they are all that is left. The old app is removed once the
/// new one is in.
pub fn swap(bundle: &Path, staged: &Path, old: &Path) -> Result<(), String> {
    if let Err(e) = fs::rename(bundle, old) {
        let _ = fs::remove_dir_all(staged);
        return Err(format!("{}: {e}", bundle.display()));
    }
    if let Err(e) = fs::rename(staged, bundle) {
        return Err(match fs::rename(old, bundle) {
            Ok(()) => {
                let _ = fs::remove_dir_all(staged);
                format!("{}: {e}", bundle.display())
            }
            Err(back) => format!(
                "{}: {e}, and the old version, now {}, could not be put back: {back} (the new one is {})",
                bundle.display(),
                old.display(),
                staged.display()
            ),
        });
    }
    let _ = fs::remove_dir_all(old);
    Ok(())
}
