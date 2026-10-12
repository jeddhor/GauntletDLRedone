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
| 48 | 7,711 | Candidate 256-color CT16 palette |
| 16 | 395 | Candidate 16-color CT16 palette |
| 146 | 53 | Not decoded |
| 130 | 8 | Not decoded |
| 0 | 8 | Candidate direct CT16 |

TEXDEF header: u32 count, u32 definition offset, u32 bitmap offset. Definition
stride is 36: label[30], i16 object-definition marker, u16 width, u16 height.
Bitmap stride is 64: marker u32 at 0, flags u16 at 4, format u8 at 8, mip count
u8 at 10, signed LOD k at 11, width/height u16 at 12/14, texture offset/size
u32 at 16/20. Unknown bytes and entire raw records/definitions are preserved.

The candidate palettized pixel readers consume a palette before linear base-level
indices. RGBA bytes retain original channels and alpha. 256-entry palette
addresses swap bits 3 and 4; 16-entry palettes remain sequential; four-bit
indices use the low nibble first. Synthetic tests exercise exact addressing,
nibble ordering, raw alpha preservation, truncation, overlap and unknown formats.
Metadata tables are bounded before allocation and texture dimensions are limited.
CT16 RGB uses the GS five-bit-to-eight-bit shift (maximum channel 248), verified
against the inspected PCSX2 ReadTexel16/ReadPixel16 conversion convention. Its
high bit selects TEXA TA0/TA1, so the viewer/export shows it as binary 0/255
only for diagnostics; actual material alpha still requires the original TEXA.
CT16 exports use a binary-texa-selector filename, not a source-alpha claim.
Pixel decoder revision ps2-base-rgba-v2 is recorded separately in new manifests.

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
and quantitative color agreement remain open. The Poison Fields environment
base table exports all 134 records. The Yellow Sorceress table exports 17/18;
YEL_HANDGLOW points beyond its package payload and is retained as an unresolved
external dependency. The native rest-hierarchy viewer renders 19 character mesh
parts with original textures, including the CT16 families. This is an asset
inspection, not acceptance of character animation/material fidelity.
M05 is not complete.

A stock single-frame GS dump was captured inside Xvfb from the preserved fixture.
The bounded research inspector reads its 8,092,131 decoded bytes, 1,806 events
and one GIF path, checking serial SLUS-20047 and CRC 15030637. Raw A+D writes
include TEXA=0x0000008000000000 (TA0=0, TA1=128), and several ALPHA/TEST states.
These support per-material experiments; they do not justify a universal blend
rule or complete material binding. The first black window capture was occluded
by the debugger; exposing the game window produced the original scene correctly.
The GS dump's blank preview is not used as visual comparison evidence. Raw dump,
register report and exposed-window capture remain private.

The schemas and shuffle were compared with pinned gdl_tools format research.
No gdl_tools implementation code was transplanted into Rust. That consultation
is recorded in THIRD_PARTY.md; this project does not claim clean-room development.
