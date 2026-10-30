# Release: installer, importer and packages (M21)

Form and rationale: docs/decisions/0005-installer.md (the project owner's
specification of 2026-10-28). User-facing text: release/README.txt and
release/KNOWN_ISSUES.txt, shipped in every package.

## What an installation holds

```
GauntletDarkLegacy/            (default ~/GauntletDarkLegacy; the user may choose)
  gdl_play[.exe]               the game
  gdl_import[.exe]             the importer (also repair: install again)
  import/catalog.json          shipped: archive entry names and decoded SHA-256
  import/supported-disc.json   shipped: the supported disc's file sizes and SHA-256
  LICENSE THIRD_PARTY.md THIRD_PARTY_LICENSES.txt README.txt KNOWN_ISSUES.txt
  LIBRARY_LICENSES.txt         Windows: the MinGW-w64 startup code's notices
  install.json                 installer marker (time, source, version)
  data/config.json             paths relative to data/ ("base": "config")
  data/catalog.json data/supported-disc.json
  data/disc/SLUS_200.47 data/disc/IRX/DCS.IRX data/disc/MPEG{,2}/*.PSS
  data/wad/manifest.json data/wad/NNNNNN-hash.dat (2,915 decoded entries)
```

779 MiB of data from the supported disc (2,953 files). Saves and settings
stay in the per-user directory of earlier builds (Linux
`$XDG_DATA_HOME|~/.local/share/gdl-native/SLUS-20047/saves`, Windows
`%APPDATA%\gdl-native\SLUS-20047\saves`), so characters survive reinstalls
and carry over from the development builds.

## The importer (crates/gdl_install, gdl_import)

- Sources: an ISO 9660 image (gdl_formats::iso: cooked 2048-byte sectors,
  raw 2352-byte BIN dumps in MODE1 or MODE2 form 1; unit tests with
  synthetic images and refusals), an optical drive's block device read as
  an image, or a mounted directory (names matched without case or ";1").
- Identity: SYSTEM.CNF's BOOT2 names SLUS_200.47; the executable's size and
  SHA-256 equal supported-disc.json's (else "a release this installer does
  not support"); every file the game uses is present. Per-file hashes, not
  the image hash: a disc read through a drive need not reproduce an image.
- Install: copy and hash the executable, DCS.IRX and the 36 movies; read
  and hash WAD.BIN (checked against the profile's and the catalog's
  source_sha256); decode its 2,915 entries, each checked against the
  catalog's decoded SHA-256; write the extraction manifest (gdl_tools
  wad-extract's format), the catalog and config.json; then swap the staging
  directory in as data/. Interrupted or failed runs leave only a staging
  directory that the next run removes (`.gdl-import-*`).
- Configuration: the Rust port of tools/python/run_play.py's generation.
  On the actual disc it equals run_play.py's play-config.json in every
  section (elf, the 61 levels, packages, monsters, audio, movies) once paths
  are mapped (private/wad → wad, private/disc → disc).
- Measured: install from the ISO 5.1 s (warm cache), peak memory 491 MB;
  `gdl_import verify` re-hashes all 2,953 files (none differing).
- Windows (found on a Windows 11 installation, 2026-10-28): the swap of the
  staging directory into data/ failed with "Access is denied (os error 5)"
  because the manifest file was still open inside it (Linux allows renaming
  a directory with open files, Windows does not). The handle is now closed
  before the swap, and swap errors name the step. No retry was needed:
  with the handle closed the swap succeeded on every Windows run.

## The game without the capture

Every new game in gdl_play starts from a start-state template (globals,
extra values, camera rest pose, enemy load tables, four player records),
of which build_game replaces the players in play, the classes, the level
and the carried session values. Until M21 the template was always the
capture export (export_game_state.py, poison-fields-game-v6). Of its 62
global and extra values, 33 equal the ELF's initial values; the other 29
were set at run time (level start values, constants the original sets in
code such as SPF 1/30 or nine keys and potions, and the session's clocks,
RandomItemNum and vb_counter). The three unused player records differ only
in their slot number (+0x0, +0x4).

