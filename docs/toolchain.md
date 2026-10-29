# Pinned toolchain and reference environment

Verified on 2026-10-12. Local development is Linux x86-64. Since 2026-10-28
the client also builds for x86_64-pc-windows-gnu ("Windows build" below);
it has run under Wine on this machine. The Windows package's installer and
importer have run on a Windows 11 installation (research/release.md); the
game itself has not yet run there.

| Component | Exact observed version / identity |
| --- | --- |
| Kernel | Linux 7.0.0-38-generic |
| CPU | AMD Ryzen Threadripper 1950X, 32 logical CPUs |
| GPU | AMD Ellesmere / Radeon RX 470–590 family; exact marketing SKU unverified |
| Rust / Cargo | 1.93.1; rustc 01f6ddf75; Cargo 083ac5135 |
| Python | 3.14.4; stdlib research utilities; system Pillow for independent image checks |
| 7-Zip | 26.00 x64 |
| Bevy | 0.18.1, exact pin; current 0.20.0 needs Rust 1.97.1, so the installed compiler dictates the deliberate older pin |
| flate2 / SHA / PNG | 1.1.10 / sha2 0.10.9 / png 0.18.1; Cargo.lock pins transitives |
| Ghidra | Snap revision 47, version 12.1.2, /snap/ghidra/47/ghidra |
| Analysis JDK | /usr/lib/jvm/java-21-openjdk-amd64; Ghidra minimum Java 21 |
| Host default JDK | 25.0.4.1; not the selected analysis runtime |
| PS2 extension | Emotion Engine: Reloaded v2.1.38, ghidra_12.1.2_PUBLIC_20261004 build |
| Extension SHA-256 | 2e6bdd8ed8e762b302b47f0ce7a0fdf55b5fe7ad6b04cfe8cfa078a9758e67ae |
| Reference binary | /usr/games/pcsx2-qt; v2.6.3+dfsg-1build1 |
| Reference binary SHA-256 | aa7a28d83c30afc34e888a4399f50b51ceda05935b3c1b3f9d7421e6de6f7275 |
| Reference source | 0f71e668b8adc950db7de1b1b15c2f5080c102e9; not rebuilt or modified |
| FFmpeg (movie video, optional runtime) | 8.0.1-3ubuntu2, /usr/bin/ffmpeg, SHA-256 bdf6aabffdba7411edff8d36c389d695257fcdf823d196020176e117612862f6 (docs/decisions/0003-movies.md) |

The Bevy build uses desktop image/sprite/text/render plus 3D mesh/PBR features.
No audio/physics/gameplay system is being represented by the texture laboratory.
Native window rendering worked locally. GPU log notes a missing optional
VK_EXT_physical_device_drm extension; it did not prevent the viewer from rendering.

tools/ghidra/run-headless.sh uses the Snap's immutable installation directly,
JDK 21, and the downloaded module in private/ghidra-extension/. It redirects
Java home/settings/cache directories into private/ without changing the user's
Ghidra preferences or the Snap installation. Language ID: r5900:LE:32:default.
Analysis completed in 56 seconds; selected function/type exports are private.

PCSX2 reference configuration lives in private/reference/config/PCSX2/ with
separate XDG configuration/cache/data roots. SCPH BIOS/NVM/MEC files were copied
into that reference root; source hashes are private/reports/reference-bios-hashes.json.
Only newly created private memory cards were formatted. Original BIOS files,
user saves and the ISO were preserved.

Baseline: software renderer (13), 1x resolution, cheats/patches/widescreen patches
disabled, fast boot, automatic 4:3 aspect and default interlacing. Exact INI,
logs and screenshots remain private. PINE is initially disabled. Later enabling
it changes the capture configuration and must be recorded before probing.
Presentation field-rate log values do not establish the game's simulation cadence.
PINE is now enabled globally and verified. Automated GUI work uses authenticated
Xvfb with Mesa software presentation; launchers refuse desktop fallback.

The stock installed PCSX2 EE debugger was opened and navigated read-only inside
Xvfb. Its visible words/operands for GetPlayerPos (0x1317d8) and InitAnimData
(0x10fa98–0x10fae0) agree with the ELF and Ghidra: scalar/FP accesses, the EE
three-operand MULT, VU LQC2/VSUB.xyzw/SQC2 and 128-bit GPR LQ/SQ. Alias spelling
differs (li versus addiu from zero). Captures and word offsets are preserved in
private/reports/debugger-decoder-crosscheck.json. This is representative decoder
validation, not certification of every opcode or decompiler type/behavior.

