# Pinned toolchain and reference environment

Verified on 2026-10-12. Local development is Linux x86-64; Windows testing has
not been performed. No native Windows package is currently claimed.

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
