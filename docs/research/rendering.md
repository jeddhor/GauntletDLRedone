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

## Projection (corrected 2026-10-20)

pbProjCalc's +0x90 matrix in the Tower reference is diag(1.73205, −2.88675,
…): a 554.27-pixel focal length over the 640×384 viewport (square pixels,
viewport +0xd0 scale 320/192, centre 2048/2016). The window angle
(MboxWinPtr +0x1c, 60°) is horizontal. gdl_play drives its projection from the
sim's pb +0x90 (it previously used 60° vertically: ~1.67× too wide).

## Particle systems (gdl_core::psys)

Instruction-derived port of the MB psys: WORLDS header words 0x1c/0x1d hold
0x138-byte records (letter at +6 = the character after "PSYS" in the node
label, GetWorldPsysIdx); preset_list (ELF 0x29a4c8, 8 records) applies first
for records with +0x10 bit 0. setWorldParms decodes counts, emit/decay and
fade times (×30 frames), cone angle, direction (else the negated first
collision normal), box, rates (/30), rate randomness, gravity (×−0.0355556),
speed (/30), four BGRA colours and sizes (MB_psys_parmscale 255/450),
texture name, flag bits (+0x2c and node 0x800000/0x40000000/0x800/0x40/0x80).
MBNewWorldPsys passes the node's file flag 0x1000 to createPsysNode (a3:
+0x2c |= 0xc0) and sets +0x2c |= 2 (loop). setupParms/setupNewPMode size the
16-bit particle ring (position index | direction index | age gap) and pick
the emitter functions; MBDrawPsys runs the emit state machine, retires,
emits, assigns age gaps (including the original's overflow quirk) and draws
newest → oldest; DrawPsysSub's sprites are camera-aligned squares (window
right/up rows), colour R/G/B = slots 2/1/0, alpha = slot 3 >> 1, MODULATE.
pbBlitSetDrawRegs: ALPHA 0x48 for node flag 0x800000, 0x89 for psys flag
0x10, FIX 0x80 for 0x20; ZMSK for node 0x80, ZTST ALWAYS for 0x40.
Verified against the Tower dump: the flame sprites' colour ramp 0, 42, 85,
127, 170, 212, 255, … and alpha 127 equal the port's output for record E;
the sprites bind WEAPONS' 64×64 P_TORCH (load order STATIC, WEAPONS,
POWERUPS before the level). HYPOTHESES: 30 Hz psys frames
(ticks_elapsed/1e7); a psys-owned rand state (the original shares one).

## Texture modifiers (texmods)

ANIM.PS2 header +8/+0xc: 0x58-byte records (+0x04 target name, +0x24 frame
name, +0x44 target record, +0x48 frame base or type, +0x4c count/period,
+0x4e start, +0x50 AFrame divisor, +0x54 counter). DoTexModSub: every
divisor AFrames the counter advances modulo |count|; frame modifiers copy
record (+0x48 + counter) into the target (MBSetRomTexture keeps the target's
flags), −1 resolves the frame base by name (MBCopyTexture); −2/−3 set the
MB_tex_shift_tex U/V add = sign·(counter mod |count|)/|count| (setTexShift).
gdl_play swaps image data per AFrame (30 Hz) and passes the shift in the
material. Fades (−4/−5) and per-node shift (0x10000000) are not ported.

## Render verification (2026-10-20)

tools/python/reference_camera.py reads the paused reference's window camera;
GDL_DEBUG_CAMERA pins gdl_play to it; tools/python/render_pixel_check.py
evaluates texel × vertex colour × lightmap from the decoded data at chosen
pixels. Tower (private/reports/render-pixel-check-l1-v1.txt): the podium
pixels' data prediction (15,15,13)/(43,47,47)/(40,41,36) matches the
reference (17,17,13)/(35,39,40)/(36,35,28). The native frame differs there
because the sim had raised the FLOOR_CYL#09 elevator onto the podium (saved
character; the reference is 4 s into a new game), not because of shading.
A second Tower dump (…20261020081101.gs.zst) confirms the same CLUT, pass
structure and a PSMCT16S render target (FRAME PSM 0x0a, two buffers).

