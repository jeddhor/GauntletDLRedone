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
- [PCSX2](https://github.com/PCSX2/pcsx2), GPL-3.0-or-later source notices. Separate reference application and read-only source investigation; no native-runtime linkage or source modifications of the checkout. Pinned v2.6.3 iFPU.cpp and VUops/microVU Upper/Macro/Misc arithmetic paths were consulted to design independently written finite scalar/VU arithmetic models; no emulator source was transplanted there. **Reused code** (since the GPL-3.0-or-later relicense, docs/decisions/0004-license.md): `crates/gdl_core/src/spu.rs` translates the SPU2 voice resampling of PCSX2 v2.6.3 into Rust — `pcsx2/SPU2/interpolate_table.h` (the 256 × 4 Gaussian table, copied value for value; PCSX2 credits its generator to Near, with credit to nocash and Ryphecha), `pcsx2/SPU2/Mixer.cpp` UpdatePitch, GetVoiceValues and GaussianInterpolate, and `pcsx2/SPU2/spu2sys.cpp` V_Voice::Start's key-on state. Notice: SPDX-FileCopyrightText: 2002-2025 PCSX2 Dev Team; SPDX-License-Identifier: GPL-3.0+ (kept in the file's header). Stock debugger experiments and their controlled-input scope are recorded in the research dossiers.
- fdlibm-derived libm in the original SLUS_200.47 executable (Sun Microsystems
  fdlibm notice, as carried by newlib's float `e_atan2f`/`s_atanf`/`k_sinf`/`k_cosf`/`e_rem_pio2f`).
  `gdl_core::libm` was written from the reviewed EE instruction listings and the
  ELF's own constant words/tables, so its algorithm structure is fdlibm's. No
  newlib/fdlibm source file was copied or inspected for this work. The fdlibm
  notice is understood (unverified upstream claim) to permit use, copying,
  modification and distribution with the notice preserved; obtain and verify
  the exact notice text and include it in release notices at M21.
- [Bevy v0.18.1](https://github.com/bevyengine/bevy/tree/v0.18.1), MIT OR Apache-2.0; native desktop dependency and API/example research.

Direct Cargo dependencies: flate2 (MIT OR Apache-2.0), serde/serde_json (MIT OR
Apache-2.0), sha2 (MIT OR Apache-2.0), png (MIT OR Apache-2.0), Bevy (MIT OR
Apache-2.0), alsa 0.9.1 on Linux (Apache-2.0 OR MIT), cpal 0.15.3 on Windows
(Apache-2.0). Cargo.lock records exact transitive versions. Transitive audit
(M21, 2026-10-28; tools/python/third_party_licenses.py → release/
THIRD_PARTY_LICENSES.txt, shipped with the programs): the 290 crates compiled
into gdl_play and gdl_import for Linux and Windows are under MIT, Apache-2.0,
Zlib, BSD-2/3-Clause, ISC, Unlicense, MIT-0, 0BSD, CC0-1.0 or Unicode-3.0
terms (one "Apache-2.0 OR GPL-2.0-only" taken under Apache-2.0), all
compatible with GPL-3.0-or-later; their license and notice files are
reproduced there.

- [Qt 6](https://www.qt.io/) (Core, Gui, Widgets and platform plugins), LGPL-3.0-only: the installer (`installer/`) links it dynamically and unmodified, and both packages carry it beside the installer — Ubuntu's Qt 6.10.2 (with ICU, double-conversion, libb2, md4c, pcre2-16, the xcb utility libraries, xkbcommon, SM/ICE and wayland-cursor) on Linux, The Qt Company's Qt 6.11.1 for MinGW (with the MinGW-w64 runtime: libgcc_s, libstdc++ under GPL-3.0 with the GCC Runtime Library Exception, winpthreads) on Windows. Each package's LIBRARY_LICENSES.txt (tools/python/library_licenses.py) lists the files with their notices; Qt's source (qtbase 6.11.1, Ubuntu qt6-base 6.10.2+dfsg-7) is distributed beside the packages. Not used by the game.
