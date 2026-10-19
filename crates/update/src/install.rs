//! Installing a downloaded update on Windows. The installer upgrades BIMCraft in place, which it can't while the app runs, and
//! the app can't wait for it, so the app hands over to a copy of itself and exits. The copy waits until the app is gone,
//! checks the installer's SHA-256 once more, runs it, and starts BIMCraft again with the same project. A copy because the
//! installer replaces the app's own file, which a running program holds.
//!
//! Nothing here uses a shell or a script: the copy is this program, started with `--finish-update`, and everything it runs is
//! given its arguments one by one, so no path can be read as a command. The copy runs from a folder that anything running as
//! the user can write to, so it runs Windows Installer by its full path, holds the installer open while that runs, does nothing
//! for arguments that aren't what an update makes, and an app with administrator rights never hands over to one.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use crate::{parse_digest, tag_version};

/// What a folder made for an update starts with, which [`sweep`] looks for.
pub const FOLDER_PREFIX: &str = "bimcraft-update-";

/// The argument that makes the program the copy that finishes an update, not the app.
const FLAG: &str = "--finish-update";

/// How old an update's folder must be to be swept: a download and an install take minutes, not a day.
const STALE: Duration = Duration::from_secs(24 * 60 * 60);

/// What Windows Installer exits with when it can't open the package it's given (ERROR_INSTALL_PACKAGE_OPEN_FAILED).
const PACKAGE_UNOPENABLE: i32 = 1620;

/// What the copy is told: the installer, the SHA-256 it was checked against, the program to start again, and the project to open in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Finish {
    pub msi: PathBuf,
    pub sha256: [u8; 32],
    pub exe: PathBuf,
    pub project: Option<PathBuf>,
}

/// Whether this program is the one the Windows installer put in Program Files, running with a user's rights: the only copy an
/// update hands over for. A build from source, or a copy kept anywhere else, is told of the new version but not replaced, and
/// so is a copy running as an administrator: it would start the copy that runs the installer with those rights, from a folder
/// other programs can write to.
pub fn installed_here() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
        && match (std::env::current_exe(), std::env::var("ProgramFiles")) {
            (Ok(exe), Ok(folder)) => is_installed(&exe.to_string_lossy(), &folder) && !exe.parent().is_some_and(writable),
            _ => false,
        }
}

/// Whether this process can write in `folder`. A program's folder under Program Files is for administrators: a process that can
/// write there is running as one (or the folder was opened to everyone, which is no better). Nothing is left behind.
pub fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".bimcraft-probe-{}", std::process::id()));
    let made = OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    if made {
        let _ = fs::remove_file(&probe);
    }
    made
}

/// [`installed_here`] for the program at `exe`, with Program Files at `program_files`: paths as Windows writes them, case aside.
pub fn is_installed(exe: &str, program_files: &str) -> bool {
    let plain = |s: &str| s.trim_start_matches("\\\\?\\").replace('\\', "/").trim_end_matches('/').to_lowercase();
    !program_files.is_empty() && plain(exe) == format!("{}/bimcraft/bimcraft.exe", plain(program_files))
}

/// The arguments the copy is started with, which [`parse`] reads back.
pub fn helper_args(msi: &Path, sha256: &[u8; 32], exe: &Path, project: Option<&Path>) -> Vec<OsString> {
    let digest: String = sha256.iter().map(|b| format!("{b:02x}")).collect();
    let mut args = vec![OsString::from(FLAG), msi.into(), format!("sha256:{digest}").into(), exe.into()];
    args.extend(project.map(OsString::from));
    args
}

/// What `args` (the program's, after its own name) ask for, when it's a handover: exactly what [`helper_args`] makes.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Finish> {
    let args: Vec<OsString> = args.into_iter().collect();
    let [flag, msi, sha256, exe, project @ ..] = args.as_slice() else { return None };
    if flag != FLAG || project.len() > 1 {
        return None;
    }
    Some(Finish { msi: msi.into(), sha256: parse_digest(sha256.to_str()?)?, exe: exe.into(), project: project.first().map(PathBuf::from) })
}

/// Hands over to a copy of this program, which the caller then lets run by exiting: the app's window closes, and the copy does the rest.
pub fn start(msi: &Path, sha256: &[u8; 32], project: Option<&Path>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't tell which program this is: {e}"))?;
    start_with(&exe, msi, sha256, project)
}

/// [`start`] for the program at `exe`: copied beside the installer, in the folder of its own, and started.
pub fn start_with(exe: &Path, msi: &Path, sha256: &[u8; 32], project: Option<&Path>) -> Result<(), String> {
    let copy = msi.with_file_name(format!("bimcraft-update{}", std::env::consts::EXE_SUFFIX));
    fs::copy(exe, &copy).map_err(|e| format!("{}: {e}", copy.display()))?;
    let mut child = Command::new(&copy)
        .args(helper_args(msi, sha256, exe, project))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", copy.display()))?;
    // The pipe stays open as long as this process lives and closes when it exits: the copy reads it to the end to know.
    std::mem::forget(child.stdin.take());
    Ok(())
}

