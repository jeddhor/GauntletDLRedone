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

- Linux: the installer is linked with DT_RPATH `$ORIGIN/lib`;
  tools/python/bundle_qt_linux.py copies Ubuntu's Qt 6.10.2 libraries, the
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
  the Qt sources beside the packages (qt-source/).

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

The Linux package needs glibc 2.43 (the game since M21; libQt6Gui too):
it runs on 2026 distributions only. Not tested: another distribution.

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

## Identities (2026-10-28, final packages)

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
(library_licenses.py): the bundled libraries' notices (Linux: 17 Debian
copyright files and the common licenses they cite; Windows: 42 components
inside the Qt files from Qt's SBOM, 19 license texts from Qt's source, the
MinGW runtime's and Ubuntu mingw-w64's notices). Qt's source is shipped
beside the packages (the LGPL's corresponding source); GCC 13.1.0's source
for the MinGW runtime DLLs is not (LIBRARY_LICENSES.txt points to it).
THIRD_PARTY.md summarises them.
