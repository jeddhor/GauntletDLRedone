# Third-party code and research provenance

Retail game files and BIOS data remain user-supplied private evidence and are not
covered by this project's MIT license or distributed with source/builds.

New Rust and research-tool implementations are original project code. The
project is not clean-room: the following pinned research was consulted.

- [haekb WAD decoder](https://github.com/haekb/gdl_wad_decoder/tree/981f4ec9586903257e8f2161e918bbbe5263a6eb), MIT. The private comparison harness calls its unchanged table reader and hash/name helpers; these files are not included in the native executable or redistributed by this project.
- [gdl_tools](https://github.com/MosesofEgypt/gdl_tools/tree/47799f19ec85fa49af87eec4c64a27ee1fc87d7c), LGPL 2.1 license text inspected. Format definitions and palette behavior were consulted; no code was transplanted into Rust. Audit precise notices before any future implementation reuse.
- [GameCube native port](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-PC-Port/tree/2b23539febf2cdd894227ff005709bc37b0bea9e), GPL-3.0-only. Documentation/tree research only; no copied or linked runtime code.
- [GameCube decompilation](https://github.com/sabishii-bit/Gauntlet-Dark-Legacy-Decompilation/tree/4fdf0492d5d828b16ee1a961c17bf855a8d2668f), documentation/tree inspection only; no blanket code-license assumption. External leaked Xbox symbol definitions were not imported.
- [Emotion Engine: Reloaded](https://github.com/chaoticgd/ghidra-emotionengine-reloaded), Apache-2.0. Separate local analysis module; the tested release identity is in docs/toolchain.md.
- [PCSX2](https://github.com/PCSX2/pcsx2), GPL-3.0-or-later source notices. Separate reference application and read-only source investigation; no native-runtime linkage or modifications.
- [Bevy v0.18.1](https://github.com/bevyengine/bevy/tree/v0.18.1), MIT OR Apache-2.0; native desktop dependency and API/example research.

Direct Cargo dependencies: flate2 (MIT OR Apache-2.0), serde/serde_json (MIT OR
Apache-2.0), sha2 (MIT OR Apache-2.0), png (MIT OR Apache-2.0), Bevy (MIT OR
Apache-2.0). Cargo.lock records exact transitive versions. A full transitive
license-notice/package audit is required at M21 and has not yet been performed.
