# M04: WAD.BIN schema and verification

Profile: profiles/ps2/slus-20047.json. Source SHA-256:
e047a4e060c2372407bd6e00c380602dcddeadb0f31637ba88630a8c029da946.

The candidate schema from the pinned haekb extractor is now verified against
the exact disc and local loader instructions. At EE:0x001d48e8, InitWad reads
four bytes for num_files_in_wad, allocates count * 0x10, seeks to byte 4 and
reads that table. GetWadInfo at EE:0x001d47f8 compares the word at row offset 8
and advances rows by 0x10. Disassembly and decompiler interpretations are kept
separately in private/reports/ghidra/.

All integer fields are little-endian. Header: u32 count. Each 16-byte row:

| Row offset | Width | Meaning |
| --- | --- | --- |
| 0 | i32 | Decoded length; negative rejected |
| 4 | i32 | Archive file offset; negative rejected |
| 8 | u32 | Original path hash |
| 12 | i32 | Stored zlib length, or -1 for uncompressed |

Observed: 2,915 entries, 200 uncompressed, 2,715 zlib-compressed, zero duplicate
hashes, zero overlapping/out-of-range payloads, 668,935,127 decoded bytes total.
Largest declared decoded entry is 7,899,952 bytes. All 2,915 Rust decodes passed
length, stream-end, full-input-consumption and checksum checks. Every table field,
stored hash and decoded hash agrees with the pinned upstream table reader and
Python's independent zlib implementation. No discarded or unsupported archive entry.

Stable identity is archive SHA-256 + index + original hash. Extraction filenames
use index-hash.dat; candidate names never become output paths. Duplicate hashes
are retained distinctly and reported, as exercised by synthetic tests. Unknown
bytes remain available in their original payloads.

GetWadInfo builds directory + backslash + filename, adds .ps2 if no dot occurs,
then calls BackslashMe and fed_upper before hash_me. The local hash function
at EE:0x001d45b0 uses initial words 0x9e3779b9, 0x9e3779b9 and 13, matching the
upstream hypothesis. Dictionary matches are still labels rather than recorded
runtime requests. The initial dictionary matches 2,909 entries, leaving six.

Structural inspection identifies entry 463 as an additional v23 texture table.
Entries 462 and 463 have the exact hashes of PLAYERS/JES/ANIM/TEXTURES.PS2 and
PLAYERS/JES/ANIM/TEXDEF.PS2 under the observed normalization. Their adjacency to
PLAYERS/JES/ANIM/ANIM.PS2 and bitmap labels support these proposed names. They
remain independently indexed and runtime lookup confirmation is pending.

Some unnamed records contain development text. They are preserved as bytes,
never executed as scripts. Name recovery is intentionally separate from extraction.

Parser policy: archive <= 1 GiB, count <= 100,000, decoded entry <= 64 MiB,
total declared output <= 2 GiB. Counts and ranges are checked before allocation;
overlaps and unknown negative sentinels are errors. One extra bounded output byte
detects a stream that exceeds its declared length. The extractor publishes a
staging directory only after all entries succeed and preserves existing outputs.
The inspector records every payload outcome even when decompression fails.

Synthetic tests cover truncation, count abuse, invalid ranges/sentinels, overlaps,
duplicate hashes, corrupt checksums, wrong output sizes and trailing compressed
bytes. Actual-data comparison covers every entry. Runtime mapping of a first
level's resource requests is still needed before the full M04 exit gate closes.
