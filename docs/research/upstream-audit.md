# M02: pinned upstream audit and reuse decision

Consulted on 2026-10-12. Raw downloaded sources and API trees remain private.
These are source/documentation inspections; no upstream game was run or claimed
compatible with this disc.

| Project | Pinned commit | Inspected evidence and decision |
| --- | --- | --- |
| haekb/gdl_wad_decoder | 981f4ec9586903257e8f2161e918bbbe5263a6eb | MIT license; PS2 table reader/hash/dictionary; all entries independently compared with native decode. Dictionary originated from another platform and names remain qualified. Research harness executes pinned helper functions; runtime does not embed them. |
| MosesofEgypt/gdl_tools | 47799f19ec85fa49af87eec4c64a27ee1fc87d7c | LGPL 2.1 license text; inspected PS2 object/texture/world/animation definitions and texture serialization. PS2 format overlap now locally confirmed for TEXDEF metadata. No implementation transplanted. Exact license -only/or-later qualifier requires additional notice audit before copying/linking code. |
| sabishii-bit/Gauntlet-Dark-Legacy-PC-Port | 2b23539febf2cdd894227ff005709bc37b0bea9e | GPL-3.0-only README/license; states USA GameCube GUNE5D input and alpha. Documentation/tree audit only; build/tests not performed. No claim of PS2 input support and no runtime reuse. |
| sabishii-bit/Gauntlet-Dark-Legacy-Decompilation | 4fdf0492d5d828b16ee1a961c17bf855a8d2668f | README/tree audit identifies GameCube Rev 1 and external leaked Xbox-symbol provenance. Those external type definitions were not downloaded/imported. Prefer local PS2 symbols. No GameCube image supplied; matching build not performed. No blanket license assumption. |
| chaoticgd/ghidra-emotionengine-reloaded | fcc5deac45d8f5c27c632bfe0db928630939676e | Apache-2.0; README/source tree audit; a separately pinned compatible release v2.1.38 is actually loaded and tested with the local ELF. |
| PCSX2 local source | 0f71e668b8adc950db7de1b1b15c2f5080c102e9 | GPL-3.0-or-later source notices; read-only disassembler/config/PINE investigation. Installed binary is separately pinned and is not assumed to match this source revision. No patches or shipped runtime linkage. |

Bevy examples and APIs are checked at v0.18.1. Its native client successfully
builds and renders original textures here. New code remains MIT; source/asset
rights are separate. This research is not clean-room.

The audit confirms the selected PS2 native reconstruction route. Optional
upstream builds and GameCube assets do not block it. MeltyTool and PS2Recomp
remain unexecuted optional leads, not accepted dependencies or compatibility claims.

R5900 tool verification: Ghidra 12.1.2 + compatible extension imports/analyzes this
ELF, exports the known symbol addresses and supplies R5900 instruction listings.
Independent generic LLVM MIPS decoding at EE:0x00100020 mislabels instruction
word 0x7c400000 as EXT. The pinned PCSX2 disassembler maps opcode 0x1f to SQ,
consistent with the R5900 zero-BSS path. A direct debugger screenshot comparison
is still pending, so tool compatibility is not promoted beyond these checks.
