# M01: exact disc and executable dossier

Verified locally on 2026-10-12 from the supplied, unmodified image.

| Item | Observation |
| --- | --- |
| Filename | Gauntlet - Dark Legacy.iso |
| Size | 620,447,744 bytes |
| SHA-256 | fb47d40667dfd5b51ffdf2cf7030a3737510b0c4f29dada2a953cb34c65545f2 |
| Filesystem | ISO9660; 2,048-byte logical sectors; volume size equals image size |
| Volume/system | GAUNTLET / PLAYSTATION |
| Publisher/preparer | MIDWAY / STEVEN BENNETTS |
| Files | 56 files in 4 directories; all directly hashed from ISO extents |
| Boot | SYSTEM.CNF: BOOT2 = cdrom0:\\SLUS_200.47;1 |
| Configuration | VER = 1.20; VMODE = NTSC |
| Region/revision | NTSC observed; USA inferred from SLUS serial; exact retail revision not independently established |
| ELF SHA-256 | 69056dc6969c3740a82422ecdf7f771b19b26e20aff208d620c6efec66a3d527 |
| Archive SHA-256 | e047a4e060c2372407bd6e00c380602dcddeadb0f31637ba88630a8c029da946 |

SYSTEM.CNF VER is a configuration field. Do not equate it to a verified retail
revision. The profile qualifies this build by hashes rather than a guessed revision.

Two separate 7-Zip extractions produced identical deterministic disc and ELF
manifests. A direct ISO9660 reader independently hashed every extent and verified
the extracted lengths, hashes, file list, dual-endian fields and bounds. Both
manifest comparisons passed. No inaccessible or failed extraction entries occurred.
Raw inventories/logs are in private/reports/, including repeat/.

The ELF is little-endian ELF32 EXEC. readelf prints the general MIPS machine
label, but flags include R5900, eabi64 and mips3. Its entry is EE:0x00100008.
There is one RWE PT_LOAD: file offset 0x1000, guest virtual/physical address
0x00100000, file size 0x1bab48, memory size 0x1e6884, alignment 0x1000.
Addresses are guest addresses, not host pointers or ISO/file offsets.

There are 30 sections, including .text, .vutext, .data, .rodata, .sbss, .bss,
.mdebug, .symtab, .strtab, .stab/.stabstr and twelve .DVP overlay payload sections.
These DVP sections are a vector-program investigation lead; their presence does
not establish that the game dynamically overlays EE gameplay code.

The symbol table contains 8,836 entries, including 3,584 named function entries.
Ghidra's first R5900 analysis exposes 3,589 functions. Names are real evidence;
inferred parameter types from the decompiler remain provisional. The first type
export found 12 library/ELF structures and no player/entity structures. Having a
.mdebug section does not imply that full game types were recovered successfully.

The disc also contains eleven IRX modules, IOPRP20/IOPRP21 images, icons, and
36 MPEG/MPEG2 PSS files. Archive ELF payloads are inventoried separately in
private/reports/archive-executables.json. Runtime loading/overlay behavior still
needs reference observation; this inventory cannot prove its absence.

Reproduce with tools/python/disc_dossier.py after extracting to private/disc/.
The current script intentionally supports this boot ELF/profile, rather than
claiming a general PS2 disc importer. The release import workflow is M21 work.
