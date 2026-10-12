# M05: texture metadata and first native proof

Verified locally: 650 dictionary-labelled TEXDEF.PS2 entries contain 33,184
records; an unnamed table at archive index 463 contributes 43 more. Native
bounded parsing passed all 651 tables: 33,227 records, all with marker 0xf00b0017.
The inventory is a count of source records, not unique textures or reachable content.

| Format ID | Observed records | Current base pixel support |
| --- | --- | --- |
| 66 | 23,152 | Candidate 256-color RGBA8888 palette |
| 34 | 1,792 | Candidate 16-color RGBA8888 palette |
| 2 | 108 | Candidate direct RGBA8888 |
| 48 | 7,711 | Not decoded |
| 16 | 395 | Not decoded |
| 146 | 53 | Not decoded |
| 130 | 8 | Not decoded |
| 0 | 8 | Not decoded |

TEXDEF header: u32 count, u32 definition offset, u32 bitmap offset. Definition
stride is 36: label[30], i16 object-definition marker, u16 width, u16 height.
Bitmap stride is 64: marker u32 at 0, flags u16 at 4, format u8 at 8, mip count
u8 at 10, signed LOD k at 11, width/height u16 at 12/14, texture offset/size
u32 at 16/20. Unknown bytes and entire raw records/definitions are preserved.

The three candidate pixel readers consume a palette before linear base-level
indices. RGBA bytes retain original channels and alpha. 256-entry palette
addresses swap bits 3 and 4; 16-entry palettes remain sequential; four-bit
indices use the low nibble first. Synthetic tests exercise exact addressing,
nibble ordering, raw alpha preservation, truncation, overlap and unknown formats.
Metadata tables are bounded before allocation and texture dimensions are limited.

All fifteen TITLE records decode to native PNGs. Independent Python decoding
assembles TITLE00–03 into a coherent 512x384 original title image. Visual comparison
with PCSX2 confirms recognizable color/orientation/layout, without a quantitative
color-pipeline acceptance claim. The Bevy 0.18.1 laboratory renders these original
pixels natively, exposes source identity and raw flags, cycles records and explicitly
reports unsupported formats. Window startup, record navigation and alpha-mode
changes were exercised locally; captures are in private/reports/native-viewer-*.png.

All 15 native PNG pixel buffers (including original alpha) also match an independent
decoder using the pinned gdl_tools palette-shuffle function. The comparison report
is private/reports/texture-independent.json. This proves the sampled disk decoding,
while source blending, GS upload and native material equivalence remain open.

Default display preserves source alpha. Opaque RGB and 0..128-to-0..255 diagnostic
modes are explicit hypotheses, not universal import conversion rules. Source
pixels are never replaced or overwritten. Mipmap decoding, upload behavior,
alpha test/blending, material linkage, environment/character/font sample comparison
and quantitative color agreement remain open. M05 is not complete.

The schemas and shuffle were compared with pinned gdl_tools format research.
No gdl_tools implementation code was transplanted into Rust. That consultation
is recorded in THIRD_PARTY.md; this project does not claim clean-room development.
