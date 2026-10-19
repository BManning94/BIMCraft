//! Which kind of install this running program is, and so what an update would replace. Each test of the environment is a function of
//! what it looks at, so a test can try it with anything; [`crate::Install::detect`] looks for real.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Arch, Handover, Target, appimage, install, macos};

/// What an update replaces: the kind of install this running program is, and where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    pub target: Target,
    /// The AppImage file or the `.app` bundle; for the Windows copy, the program the installer upgrades.
    pub place: PathBuf,
}

impl Install {
    /// What this running program is, when an update could replace it where it stands. Anything else (a package from the distribution,
    /// a tarball, a build from source) is told of a newer release and sent to its page.
    pub fn detect() -> Option<Install> {
        let exe = std::env::current_exe().ok()?;
        if install::installed_here() {
            return Some(Install { target: Target::Windows, place: exe });
        }
        match std::env::consts::OS {
            "linux" => appimage_install(std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR"), &exe, std::env::consts::ARCH),
            "macos" => mac_install(&exe),
            _ => None,
        }
    }

    /// Whether this user can write where an update goes: beside the AppImage or the app, since the new one is copied there and renamed
    /// in. It is asked when there is an update to offer, not at startup, because the probe creates a file, and on macOS some folders
    /// (Documents, Desktop) ask the user before an app does that.
    pub fn replaceable(&self) -> bool {
        self.target == Target::Windows || self.place.parent().is_some_and(install::writable)
    }

    /// Replaces what this is with the update in `job`, which has passed its checks, and starts the new version with the project open;
    /// the caller closes this window afterwards. The folder the update was downloaded to goes with a success, and stays after a
    /// failure so that it can be tried again (the Windows copy that finishes the job removes its own).
    pub fn hand_over(&self, job: &Handover) -> Result<(), String> {
        // The file has sat in the temp folder since it was checked. (The Windows copy that finishes the job checks it once more, after
        // this program has gone.)
        if crate::sha256_file(&job.file).ok() != Some(job.sha256) {
            return Err("the downloaded file is not the one that was checked".into());
        }
        let folder = job.file.parent().ok_or("the download has no folder")?;
        let (version, project) = (job.version.to_string(), job.project.as_deref());
        let done = match self.target {
            Target::Windows => install::start(&job.file, &job.sha256, project),
            Target::AppImage(_) => appimage::replace(&job.file, &self.place, &version, project),
            Target::MacApp => macos::replace(&job.file, &self.place, folder, &version, project),
        };
        if spent(self.target, done.is_ok(), folder) {
            let _ = fs::remove_dir_all(folder);
        }
        done
    }
}

/// Whether the folder an update was downloaded to is to be removed: when the install went through, and only a folder an update made.
/// The Windows copy that finishes the job still needs its folder, and removes it itself.
pub fn spent(target: Target, installed: bool, folder: &Path) -> bool {
    let made_for_it = folder.file_name().is_some_and(|n| n.to_string_lossy().starts_with(install::FOLDER_PREFIX));
    installed && target != Target::Windows && made_for_it
}

/// The AppImage this program runs from: the file the runtime names (a link to it is followed, so the file it points to is the one
/// replaced) is there, and is built for a processor an AppImage is made for.
pub fn appimage_install(appimage: Option<OsString>, appdir: Option<OsString>, exe: &Path, arch: &str) -> Option<Install> {
    let file = fs::canonicalize(appimage_file(appimage, appdir, exe)?).ok()?;
    let target = Target::AppImage(Arch::from_name(arch)?);
    file.is_file().then_some(Install { target, place: file })
}

/// The app this program stands in: BIMCraft's own.
pub fn mac_install(exe: &Path) -> Option<Install> {
    let bundle = fs::canonicalize(mac_bundle(exe)?).ok()?;
    let plist = fs::read_to_string(bundle.join("Contents").join("Info.plist")).ok()?;
    names_bundle(&plist).then_some(Install { target: Target::MacApp, place: bundle })
}

/// The identifier of BIMCraft's macOS bundle: `CFBundleIdentifier` in its Info.plist.
const BUNDLE_ID: &str = "io.github.ipedrax.bimcraft";

/// The AppImage this program runs from. Its runtime sets `APPIMAGE` to the file and `APPDIR` to where the image's files are mounted,
/// and this program (`exe`) stands inside that mount. All of that must hold: a variable left over from another run, or typed, doesn't
/// make some file this program's own.
pub fn appimage_file(appimage: Option<OsString>, appdir: Option<OsString>, exe: &Path) -> Option<PathBuf> {
    let (file, dir) = (PathBuf::from(appimage?), PathBuf::from(appdir?));
    let sound = file.is_absolute() && file.file_name().is_some() && exe.starts_with(&dir) && !file.starts_with(&dir);
    sound.then_some(file)
}

/// The `.app` this program stands in: `X.app/Contents/MacOS/<program>`.
pub fn mac_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let right = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && bundle.extension()? == "app";
    right.then(|| bundle.to_path_buf())
}

/// Whether `plist` (an Info.plist in XML) gives BIMCraft's identifier as the bundle's own.
pub fn names_bundle(plist: &str) -> bool {
    plist_string(plist, "CFBundleIdentifier") == Some(BUNDLE_ID)
}

/// Whether this is a Flatpak: its sandbox sets `FLATPAK_ID` and has a `/.flatpak-info` (`marker`). Either will do, because the sandbox
/// has no network, and an update check there could only fail.
pub fn in_flatpak(id: Option<OsString>, marker: bool) -> bool {
    marker || id.is_some_and(|id| !id.is_empty())
}

/// The string that `key` has in `plist`, an Info.plist in XML (a binary one has none): the first `<string>` that follows the key.
pub fn plist_string<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
    let after = plist.split(format!("<key>{key}</key>").as_str()).nth(1)?;
    let (value, _) = after.trim_start().strip_prefix("<string>")?.split_once("</string>")?;
    Some(value)
}
