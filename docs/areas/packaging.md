# Packages

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

- Packages: `packaging/linux/package.sh` writes the AppImage, deb, rpm and tarball into `dist/release` (build on Ubuntu
  22.04, as CI does, so they run on its glibc and later; riscv64 is cross-built on 24.04, deb, rpm and tarball only, as the
  `linux-riscv64` job shows); `flatpak-bundle.sh` and `arch-package.sh` repackage that tarball as a Flatpak and an Arch
  package; `packaging/macos/package.sh` writes the DMG, on a Mac. After touching `packaging/`, run shellcheck on the scripts
  and actionlint on the workflow, run the Linux one through, and install the deb in a clean container. The app id is
  `io.github.ipedrax.bimcraft` everywhere (`APP_ID`, the desktop file, AppStream, the Flatpak, the bundle); the icons come
  from `packaging/icons.sh`.
