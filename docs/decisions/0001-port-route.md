# 0001: Native reconstruction from the supplied PS2 disc

Use bounded native asset readers, a Bevy-independent Rust simulation and Bevy
presentation. PCSX2 is a separate reference. Preserve original data in private/.

The user explicitly selected this route and PS2 image. The existing GameCube port
is a research lead; its supported GameCube input cannot substitute for this disc.
No architecture switch is authorized by its existence. Recompilation remains a
bounded fallback experiment after a measured blocker, not the current product.

Source review found .symtab and .mdebug in this exact ELF, so investigate local
symbols/types before consulting cross-console type layouts. Preserve guest virtual
addresses separately from file offsets. Generic MIPS disassemblers are insufficient
for R5900 instructions (see the executable dossier).

Own new code uses MIT; no upstream runtime code has been incorporated. Third-party
research stays pinned and separately attributed. This decision is reversible before
any license-affecting code reuse, which must be explicit.
