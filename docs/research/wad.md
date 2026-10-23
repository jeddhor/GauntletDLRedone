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
bytes. Actual-data comparison covers every entry.

## Runtime requests of the first level (2026-10-23, actual reference)

GetWadInfo 0x1d47f8 formats every request into wad_last_filename 0x2a1d28
(sprintf of directory and name, ".ps2" appended when there is no dot,
BackslashMe, fed_upper), hashes it (hash_me 0x1d45b0) and searches the
table at 0x2a1ca4 (count 0x2a1ca0, 0x10-byte records, hash at +8),
returning the record or 0. `reference_warp.py --sample-filename` reads that
buffer after every vblank. The Tower — the first level a new game enters —
was loaded from G1's exit (the WARPED capture w7l1-exit-v1, idle, then the
staged capture w7l1-exit-tally-v1 at the tally: four crosses through the
tally, shop, level and inventory panels, Up to Done, cross; the Tower starts
at step 864). Reports requests-G1exit-tower-v1/-v2 and
requests-tally-tower-v1/-v2 (each pair equal); map
level-requests-tower-v2.json (tools/python/request_map.py).

Seen, in order: AUDIO\SHOP_G.VBK, SELECT\OBJECTS.PS2, SELECT\TEXTURES.PS2,
FONTS\SHOPATT9.FNT, SHPDATA\SHOP.WAD, ITEMS\LEVELL\{OBJECTS,TEXTURES,
ANIM}.PS2, LEVELS\LEVELL1\{WORLDS,OBJECTS,TEXTURES,WORLDS,ANIM}.PS2 (during
the tally: the next level loads behind it), AUDIO\SELECT.VBK (the select
column), then after Done ITEMS\LEVELL1\OBJECTS.PS2, AUDIO\WIZTOWER.VBK,
AUDIO\TOWAMB.VBK, STREAMS\TOWER.ADS. Sixteen names are dictionary-named
entries (2758, 2443, 2441, 2669, 2690, 2335, 2333, 2336, 2069, 2072, 2070,
2073, 2765, 2746, 2781, 2813), now runtime-confirmed; gdl_play loads each
(config paths or catalog labels). ITEMS\LEVELL1\OBJECTS.PS2 has no archive
entry: the original's lookup returns 0 and LoadItems goes on without
item_anim2, as the native resolver does (it requires ITEMS/LEVEL<code>/
ANIM.PS2). Not seen at this granularity (one sample per vblank shows only a
vblank's last request): TEXDEF.PS2 of each package, which gdl_play loads
with its package. The player's package was already resident (no class
change), so no PLAYERS request occurs on this path.