An installation has no capture: gdl_play embeds boot_state.json
(tools/python/export_boot_state.py), the same values with player 1's
captured character replaced by the unused-slot record (slot number 0).
The values remain MEASURED, not recovered from the original's boot code
(unknowns.md). Equivalence (private/runs/boot-equiv-v2, -v3): a new TEST
character with no input, the developer configuration (capture template)
against the installed one (embedded template), per-update simulation
hashes over 600 updates are equal from the first update in G1, A1, K2 and
the Tower (welcome scrolls closed by triangle presses).

The presentation values the capture's sim_start gave (MboxWinPtr +0x1c,
π/3; the attention offset (0, 4.4, 0), replaced by the player's own at
game start) are constants in gdl_play for configurations without it.

## The installer (installer/)

Pages: welcome, the game disc, the destination, the installation, the end
(decisions/0005). Checks on this machine (Linux, Qt 6.10.2):

- `--batch` into a fresh home (HOME/XDG directories under private/runs):
  probe, payload copy, import 7.4 s, menu and desktop entries;
  `gdl_import verify` none differing; the installed gdl_play started from
  its folder with no argument reaches G1 (TEST autostart, 200 updates) and
  saves into the fresh home.
- GUI on the private Xvfb (tools/python/installer_gui_check.py, report
  installer-gui-v2): the ISO attached read-only through udisks and mounted
  like a disc (iso9660 at /run/media/$USER/GAUNTLET) is listed as a drive
  and probed supported; the default destination; the import (9.1 s); the
  end page; "Start the game now" starts the installed game (its opening
  movie played); both shortcuts in the fresh home. The ISO was unmounted
  and detached afterwards. Rerun on the final package (installer-gui-v3):
  the destination page's button reads "Install" (the commit point; no Back
  into a running import), import 6.1 s, the game started.
- Cancelling (installer-gui-v4, -v5): Escape two seconds into the import,
  then Yes: no data/ and no staging directory remain; since v5 a cancelled
  or failed first installation also removes the programs it copied and the
  folder (a reinstall keeps the previous installation).
