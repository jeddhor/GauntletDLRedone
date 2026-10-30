# M21 final acceptance report — supported complete port

Revision SLUS-20047 (USA, profile ps2-slus-20047-fb47d40667df). Release
1.0.0 (programs built at commit fbbdb54), packages for Linux x86_64 and
Windows x86_64 (identities below), published as the GitHub release v1.0.0
of the private repository jeddhor/GauntletDLRedone. Scope label:
**supported complete port** (the owner's decision, 2026-10-29), with the
limitations listed at the end and in release/KNOWN_ISSUES.txt. Acceptance
is the owner's (charter): on 2026-10-29 the owner tested the 0.1.0
packages — installing from the ISO and from the physical disc on Linux and
on Windows, and playing ("everything seems to work") — and asked for them
as release 1.0.0. The 1.0.0 packages differ from those only by the version
number (below).

## What a player gets

- A Qt 6 installer (Linux and Windows; Qt carried in both packages) that
  imports the game from the player's own disc in a drive, an image (.iso,
  raw .bin) or a folder, checks every file against the supported release,
  and copies the native programs into a folder in the home directory, with
  desktop and menu shortcuts (decisions/0005-installer.md).
- The native game (Rust, Bevy): the original game's rules from the ELF and
  data, the whole campaign, up to four local players, saves, the original
  presentation and the native enhancements (widescreen, blending, MSAA,
  key bindings, Xbox button prompts, texture packs; research/enhancements.md).
- README.txt (the user guide), KNOWN_ISSUES.txt, LICENSE, THIRD_PARTY.md,
  THIRD_PARTY_LICENSES.txt, LIBRARY_LICENSES.txt; the source archive and
  the bundled libraries' source (library-source/) beside the packages.
