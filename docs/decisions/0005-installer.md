# 0005: Release — a Qt 6 installer that imports from the user's disc

The plan's M21 asks for an import-first workflow, packages, a user guide and
a final acceptance report. The project owner specified the form on
2026-10-28: a Qt 6 installer for Windows and Linux that copies the
precompiled game and what it needs into a new folder in the user's home,
lets the user pick the optical drive or the ISO with the original game, and
extracts and copies the game data into the installation.

Decision:

- **Installer** (`installer/`, C++17, Qt 6 Widgets, CMake): a wizard —
  welcome; the game disc (mounted optical volumes found by file system or
  SYSTEM.CNF, Linux's unmounted `/dev/sr*` drives, or a chosen image file or
  folder), each choice checked by `gdl_import probe`; the destination
  (default `~/GauntletDarkLegacy`, free space checked, menu and desktop
  shortcuts); the installation (the payload copied, then `gdl_import
  install` with its progress, cancellable); the end (start the game). A
  `--batch` mode runs the same work without pages for automated checks.
- **Importer** (`crates/gdl_install`, binary `gdl_import`): the disc read
  as an ISO 9660 image (cooked 2048-byte sectors or raw 2352-byte BIN dumps;
  `gdl_formats::iso`), a raw drive, or a mounted directory; each file the
  game uses checked against `supported-disc.json` (per-file SHA-256: a disc
  read through a drive need not reproduce an image's bytes); WAD.BIN
  extracted with every decoded entry checked against the shipped
  `catalog.json`; the game's configuration written (the Rust port of
  run_play.py's, equal to it on the actual disc); all in a staging directory
  that replaces `data/` only when complete. JSON lines on stdout.
- **Shipped reference data** (`release/import/`): the supported disc's file
  hashes and the name catalog (entry index, path hash, decoded SHA-256,
  names — from the MIT-licensed haekb/gdl_wad_decoder dictionary). No game
  data is distributed.
- **Game** (`gdl_play`): without an argument it reads `data/config.json`
  beside itself, whose paths are relative to it; saves stay in the per-user
  directory used before (so existing characters carry over); the new-game
  start-state template is built in (`boot_state.json`, the capture's
  measured values without the captured character; see
  research/release.md); fatal errors go to `last-error.txt` and, on Windows,
  a message box; FFmpeg is looked for beside the executable first.
- **Packages** (`tools/package.sh`): a folder and archive per platform with
  the installer and `payload/` (gdl_play, gdl_import, import/, LICENSE,
  THIRD_PARTY.md, THIRD_PARTY_LICENSES.txt, README.txt, KNOWN_ISSUES.txt),
  and the source archive. Linux uses the system's Qt 6; Windows ships Qt's
  DLLs.

Why: the owner's specification; one importer shared by the installer, the
command line and repair; the game's runtime keeps no dependency on Python,
PCSX2, a BIOS or the development tree.

Not decided here: Qt for Windows is not on this machine (Ubuntu ships no
MinGW Qt), and fetching it (aqtinstall from PyPI or Qt's online installer)
is a download of external code that needs the owner's approval. Until then
the Windows installer is built only where a Qt for Windows is supplied
(`QT_WIN_DIR`); the Windows game and importer build and run (under Wine).

Reversible: the installer is a thin front end over `gdl_import`; the
packages can be repackaged (AppImage, MSI) without changing the importer.