- Refusals (`gdl_import probe` on prepared folders): another PS2 game ("a
  different PlayStation 2 game (SLES_501.23)"), another release (a
  different executable: "a release this installer does not support"),
  missing files (listed). A damaged WAD.BIN (one byte flipped) passes the
  quick probe and is refused by the install ("WAD.BIN differs from the
  supported disc's"), nothing published.
- Raw BIN dump: the ISO rewritten as 2352-byte MODE2 form 1 sectors probes
  supported and installs; verify none differing.

## Qt in the packages (2026-10-28)

The owner's decision (2026-10-28): both packages carry the installer's Qt,
so that the user installs nothing first. docs/toolchain.md has the build
details, the bundle rule and the Qt source identities.

- Linux (until 2026-10-29; since then Qt built for Ubuntu 20.04, "Older
  Linux distributions" below): the installer is linked with DT_RPATH
  `$ORIGIN/lib`; tools/python/bundle_qt_linux.py copied Ubuntu's Qt 6.10.2 libraries, the
  xcb and wayland platform plugins (xdg-shell, bradient, compose and IBus
  input) and their less common dependencies (ICU 78, double-conversion,
  libb2, md4c, pcre2-16, the xcb extension/utility libraries, xkbcommon,
  SM/ICE, wayland-cursor) beside it with a qt.conf (38 files, 64.2 MiB;
  package 132 MiB, 47 MiB compressed). `--batch`, `--list-discs`,
  `--help` and `--version` run on a QCoreApplication (no display needed).
- Windows: built on the owner's Windows 11 machine over SSH with Qt 6.11.1
  (MinGW 13.1) and windeployqt: Qt Core, Gui, Widgets, the qwindows and
  qmodernwindowsstyle plugins and the MinGW runtime DLLs (9 files beside
  the installer; package 108 MiB, 39 MiB zipped).
- Notices: LIBRARY_LICENSES.txt in each package (library_licenses.py);
  the library sources beside the packages (library-source/; qt-source/
  until 2026-10-29).

Checks of the bundle (tools/python/qt_bundle_check.py, a bubblewrap
sandbox in which every system libQt6 file, every system copy of a bundled
library and Qt's plugin folder are masked; LD_DEBUG=libs records where the
installer's objects come from):

- qt-bundle-batch-v5: `--batch` from the ISO without a display, 142 files
  masked: 3.5 s, verify none differing; Qt Core, DBus, Gui, Widgets and
  their dependencies from the package, nothing from the system.
- qt-bundle-wayland-v3: the wizard on a `kwin_wayland --virtual` session
  (QT_QPA_PLATFORM=wayland) up for 8 s, the wayland platform and xdg-shell
  plugins from the package — sandboxed (kwin gives sandboxed clients no
  outputs and the client reports the broken connection when the sandbox
  is stopped; the system's Qt under bubblewrap logs the same) and
  unsandboxed (nothing logged).
- installer-gui-qt-v5 (installer_gui_check.py --qt-sandbox, the system's
  116 Qt files masked; the other bundled libraries stay, since the game the
  installer starts needs the system's xkbcommon-x11 and the Vulkan driver
  the xcb extensions): the ISO attached through udisks and listed as a
  drive, installed in 4.1 s, both shortcuts, the game started; all 32 of
  the installer's Qt-side objects from the package. Earlier attempts
  (qt-v1–v3) masked the bundled libraries too and so broke the game (no
  xkbcommon-x11, then no Vulkan device); qt-v4 and v5 pass.

Until 2026-10-29 the Linux package needed glibc 2.43 (the game's linked
symbol versions; libQt6Gui too): 2026 distributions only.

## Older Linux distributions (2026-10-29)

The owner's decision (2026-10-29): older distributions are supported. The
Linux package is built in an Ubuntu 20.04 container with Qt built there
from the shipped source (docs/toolchain.md "Linux release build"); every
ELF file of it needs at most GLIBC_2.30 and GLIBCXX_3.4.28 (checked by
the build). The game needed no newer functions: GLIBC_2.43 had come only
from linking against the host's C library (new versions of acosf, asinf,
atan2f, coshf and sinhf). Bundled Qt: 26 files, 38.4 MiB (no ICU);
package 37 MB compressed.

- Same game, older libraries: the container-built gdl_play and the host
  build give equal per-update simulation hashes over 1000 updates (Tower →
  G portal, Xbox prompts) and 1500 updates of G1 with enemies attacking
  (RX 580, kwin virtual session).
- tools/python/old_distro_check.py, report old-distro-focal4 (pass): the
  package in gdl-runtime-focal (Ubuntu 20.04.6, glibc 2.31, libstdc++ of
  GCC 10, Mesa 21.2.6; runtime libraries only, no network, the GPU's
  /dev/dri, the kwin session's X11 socket): all 29 ELF files resolve (the
  bundled ones with lib/ first, as the installer's inherited DT_RPATH
  resolves them); `--batch` from the ISO, verify none differing of 2953;
  the wizard up on the X display (its welcome page, DejaVu font); the
  installed game started with no argument rendered on RADV POLARIS10
  (Mesa 21.2.6) and ran 600 updates; their hashes equal the same
  installation's game on this host (glibc 2.43, Mesa 26.0). Earlier runs:
  focal1 (no `file` in the image, the Tower's welcome scroll left open),
  focal3 (ldd on bundled libraries without lib/: false "not found").
- On this host (2026): qt-bundle-batch-v6 — `--batch` with the system's Qt
  and every system copy of a bundled library masked (134 files): 4.6 s,
  verify none differing, every Qt object from the package;
  installer-gui-qt-v6 — the full wizard on the private Xvfb with the ISO
  attached read-only through udisks: the drive listed and probed
  supported, installed in 4.1 s, both shortcuts, the game started, the
  installer's Qt objects from the package (116 system Qt files masked).
- Not tested: other distributions of that age (Debian 11, Fedora 32,
  RHEL 9) — the same C library and C++ library floor applies; the Wayland
  platform plugin (not built: Wayland desktops run the installer through
  XWayland, as the game).

## Updates (2026-10-29)

tools/python/update_check.py, report update-v3 (pass): in a fresh home on
tmpfs, the previous release's Linux package (b9f7496, private/dist-b9f7496)
installed from the ISO; its game played a TEST character, saved it (card
file 0) and stored changed settings (widescreen, MSAA 2); the new package
installed over the same folder (`--batch`). The save, its backup and
settings.json byte-identical after the update; gdl_play and gdl_import
replaced by the new package's; verify none differing; no staging or
previous-data folder left; the new game started from the old save
(GDL_TEST_LOAD_SLOT=1: the store numbers card file k as k + 1) and ran 300
updates. Earlier runs: v1 (the test loaded slot 0: no such save), v2 (the
settings TEST action falls on a rendered frame at its update; a frame that
steps two updates missed it — the action is now repeated). Rerun on the
release's packages: update-v4 (pass).

Windows 11 (the owner's machine over SSH, private/runs/windows-update-v1):
the previous release's installer `--batch` from the ISO staged on P:
(exit 0, 27 s; APPDATA redirected to a test folder), a save, its backup and
settings.json placed in APPDATA, the release's installer over the same
folder (exit 0, 23 s): the saves byte-identical, gdl_play.exe replaced and
both programs equal to the package's, verify none differing, no leftover
folder; the test folder removed afterwards. The installer is a GUI-subsystem
program: PowerShell waits for it only through Start-Process -Wait.

Under Wine 10.0 on the RX 580 (private/runs/wine-equiv-final-v1): the
release's Windows package, imported by its gdl_import.exe, and its Linux
package, imported by its gdl_import: 600 updates of a G1 TEST start give
equal per-update hashes (and equal to the 2026-10-28 installed game's,
boot-equiv-v2).

## Private-data audit (2026-10-29)

tools/python/release_audit.py checks every file of the package folders and
every member of their archives and of the source archive against the
SHA-256 of the disc's files, the WAD's extracted entries, the ISO, the
reference captures and memory cards, the start states and the BIOS (3142
identities); disc-format names (PSS, IRX, ELF, SLUS_*, WAD.BIN, savestates,
cards, BIOS parts); the user's saves (by content, read only, and save*.json
names); member paths under private/, target*/, reports/ or runs/; and the
user's name in home or mount paths and the user's e-mail address (UTF-8 and
UTF-16LE). Report release-audit-pre1 (new Linux package, source archive of
3e62932): no game data, saves or private paths; two documentation lines
named this machine's folders (docs/state.md, research/release.md; made
generic). The previous Windows game (b9f7496) carried the build machine's
folders in 5,315 panic locations (the Cargo registry under the user's home,
the repository's mount path); the Windows build now remaps them to /cargo
and /src (package.sh), as the Linux container's mounts already do.
release-audit-final1 (the release's packages and the source archive of
5d5d363): one finding — the audit tool itself held the owner's e-mail
address as a search string; it now reads it from the git configuration at
run time. release-audit-final2 (both packages, their archives, the source
archive of 1d450fc; 3142 private identities, the owner's 4 save files):
no findings.

## Windows

On the owner's Windows 11 Pro machine (GeForce GTX 1080; over SSH, so in
session 0), with the final package (identities below), P:\gauntlet:

- `--batch` from the ISO staged on P: (SHA-256 equal to the original):
  exit 0, 29–102 s (the same command; a large data drive with the virus
  scanner reading each new file; Linux takes 4–6 s); `gdl_import verify`
  none differing; install.json written. The first run, before the fix
  above, failed at the staging swap and the fresh-installation cleanup
  removed everything it had copied.
- The ISO attached with Mount-DiskImage (a CDFS volume, like a disc in a
  drive): `--list-discs` lists "GAUNTLET  (E:\)" (nothing before
  mounting); `--batch --source E:\` installs (55–64 s), verify none
  differing; detached afterwards.
- `--shortcuts` (a reinstall over an installation, which also moves the
  previous data aside): "Gauntlet Dark Legacy.lnk" on the desktop and in
  the Start menu's Programs, target gdl_play.exe, working folder the
  installation; both removed after the check.
- The wizard starts and stays up; its 9 Qt and MinGW modules are loaded
  from the package, none from elsewhere. Its pages cannot be shown in
  session 0, so they have not been driven on Windows.
- The game in session 0 finds the GTX 1080 (Vulkan, NVIDIA 582.78) and
  creates its window but renders no frame in 60 s (session 0's
  non-interactive window station shows no window; the frame loop waits
  for redraws): the game has not yet run on a Windows installation.
- The owner's tests on the desktop (2026-10-29, the 0.1.0 packages,
  owner's report): installing from the ISO and from the physical disc
  works on Windows and on Linux, and "everything seems to work" (the game
  played on the Windows desktop; the owner's report names no individual
  feature).

Under Wine 10.0 on the RX 580 (kwin virtual session), the final package's
gdl_play.exe installed by its gdl_import.exe (verify none differing): the
per-update simulation hashes of 600 updates of a G1 TEST start equal the
installed Linux game's (private/runs/boot-equiv-v2/inst-G1.state).

## Missing dependencies (2026-10-29)

- No Vulkan device (Linux: VK_DRIVER_FILES / VK_ICD_FILENAMES pointing to
  an empty manifest, so the loader finds no driver; RX 580 machine, kwin
  virtual session): Bevy's renderer panics "Unable to find a GPU!". Before:
  last-error.txt held the raw panic with a source path and a desktop
  launch showed nothing. Now the panic hook names the remedy ("No graphics
  card with Vulkan support was found … Install or update the graphics
  card's driver"; Windows: "Vulkan or DirectX 12"), written to
  last-error.txt; on Windows in the message box, on Linux — stderr not a
  terminal and no GDL_* variable (test and developer runs set them) — in
  the desktop's dialog (kdialog on KDE, else zenity; the other only when
  the first is not installed). Seen in the session (Qt forced to the
  session's Xwayland: under a Wayland desktop Qt otherwise opens the
  user's own display): the kdialog box with the message, then exit 101.
  The Windows message box was not shown in session 0.
- No FFmpeg (GDL_FFMPEG=/nonexistent/ffmpeg, title_start_check
  noffmpeg1, --attract --wait-title): the boot's SONY screen during the
  card check, the intro movie fails to start and is skipped, Press Start
  after 31 frames (the movie played out: 3711), START → the title menu.
  The realm and ending movies take the same start-failure path
  (present_movie: no player → the movie ends).

## Release 1.0.0 and the GitHub repository (2026-10-29)

The owner asked for the repository on GitHub (jeddhor/GauntletDLRedone,
private) and for the tested packages as release 1.0.0. The version is set
in Cargo.toml's workspace and installer/CMakeLists.txt (it is recorded in
the import manifest's converter_version and install.json; nothing compares
it). Packages built at fbbdb54 (identities in research/m21-acceptance.md
"Release identities (1.0.0)"): against the 0.1.0 packages only the
installers' version strings, the programs (code layout), KNOWN_ISSUES.txt
and the Windows notice's installer hash differ. Checks of the 1.0.0
packages, all passing: old-distro-focal6 (Ubuntu 20.04), update-v5 (Linux,
over 0.1.0), private/runs/windows-update-v2 (Windows 11 over SSH, over
0.1.0; the test saves of client-m19-ovl1), qt-bundle-batch-v8,
installer-gui-qt-v9, private/runs/wine-equiv-1.0.0-v1 (600 G1 updates: the
1.0.0 Linux and Wine runs equal each other and the 0.1.0 runs),
title-start-m21-release-v100 (6/6, the packaged gdl_play).

Before the first push every blob of the history was checked with
release_audit.py's tests (a scratch scan of 473 commits, then
tools/python/history_audit.py, report history-audit-v100a: 475 commits,
2829 blobs, 3142 private identities, the owner's 4 save files): no game
data, no save, no private or disc-format path, no credential pattern;
nothing at HEAD. 124 superseded file versions hold an identity string —
120 of docs/state.md this machine's home folder, 3 of research/release.md
its mount path, one earlier release_audit.py the owner's e-mail local part
— and every commit's author line carries the owner's name and address
(by design; worth deciding before the repository is made public). A clone needs no
private/source-control.git: tools/git-local.sh falls back to the clone's
.git (checked on a fresh clone: same tree as the local HEAD).

## Identities (2026-10-29, the M21 release)

Commit 5d5d363 (programs): Linux package e2122433e2ae51a4… (tar.gz;
installer fed81993b5062b2f…, gdl_play 380722debf06f424…, gdl_import
cc60906056b154c6…); Windows package f73e7dff1a03647d… (zip; installer
2d8496c4890fc997…, unchanged, gdl_play.exe cc563344a8294b9e…,
gdl_import.exe 887fb20d1cdbbb13…). The checks of these packages:
research/m21-acceptance.md.

## Identities (2026-10-28, packages of b9f7496)

Linux package (tools/package.sh): installer 5d7869977d3aaf93…, gdl_play
a7398570c4836219… (unchanged since M21), gdl_import f4be7b9606dad7e1…;
Windows package: installer 2d8496c4890fc997…, gdl_play.exe
32a7fc7f21205106…, gdl_import.exe 9ff11180a28bab58…. The Windows programs
are reproducible: linked without the PE time stamp and stripped at link
time (.cargo/config.toml, installer/CMakeLists.txt), two clean builds are
byte-identical. release/import/catalog.json 984ec256e6919eab…,
supported-disc.json f09f85dba2890fb2…; crates/gdl_client/src/boot_state.json
9dfcf128687ab094….

Test installs (~780 MiB each) once filled the data drive; they are made in
/tmp's tmpfs or deleted after their checks.

## Licenses

release/THIRD_PARTY_LICENSES.txt (tools/python/third_party_licenses.py):
290 crates in gdl_play and gdl_import for both platforms, all permissive and
GPL-3.0-compatible, with their notice files; the name dictionary's MIT
notice; Qt's LGPL-3.0 statement. LIBRARY_LICENSES.txt per package
(library_licenses.py): the bundled libraries' notices (Linux, since
2026-10-29: 37 components inside the built Qt files from its SBOM, their
license texts from Qt's source, 11 Debian copyright files of Ubuntu 20.04's
packages and the common licenses they cite — until then 17 copyright files
of Ubuntu 26.04's; Windows: 42 components
inside the Qt files from Qt's SBOM, 19 license texts from Qt's source, the
MinGW runtime's and Ubuntu mingw-w64's notices). Qt's source is shipped
beside the packages (the LGPL's corresponding source); since 2026-10-29
(the owner's decision) so is the MinGW runtime DLLs' (GCC 13.1.0's release
archive, the MinGW-Builds scripts and patches at the commit the kit's build
used, and the kit's build-info.txt; docs/toolchain.md).
THIRD_PARTY.md summarises them.
