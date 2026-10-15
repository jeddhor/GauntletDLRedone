# Third-party code and research provenance

Retail game files and BIOS data remain user-supplied private evidence and are not
covered by this project's MIT license or distributed with source/builds.

New Rust and research-tool implementations are original project code. The
project is not clean-room: the following pinned research was consulted.

- [haekb WAD decoder](https://github.com/haekb/gdl_wad_decoder/tree/981f4ec9586903257e8f2161e918bbbe5263a6eb), MIT. The private comparison harness calls its unchanged table reader and hash/name helpers; these files are not included in the native executable or redistributed by this project.
- [gdl_tools](https://github.com/MosesofEgypt/gdl_tools/tree/47799f19ec85fa49af87eec4c64a27ee1fc87d7c), LGPL 2.1 license text inspected. Format definitions, palette behavior, VIF encodings, strip assembly and world graph interpretation were consulted; no code was transplanted into Rust. The private comparison harness invokes its unchanged geometry decoder and original padding helper; these are not linked or distributed with the runtime. Audit precise notices before any future implementation reuse.
- [GameCube native port](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-PC-Port/tree/2b23539febf2cdd894227ff005709bc37b0bea9e), GPL-3.0-only. Documentation/tree research only; no copied or linked runtime code.
- [GameCube decompilation](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-Decompilation/tree/4fdf0492d5d828b16ee1a961c17bf855a8d2668f), documentation/tree inspection only; no blanket code-license assumption. External leaked Xbox symbol definitions were not imported.
- [Emotion Engine: Reloaded](https://github.com/chaoticgd/ghidra-emotionengine-reloaded), Apache-2.0. Separate local analysis module; the tested release identity is in docs/toolchain.md.
- [PCSX2](https://github.com/PCSX2/pcsx2), GPL-3.0-or-later source notices. Separate reference application and read-only source investigation; no native-runtime linkage or source modifications. Pinned v2.6.3 iFPU.cpp and VUops/microVU Upper/Macro/Misc arithmetic paths were consulted to design independently written finite scalar/VU arithmetic models; no emulator source was transplanted. Stock debugger experiments and their controlled-input scope are recorded in the research dossiers.
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
Apache-2.0). Cargo.lock records exact transitive versions. A full transitive
license-notice/package audit is required at M21 and has not yet been performed.
