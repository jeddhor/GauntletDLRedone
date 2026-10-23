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
| 146 | 53 | 4-bit indices, no inline palette (fonts; lightmaps are object bitmaps of the same format) — bound to the runtime 16-entry white alpha ramp |
| 130 | 8 | 8-bit indices, no inline palette (CREDITS, SHOPATT9, KANJI10A/10B/20A/20A1, PARTICLE1_A/2_A) — bound to the runtime 256-entry identity alpha CLUT |
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
M05 status: see "Reference upload and memory checks" below.

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

## Reference upload and memory checks (2026-10-23, actual reference)

Tools: tools/python/gs_uploads.py walks a GS dump's GIF streams (BITBLTBUF,
TRXPOS, TRXREG, TRXDIR and the IMAGE data) and matches every host-to-local
transfer by SHA-256 against record-aligned slices of all 650 TEXDEF/TEXTURES
pairs (palette, base level, each mip, laid out as the native decoder reads
them: palette first, base indices, then mips); tools/python/gs_vram.py reads
the dump's initial GS memory (GSState::Freeze version 9: 425 bytes, then
4 MiB; PSMCT32/PSMT8 swizzles from PCSX2's GSTables) to check textures that
are resident rather than uploaded in the frame. The reader is validated on
re-uploads: in the G1 frame 330/342 PSMT8 and 82/86 PSMCT32 transfers equal
what memory already held at the same address (the rest are slots that held
another texture the previous frame).

Dumps (private/reference/config/PCSX2/snaps): G1 (…20261012144427), the
Tower (…20261020081101), the shop after G1's exit (…20261023004347,
reference_warp.py --gs-dump from w7l1-exit-tally-v1), the attract group shot
(…20261023005628) and the credits (…20261023010839; reference_driver.py
gsdump, credits forced with R1+R2 held at the title's timeout).

| Frame | Uploads | Equal to a TEXDEF slice | Object-bitmap lightmaps (TEXTURES offsets) | Other |
| --- | --- | --- | --- | --- |
| G1 | 500 | 474 | 24 | 2: the runtime CLUT block at DBP 0x2300 |
| Tower | 1296 | 1262 | 32 | 2: same block |
| Shop | 146 | 112 | 32 (the Tower loading behind) | 2: same block |
| Group shot | 68 | 48 | 18 (A2, the next flyby) | 2: same block |
| Credits | 22 | 20 | 0 | 2: same block |

Every upload that comes from the disc is byte-identical to the slice the
native decoder reads; formats seen: 66, 48, 34, 16, 2, 146 (G1, Tower), 0
(GROUP1 records 0–3, base level and every mip, as PSMCT16) and 130 (CREDITS
record 29 as PSMT8). Palettes: format 66/34 palettes go up as PSMCT32 CLUT
images and format 48 palettes as PSMCT16, i.e. the file's palette bytes are
the CSM1 CLUT image (the decoder's bit 3/4 swap is CSM1's entry order). Mips
are uploaded as the consecutive slices after the base level.
Resident textures: all 21 PSMT8 textures drawn in the shop frame (panels,
shop icons, SCORE digits; format 66) and all 5 in the credits frame
(CRED_ALL00–03 and the CREDITS font page) equal their records in GS memory.
The runtime block at 0x2300 is built by code: its CLUT at CBP 0x2300 is an
identity alpha ramp (entry i = 0x00ffffff | i << 24 for all 256 entries,
CSM1 order), which the credits font (format 130) is drawn with — TFX 0
MODULATE, TCC 1, sprites with vertex colour (128, 128, 128, fade alpha),
ALPHA (Cs − Cd)·As + Cd, ATE GREATER AREF 1 — so the native "white, texel
as GS alpha" reading of format 130 is verified (no longer a hypothesis);
CBP 0x2308 is the 16-entry ramp (alpha 8·i) used by format 146 fonts and
lightmaps (rendering.md).

Mip selection (2026-10-23): setTexInfo 0x1be858 writes TEX1 from the record
(MXL = its mip count, MIPTBP1/2 from the record) and the render settings at
*(*(0x25d870) + 0x48) — +0x370 MMAG, +0x36c MMIN, +0x368 L, +0x364 K bias,
which hold 1, 5 (LINEAR_MIPMAP_LINEAR), 0 and 0 in the fixture's RAM — with
K = the bias + pbDrawObject's argument, the part's +0x16 (Part::lod_k): all
49 distinct K values of the G1 dump's 1,206 mipmapped draws are lod_k values
of the loaded packages. With LCM 0 and L 0 the GS level is log2(1/Q) + K;
the Sorceress's draws carry 1/Q ≈ 24.0 where the fixture camera puts her
centre about 25 units away (1/Q taken as the view depth, approximate).
Native (gdl_client materials_for / object_meshes / gs.wgsl): records with
mips get their decoded chain (Bitmap::decode_level_rgba; synthetic tests
for palette sharing, CSM1 entries and format 0) and a trilinear sampler
clamped at MXL; each part's material carries K = lod_k / 16 and samples at
max(log2(view depth) + K, 0). GUI check: scene-g1-mip-v1 (G1 at the
fixture camera; distant ground now filtered like the reference; different
moment, so not a pixel comparison).