No Python packages were installed for the standard-library research scripts.
Pillow is used only for independent local image inspection. Record its version
alongside any future publication-quality image comparisons. No mutable latest
dependency is part of the build.

## Audio output dependency (2026-10-16)

`gdl_client` adds `alsa = "=0.9.1"` (with `alsa-sys 0.3.1`; its other
dependencies were already locked) for Linux audio output in the interim
playtest. Both crates were in the local Cargo registry cache. The two package
entries were added to Cargo.lock by hand with their registry checksums: an
offline `cargo update` re-resolved unrelated crates to older cached versions
(seen first with `cpal 0.15.3`, commit 21ab354), so that lockfile was restored
and every pre-existing version is unchanged. Windows output is deferred to
M20. Automated `--virtual` runs set `GDL_NO_AUDIO=1` so tests never play on
the host's speakers. No recorded comparison depends on this change.

## Windows build and audio (2026-10-28, M20)

`alsa` became a Linux-only dependency of gdl_client and `cpal = "=0.15.3"`
(already in the local registry cache) the Windows one: the mixer feeds the
default WASAPI device through cpal as stereo f32 at the device's rate
(audio.rs `start_cpal`). Resolving cpal added 33 packages to Cargo.lock
(its per-platform dependencies, e.g. jni/ndk for Android, coreaudio for
macOS, windows 0.54) and changed no existing version; `cargo metadata`
needs the network for the Android-only ones, Linux and Windows builds do
not.

The Windows executable is cross-built with the pinned 1.93.1 toolchain from
rustup (its x86_64-pc-windows-gnu standard library is installed; the
system `cargo`/`rustc` 1.93.1 has no Windows target) and MinGW-w64's GCC as
linker:

```sh
TC=~/.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin
RUSTC=$TC/rustc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  CARGO_TARGET_DIR=target-win $TC/cargo build --release --offline \
  --target x86_64-pc-windows-gnu -p gdl_client --bin gdl_play
```

