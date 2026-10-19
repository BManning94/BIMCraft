//! Updating a Linux AppImage in place. An AppImage is one file, and Linux lets a program's file be replaced while it runs, so the
//! update is the new file renamed over the old one, once the new one has shown that it runs here. No helper copy is needed.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::exec;

/// How long the new build gets to print its version: an AppImage run without FUSE is unpacked first.
const TRY: Duration = Duration::from_secs(60);

/// Replaces the AppImage at `target` with `new`, a download that passed its checks, and starts it with `project` open. The new file
/// is copied beside the target, so that renaming it over the target is one step, and it must run and say it is `version` before
/// that: if it doesn't, the target is as it was. The caller closes this window afterwards.
pub fn replace(new: &Path, target: &Path, version: &str, project: Option<&Path>) -> Result<(), String> {
    replace_within(new, target, version, project, TRY)
}

/// [`replace`], waiting at most `limit` for each try of the new build to answer.
pub fn replace_within(new: &Path, target: &Path, version: &str, project: Option<&Path>, limit: Duration) -> Result<(), String> {
    let name = target.file_name().ok_or("the AppImage has no name")?.to_string_lossy();
    let staged = target.with_file_name(format!(".{name}.update-{}", std::process::id()));
    let done = stage(new, target, &staged, version, limit)
        .and_then(|unpack| fs::rename(&staged, target).map(|()| unpack).map_err(|e| format!("{}: {e}", target.display())));
    let unpack = match done {
        Ok(unpack) => unpack,
        Err(e) => {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
    };
    // The window closes next, which is no time to learn that the new one won't start.
    relaunch(target, project, unpack).spawn().map(drop).map_err(|e| format!("the new version is installed, but starting it failed: {e}"))
}

/// The new file beside the target, as runnable as the target and shown to run: whether it had to be asked to unpack itself for that.
fn stage(new: &Path, target: &Path, staged: &Path, version: &str, limit: Duration) -> Result<bool, String> {
    copy_new(new, staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    runnable(staged, target)?;
    let ask = |unpack: bool| {
        let mut command = Command::new(staged);
        command.arg("--version");
        if unpack {
            command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        }
        exec::run(&mut command, limit)
    };
    // As the user would run it, from its own mount. Where that fails (no FUSE, say), once more asking it to unpack itself first,
    // which needs none, and then it is started that way too.
    match ask(false) {
        Ok(said) => exec::says_version(&said, version).map(|()| false),
        Err(_) => exec::says_version(&ask(true)?, version).map(|()| true),
    }
}

/// The bytes of `new` in a file made now at `staged`, which must not be there already: a name that someone else put there, as a link
/// to a file of yours, is no place to write.
fn copy_new(new: &Path, staged: &Path) -> io::Result<()> {
    let mut from = File::open(new)?;
    let mut to = OpenOptions::new().write(true).create_new(true).open(staged)?;
    io::copy(&mut from, &mut to).map(drop)
}

/// `file` with the mode of `like` and the right to run.
#[cfg(unix)]
fn runnable(file: &Path, like: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(like).map_err(|e| format!("{}: {e}", like.display()))?.permissions().mode();
    fs::set_permissions(file, fs::Permissions::from_mode(mode | 0o111)).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(not(unix))]
fn runnable(_file: &Path, _like: &Path) -> Result<(), String> {
    Ok(())
}

/// The command that starts the AppImage again with `project` open, asking it to unpack itself first when it had to be asked to be
/// tried. Nothing of this run's AppImage environment goes along: the new runtime sets its own.
pub fn relaunch(target: &Path, project: Option<&Path>, unpack: bool) -> Command {
    let mut command = Command::new(target);
    command.args(project).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for name in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
        command.env_remove(name);
    }
    if unpack {
        command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
    }
    command
}