/// Whether `finish` is what [`start`] makes: an installer named for a version, in a folder an update made, and the program in
/// Program Files (`program_files`, where Windows keeps programs; none leaves that out) to start again. Anyone can run this program
/// with these arguments, so the copy checks them before it deletes or starts anything.
pub fn plausible(finish: &Finish, program_files: Option<&str>) -> bool {
    let name = finish.msi.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let version = name.strip_prefix("bimcraft-").and_then(|n| n.strip_suffix("-windows-x64.msi"));
    let named = version.and_then(|v| tag_version(&format!("v{v}"))).is_some();
    let folder = finish.msi.parent().and_then(Path::file_name).and_then(|f| f.to_str()).is_some_and(|f| f.starts_with(FOLDER_PREFIX));
    named && folder && program_files.is_none_or(|pf| is_installed(&finish.exe.to_string_lossy(), pf))
}

/// The copy's part, run from `main` for `--finish-update`: the exit code says whether the installer ran.
pub fn run(finish: &Finish) -> ExitCode {
    let program_files = std::env::var("ProgramFiles").ok();
    if run_with(finish, &mut io::stdin(), program_files.as_deref(), std::env::var_os("SystemRoot")) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// [`run`] on this `parent` (its end is the app exiting) and Windows' own folders: nothing at all happens unless `finish` is
/// [`plausible`]. Whether the installer ran.
pub fn run_with(finish: &Finish, parent: &mut dyn Read, program_files: Option<&str>, system_root: Option<OsString>) -> bool {
    plausible(finish, program_files) && finish_with(finish, parent, &msiexec_path(system_root))
}

/// Windows Installer's program by its full path, given Windows' folder: the copy runs from a folder that anything running as the
/// user can write to, and a program started by its bare name is looked for in the folder it runs from first.
pub fn msiexec_path(system_root: Option<OsString>) -> PathBuf {
    system_root.map_or_else(|| PathBuf::from("msiexec"), |root| Path::new(&root).join("System32").join("msiexec.exe"))
}

/// Waits until `parent` reaches its end (the app has exited, taking its end of the pipe with it), runs `installer` on the file
/// if it is still the one that was checked, and starts the app again whatever came of that: an installer the user cancelled
/// leaves the old version to run. Whether the installer ran.
pub fn finish_with(finish: &Finish, parent: &mut dyn Read, installer: &Path) -> bool {
    finish_after(finish, parent, installer, PACKAGE_UNOPENABLE)
}

/// [`finish_with`], told the exit code that means the installer couldn't open the file it was given (a stand-in for the installer
/// in a test can't exit with 1620, which a shell cuts to eight bits).
pub fn finish_after(finish: &Finish, parent: &mut dyn Read, installer: &Path, unopenable: i32) -> bool {
    let _ = io::copy(parent, &mut io::sink());
    let ran = run_checked(finish, installer, unopenable);
    let _ = fs::remove_file(&finish.msi);
    let _ = Command::new(&finish.exe).args(&finish.project).spawn();
    ran
}

/// Opens the installer for reading, and on Windows so that nothing else can write to it or replace it while it's open.
fn hold(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    // FILE_SHARE_READ: others may read it, as Windows Installer does, and nothing more.
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 1);
    options.open(path)
}

/// Runs `installer` on the file if it is still the one that was checked, and says whether it did. The file is held open while it
/// runs, so nothing can swap it between that check and the installer reading it (the time you take over the permission prompt
/// included). Should Windows Installer refuse a file held so (it exits with `unopenable`), the hold is let go, the file is
/// checked once more and run without it.
fn run_checked(finish: &Finish, installer: &Path, unopenable: i32) -> bool {
    let Ok(mut held) = hold(&finish.msi) else { return false };
    if !crate::sha256_of(&mut held).is_ok_and(|digest| digest == finish.sha256) {
        return false;
    }
    // `/passive` shows a progress bar and asks for nothing; the log is for finding out what went wrong.
    let log = finish.msi.with_file_name("install.log");
    let install = || {
        let mut command = Command::new(installer);
        command.arg("/i").arg(&finish.msi).args(["/passive", "/norestart", "/l*v"]).arg(&log).stdin(Stdio::null());
        command.status().ok().and_then(|status| status.code())
    };
    if install() == Some(unopenable) {
        drop(held);
        if crate::sha256_file(&finish.msi).is_ok_and(|digest| digest == finish.sha256) {
            install();
        }
    }
    true
}

/// Removes what earlier updates left in `parent`: their folders, which hold the copy of the app that couldn't remove itself.
/// Only a day old and older, and only real folders by that name; not `keep`, the folder of an update that is ready to install.
pub fn sweep(parent: &Path, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().ok().filter(fs::Metadata::is_dir).and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
        let ours = entry.file_name().to_string_lossy().starts_with(FOLDER_PREFIX) && keep != Some(entry.path().as_path());
        if ours && old.is_some_and(|age| age > STALE) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