The result imports only system DLLs (kernel32, user32, gdi32, ws2_32, …;
the MinGW runtime is static). Under Wine (10.0, Ubuntu 10.0~repack-12ubuntu1;
a scratch WINEPREFIX) it renders on the RX 580 through winevulkan/RADV and
its per-update simulation hashes equal the Linux build's over a 600-update
G1 walk. Not tested: a Windows installation, WASAPI output (a test would
play on the host's speakers), DirectX 12.


## Installer and packages (2026-10-28, M21)

The installer (`installer/`, docs/decisions/0005-installer.md) is C++17 on
Qt 6 Widgets, built with CMake and Ninja. Both packages carry its Qt (the
owner's decision, 2026-10-28: the user installs nothing first).

| Component | Version |
| --- | --- |
| Qt (Linux, system; bundled into the package) | 6.10.2 (Ubuntu qt6-base-dev 6.10.2+dfsg-7) |
| CMake / Ninja (Linux) | 4.2.3 (4.2.3-2ubuntu2) / 1.13.2 |
| GCC (Linux) | 15.2.0 |
| bubblewrap (bundle checks) | /usr/bin/bwrap (Ubuntu 26.04) |
| Windows build host | the owner's Windows 11 Pro machine over SSH (OpenSSH, PowerShell 7.6.7), working folder P:\gauntlet |
| Qt (Windows) | 6.11.1 mingw_64, The Qt Company's official binaries (C:\Qt\6.11.1\mingw_64) |
| MinGW-w64 GCC (Windows, Qt kit) | 13.1.0 x86_64-posix-seh-rev1, MinGW-Builds (C:\Qt\Tools\mingw1310_64) |
| CMake / Ninja (Windows, Qt kit) | 3.30.5 / 1.12.1 |
| MinGW-w64 GCC (Windows cross, game and importer) | 13.2.0, POSIX threads (g++-mingw-w64-x86-64-posix 13.2.0-6ubuntu1+26.1) |
| Wine (Windows checks) | 10.0~repack-12ubuntu1 |

Linux: `bash tools/package.sh linux OUT` builds the release programs and
the installer (DT_RPATH `$ORIGIN/lib`, installer/CMakeLists.txt), then
tools/python/bundle_qt_linux.py copies Qt's libraries, the X11 and Wayland
platform plugins (with xdg-shell, the bradient decorations and the compose
and IBus input methods) and their less common dependencies into lib/ and
plugins/ with a qt.conf: 38 files, 64.2 MiB (half of it ICU's data). Left
to the system: the C library, the GCC runtime (libstdc++, libgcc_s,
libgomp, libatomic), the GL/EGL dispatch libraries, core X11/xcb and
wayland-client, fontconfig/freetype/harfbuzz, GLib, D-Bus, systemd and
the compression libraries — every desktop has them and they must match
the running system. bundled-libraries.txt records each file's Debian
package. The package needs glibc 2.43: the game itself already does
(acosf, asinf, atan2f, coshf and sinhf at GLIBC_2.43; pidfd_spawnp at
2.39), and so does libQt6Gui; the libstdc++ floor is GLIBCXX_3.4.30 and
CXXABI_1.3.15. A build for older distributions needs an older build
environment (not done).

Windows: `GDL_WINDOWS_HOST=10.1.0.91 bash tools/package.sh windows OUT`
cross-builds the game and the importer as in "Windows build" above, copies
installer/ and tools/windows/build-installer.ps1 to the host, builds the
installer there with the Qt kit's MinGW and deploys it with windeployqt
(Qt Core, Gui, Widgets, qwindows, qmodernwindowsstyle and the MinGW
runtime DLLs; no translations, shader compilers, software OpenGL, Network,
Svg or image/icon/TLS/touch plugins: 9 files), and fetches the deployment
and its notice inputs (the kit's qtbase SBOM and MinGW license files).
`WIN_INSTALLER_DIR=DIR` packages an earlier deployment instead. The
installer links with `-s -Wl,--no-insert-timestamp` (strip would insert a
PE time stamp): two clean builds are byte-identical. The host's
`deployed-files.txt` hashes are checked after the copy.

Notices: tools/python/library_licenses.py writes each package's
LIBRARY_LICENSES.txt — Linux: the Debian copyright file of every bundled
binary package and the common-licenses texts they refer to; Windows: the
42 third-party components Qt's SBOM (qtbase-6.11.1.spdx.json) records in
the deployed Qt files with their copyright texts, their license texts from
Qt's own LICENSES/, and the MinGW runtime's and Ubuntu mingw-w64's notices.

Qt source (the LGPL's corresponding source), in private/qt-src/ and
copied by package.sh to OUT/qt-source/ beside the packages:

| File | SHA-256 | Verification |
| --- | --- | --- |
| windows/qtbase-everywhere-src-6.11.1.tar.xz | d9594a31228aa23ad6b531719a29b45f0f3989fe6c136d45767ea179f233c1ac | MD5 755fcf32… as download.qt.io's md5sums.txt; 22,492 of the 22,895 files of the kit's qtbase-6.11.1.source.spdx have its SHA-1 (CRLF line ends as on Windows), 402 are absent (tests, git files), 1 differs (.tag, written by git export) |
| ubuntu/qt6-base_6.10.2+dfsg-7.dsc | 002cf6224c7569157884020e2575fbdb5588fe6558568ff791d64a1f99d1efc9 | Ubuntu resolute InRelease (gpgv: Ubuntu Archive Automatic Signing Key (2018)) → universe Sources.xz → these three SHA-256 |
| ubuntu/qt6-base_6.10.2+dfsg.orig.tar.xz | b5f8b150b1f0dd8a3ec51764cfd7817562f6b543b5f37dc5cab7c7f1823d313d | as above |
| ubuntu/qt6-base_6.10.2+dfsg-7.debian.tar.xz | 5071476712a4e641cbb730efb92c59bd9a15bf5be9f953ce114b99e4f8cfa758 | as above |

Every bundled Qt file on Linux comes from qt6-base's binary packages. Not
redistributed: the source of the MinGW runtime DLLs (GCC 13.1.0 with the
GCC Runtime Library Exception; LIBRARY_LICENSES.txt points to it).