## Node state from game code and package trees (2026-10-20)

- Game::world_labels / node_mb_set / node_mb_clear / node_alpha: ported
  functions record MBTreeSetFlags / MBTreeClearFlags on named world nodes
  (FindWorldObject). EnterTower hides L1XPLIGHTRAY01 unless the present
  players hold all eight shards (class +0x1d7c & 0x1fe). gdl_play keeps every
  modelled node (file-hidden ones too) with its ancestor chain and applies
  flag 2 (subtree) / 1 (node) whenever the state changes.
- ProcessItemWobjs fade objects (trigger kind 0x10xx, e.g. the Tower's
  L1LITPODIUM, record 205, trigger 41) hide with their subtree when
  items.object_visible is false.
- MBSetupObject distance alpha for MB flags 0x100400 (alpha 255/100 or
  255/200 × view depth) on per-entity materials.
- Package tree nodes (items, missiles, enemy missiles) carry their MB flags
  (ANIM node +0x30, AtreeNodeInit; DUMMY and +0x2e bit 0 hidden): draw
  variants and CameraFace (CameraFaceLocal under item slots). The Tower's
  crystals now show their additive glow nodes.
- Package texmods (items, WEAPONS/POWERUPS, monsters, STATIC) run like the
  world's (SetupItemTexMods / SetupWeaponPowerupTexMods / SetupEnemyTexMods).

## Vertex lighting of colourless parts (Tower dump fit)

The character model (PLAYERS/SOR/YEL00) carries no colour stream; its GS
vertex colours (728 samples matched by UV) are greys 102..128 following
128 × min(1, ambient + max(0, n·(−L))) with the level's light (LEVL +0xec
ambient 0.8, +0xf0 direction (−1,−6,2), +0xfc colour × +0x108): mean
absolute error 3.4 with one fitted yaw (body parts are articulated, so the
residual is expected). GsMaterial.lit applies it per fragment to parts with
neither colours nor a lightmap. HYPOTHESIS: the same model for coloured
non-lightmapped parts is not established (no unique UV matches in the dump);
they keep their vertex colours. The VU1 microcode was not read.

## Bodies and effect trees

- load_player: the held weapon is the model package's WEAP_HOLD (base
  classes; WEAP_<COL>_HD<n> for single-model classes) under the right hand
  node player_rhand[class] (ELF table); SHADOWL1 (MB flags 0x880, z mod −36)
  follows UpdatePlayerShadow (player x/z, max(floor, water) + 0.1; the splash
  alternate texture is not ported).
- Effect slots draw EffectHeaders' trees (WEAPONS, POWERUPS or the critter
  file's model) and FamiliarSpit trees with their node flags, posed each
  frame by tree_pose at (ATime − start) × 900 / timing (sequence 0).
  HYPOTHESES: sequence 0 and the frame rate from StartFXTree's end time;
  texture-mod (type 3) and OANIM (type 2) tree nodes are not ported.

## Not yet implemented

Texture fades and per-node shifts, character texmods (SetupPlayerTexMods),
alpha fades driven by MBTreeSetAlpha callers (node_alpha is recorded, not
yet drawn), tree psys nodes (type 4, InitPsysData), chrome (0x8000/0xa0000, setChrome),
distance alpha fade, MBTreeSetAlpha consumers (item/enemy fades), the colour
scale of setPrimColor and DoLighting, VU1 vertex lighting of characters
(point lights MBPosLight*, setPrimColor scales, AmbientSpecial flashes), fog (InitFog/FogFadeIn; FGE was 0 in the
G1 frame), Z modifiers, sorted transparency order.
