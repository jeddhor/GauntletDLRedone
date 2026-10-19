# World rendering: GS material state, lightmaps, node draw flags

Revision: SLUS-20047 (profiles/ps2/slus-20047.json). Evidence:
private/reports/gs-draws-g1-v1.json (tools/python/gs_draws.py over the stock
PCSX2 GS dump "…20261012144427.gs.zst", G1 Poison Fields), Ghidra exports
private/reports/ghidra-render1..7 (2026-10-19).

## Observed GS state (G1 dump, actual disc)

| Pass | State |
|---|---|
| Base | PSMT8 (CT32 CLUT), TFX 0 MODULATE, TCC 1; ALPHA_1 0x44 (Cs−Cd)·As+Cd; TEST_1 0x5001d (ATE GREATER AREF 1, ZTE GEQUAL); TEX1 MMAG/MMIN linear (+ mip levels, K per draw); TEXA TA0 0, TA1 0x80 |
| Lightmap | same geometry right after the base draw: PSMT4, TFX 1 DECAL, TCC 1; ALPHA_1 0x89 Cd·As/128; TEST_1 0x5c000 (DATE, DATM); TEX1 0x60 (bilinear); the base draw's CLAMP_1 |
| Additive | ALPHA_1 0x48 Cs·As+Cd |

- Vertex colours reach the GS as (c << 2) | (c >> 3) of the packed 5-bit
  channel (every sampled value fits: 66, 70, 74, 78, 90, 99, 103, 107, 127),
  alpha bit 15 → 0x80. MODULATE makes 0x80 = 1.0.
- The lightmap CLUT (CBP 0x2308, a PSMCT32 upload at DBP 0x2300 in the same
  dump) is white with alpha 8·i (i = 0..15). Lightmaps therefore only darken
  (×i/16, at most 15/16). The generating code is not identified; other
  packages using the same CLUT is a HYPOTHESIS.
- All 12 G1 lightmap uploads (DPSM PSMT4, 32768 bytes) are byte-identical to
  the format-146 bitmap records 139–150 in TEXTURES: 4-bit indices, two per
  byte, low nibble first (ObjectBitmap::decode_lightmap_indices).

## Renderer code (instruction-derived)

- pbDrawObject 0x1be3d0: draw flags = node +0x60 & 0x1090d7c0 (| 0x20000 for
  object +8 bit 0x100 or flag 0x8000); first subobject texture/lightmap =
  object +0x12/+0x14, the rest from 8-byte descriptors (+2 texture, +4
  lightmap); lightmap index 0 = none.
- pbSetDORegs 0x1bfac8: a lightmap unless flag 0x4000; sets 0x2000.
  setLmapInfo: TEST 0x5c000, ALPHA 0x89, base ALPHA pb_lm_tex_alpha_reg
  (0x44). setPrimInfo 0x1beea0: flag 0x80 → ZBUF_1 ZMSK; flag 0x40 → TEST
  0x3001d (ZTST ALWAYS); without a lightmap ALPHA 0x48 for flag 0x800000,
  else 0x44. setPrimAlpha: flags 0x100600 → alpha = node byte +0x53 ×
  0.5019608 (MBSetupObject fades it by distance for 0x100400 with
  ALPHADISTMAX/ALPHADIST2MAX), else 128. setPrimColor: colour scale from
  pbg+0x1c +0xb0 (MBLightmapMod, DoLighting) for lightmapped draws, +0xa8
  for flags 0x5000, +0xb8 otherwise (the VU1 use is not recovered).
- setTexInfo 0x1be858: CLAMP_1 = clampval[(record flags >> 2) & 3] =
  [0, 1, 4, 5] (flag 0x4 WMS clamp, 0x8 WMT clamp); record flag 0x100 →
  default texture; flag 0x40 → texture shift (MB_tex_shift_tex, scrolling).
- World nodes (CreateWorldNode 0x11b9a0): MB flags = WORLDS record +0x18
  (MBTreeSetFlags on the node only), | 4 when file flags & 0x1001 == 1,
  | 1 (not drawn) for file flag 0x8000; 0x400 → MBTreeSetZMod −2.
  NewWorldObject sets a model only for +0x28 == 1 (or 0x80000000); file
  flag 0x800 nodes are particle systems (WorldPsysActivate), not meshes.
- pbTraverseDrawObjects: MB flag 2 skips the node and its subtree, flag 1
  the node only; flags & 0xf000000 → CameraFace 0x1c07e0 after PushMatrix:
  0x1000000 FaceCamMat(0) (yaw), 0x3000000 yaw+pitch, 0x5/6/7000000 pitch
  clamped to 0.2618/0.5236/0.7854, 0x2000000 QuickYawMat (camera forward +
  π), 0x4000000 diag(−1,1,−1)·camera→world rows, 0x8000000 TopFaceMat. The
  face matrix is inherited by children. YawMat3/PitchMat3 rotate the local
  frame (row0/row2, row1/row2).
- MBSetupObject: flag 0x800 (under a parent with node flag bit 0) defers to
  the distance-sorted list.

## Native implementation (crates/gdl_client)

GsMaterial (lib.rs, gs.wgsl) folds base and lightmap passes into one
fragment: rgb = min(tex·vcol, 1) × lightmap alpha/128, alpha = tex·vcol
alpha, discard when ⌊alpha·128⌋ ≤ 1, then converted from GS (display) space
to the sRGB target. Base textures and lightmaps are raw Rgba8Unorm/R8Unorm,
bilinear, with the record's clamp. GsDraw applies flags 0x800000/0x80/0x40.
gdl_play applies the node rules above (model presence, flags 1/2, particle
nodes skipped, CameraFace on translation-only nodes with the StdCam eye/rows).

Presentation choices (not fidelity claims): one folded pass (translucent
lightmapped texels multiply the background by the lightmap in the GS but not
here); blending happens in linear space on the sRGB target; level-0 textures
only (the mip chain and per-draw K are not used); the GS's integer
truncation is not modelled; nested CameraFace nodes use the nearest one.

## Verification

- Actual disc: the decoded lightmaps equal the dump's uploads (12/12); the
  CLUT and state values above are read from the dump.
- Synthetic: objects::tests::lightmap_indices_low_nibble_first.
- Visual (Xvfb, not a pixel comparison): the Tower and G1 now show lightmap
  shading, additive brazier/beam glows and bilinear textures;
  private/reference/captures/render/g1-fixture-v1.png is a PCSX2 reference
  frame from the slot-1 fixture (tools/python/reference_screenshot.py).

## Not yet implemented

Particle systems (PSYS nodes, MBDrawPsys), texture shift/scroll (record
flag 0x40, node flag 0x10000000), chrome (0x8000/0xa0000, setChrome),
distance alpha fade, MBTreeSetAlpha consumers (item/enemy fades), the colour
scale of setPrimColor and DoLighting, VU1 vertex lighting of characters
(MBDirLight/MBAmbLight/pos lights), fog (InitFog/FogFadeIn; FGE was 0 in the
G1 frame), Z modifiers, sorted transparency order.