- Supported build list: one release, SLUS-20047 (release/import/
  supported-disc.json: every disc file's SHA-256; README "What you need").

## Exit criteria

| Criterion | Evidence | Kind |
| --- | --- | --- |
| A fresh installation imports a supported image | Linux, fresh home: `--batch` from the ISO with the system's Qt masked (qt-bundle-batch-v6), the full wizard with the ISO attached as a drive (installer-gui-qt-v6), and on Ubuntu 20.04 (old-distro-focal4); verify: none of 2953 files differing. Windows 11: `--batch` from the ISO and from the ISO mounted as a drive (research/release.md "Windows") | Actual disc image; real GPU / private Xvfb; Windows 11 over SSH |
| Launches natively | Linux: the installed game with no argument (RX 580; Ubuntu 20.04 with Mesa 21.2.6, hashes equal to this host's). Windows: the owner's Windows desktop (2026-10-29, 0.1.0 packages, installed from the ISO and from the disc); under Wine the installed game's hashes equal Linux's | GUI runs; owner's test; Wine |
| Supports the advertised campaign and co-op | M15: the campaign played legitimately to the ending (bot-driven, 2026-10-27), all eleven bosses compared with reference traces; M14: all 61 levels traverse; M16: one to four players, two- and four-player sessions replayed update by update against the reference (Tower, G1, B6); M12 accepted by the owner; the owner's playtests (research/playtest.md) | Reference replays, actual-disc runs, owner's play |
| Saves and reloads reliably | M18: versioned saves with backups and recovery through the client, PS2 card import; update-v3: a save and the settings made by the previous release survive installing this one, and the new game loads that save | Tests, GUI scenarios |
| No BIOS or PCSX2 at runtime | The game starts from its built-in start state (research/release.md "The game without the capture": hashes equal to the capture template in G1, A1, K2 and the Tower); the installation holds only the programs and the imported disc files; release-audit: no BIOS, capture or disc file in the release | Hash comparison, audit |
| Unsupported revisions rejected | The importer refuses another PS2 game, another release of this one (a different executable), missing files and a damaged WAD.BIN, naming why; nothing is published (research/release.md "The installer") | Prepared folders and images |

## Tasks of M21

| Task | Result |
| --- | --- |
| Import-first workflow | The installer; `gdl_import probe/install/verify` underneath (research/release.md) |
| Version-mismatch messages | The refusals above; saves of another schema or revision are refused (M18) |
| Cache rebuilding | Installing again repairs: the data is re-imported and swapped in whole; `gdl_import verify` checks an installation |
| Missing dependencies | No Vulkan device: a plain message, last-error.txt, Windows message box, Linux desktop dialog; no FFmpeg: movies skipped, the game goes on; older Linux: the package's floor is GLIBC_2.30 / GLIBCXX_3.4.28, every library resolves on Ubuntu 20.04 (research/release.md "Missing dependencies", "Older Linux distributions") |
| Cancellation, interrupted conversion | Cancelling leaves nothing (installer-gui-v4/v5); an interrupted import leaves only its staging folder, removed by the next run |
| Updates | update-v3; 1.0.0 over 0.1.0: update-v5 (Linux), windows-update-v2 (Windows 11) (research/release.md "Updates", "Release 1.0.0") |
| Fresh user account, no extracted assets | Fresh HOME/XDG folders (Linux) and a redirected APPDATA (Windows) with no extracted data; not a newly created operating-system account |
| Licenses and source notices | THIRD_PARTY_LICENSES.txt (290 crates), LIBRARY_LICENSES.txt per package (Qt's SBOM components, Debian copyright files, the MinGW runtime), the source archive, Qt's and the GCC runtime's source (library-source/) |
| Private-data exclusion | release_audit.py over both packages, their archives and the source archive (research/release.md "Private-data audit"; report below) |
| Support docs | README.txt, KNOWN_ISSUES.txt |
| Coverage ledger | docs/coverage.md, every milestone row closed |

## Release identities (1.0.0)

Built by tools/package.sh at commit fbbdb54 (programs; the version set to
1.0.0) — the release's source archive is cut from the commit tagged v1.0.0,
which adds these records (the programs' sources are unchanged by it):

| File | SHA-256 |
| --- | --- |
| GauntletDarkLegacy-1.0.0-linux-x86_64.tar.gz (37 MB; 106 MiB unpacked) | 0f15295457e5a6a2… |
| GauntletDarkLegacy-1.0.0-windows-x86_64.zip (40 MB; 108 MiB unpacked) | 1f6669e2223c3f64… |
| Linux: GauntletDarkLegacy-Installer / gdl_play / gdl_import | 0916c848aff6c614… / d00ca65fbd110aa3… / e5f2f6da4177a983… |
| Windows: GauntletDarkLegacy-Installer.exe / gdl_play.exe / gdl_import.exe | 7d8160684c12f1ed… / 8318a8e268bda3bc… / 928619134fd1a0c7… |
| library-source/ (179 MiB) | unchanged: qtbase 6.11.1, qt6-base 6.10.2+dfsg, GCC 13.1.0, mingw-builds 7ff96692, the kit's build-info (docs/toolchain.md) |

Against the 0.1.0 packages the owner tested, file by file: the installers
differ only in the version string (Windows: 4 bytes, "0.1.0" → "1.0.0" in
two places; Linux: 22 bytes, the version and the build ID); every bundled
Qt, MinGW and xcb library is byte-identical; KNOWN_ISSUES.txt records the
owner's Windows tests; the Windows LIBRARY_LICENSES.txt changes only in
the installer's listed SHA-256. gdl_play and gdl_import differ throughout
(the crate version enters rustc's symbol hashes and so the code layout):
their behaviour is compared below.

Checks of the 1.0.0 packages (2026-10-29):

| Check | Result | Evidence |
| --- | --- | --- |
| Ubuntu 20.04: libraries (29 ELF files), install, verify, wizard, game, hashes vs this host | pass | old-distro-focal6 |
| Update over release 0.1.0 (Linux): saves, settings, programs, verify, the 0.1.0 save loaded | pass | update-v5 |
| Update over release 0.1.0 (Windows 11, `--batch`): saves, programs, verify, no leftovers | pass, 130 s + 89 s | private/runs/windows-update-v2 |
| Linux 2026 host, system Qt masked: `--batch` | pass, 3.6 s | qt-bundle-batch-v8 |
| Linux wizard with the ISO attached as a drive, system Qt masked, game started | pass, 8.1 s | installer-gui-qt-v9 |
| 600 G1 updates: 1.0.0 Linux, 1.0.0 Windows under Wine, each imported by its own gdl_import | all equal, and equal to 0.1.0's Linux and Wine runs | private/runs/wine-equiv-1.0.0-v1 |
| Title → select on the RX 580 with the packaged gdl_play | 6/6 | title-start-m21-release-v100 |

Checks of the 0.1.0 packages (2026-10-29; their identities in
research/release.md "Identities (2026-10-29, the M21 release)"):

| Check | Result | Evidence |
| --- | --- | --- |
| Ubuntu 20.04: libraries, install, verify, wizard, game, hashes vs this host | pass | old-distro-focal5 |
| Linux 2026 host, system Qt masked: `--batch` | pass, 3.6 s | qt-bundle-batch-v7 |
| Linux wizard with the ISO attached as a drive, game started | pass, 4.1 s | installer-gui-qt-v7 |
| Update over the previous release (Linux): saves, settings, programs, verify, old save loaded | pass | update-v4 |
| Update over the previous release (Windows 11, `--batch`): saves, programs, verify | pass, 27 s + 23 s | private/runs/windows-update-v1 |
| Windows game under Wine vs the Linux game, both imported by their package's importer: 600 G1 updates | equal (and equal to the 2026-10-28 reference) | private/runs/wine-equiv-final-v1 |
| Title → select on the RX 580 (window found by its release title) | 6/6 | title-start-m21-release1 |
| Private-data audit of both packages, archives and the source archive | see research/release.md "Private-data audit" | release-audit-final2 |

## Remaining limitations

Precisely, as release/KNOWN_ISSUES.txt states them to players:

- Windows: tested on Windows 11 only (the owner's desktop, and over SSH);
  older Windows versions are untested.
- A physical disc in an optical drive: the owner's installs on Linux and
  Windows (2026-10-29); no automated check reads a physical disc.
- Linux distributions other than Ubuntu 20.04 and Ubuntu 26.04 are untested;
  the floor is the C library 2.31 and GCC 10's libstdc++. The installer
  has no native Wayland plugin (XWayland, as the game).
- A new game starts from start values measured on a PlayStation 2 session,
  not from the original's own start-up code (equal play, measured origin).
- Versus play and team moves are ported from the original's code but not
  compared with the original running.
- An item rising out of a chest sits at a slightly different height while
  it rises; the realm map keeps the finished level loaded.
- Performance: on the RX 580 a single late frame occurs about once in
  50,000 (M20 residual: 2 of 107,827 frames over 33.4 ms in a 30-minute
  four-player session).
- Xbox button prompts: the Controls page still pictures a PS2 controller;
  Nintendo-layout pads get the Xbox letters by position.
- With widescreen, enemies in the widened margins are outside the original
  game's view and may wait until they come into it.
