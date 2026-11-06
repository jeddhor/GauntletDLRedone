# Third-party code and research provenance

The project is licensed GPL-3.0-or-later (docs/decisions/0004-license.md; MIT
until 2026-10-27). Retail game files and BIOS data remain user-supplied private
evidence and are not covered by this project's license or distributed with
source/builds.

New Rust and research-tool implementations are original project code. The
project is not clean-room: the following pinned research was consulted.

- [haekb WAD decoder](https://github.com/haekb/gdl_wad_decoder/tree/981f4ec9586903257e8f2161e918bbbe5263a6eb), MIT (Copyright (c) 2016 Housey). The private comparison harness calls its unchanged table reader and hash/name helpers; its code is not included in the native programs. Since M21 the release's `import/catalog.json` carries the archive entry names its dictionary gave (matched by path hash); the MIT notice is in THIRD_PARTY_LICENSES.txt.
- [gdl_tools](https://github.com/MosesofEgypt/gdl_tools/tree/47799f19ec85fa49af87eec4c64a27ee1fc87d7c), LGPL 2.1 license text inspected. Format definitions, palette behavior, VIF encodings, strip assembly and world graph interpretation were consulted; no code was transplanted into Rust. The private comparison harness invokes its unchanged geometry decoder and original padding helper; these are not linked or distributed with the runtime. Audit precise notices before any future implementation reuse.
- [GameCube native port](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-PC-Port/tree/2b23539febf2cdd894227ff005709bc37b0bea9e), GPL-3.0-only. Documentation/tree research only; no copied or linked runtime code.
- [GameCube decompilation](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-Decompilation/tree/4fdf0492d5d828b16ee1a961c17bf855a8d2668f), documentation/tree inspection only; no blanket code-license assumption. External leaked Xbox symbol definitions were not imported.
- [Emotion Engine: Reloaded](https://github.com/chaoticgd/ghidra-emotionengine-reloaded), Apache-2.0. Separate local analysis module; the tested release identity is in docs/toolchain.md.
- [PCSX2](https://github.com/PCSX2/pcsx2), GPL-3.0-or-later source notices. Separate reference application and read-only source investigation; no native-runtime linkage or source modifications of the checkout. Pinned v2.6.3 iFPU.cpp and VUops/microVU Upper/Macro/Misc arithmetic paths were consulted to design independently written finite scalar/VU arithmetic models; no emulator source was transplanted there. **Reused code** (since the GPL-3.0-or-later relicense, docs/decisions/0004-license.md): `crates/gdl_core/src/spu.rs` translates the SPU2 voice resampling of PCSX2 v2.6.3 into Rust — `pcsx2/SPU2/interpolate_table.h` (the 256 × 4 Gaussian table, copied value for value; PCSX2 credits its generator to Near, with credit to nocash and Ryphecha), `pcsx2/SPU2/Mixer.cpp` UpdatePitch, GetVoiceValues and GaussianInterpolate, and `pcsx2/SPU2/spu2sys.cpp` V_Voice::Start's key-on state. Notice: SPDX-FileCopyrightText: 2002-2025 PCSX2 Dev Team; SPDX-License-Identifier: GPL-3.0+ (kept in the file's header). `crates/gdl_client/src/movie.rs` `ipu_csc` translates `pcsx2/IPU/yuv2rgb.cpp` yuv2rgb_reference (reference source 0f71e668: the IPU colour space conversion's integer coefficients and rounding; notice SPDX-FileCopyrightText: 2002-2026 PCSX2 Dev Team, GPL-3.0+). The movie decoder's VLC tables (`crates/gdl_formats/src/mpeg2.rs`) were written from the MPEG-2 standard's tables and compared with `pcsx2/IPU/mpeg2_vlc.h` and IPU_MultiISA.cpp's scans by tools/python/mpeg2_tables_check.py; not copied. Stock debugger experiments and their controlled-input scope are recorded in the research dossiers.
- [FFmpeg](https://ffmpeg.org/) n8.0.1, LGPL-2.1-or-later. **Reused code:** the inverse DCT of `crates/gdl_formats/src/mpeg2.rs` (docs/decisions/0007-native-movie-video.md) implements FFmpeg's 8-bit "simple" IDCT, `libavcodec/simple_idct_template.c` (constants W1–W7, the row pass with its DC-only shortcut, the column pass and their rounding), so that the decoded movie pictures equal FFmpeg's. Notice: "Simple IDCT, Copyright (c) 2001 Michael Niedermayer <michaelni@gmx.at>", based upon code from mpeg2dec (idct_mmx.c, Aaron Holtzman); used under GPL-3.0-or-later as LGPL 2.1 section 3 permits; the notice is in the source and in THIRD_PARTY_LICENSES.txt. FFmpeg 8.0.1 (docs/toolchain.md) is also the reference of tools/python/movie_check.py; until release 1.3.0 the game ran the FFmpeg program for movie video (docs/decisions/0003-movies.md); it does not any more.
- fdlibm-derived libm in the original SLUS_200.47 executable (Sun Microsystems
  fdlibm notice, as carried by newlib's float `e_atan2f`/`s_atanf`/`k_sinf`/`k_cosf`/`e_rem_pio2f`).
  `gdl_core::libm` was written from the reviewed EE instruction listings and the
  ELF's own constant words/tables, so its algorithm structure is fdlibm's. No
  newlib/fdlibm source file was copied or inspected for this work. The fdlibm
  notice is understood (unverified upstream claim) to permit use, copying,
  modification and distribution with the notice preserved; obtain and verify
  the exact notice text and include it in release notices at M21.
- [Bevy v0.18.1](https://github.com/bevyengine/bevy/tree/v0.18.1), MIT OR Apache-2.0; native desktop dependency and API/example research. **Reused code:** `crates/gdl_client/src/ssao_preprocess_depth.wgsl` and `ssao_gtao.wgsl` are bevy_pbr 0.18.1's `src/ssao/preprocess_depth.wgsl` and `src/ssao/ssao.wgsl` (SHA-256 5f7ff08d…feac1, 393ba17a…4778) with one change each marked GDL CHANGE (the 3D viewport inside the prepass textures); their notices are Bevy's in THIRD_PARTY_LICENSES.txt.
- [Real-ESRGAN v0.3.0](https://github.com/xinntao/Real-ESRGAN/tree/v0.3.0), BSD-3-Clause (Copyright (c) 2021, Xintao Wang). The HD texture generator (`gdl_client::hd::esrgan`, a NATIVE ENHANCEMENT) is an independently written wgpu implementation of its SRVGGNetCompact network and embeds weights derived from its released checkpoints realesr-general-x4v3.pth (SHA-256 8dc7edb9…6292) and realesr-general-wdn-x4v3.pth (1641f8c4…6dca), release assets v0.2.5.0, mixed 0.5 / 0.5 in float32 by tools/python/hd_weights.py into crates/gdl_client/assets/hd/realesr-general-x4v3-dn05.bin (SHA-256 e6626f1f…ef4b; provenance in the .json beside it). The license text is crates/gdl_client/assets/hd/LICENSE-Real-ESRGAN.txt and goes into the release's notices. Research tooling (private, not distributed) also ran its Python inference code's architecture in numpy and the realesrgan-ncnn-vulkan 20220424 program for comparison.

Direct Cargo dependencies: flate2 (MIT OR Apache-2.0), serde/serde_json (MIT OR
Apache-2.0), sha2 (MIT OR Apache-2.0), png (MIT OR Apache-2.0), Bevy (MIT OR
Apache-2.0), alsa 0.9.1 on Linux (Apache-2.0 OR MIT), cpal 0.15.3 on Windows
(Apache-2.0). Cargo.lock records exact transitive versions. Transitive audit
(M21, 2026-10-28, regenerated for release 1.1.0, 2026-11-01; tools/python/
third_party_licenses.py → release/THIRD_PARTY_LICENSES.txt, shipped with the
programs): the 295 crates compiled
into gdl_play and gdl_import for Linux and Windows are under MIT, Apache-2.0,
Zlib, BSD-2/3-Clause, ISC, Unlicense, MIT-0, 0BSD, CC0-1.0 or Unicode-3.0
terms (one "Apache-2.0 OR GPL-2.0-only" taken under Apache-2.0), all
compatible with GPL-3.0-or-later; their license and notice files are
reproduced there.

- [Qt 6](https://www.qt.io/) (Core, Gui, Widgets and platform plugins), LGPL-3.0-only: the installer (`installer/`) links it dynamically and unmodified, and both packages carry it beside the installer — Qt 6.10.2 built from its source for Ubuntu 20.04 (tools/linux/build-qt.sh; with Ubuntu 20.04's xcb utility libraries and xkbcommon) on Linux, The Qt Company's Qt 6.11.1 for MinGW (with the MinGW-w64 runtime: libgcc_s, libstdc++ under GPL-3.0 with the GCC Runtime Library Exception, winpthreads) on Windows. Each package's LIBRARY_LICENSES.txt (tools/python/library_licenses.py) lists the files with their notices; their source (qtbase 6.11.1, qt6-base 6.10.2+dfsg, and for the MinGW runtime GCC 13.1.0 with the MinGW-Builds scripts and patches) is distributed beside the packages (library-source/). Not used by the game.
