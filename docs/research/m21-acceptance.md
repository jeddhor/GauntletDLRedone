# M21 final acceptance report — supported complete port

Revision SLUS-20047 (USA, profile ps2-slus-20047-fb47d40667df). Release
0.1.0, source commit 602e27d, packages for Linux x86_64 and Windows x86_64
(identities below). Scope label: **supported complete port** (the owner's
decision, 2026-10-29), with the limitations listed at the end and in
release/KNOWN_ISSUES.txt. Acceptance is the owner's (charter); the one
check left for the owner is the game on a Windows desktop (below).

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
| Launches natively | Linux: the installed game with no argument (RX 580; Ubuntu 20.04 with Mesa 21.2.6, hashes equal to this host's). Windows: under Wine the installed game's hashes equal Linux's; **not yet run on a Windows desktop** (the owner's test) | GUI runs; Wine |
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
| Updates | update-v3 (research/release.md "Updates") |
| Fresh user account, no extracted assets | Fresh HOME/XDG folders (Linux) and a redirected APPDATA (Windows) with no extracted data; not a newly created operating-system account |
| Licenses and source notices | THIRD_PARTY_LICENSES.txt (290 crates), LIBRARY_LICENSES.txt per package (Qt's SBOM components, Debian copyright files, the MinGW runtime), the source archive, Qt's and the GCC runtime's source (library-source/) |
| Private-data exclusion | release_audit.py over both packages, their archives and the source archive (research/release.md "Private-data audit"; report below) |
| Support docs | README.txt, KNOWN_ISSUES.txt |
| Coverage ledger | docs/coverage.md, every milestone row closed |

## Release identities

Built by tools/package.sh at commit 5d5d363 (programs) — the release's
source archive is cut from the commit that adds this report (the programs'
sources are unchanged by it):

| File | SHA-256 |
| --- | --- |
| GauntletDarkLegacy-0.1.0-linux-x86_64.tar.gz (37 MB; 106 MiB unpacked) | e2122433e2ae51a4… |
| GauntletDarkLegacy-0.1.0-windows-x86_64.zip (40 MB; 108 MiB unpacked) | f73e7dff1a03647d… |
| Linux: GauntletDarkLegacy-Installer / gdl_play / gdl_import | fed81993b5062b2f… / 380722debf06f424… / cc60906056b154c6… |
| Windows: GauntletDarkLegacy-Installer.exe / gdl_play.exe / gdl_import.exe | 2d8496c4890fc997… (byte-identical to the previous release's) / cc563344a8294b9e… / 887fb20d1cdbbb13… |
| library-source/ (179 MiB) | qtbase 6.11.1, qt6-base 6.10.2+dfsg, GCC 13.1.0, mingw-builds 7ff96692, the kit's build-info (docs/toolchain.md) |

Checks of these exact packages (2026-10-29):

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

- Windows: the installer's pages and the game have not run on a Windows
  desktop (SSH sessions are session 0: no visible window); Windows sound
  output (WASAPI), a physical controller and audio-device changes on
  Windows are untested. The owner tests these.
- A physical disc in an optical drive is untested (images and an image
  attached as a drive are tested on both systems).
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
