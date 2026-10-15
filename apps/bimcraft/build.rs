//! Gives bimcraft.exe the logo as its icon, which Explorer, the Start menu and a pinned taskbar button show
//! for the file; the window sets its own icon once it opens. The .ico's images go in as a Win32 resource
//! file, which the MSVC linker takes as one of its inputs.

use std::{env, fs, path::Path};

const ICO: &str = "../../assets/app-icon/bimcraft.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed={ICO}");
    // ponytail: MSVC only, the toolchain the installer is built with. A windows-gnu build would need windres
    // to turn the .res into an object first.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return Ok(());
    }
    let ico = fs::read(ICO).map_err(|e| format!("{ICO}: {e}"))?;
    let res = resource(&ico).ok_or_else(|| format!("{ICO} isn't an icon file"))?;
    let out = Path::new(&env::var("OUT_DIR").map_err(|e| e.to_string())?).join("bimcraft.res");
    fs::write(&out, res).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("cargo:rustc-link-arg-bins={}", out.display());
    Ok(())
}

/// The .ico as a .res file: each image an RT_ICON resource numbered from 1, then the .ico's directory, with
/// those numbers in place of file offsets, as RT_GROUP_ICON 1. None when it isn't an icon file.
fn resource(ico: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let dword = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    let count = word(4)?;
    if word(0)? != 0 || word(2)? != 1 || count == 0 {
        return None;
    }
    let mut res = Vec::new();
    add(&mut res, 0, 0, &[]); // every .res file opens with an empty resource
    let mut group = ico.get(..6)?.to_vec();
    for id in 1..=count {
        let at = 6 + 16 * usize::from(id - 1);
        let (size, offset) = (dword(at + 8)? as usize, dword(at + 12)? as usize);
        add(&mut res, RT_ICON, id, ico.get(offset..offset.checked_add(size)?)?);
        group.extend(ico.get(at..at + 12)?);
        group.extend(id.to_le_bytes());
    }
    add(&mut res, RT_GROUP_ICON, 1, &group);
    Some(res)
}

/// Appends a resource with a numbered type and name, its data padded to four bytes. Version, language and
/// memory flags stay zero: the icon is language neutral, and Win32 ignores the flags.
fn add(res: &mut Vec<u8>, kind: u16, name: u16, data: &[u8]) {
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes()); // the header's own size
    res.extend([0xff, 0xff]);
    res.extend(kind.to_le_bytes());
    res.extend([0xff, 0xff]);
    res.extend(name.to_le_bytes());
    res.extend([0; 16]);
    res.extend(data);
    res.resize(res.len().next_multiple_of(4), 0);
}
