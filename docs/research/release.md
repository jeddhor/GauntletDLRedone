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

## Windows

The game and the importer cross-build (docs/toolchain.md). Under Wine 10.0
(a scratch prefix): `gdl_import.exe probe` accepts the ISO and a directory
holding the disc's files; `install` from the ISO into a Wine path in 6.4 s;
`verify` none differing. The installed gdl_play.exe started with no
argument under Wine on the RX 580 (winevulkan/RADV in a kwin virtual
session, tools/python/kwin_session.py): per-update simulation hashes over
600 updates of a G1 TEST start equal the installed Linux game's. Under
Wine on the Xvfb's software Vulkan the device request failed ("Device
lost"); the new fatal-error path showed it in a message box and
last-error.txt. The Windows installer needs a Qt for Windows, which is not
on this machine (decisions/0005, toolchain.md).

## Identities (2026-10-28)

Linux package (tools/package.sh, stripped copies): installer
780f85b72d91366e…, gdl_play a7398570c4836219…, gdl_import
08912316cea9f653…; Windows builds gdl_play.exe bb23e0b4229ef522…,
gdl_import.exe 5ae3a59ab0a7e652…; release/import/catalog.json
984ec256e6919eab…, supported-disc.json f09f85dba2890fb2…;
crates/gdl_client/src/boot_state.json 9dfcf128687ab094….

Test installs (~780 MiB each) once filled the data drive; they are made in
/tmp's tmpfs or deleted after their checks.

## Licenses

release/THIRD_PARTY_LICENSES.txt (tools/python/third_party_licenses.py):
290 crates in gdl_play and gdl_import for both platforms, all permissive and
GPL-3.0-compatible, with their notice files; the name dictionary's MIT
notice; Qt's LGPL-3.0 statement. THIRD_PARTY.md summarises them.
