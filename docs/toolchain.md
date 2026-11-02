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
Since 2026-10-30 also `bevy_anti_alias` (contrast-adaptive sharpening,
research/enhancements.md "Graphics"): bevy_anti_alias 0.18.1 was already in
Cargo.lock and the registry cache, so the lockfile is unchanged. Its
plugin group is not used (TAA needs bevy_post_process, not built); FXAA and
CAS are added singly.
Since 2026-10-31 (lighting, research/enhancements.md "Lighting") also the
Bevy features `bevy_post_process` (bloom), `smaa_luts`, `ktx2` and
`zstd_rust` (SMAA's lookup textures are KTX2 with zstd supercompression;
the pure-Rust decoder keeps the Windows cross-build free of C code). New
crates in Cargo.lock, fetched from crates.io: bevy_post_process 0.18.1
(MIT OR Apache-2.0, the pinned Bevy release), ktx2 0.4.0 (Apache-2.0),
ruzstd 0.8.3 (MIT), twox-hash 2.1.3 (MIT). gdl_play disables the
post-processing plugin group and adds BloomPlugin and SmaaPlugin singly
(the group's MSAA writeback duplicates gdl_client::writeback); the
texture laboratory and gdl_scene disable it. THIRD_PARTY_LICENSES.txt
(tools/python/third_party_licenses.py) lists them since release 1.1.0, with
bevy_anti_alias and Real-ESRGAN's notice (295 crates, 290 before).
Since 2026-10-31 (HD textures, research/enhancements.md) gdl_client names
wgpu =27.0.1 directly, without default features — the version and the
features Bevy 0.18.1 already builds (Cargo.lock gains only the dependency
edge, no package): the generator opens its own compute device.
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

## Upscaler research tools (2026-10-30, Checkpoint B)

Research only, nothing ships: private/upstream/upscalers holds
Real-ESRGAN v0.3.0 (source tarball SHA-256 4fbaa9470fc2e2bffa2f6b0e9b7304b3102d7b4d0c4b9dc3a7ff3d237499fed1,
BSD-3-Clause), its v0.2.5.0 release weights realesr-animevideov3.pth
(b8a8376811077954d82ca3fcf476f1ac3da3e8a68a4f4d71363008000a18b75d), realesr-general-x4v3.pth
(8dc7edb9ac80ccdc30c3a5dca6616509367f05fbc184ad95b731f05bece96292) and realesr-general-wdn-x4v3.pth
(1641f8c4464b9f097c9fdda5589273713f67cf59f3d909e0bd688f0cee269dca), realesrgan-ncnn-vulkan-20220424-ubuntu.zip
(e5aa6eb131234b87c0c51f82b89390f5e3e642b7b70f2b9bbe95b6a285a40c96; binary 22c2296a39b40cc43ac92615c827ab6d4b60b7142e8cee59f718e3f8e2cdbda9) and xBRZ 1.9
(xBRZ_1.9.zip b2dff73b3abd24a18a7cde78d5ff5ed8f0922296dce6ed734dce2264cd0a0fc9, GPL-3.0; built by
tools/python/upscale_compare.py with the system g++ 15.2.0 -O2). numpy
2.3.5 and Pillow 12.1.1 are the system packages.

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
| Linux release build environment (since 2026-10-29) | podman 5.7.0, image gdl-build-focal:2 (tools/linux/Containerfile; id e4c9337b6feafa6c…) from docker.io/library/ubuntu:20.04@sha256:c664f8f86ed5a386b0a340d981b8f81714e21a8b9c73f658c4bea56aa179d54a |
| GCC (Linux release) | gcc-10 / g++-10 10.5.0-1ubuntu1~20.04 (Qt and the installer); the system gcc 9 links the Rust programs |
| GNU C library / libstdc++ (Linux release) | 2.31-0ubuntu9.18 / 10.5.0-1ubuntu1~20.04 (GLIBCXX_3.4.28) |
| CMake / Ninja (Linux release) | 3.31.6 (PyPI cmake==3.31.6; Qt 6.10 needs 3.22, the release has 3.16) / 1.10.0 |
| Qt (Linux, bundled into the package) | 6.10.2, built by tools/linux/build-qt.sh from qt6-base_6.10.2+dfsg.orig.tar.xz (73 MiB install in private/build/qt-6.10.2-focal) |
| Runtime check image (older distribution) | gdl-runtime-focal:1 (tools/linux/Containerfile.test; id 5d53a541e3c5fca0…): libc6 2.31-0ubuntu9.17, libstdc++6 10.5.0, libvulkan1 1.2.131.2-1, mesa-vulkan-drivers 21.2.6-0ubuntu0.1~20.04.2 |
| Qt (Linux, until 2026-10-28) | 6.10.2 (Ubuntu qt6-base-dev 6.10.2+dfsg-7; needed glibc 2.43) |
| CMake / Ninja / GCC (Linux host: development builds) | 4.2.3 / 1.13.2 / 15.2.0 |
| bubblewrap (bundle checks) | /usr/bin/bwrap (Ubuntu 26.04) |
| Windows build host | the owner's Windows 11 Pro machine over SSH (OpenSSH, PowerShell 7.6.7), working folder P:\gauntlet |
| Qt (Windows) | 6.11.1 mingw_64, The Qt Company's official binaries (C:\Qt\6.11.1\mingw_64) |
| MinGW-w64 GCC (Windows, Qt kit) | 13.1.0 x86_64-posix-seh-rev1, MinGW-Builds (C:\Qt\Tools\mingw1310_64) |
| CMake / Ninja (Windows, Qt kit) | 3.30.5 / 1.12.1 |
| MinGW-w64 GCC (Windows cross, game and importer) | 13.2.0, POSIX threads (g++-mingw-w64-x86-64-posix 13.2.0-6ubuntu1+26.1) |
| Wine (Windows checks) | 10.0~repack-12ubuntu1 |

Linux release build (2026-10-29; the owner's decision: older
distributions supported): `bash tools/package.sh linux OUT` runs everything
in an Ubuntu 20.04 container (podman, rootless, `--network=none`), so the
programs need no newer C library than 2.31. The host's pinned Rust toolchain
(~/.rustup, read-only) and Cargo registry (offline) are mounted; the build
tree is GDL_LINUX_WORK (default /tmp/gdl-linux-build). Qt is built once from
the shipped qtbase source (tools/linux/build-qt.sh: Core, Gui, Widgets,
DBus, the XCB platform plugin and the compose/IBus input methods, the AT-SPI
accessibility bridge; no OpenGL, Vulkan, Wayland, network, SQL or ICU;
HarfBuzz, PCRE2, libpng and the other third-party code from Qt's own
copies, FreeType and Fontconfig the system's). tools/linux/build-package.sh
then builds gdl_play and gdl_import (cargo, the system gcc as linker) and
the installer (g++-10; DT_RPATH `$ORIGIN/lib` only, installer/CMakeLists.txt),
copies Qt and its less common dependencies beside it (bundle_qt_linux.py:
26 files, 38.4 MiB; Ubuntu 20.04's xcb-util, xkbcommon and other xcb
libraries), writes LIBRARY_LICENSES.txt, and fails when any ELF file of the
package needs more than GLIBC_2.31 or GLIBCXX_3.4.28: the release needs
GLIBC_2.30 and GLIBCXX_3.4.28. Left to the system: the C library, the GCC
runtime, core X11/xcb (libX11, libxcb, libXau, libXdmcp, libX11-xcb),
fontconfig/FreeType/libpng, D-Bus, expat, systemd and the compression
libraries. Wayland desktops run the installer through XWayland, as they run
the game. The linked symbol versions do not change the game: its per-update
hashes equal the host build's over 1000 Tower and 1500 G1 updates, and the
installed game on Ubuntu 20.04 (glibc 2.31, Mesa 21.2.6) equals the same
installation on this host (glibc 2.43, Mesa 26) over 600 updates
(research/release.md "Older Linux distributions").

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

The Windows programs' panic locations name /cargo and /src instead of the
build machine's folders (`--remap-path-prefix`, package.sh; the Linux
container mounts them there).

Notices: tools/python/library_licenses.py writes each package's
LIBRARY_LICENSES.txt — Linux: the third-party components Qt's SBOM
(sbom/qtbase-6.10.2.spdx, tag-value, written by the Qt build) records in
the bundled Qt files (37), their license texts from Qt's LICENSES/, and the
Debian copyright file of every other bundled binary package with the
common-licenses texts they refer to; Windows: the 42 components Qt's SBOM
(qtbase-6.11.1.spdx.json) records in the deployed Qt files with their
copyright texts and license texts, and the MinGW runtime's and Ubuntu
mingw-w64's notices, with the GCC runtime's corresponding source.

Library sources (the LGPL's and GPL's corresponding source), in
private/library-src/{linux,windows} (hard links to private/qt-src/ for Qt)
and copied by package.sh to OUT/library-source/ beside the packages:

| File | SHA-256 | Verification |
| --- | --- | --- |
| windows/qtbase-everywhere-src-6.11.1.tar.xz | d9594a31228aa23ad6b531719a29b45f0f3989fe6c136d45767ea179f233c1ac | MD5 755fcf32… as download.qt.io's md5sums.txt; 22,492 of the 22,895 files of the kit's qtbase-6.11.1.source.spdx have its SHA-1 (CRLF line ends as on Windows), 402 are absent (tests, git files), 1 differs (.tag, written by git export) |
| ubuntu/qt6-base_6.10.2+dfsg-7.dsc | 002cf6224c7569157884020e2575fbdb5588fe6558568ff791d64a1f99d1efc9 | Ubuntu resolute InRelease (gpgv: Ubuntu Archive Automatic Signing Key (2018)) → universe Sources.xz → these three SHA-256 |
| ubuntu/qt6-base_6.10.2+dfsg.orig.tar.xz | b5f8b150b1f0dd8a3ec51764cfd7817562f6b543b5f37dc5cab7c7f1823d313d | as above |
| ubuntu/qt6-base_6.10.2+dfsg-7.debian.tar.xz | 5071476712a4e641cbb730efb92c59bd9a15bf5be9f953ce114b99e4f8cfa758 | as above |
| windows/gcc-13.1.0.tar.xz | 61d684f0aa5e76ac6585ad8898a2427aade8979ed5e7f85492286c4dfc13ee86 | ftp.gnu.org; gpgv against gnu-keyring.gpg: good signature (RSA 6C35B99309B5FA62, Jakub Jelinek); signature and keyring in private/library-src/verify |
| windows/mingw-builds-7ff96692a8032d893781523b311ac9b6cbbda0a1.tar.gz | cb92e4c9df87cd3536a4f9318620a3153bae3de813207aded08a6747ba2d0fc1 | github.com/niXman/mingw-builds archive of that commit: the develop branch's head (2023-05-24 13:06 UTC) when the kit's build ran (its build-info: 2023-05-24 22:40; mingw-builds-binaries' build.yml at tag 13.1.0-rt_v11-rev1 checks out develop); holds scripts/gcc-13.1.0.sh and the ten gcc patches build-info lists, in its order |
| windows/mingw-builds-13.1.0-rev1-build-info.txt | b695a15b07be3e1a974d320fe06cc103c81fca3f7e247ed023aaf31177678823 | the kit's C:\Qt\Tools\mingw1310_64\build-info.txt (MinGW-W64-builds-5.0.0, --mode=gcc-13.1.0 --rev=1 --rt-version=v11, sources, patches and configure lines) |

Linux (since 2026-10-29): the bundled Qt is built from the orig tarball
alone (linux/qt6-base_6.10.2+dfsg.orig.tar.xz, the same file); the Debian
files stay in private/qt-src as its provenance. Windows: the MinGW runtime
DLLs' source (GCC 13.1.0 with the GCC Runtime Library Exception) is
redistributed since 2026-10-29 (the owner's decision).
