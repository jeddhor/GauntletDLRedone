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
alpha, discard when ⌊alpha·128⌋ ≤ 1, output as GS display values. Every
camera renders into the shared Hdr (Rgba16Float) frame, so all blending
works on display values as the GS does; the last camera's DisplayToLinear
pass (FullscreenMaterial) decodes the frame once for the sRGB swapchain.
Bevy-native sprites/text are given display-valued colours (the title picture
is a raw Rgba8Unorm image). Base textures and lightmaps are raw Rgba8Unorm/R8Unorm,
bilinear, with the record's clamp. GsDraw applies flags 0x800000/0x80/0x40.
gdl_play applies the node rules above (model presence, flags 1/2, particle
nodes skipped, CameraFace on translation-only nodes with the StdCam eye/rows).

Presentation choices (not fidelity claims): one folded pass (translucent
lightmapped texels multiply the background by the lightmap in the GS but not
here); blending is in floating point; level-0 textures only (the mip chain
and per-draw K are not used); the GS frame's 16-bit truncation is applied
once to the final value (DisplayToLinear, see "GS output stage"), not per
draw; nested CameraFace nodes use the nearest one; VU1 lighting is
evaluated per fragment (the VU lights vertices).

NATIVE ENHANCEMENTS (post-1.0; enhancements.md "Graphics"): sharp texture
filtering (texfilter.rs), full colour precision, smooth lightmaps and
sharpening change this path only when set; the original presentation is
unchanged. Their shader switches ride in the shared point-light buffer
(gs.wgsl PointLights.enhance, lib.rs write_enhance), so turning one on
re-prepares no material.

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
After the globals and an emitter's own (world psys) texture, the search
visits the current level's item, monster and critter packages in the
order the client loads them, the level's own package, then the players'
packages (gdl_play `search_order`, also used by the IT_SIGN and hand
object lookups). HYPOTHESIS: the original's order among the level's
packages is unread. It decides C2's MISTPART tree sprites, which exist in
both ITEMS_C and the level package (different 32×32 images): ITEMS_C's
now; before 2026-10-28 a HashMap picked either per process.

## Texture modifiers (texmods)

ANIM.PS2 header +8/+0xc: 0x58-byte records (+0x04 target name, +0x24 frame
name, +0x44 target record, +0x48 frame base or type, +0x4c count/period,
+0x4e start, +0x50 AFrame divisor, +0x54 counter). DoTexModSub: every
divisor AFrames the counter advances modulo |count|; frame modifiers copy
record (+0x48 + counter) into the target (MBSetRomTexture keeps the target's
flags), −1 resolves the frame base by name (MBCopyTexture); −2/−3 set the
MB_tex_shift_tex U/V add = sign·(counter mod |count|)/|count| (setTexShift).
gdl_play swaps image data per AFrame (30 Hz) and passes the shift in the
material. Tree-node fades (−4/−5) and scrolls (−2/−3, per-node shift
0x10000000) are drawn through per-mesh material requests (see Bodies and
effect trees).

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
  L1LITPODIUM, record 205, trigger 41; branch at 0x154a18, read from the
  instructions 2026-10-22): the condition (kind bit 0x20 clear: +0x16 & 0xf
  == 0; set: +0x16 & 0x2f != 0) FADES THE TREE OUT: +0x35 = 0xff, the
  transparency (0xff − node +0x53 under flag 0x200) rises by 8 × vb_elapsed
  (0xff at once during start_of_stage) and above 0xf7 MBTreeSetFlags(tree,
  2) hides it; otherwise MBTreeClearFlags(tree, 2) and the transparency
  falls back to 0. Transparency goes through MBTreeSetAlpha(tree, a, 1)
  (+0x53 = ~a, flag 0x200 for a ≥ 1). The earlier port had the visibility
  inverted: the Tower's lit podium (with the green beams over Sumner) was
  drawn from a new game's start while the reference hides it. While the
  transparency changes (0x154af0, 0x154b30) the object also counts as
  moving: +0x35 = 1 (excluded from FloorCollide's mask 1), +0x10 |=
  0x20000000 and +0x16 |= 0x10 (gameplay.md, Tower trace).
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
  follows UpdatePlayerShadow (player x/z, max(floor, water) + 0.1; while the
  water +0x8a4 is above the floor +0x88c, MBTreeSetAltTex(−2, TextureSplash
  = POWERUPS "SPLASH" from InitEffects), else (−1)). The splash is drawn
  natively but not seen yet: the scripted and teleported game-run checks
  of A4/C2/C4/G3/I3/I4/D1 never put the player in water.
- Alternate textures (MBTreeSetAltTex 0x1cff48 writes node +0x5c mode and
  +0x58 texture over the subtree): pbSendObjTextures 0x1bd828 loads only
  the alternate texture for −2/−3, both for −4 and other modes; pbObjTexSub
  0x1bd980 picks each subobject's texture: −1 own; −2 the alternate (plain
  replacement); −3 the alternate with draw flag 0x80000 (pbSetDORegs →
  setChrome 0x1bef68, environment mapping); −4 the own texture with flag
  0x8000000 (pbSetDORegs: +0xdc = the alternate, setupKeepA 0x1bf6f8 in
  place of the lightmap pass: TEST 0x5c000 DATE/DATM, ALPHA 0x44, TFX
  MODULATE — a second pass of the alternate texture blended by its alpha
  where the base wrote alpha bit 7); ≥ 0: the alternate for subobjects
  whose texture equals the mode (texmod swaps). gdl_play folds the keep-A
  pass into the fragment (GsMaterial::overlay). It uses the base UVs:
  VU1 program A kicks the same vertex block for it (see "VU1 microcode").
- Effect slots draw EffectHeaders' trees (WEAPONS, POWERUPS or the critter
  file's model) and FamiliarSpit trees with their node flags, posed each
  frame by tree_pose at (ATime − start) × 900 / timing (sequence 0).
  HYPOTHESES: sequence 0 and the frame rate from StartFXTree's end time.
  Texture-mod (type 3) tree nodes: AtreeNodeInit points the node at the
  0x58-byte texmod record at the tree's header[0] base + node +0x34;
  DoTexModNode applies it to the node's subtree each frame — frame base ≥ 0:
  MBTreeSetAltTex(target record +0x44 → frame base + clamp((frame − start)
  / divisor, 0, count − 1)); −4/−5: DoTexFadeSub MBTreeSetAlpha over count
  frames from start (−4 fades in, −5 out); −2/−3: DoTexScrollSub →
  CalcTexScroll 0x110988(frame − start, divisor, count, frame) →
  MBTreeSetUVScaleAdd 0x1d0300 on the subtree (U for −2, V for −3; a slot
  of MB_tex_shift_node 0x2a0a40 indexed by node +0x5e, flag 0x10000000;
  u' = u · scale + add). CalcTexScroll with min(count, divisor) > 0 wipes:
  t ≤ 0 → (add 0, scale 0); t < m → add = −2q + t/m · (1 − q + 2q), scale
  1 − add (q = count/m); count < divisor → (0, 1); t < count → add 1 − q →
  0, scale g(q − 1) + 1 − add; else add (t − count)/m (≤ 1), scale q − add.
  With min = 0 the add is (frame mod count)/count and the scale stays 0
  (f0 cleared at 0x1109a0; instruction-checked). Used by 178 nodes of
  monster attack trees (LICH EXECAUSTI, DJINN, WRAITH, PBOSS); seen
  natively as the Lich's spreading caustic in G5.
  gdl_play applies swaps and fades on per-mesh materials for effect and
  item trees (e.g. RUNE1's GARDUST dust frames 24.. from frame 38 and its
  TEXFADEIN). HYPOTHESIS: the frame passed is (int)(frame + 0.5).
  OANIM (type 2) nodes are not hidden without an object (AtreeNodeInit
  hides only other types) and show DoObjAnimation's frame object in
  effect and item trees too. OANIM (type 2) nodes:
  the node's list at tree header +8 + node +0x34 holds one 0x28-byte entry per
  sequence (first object name, +0x24 count, +0x26 start); DoObjAnimation
  shows object base + (int)(frame + 0.5) − start inside the range (one-frame
  entries always their object). gdl_formats parses them (2,279 entries in
  651 ANIM files); enemy bodies use them.
- Type-4 tree nodes are particle systems (AtreeNodeInit → InitPsysData →
  MBNewWorldPsys with a3 = 1, no name): the record at the tree's header[0]
  base + node +0x34 (e.g. SFXRED's POOLFIRE weapon flames, SFXYEL's
  YEL_DUST throw trails), direction at node +0x14. Node +0x34 is a signed
  word: the SFX packages' WEAP_HOLD_* / WEAP_TW_* psys records lie 0x518
  bytes before the header-0 base (the client dropped them until 2026-10-22).
  gdl_play spawns them with item/effect/missile trees; the emitter follows
  an anchor in the slot's hierarchy and stops emitting while hidden.
- Texmod targets without texels (bitmap record flags 0x20, e.g. SFX<col>'s
  REDWEAPTEX/PIXIE_R/LIGHT) are named by their texmod's +0x04 target name,
  and a −1 frame base is found by name in another loaded package
  (MBCopyTexture: REDWEAPTEX01.. are WEAPONS records). HYPOTHESIS: the
  search order (own package, then the level's load list).
- PlayerProcessPowerups' held-weapon effect: with a weapon powerup
  (+0x130 & 0xf) WeapHoldFxTree[player][type] ("WEAP_HOLD_<DmgTypeDesc>" of
  the SFX package) hangs under the right hand (+0x6a8) at PlayerData +0x68 +
  tier × 12, scaled by +0xe0 + tier × 12 when non-zero (tier = level / 10);
  without a powerup it is deleted. gdl_play draws it (HoldFx). Not
  modelled: the +0x708 held object hiding it.

## Not yet implemented

## Missile streaks (SfxSetStreak / UpdateFXStreak, 2026-10-23)

StartMissile (player missiles) calls SfxSetStreak 0x17e0f0(0, PDAT +0x17c,
slot, WeaponStreakTex, colour, alpha): effect flag 0x40000, +0xd8 = the
class value, +0xdc = the radius (+0x98, the width argument being 0), and
MBNewPoly(0, 4, …) — a 4-vertex poly instance with WEAPONS' WEP_STREAK
(InitPlayerMissiles' MBOX_FindTexture), UVs from stvals 0x2b7018 ((0,0)
(1,0) (1,1) (0,1)), colour streak_color 0x24eed0[player +4] (yellow, blue,
red, green) and GS alpha (0xff − streak_alpha 0x24eee0[player +8]) >> 1
(64 or 192; +0x40 with flag 0x2000000; white 0x40 when flags & 0x2100000 =
0x100000). DoPlyrSfxSub's flag 0x20000 makes one with width 2.0, length
1.0, alpha 0x40. ProcessEffects runs UpdateFXStreak 0x17ae48 for every
slot with a poly: side = normalised velocity × Cam[0]'s row 2, normalised
and scaled by its length (magnitude at least 0.25); front = position +
velocity × +0xd8/30 (±0.8w), back = position − velocity × min(w/2, age)
(±0.1w). DoPolyInstSub draws it as a textured Gouraud triangle fan with
alpha blending (PRIM 0x15D, UV/RGBAQ/XYZ2) under the blit draw registers.
Native: EffectNode::streak (gdl_core), gdl_play's present_streaks (one
blended mesh, no depth writes). Ported from the instructions; no reference
frame compared (GUI flow menu-flows-m17-v15 streak shows the trail).

## Scene lighting and skins (2026-10-21)

- InitLighting 0x180a30 (init_next_level): Ambient = LEVL +0xec, one
  directional light (+0x108 × +0xfc along +0xf0), GlobalInten 1. DoLighting
  0x180888 (main, every frame): MBPosLightDeleteAll; AmbientSpecialValue
  decays (× 0.6 once AmbientSpecialTime passed, 0 below 0.05);
  AmbientSpecialCurValue follows it by +0.05 / −0.25 per frame;
  MBSetAmbient(clamp(Ambient · GlobalInten + current, 0, 1)),
  MBLightmapMod(1, current), MBNoShadeMod(1, current). LEVL +0 flag 8
  (DoLighting forces AmbientSpecialValue = CurValue = −1, so ambient 0 and
  the lightmap/no-shade scale 0; do_players adds the player light) is set
  only by S5 (+0 = 0xc; S1..S4 and S6..S9 have 0x4, every other record 0;
  gdl_tools levl-list, 2026-10-22). main calls DoLighting after
  AudioQueUpdate/AudioPoll and before game_main (0x1001e0), so lights made
  by game_main are drawn that frame and deleted by the next DoLighting
  (MBPosLightDeleteAll); Game::step runs it in that order. SetAmbientSpecial callers: the class specials'
  DAMG flags (−0.8/−0.6/−0.4), the quest item (−0.8), A5/F2 lightning
  (+0.5 for 0.1 s; GauntWorldInit hides A5XPXLNC_LNING1..4 /
  F2XPNC_LNING1..4, each flashing for 0.2 s every 5 + Random(10) s).
  world_update's end of stage: GlobalInten 1 − t, then −1 and
  end_of_stage 2 after 2 s. gdl_core::game::lighting holds the state. The
  same block shows the TransitionBlit (TransitionBlitShow(0) 0x126780 unless
  BossType 0x2c: STATIC's TRANSITION_SCREEN at (0, 0), 0x200 × 0x140, z
  64100; alpha 0xff, then (1 − t/2) × 255, then 0 — it fades in over two
  seconds); init_shop shows it again after ResetModels, do_shop hides it
  once loaded unless the shop is skipped (TransitionBlitHide 0x126848);
  FogOff is MBFogInit (empty). Native: Hud::transition_show/hide
  (2026-10-23; GUI flow menu-flows-m17-v12 level-end-fade).
- setPrimColor 0x1beca8 puts a scale (+0x10..+0x18 = scale × 128) and an
  ambient term (+0x48 = node +0x6a ambient add + the mod's add + 1 for
  lightmapped/no-shade draws, + 0 for directionally lit ones) into the
  object info. HYPOTHESIS (VU1 microcode unread): the term scales the
  colour; gdl_play draws unlit materials × (1 + add) and adds the ambient
  add / 255 to lit ones' ambient.
- Skins (gdl_core::game::skin): SetSkinFX(rate, skin, texture, end,
  repeats) writes six floats (enemy +0x1f8, player +0x7b4, critter +0xec);
  ProcessSkinFX advances current by (vb >> 1) × rate; while end > 0
  DoProcessSkinFX sets MBTreeSetAltTex(−4, or −3 for CHROMESILVER/GOLD;
  texture + (int)current) and MBTreeSetAmbientAdd(999). Uses: enemy deaths
  (DEATHBLOOD/FIRE/ELEC/LIGHT/ACID texmod frames by damage type, DEATHALT,
  DEATHGOLEM; 10 or 15 frames at 0.5) with ambient add 999 — the dying
  enemy stays (state 8) while its skin runs; knockback white flashes
  (AAAWHITE 1 frame, 1 repeat) for enemies and players; the exit glow
  (DEATHLIGHT, 0.4 / 0.5 rate); gold/silver powerup chrome blinking in its
  last 3 s; PlayerProcessSkinFX's special flash (+0x7d4 × 255, × 0.9 per
  frame). The record's texture word holds a SkinTex code (pointer-class
  word). gdl_play draws −4 as the keep-A overlay and −3 as a replacement
  with setChrome's environment mapping, and adds the ambient. setChrome
  0x1bef68 under pb_debug_chrome's defaults (struct 0x2721f0: +0x18, +0x1c,
  +0x20 set, +0x24 clear): e = camera position (camera block +0x20) −
  object matrix +0x30; u = e × up (block +0x40, (0, 1, 0) from
  pbInitCamera; pbCameraUpdate never writes it); v = u × e; both taken
  through the inverse object matrix, normalised, × 0.13333334 and sent to
  VU1; the block gets atan(e.y / |e.xz|) / π (+0x50) and atan2(e.x, e.z)
  / 2π (+0x54). HYPOTHESIS (VU1 unread): uv = (n·u + az, n·v + el).
  Verified natively only as drawing (GDL_DEBUG_SHIELD=110000:60 in G1:
  a gold, normal-shaded body). CritterUpdateSkinfx
  0x192f18 (ProcessCritter, root and children): ProcessSkinFX(+0xec), the
  collision parts' flash counters (+0x540 + 0x60k + 0x54), the tree flash
  +0xb44, the quest-item texture time +0xb4c (−vb per call; it also gates
  CritterAnimate, so the boss 0x22 freeze now ends after 1200 vblanks) and
  the ambient add +0xb46 (−0x10 per call). The +0xb48 texture handle is
  not resolved.
- Point lights: SfxSetLight 0x17f8b8 (radius at slot +0x10, colour
  +0..+8; DeleteEffect clears the radius) from the player DAMG effects
  (2 × +0xc or +8, plight_color[class]), barrel explosions (2 × blast,
  red) and suicide explosions (20, red); ProcessEffects lights every live
  effect with MBPosLightNew(radius × sfx_lightradmul 1.0 × fade,
  sfx_lightattn 2.0, position + 1 up, colour). do_players' dark-level light
  (LEVL flag 8): MBPosLightNew(player_lightrad 20, player_lightattn 10,
  attention point + (0, player_lightdy 10, 0), player_light_color[colour
  +4]) — ELF 0x220f10..0x220f58, colours (2,2,1.5) (1.5,1.5,2) (2,1.5,1.5)
  (1.5,2,1.5); Game::lighting.pos_lights. The VU1 formula is in "VU1
  microcode". gdl_play passes the nearest 8 to the camera to every
  material (NATIVE ENHANCEMENT "Dynamic lights": up to 32 in view,
  research/enhancements.md "Lighting"). psys sprites are drawn by pbBlit,
  outside the VU1 programs, and take no point lights (until 2026-10-31
  the port lit them through their +z normal). HYPOTHESIS: no life fade.
  Every SfxSetLight caller is ported
  (ghidra-fxlight1): StartMagicFX / StartThrowMagicFX / StartShieldFX (1.5
  × power, light_color[mclr_idx[potion colour]]); StartComboFX (10,
  light_color[pclr_idx[colour b]] = (3, 2, 0, 1): yellow, blue, red, green;
  the tree coloured streak_color[b] with MBTreeSetAmbientAdd 0x1ff, the
  second tree ambient add 0x1ff too); StartFamiliarFireBall (3,
  light_color[pclr_idx[player colour]]) — the original passes the
  FamiliarSpit index as the effect (a0 = s2 at 0x17e494), so the light goes
  to effect slot 0..4, not the fireball; the port does the same;
  EnemyStartMissile kind 1 (5, color.27 0x24ef10 = red), with ScaleFX by
  EnemyShrink below 1.
- Critter shadows: CritterInitGeo calls CritterAddShadow 0x193400 for types
  with +0x5c bit 0: SHADOW<n>L1 (shadowdesc; n = slot + 1, slots ≥ 3 → 2)
  of the critter's model, MBNewObject flags 0x880, z mod −32, at the
  critter position; ProcessCritter (kinds 0/1/2/5/6) keeps it at x/z and
  the floor + 0.1 with an identity rotation. Not verified visually (no
  shadowed critter met in the checks).
- Fog: InitFog 0x162d40 / FogFadeIn 0x162de0 pass LEVL +0x70..+0x88 to
  MBFogInit 0x1cfa30, which (like MBFogSet) is an empty function in this
  build — there is no fog to draw (consistent with FGE 0 in the G1 dump).
- Hit effects: AddEnemyHitCol 0x17db28 (damage_enemy hurt/kill,
  enemy_ai_control's boss-death kill, CritterDamage) and AddPlayerHitCol
  0x17dd70 (PlayerKnockback) start EffectHeaders from Damage/Kill
  Alt/Hit/Blood tables (gore_level 0: blood; 4/5 alternate 6/7 and 8/9 by
  sfx_ene_bloodidx), scaled (half-height × 0.5) and oriented by the
  victim's matrix; weapon wall/item hits and the effects' HIT_FX are
  StartFXSub calls too. The class SFX package's texture modifiers run with
  the level's (their flag-0x20 textures are filled by them). The interim
  spark/blast spheres are gone.
- Package tree nodes: the named object only without +0x2e bit 0 (an OANIM
  node draws its frame object instead), e.g. barrels' BAROBJ/BAREXP/BARPOI
  break animations.

MBTreeSetAlpha on items (Game::item_alpha) and world nodes
(Game::node_alpha) is drawn as material alpha × (255 − alpha)/255
(HYPOTHESIS: the +0x53 byte's use in the draw path is not traced).

Texture scrolls and per-node shifts, character texmods (SetupPlayerTexMods),
enemy MBTreeSetAlpha fades, chrome (0x8000/0xa0000, setChrome), the colour
scale of setPrimColor and DoLighting, VU1 vertex lighting of characters
(point lights MBPosLight*, setPrimColor scales, AmbientSpecial flashes), fog (InitFog/FogFadeIn; FGE was 0 in the
G1 frame), Z modifiers, sorted transparency order.

## Effect colour and alpha (2026-10-21)

StartBlockFX 0x17f200 (damage_player's block, f12 = the clamped block factor
k as the effect time): EffectHeaders 0x42, MBTreeSetColor(streak_color
0x24eed0[player colour] = 0xffff00 / 0xff / 0xff0000 / 0xff00, 0xRRGGBB),
MBTreeSetAlpha 0x40, SfxSetParent to the player's node. MBTreeSetAlpha
stores ~value at node +0x53 and sets flag 0x200; setPrimAlpha draws
+0x53 × 0.5019608 / 128, i.e. (255 − value) / 255. MBTreeSetColor stores
node +0x64 with flag 0x100; the client modulates by the colour
(HYPOTHESIS, unknowns.md). StartGenFX 0x17f580: EffectHeaders 0x50 + level
for generator spawns of levels 1..3 at the enemy's position.

InitEffects 0x179e20 binds EffectHeaders 0x50.. per level (ported
2026-10-22; InitEffect always writes the AtreeMatch result, none when absent):
per loaded enemy type SUICIDEEXP (0x50, types not loaded as aux),
GENFX1..3 (0x51..0x53 while unset; enemy_genfx = 1 for each type that tries),
TREEHIT/TREEDIE (types 0xb, 0x15 → 0x54/0x55), ATK1FX/ATK2FX/DEATHFX1/
DEATHFX2 (0x1b → 0x56..0x59), DEATH_ARC/DEATH_EXP (0x1e → 0x5f/0x60); boss
levels LEGENDFX/LEGENDFX2/LEGENDHLD/LEGENDPRJ (0x5a/0x5b/0x5c/0x5d) from
item_anim2; 0x52/0x53 fall back to 0x51; InLevel B6 or I5 rebinds 0x1e (GENDEST
elsewhere) to item_anim2's SAFEREXP (trace-B6-bosskill-v1: the spew blast's
bursts last 1.53 s); WORLD_EXP (0x5e) from item_anim, else item_anim2. The
tools now register the item packages as effect sources with their role
(item_anim = the realm's, item_anim2 = the level's).

AddSpecialTexmod 0x111180 / DoSpecialTexmods: load_player gives a player
of class & 7 == 7 (Jester, Hyena) a special texmod on the held weapon — the
model's BOMB texture shows the SFX package's BOMB00 + k, k stepping 0..4
every AFrame (rate 1). The client tags the weapon meshes and replaces
their texture each update (the BOMB record itself is a placeholder that
drew magenta).

## GS output stage (2026-10-22, both dumps)

FRAME_1 is PSMCT16S (PSM 0x0a, FBW 10, double buffer at 0x46/0) in the G1
and Tower dumps, with DTHE 1, COLCLAMP 1 and DIMX 0x6071243571603524 (the
4×4 matrix −4 2 −3 3 / 0 −2 1 −1 / −3 3 −4 2 / 1 −1 0 −2). Every pixel write
stores clamp(c + DM[y & 3][x & 3], 0, 255) >> 3 per channel and the display
shows v << 3 (the reference snapshot peaks at 248). The lightmapped world's
base draws set FBA_1 = 1 (textures 185, 404, 502, 123, 415 in the Tower
dump), so the lightmap pass's DATE/DATM test passes wherever the base wrote;
draws with FBA 0 (characters, effects) keep the texel alpha's bit 7, which
the keep-A pass tests. gdl_play quantises the final frame once per PS2 pixel
in DisplayToLinear and, for opaque lightmapped texels, the base pass's own
write before the lightmap multiply (gs.wgsl gs_write at the fragment's PS2
pixel from the view viewport; GDL_NO_DITHER=1 disables both). Blended draws
and stacked additive glows truncate per draw on the GS and are not modelled
(fixed-function blending). Tower at the reference camera: the dark floor
blocks went from ~30 to ~25 summed RGB against the reference's ~24
(private/reports/render-tower-native-v3-basewrite.png); overall mean
36.2/26.1/11.8 against 33.5/24.5/10.3.

## VU1 microcode (2026-10-22, Tower state)

vu1MicroMem.bin from private/reference/captures/tower-newgame-v1.p2s (state
SHA-256 c42371e3…d81f; micro memory e2905fc1…29eb, data memory
c1bf889d…ac62; private/reference/captures/tower-vu1/) disassembled with
tools/python/vu_disasm.py (PCSX2 DisVUmicro.h/DisVUops.h tables; checked
against the documented VU0 fqdist pairs 68..75) into
private/reports/vu1-tower-disasm-v1.txt. A dispatcher at pairs 0..400 jumps
through program addresses in the uploaded object header (JR vi03 after ILW
264(vi12)); trace markers (T bit + ISW to 6(vi00)) are spread through the
code. Two programs light vertices:

- Program A (colourless parts; vertex loop to pair 763): base = min(vf09 ×
  vf19.x + vf12, 128) (light colour × the directional term + ambient), then
  the point lights are added and the sum is clamped to 255 before FTOI0 —
  point lights can take a part to twice its texture colour.
- Program B (vertex colours, base + lightmap pass; pairs 841..1180): base =
  min(vertex colour × vf12, 128), plus the point lights, clamped to 255; the
  same colour is written for both passes (STQ from vf23 and vf24).
- Light loop (pairs 652..716 and 1026..1090): vi05 = mem[1].x steps down by
  2 to 38 (pbSetupPosLights uploads the lights to VU address 0x26, two
  quadwords each). pbWinSetupPosLights packs each MBPosLightNew slot
  (colour +0, position +0x10, radius +0x20, attenuation +0x24) as position
  with w = 1 / (64 r²), colour × 128 with w = attenuation × (+0xc0, 1.0 from
  pbResetLights; MBPosLightScale has no callers) × 8, reach r; positions are
  taken into object space × 8. Per light: Δ = light − vertex, f = 1 −
  |Δ|²/(64 r²), skipped when Δ·N < 0 or f < 0 (FMAND on the sign flags),
  else added: min(colour·128 × attenuation·8 × f × (Δ·N) / |Δ|², colour·128).
  Vertices arrive as ITOF4 positions (raw / 16 = 8 × the parser's raw / 128)
  and ITOF4 − 7.5 normals (5-bit unpack << 3: 7.5 × the parser's c/15 − 1),
  so in world units the gain is min(7.5 × attenuation × (1 − d²/r²) ×
  cosθ / d, 1) of the light colour. pb_do_poslights (0x272004) is 1 and only
  pbDrawObject reads it: every object gets the lights within its reach,
  world draws included.
- Second pass of program A (pairs 765..836): with header word 1.y bit
  0x80 the subroutine at pair 267 writes a register block after the
  vertices and the vertex block (its start saved at mem[9].w, pair 474) is
  XGKICKed again under a new GIF tag — the keep-A pass (alternate texture,
  −4) reuses the base pass's STQ, colours and positions (read; the earlier
  HYPOTHESIS holds).
- Chrome in program A (pairs 591..627): with mem[17].w set, u = N·mem[62],
  v = N·mem[63] (setChrome uploads its two axes × 0.1333 = 1/7.5 there with
  UNPACK V3-32 at 0x3e, so these are n̂·axis); header word 1.y bit 0x40
  warps both to 0.6 t + 0.4 t³; then uv = uv × mem[15].xy + mem[16].xy, the
  per-object UV scale and offset from the object header (pairs 131..157;
  the init program sets 256 / 0, i.e. ITOF15 UVs / 128). For chrome draws
  setTexShift 0x1beac0 (instructions read with the ELF's pb_debug_chrome
  block at 0x2721f0: +0x14 0, +0x18 1, +0x2c/+0x30 1.0, +0x34/+0x38 −1.0;
  texsh_def.13 [1, 0, 1, 0]) writes scale (−0.5, −0.5) and offset
  (−0.5 + yaw/2π, −0.5 + pitch/π), the angles setChrome left in the camera
  block (+0x54 → u, +0x50 → v). Since the normals arrive as 7.5 × n̂, t =
  N·axis is the unit dot product. gdl_play (2026-10-23): unit axes and
  uv = −0.5 t − 0.5 + angle; before, the axes kept the 1/7.5 against unit
  normals and the scale was 1, so the chrome was nearly flat. The warp's
  header bit 0x40 (pbSetDORegs from pbg +0x1c +0xa4) is not traced.
- gdl_play: lit materials min(min(ambient + N·L light, 1) + Σ, 255/128);
  vertex-coloured materials min(min(colour × (1 + add), 1) + Σ, 255/128).
  HYPOTHESIS: vf12 of program B is MBLightmapMod/MBNoShadeMod's 1 + add
  (the setup at pairs 168..250 combines VU memory 18, 37 and 0.z; its data
  is not traced). The clamp matches the reference: the Tower's crystal
  glows (ambient adds) fell from 281/256 to 227/198 against 231/195.

## Tower lighting comparison (2026-10-22)

Native pinned to the reference camera (GDL_DEBUG_CAMERA with
tower-newgame-v1-camera.json) against tower-newgame-v1-viewport.png,
64×32-block means: private/reports/render-tower-compare-v2.txt (native
frames render-tower-native-v2-fade.png, -final.png). Overall mean RGB
(reference 33.5/24.5/10.3): 47.9/39.5/22.9 before, 44.4/34.5/18.6 after the
fade fix, 39.5/29.7/13.9 with the 16-bit output, 38.0/27.8/13.3 with the VU1
clamps. Pixel checks (texel × vertex colour × lightmap, alpha-tested decal
texels skipped): native equals the data formula on the floor and podium
(e.g. FLOOR_CYL#08 43,42,37 vs 42,42,37); the reference is 3..8 lower per
channel (per-draw truncation). Open: the reference's L1LINE154/131 emblem
shows dark texel-edge outlines native lacks (same TEX1: MMAG linear, LOD
< 0). S5 with the player light: render-s5-player-light-v1.png (no
reference capture of S5).

## G1 lighting comparison (2026-10-22)

Reference: slot 1 (poison-fields-original.p2s) after 12 vblanks
(reference_screenshot.py; private/reference/captures/render/g1-fixture-v4.png,
camera g1-fixture-v4-camera.json; the 2-vblank attempt showed a black frame
and a first attempt captured the native window, both discarded). Native:
GDL_DEBUG_LEVEL=G1 pinned to that camera (private/reports/render-g1-native-v1.png;
different moment: enemies near the player, an unbroken barrel). Lightmapped
ground at matching pixels: data formula 52/37/20, native 52/36/14,
reference 44/28/11. The G1#079 water (texture 123, no colour stream, no
lightmap, flag 0x1000) is about twice as bright natively in block means;
the G1 dump's water draw received vertex colour 128 (as native's factor 1),
so the difference is attributed to the water texmod frame at capture time
(not verified). Overall mean (rows 100..352): reference 21.7/22.3/7.5,
native 25.4/27.3/11.4.

## Integer truncation at the GS multiplies (2026-10-23)

gs.wgsl now computes TFX MODULATE as the GS does, (Cv × Ct) >> 7 per
channel in integers (Cv the vertex colour as sent, 0..255 with 128 = 1.0;
Ct the texel), and the lightmap pass as Cd × As >> 7 (Cd the frame value
after the base pass's 16-bit write, As the lightmap's CLUT alpha). Tower at
the reference camera (new game, welcome scroll closed; the reference
tower-newgame-v1-viewport.png), 64×32-block means over the 3-D area:
overall mean RGB native 35.3/26.5/12.5 → 33.7/25.4/11.9 against
32.7/24.5/10.9; block mean absolute error 2.71/2.09/1.72 → 2.38/1.92/1.37;
pixel mean absolute error 7.93/6.88/4.98 → 7.51/6.91/4.90 (runs
client-towercmp-b / -c). The left wall's bright blocks (rows 64..96, x
0..128) went from 7..15 above the reference to 21..26 below it: the
remaining open difference there (not explained yet; candidates: the
Gouraud interpolation of integer vertex colours, additive glow passes).

## Additive As and no-shade draws (2026-10-23)

- ALPHA_1 0x48 (Cs·As + Cd) takes As = Av·At >> 7 up to 0xff (×2) and
  COLCLAMP saturates the sum. gs.wgsl now premultiplies (Cs × As) >> 7 in
  integers for additive draws and blends One/One (the former SrcAlpha blend
  capped As at 0x80). In the Tower dump every additive draw has vertex alpha
  128 (psys sprites 127) and textures with alpha ≤ 0x80 (GEM_ORANGE_IN ≤ 90,
  GLOWRING_ORANGE00..03 ≤ 128), so the Tower frame does not depend on it;
  it applies to draws whose vertex or texel alpha exceeds 0x80.
- setPrimColor 0x1beca8, node flags 0x5000 (0x1000 or 0x4000) without a
  lightmap: MBNoShadeMod's block (pbg+0x1c +0xa8), +0x48 = node +0x6a + add
  + 1, and object header word 1.y |= 1. VU1 setup (vu1-tower-disasm-v1
  pairs 201..246): bit 1 → vf12 × vf31.z × 1/248 (pairs 237..246, use not
  traced); bit 0 → vf12 × vf31.z with the directional rows vf09..vf11
  zeroed (pairs 227..230), so program A's base min(vf09 · N + vf12, 128) is
  the constant 128 × scale × (1 + add); neither → vf12 × (mem[37] + vf31.z)
  with the directional light. The point-light loop (pairs 652..716) runs in
  every case (mem[1].x lights). Tower dump: the crystals' XSNS_C sheen
  (GEMSHEEN_ORANGE, PSMT8 at TBP 15072, 165 strips = 15 crystals × 11;
  ALPHA 0x44, TEST GREATER AREF 1, TEXA TA0 0 / TA1 0x80, so binary alpha),
  their additive bodies XP_CRY (node flags 0xc01880) and glow rings CFXP_G
  (0x4c01880) all arrive with vertex colour (128, 128, 128, 128).
  gdl_play: GsMaterial::with_draw clears `lit` for these flags (the unlit
  path, colour × (1 + add)); draw-variant keys include 0x1000.
  Correction: an earlier session note had both crystal body meshes drawn
  additively; the sheen is alpha-tested and opaque, and the dump draws every
  sheen first, then each crystal's body and glow ring — the native
  opaque-then-additive order agrees.
- Tower at the reference camera (runs client-towercmp-c → -e (additive) →
  -f (no-shade)): left-wall region (rows 64..96, x 0..128) 112.5/64.5/12.3
  → 153.1/93.1/18.2 → 162.2/103.0/23.9 against the reference's
  125.1/72.5/15.0; overall 34.4/25.2/11.3 → 36.9/27.1/11.7 →
  37.7/28.0/12.1 against 33.5/24.5/10.3. The captures are wall-clock timed:
  the crystals spin and their glow rings play texmod frames, so differences
  in this region mix animation phase with the renderer changes (c → e
  cannot come from the additive change, which leaves this frame's draws
  unchanged). Open: native crystal bodies are brighter than the
  reference's (summed RGB p99 547 against 441/448 in rows 40..200, x
  0..200 of the reference viewport and the dump's own snapshot); not
  explained. Other levels at their reference cameras (client-addchk-a1,
  -g1, -e1, -j1, -d1, -c1, -h1; client-noshade-g1, -a1): no visible change
  besides moment-dependent content.
- GDL_FREEZE_AT=N (gdl_play TEST switch): no level step after N updates;
  texmods and particles then advance with stepped updates (÷ 59.94) instead
  of render time, item and world texmod lists from their first run. Two G1
  captures (GDL_DEBUG_LEVEL=G1, autostart, the scene-g1-v1 camera, update
  600; runs client-g1fz-a/-b) are bit-identical. Two Tower captures at
  update 400 (client-towerfreeze-e/-f) still differed in the crystals' spin
  and the player's area. Rechecked 2026-10-23 on build de90493: two
  frozen Tower captures at update 400 (client-tfz2-a/-b) are bit-identical,
  and the per-update simulation hashes of two Tower runs (GDL_EXIT_AFTER
  600, uncapped and GDL_RENDER_FPS 9; tdet-a/-b) are equal at all 600
  updates. The earlier difference does not reproduce; its cause was not
  identified.
- Corrected 2026-10-28 (M20): the harness clock counted one update as
  1/59.94 s, i.e. texmods and particles ran at half rate in frozen
  captures (an update is one AFrame: SPF 0x3d088889 = 1/30), and the psys
  clock still dropped the steps past four in one render frame — a load
  burst lost particle frames depending on frame timing, which is why H4's
  flames differed between runs. Now, under GDL_FREEZE_AT, texmods take one
  AFrame per update since their list's first run and particles one psys
  frame per update, none dropped (render time and its four-step cap are
  unchanged in play). The other source of run-to-run difference was a
  texture search through a HashMap (per-process order): C2's MISTPART
  sprites bound ITEMS_C's or the level's own texture at random ("Particle
  systems" below). Checked on the RX 580 with GDL_RENDER_FPS 60 and 30
  (private/runs/m20-determinism-v1): G1, F1, H4, J5, C2 frozen frames
  bit-identical across the rates; I1 differed once (lantern and upper-deck
  lighting) in seven runs, not reproduced since — cause unidentified,
  first observation kept there. GDL_SCENE_DUMP=FRAME writes every drawn
  mesh (world matrix, mesh and texture content digests, material
  parameters) for diffing two runs.

## Weapon trails and the held weapon's flags (2026-10-23)

PlayerDoWeapTrail 0x14f428 (do_players, after each PlayerProcessSkinFX) is
ported from instructions (game::weaptrail): while +0x7dc is held (DoPlayer-
Action cases 0x21, 0x25, 0x3c) each update copies the held weapon at its
world matrix into one of eight slots; copies fade by weaptrail_dalpha (32
in the ELF data) × vb_elapsed and go at 0xff; +0x7d8 tracks the top slot.
gdl_play draws the copies (the WEAP_HOLD parts at the copy's matrix, alpha
× (255 − fade) / 255) and hides the held weapon while DoPlayerAction holds
MBTreeSetFlags(+0x6b8, 2). Native records carry load_player's node
markers (+0x80 and +0x6b8 = 1; weapon_anim reads 0xf90000 at runtime, so
the reference always has the weapon node). Evidence: the reference's first
update after a reload sets +0x7d8 −1 in every record (trace-tower-4p-v2,
matched natively); no reference trace yet shows a trail (the Sorceress
traces never enter 0x21/0x25/0x3c), so the copies' drawing is not compared.

## The lightmap pass of translucent world parts (2026-10-23)

Tower dump (…20261020081101): every blended 256×256 PSMT8 world draw
(ALPHA_1 0x44, TEST 0x5001d) is followed by its lightmap pass over the same
vertices (ALPHA_1 0x89 Cd × As, TEST 0x5c000: DATE/DATM, no alpha test) —
102 pairs (gs_pairs research script). The pass multiplies the frame, so at
a decal's soft edge the GS gives (B·As + D·(1 − As)) · L, not the folded
B·L·As + D·(1 − As): the floor under the edge band is darkened by the
decal's lightmap — the dark texel-edge outlines of the Tower emblem
(L1LINE154/131, texture DRAGONBODY2: alpha 0/17/43/77/105/121/128, black
where 0). gdl_play now draws lightmapped parts whose texture is not opaque
as two draws: the base (texture × colour, no lightmap) and a child draw of
the same geometry (GsDraw::lightmap_pass: blend Dst × Src, depth
GreaterEqual without writes, sorted just after the base by a 0.001 depth
bias) that applies the base pass's alpha test and outputs the lightmap's
As / 128. The multiply covers only the pixels the base pass wrote: a first
attempt multiplying the whole polygon blackened large floor areas and cut
rectangles into torch glows, unlike the reference, which shows DATE/DATM
limits it to the base pass's own pixels (FBA 1 sets their alpha bit; the
floor's own lightmap pass, written with the lightmap's alpha < 0x80, leaves
the floor's bit clear — inferred from that frame, not traced). Tower at the
reference camera (client-towercmp-h): the emblem's outlines and the floor
decals' edges as in the reference; emblem region (rows 200..384, x
150..500) 18.8/15.1/8.7 → 18.1/14.7/8.4 against 16.6/13.3/7.4. G1, A1 and C1
at their reference cameras (client-lmpass-*): no visible change otherwise.

## The X-ray chest view (2026-10-23)

do_see_thru / end_see_thru / ClosestChest are ported (game::seethru; see
its module note for the rules). Reference: a labelled powerup write (kind
9, flags 2, reference_warp --poke) with the player beside G1's chest 43
(private/reports/xray-ref-v1.png): a rounded window (POWERUPS "SEETHRU",
texture XRAY_A) is cut through the chest's front, the ground behind and
the contents' icon visible inside. gdl_play draws the icon at the chest's
matrix × 0.65 sorted before all translucent draws, the window as a
depth-only, alpha-tested draw (no colour writes, depth always written)
sorted first among the rest and moved towards the camera by the chest
definition's radius (scaled to keep its screen size), and the chest's parts
sorted after it — so the chest fails the depth test inside the window.
HYPOTHESIS: the node Z modifier −800 (MBTreeSetZMod; VU1's use of it is not
read) is approximated by that offset; the icon tree's MB flag 8 is not
modelled. Native capture: private/runs/client-xray-g1f (GDL_DEBUG_POWERUP=
9:2:120 with GDL_DEBUG_TELEPORT next to chest 43). The contents differ by
moment: random contents resolve through RandItemIdx without advancing, so
they follow the item random state, in both.

## Back-face culling (2026-10-23)

VU1 programs A and B (vu1-tower-disasm-v1) keep the last three screen
positions (pairs 537..539 / 915..917), and for a vertex whose clip flags
(FCGET, pair 530) are clear take the cross product of the two edges from
the oldest vertex (OPMULA/OPMSUB, pairs 562..563 / 940..941), multiply its
z by vf08.z and test its sign (MUL.z, then FMAND with 32, the z sign bit;
pairs 567..572). vf08 is the packet's first quadword (LQ 0(vi12), pair 409):
z is the strip header's direction (±1.0 at +8, the parser's
`Strip::direction`) and w alternates it per vertex (MULw.z, pair 549). The
store (pairs 742..758) chooses between w = 0x7fff (vi14) and ADC 0x8000
(vi13) from the clip flags, that sign bit and the input's w; the exact
path that leaves ADC on a back-facing triangle runs through the in-place
vertex buffer and is not traced (HYPOTHESIS: the output word starts with
ADC and the sign bit clears it). The evidence that back faces never reach
the GS is the dump check below.
Header word 1.z (bits 8/16/32/64: zero, flip or freeze the sign, pairs
429..446) is pb_obj_info +0x78, pbSetDORegs' pbDebugMode() — 0 in play, so
the test always runs.

Dump check (tools/python/gs_cull.py, on-screen triangles 1740..2350 ×
1840..2250): per strip tag, drawn and ADC-suppressed triangles have
opposite windings in 323 tags and the same in 14 (the Tower,
…20261020081101); G1 (…20261012144427) 48 against 8. Tags joining
several strips restart the parity and count as mixed.

Client: GsDraw::cull for object parts (material_set's base materials and
from_node_flags; psys sprites and missile streaks keep both faces) with
wgpu cull_mode Back — the render root's x mirror reverses screen winding,
and the Tower at the reference camera keeps its floor and walls with Back
and loses them with Front (runs client-cull-back / -front / -none;
GDL_CULL_FACE overrides). The crystals' brightness gap is unchanged by it
(rows 40..200, x 0..200, summed-RGB p99 537 without culling, 558 with,
against 441 in the reference; the opaque sheen core already hides the
bodies' back halves): still open. Without point lights
(TEST switch GDL_NO_POINT_LIGHTS, run client-cull-nopl) the region is the
same (p99 541, 7503 pixels above 250 against 7983 with and 4916 in the
reference), so the lights native gives the crystals (the dump's crystal
vertices are all 128) are not the cause either. What remains visible at
4× zoom: native additive tips and halos saturate to yellow-white where the
reference stays orange — consistent with the GS truncating each blended
write to its 16-bit frame (unknowns.md "GS per-draw truncation", not
modelled for blended and additive draws) — and the native frame is
rendered at 1280×768 and downsampled, sharper than the reference.

## Additive writes (2026-10-23)

Each GS write stores clamp(C + DM[y & 3][x & 3], 0, 255) >> 3 in the
PSMCT16S frame (DTHE 1). For an additive draw over a frame value Cd that is
already a multiple of 8, the result is Cd + floor((v + DM) / 8) × 8 with v =
Cs·As >> 7 — a step that depends only on the draw (−8 where v + DM < 0;
the clamps at 0 and 248 agree). gs.wgsl emits that step for additive draws
(GsParams.extra.w, object parts and psys sprites; GDL_NO_DITHER turns it
off) into the HDR target and the final pass quantises the frame below once,
so any stack of additive layers over a written frame gets the GS value.
Alpha-blended draws (0x44) cannot be done this way: their step depends on
Cd. Tower at the reference camera (run client-addq-a against client-cull-
back): crystal region (rows 40..200, x 0..200) summed-RGB p90 376 → 355 and
p99 558 → 500 (reference 290 / 441), bright pixels 7983 → 7139 (4916),
their mean blue 35.6 → 27.6 (31.3); overall 36.7/27.1/11.6 → 35.9/26.0/10.6
(33.5/24.5/10.3). Halos turn orange where they were yellow-white; the
reference's halos still show sharper rays and its frame is softer. G1, H1
and A1 at their reference cameras show no artifacts (client-addq-g1/-h1/-a1).


## GS dump software replay (2026-10-24)

tools/python/gs_replay.py replays a stock PCSX2 GS dump's GIF data in event
order through a small GS model (PACKED/REGLIST/IMAGE tags and local copies,
the vertex queue with ADC, STQ texturing with the TEX1 LOD and the
bilinear/mipmap filters, PSMCT32/PSMCT16 CLUTs through TEXA, TFX, the alpha
and destination-alpha tests, 24-bit depth, ALPHA_1 with COLCLAMP, DIMX
dithering, FBA and FBMSK into a PSMCT16S frame). Textures come from the
dump's own uploads kept linearly by DBP and PSM (no block swizzle), resident
PSMT8/PSMCT32 textures from the frozen memory. Approximations: integer pixel
sampling with a top-left rule, bilinear weights in floating point, per-pixel
LOD. Research only.

Tower dump (…20261020081101, two frames, 11,662 and 11,680 drawn
triangles, every texture resolved; private/reports/gsreplay-tower-v2):
against the dump's own screenshot (resampled to 640×448) the static part
of the frame (x 312..628, y 200..340) differs by 2.9 per channel (resampling
blur included), the whole frame's mean RGB is 31.0/23.5/10.7 against
30.5/23.5/10.8, and the left fire bowl with its additive NICEGLOW (x
160..240, y 110..200) has summed-RGB p99 640 against 647. The crystal
region (x 0..200, y 40..200) does not align with the screenshot (error
9.1): the crystals spin, and the screenshot is not one of the two replayed
frames.

The crystal brightness question (2026-10-23: native p99 500 against 441)
is settled as phase spread, not a bias. Summed-RGB p99 and mean RGB in that
region: reference frames — replay frame 0 504 (68.5/40.5/11.8), frame 1 472
(61.5/38.0/11.1), the dump screenshot 446 (65.4/41.5/12.3), scene-tower-v3's
reference 525 (56.0/36.6/11.6); native frames at the scene-tower-v3 camera
(client-scenetower-v3: 12 s, the compared frame, 20 s) 456, 397, 461
(62.9/39.4/11.8, 61.1/36.7/10.1, 62.2/38.8/11.4). The native values fall
inside the reference's frame-to-frame range (the reference's own consecutive
frames differ by 32). The dump and scene cameras are close but not
identical (both the Tower start).

## Hit flash (2026-10-24)

The knockback flash (pcontact: SetSkinFX(1, TextureWhite, 1, 1);
DoProcessSkinFX: MBTreeSetAltTex −4 and MBTreeSetAmbientAdd 999) draws the
player as a flat pale-green silhouette, not white: POWERUPS' AAAWHITE is an
8×8 format-16 texture whose every index is CLUT entry 0, 0xdf97 =
(184, 224, 184), and the keep-A pass modulates it by program A's base,
which the add saturates at 128 (min(ambient + light, 128)). Reference: the
B4 combat capture lvl-B4-play-v1 stepped to vblank 132 with the trace's
schedule (trace-B4-combat-v1 shows +0x7b4 = 1 at steps 132..135; no guest
write) — screenshot ref-b4-flash-v1.png and GS dump …20261024040218,
replayed by tools/python/gs_replay.py (gsreplay-b4flash-v1): the silhouette
averages 183/220/183. Native frames of B4, C3 and J6 (scene batch
2026-10-24) show the same silhouette at 179/219/179.

A new enemy starts without a skin: SetEnemyObj's MBNewNode runs MBNodeInit
0x1cf528 (+0x5c = −1, +0x58 = 0: MBTreeSetAltTex's mode and texture;
+0x6a = 0: MBTreeSetAmbientAdd), so init_enemy (and Death's statue swap)
reset the slot's Game::skin_views entry. Before 2026-10-30 a reused slot
kept the previous enemy's view — most often damage_enemy's death add 999
(0x13a57c) — and the new enemy drew near-white until its own skin ended
(research/playtest.md "Playtest after 1.0").

## Sequence texture modifiers (2026-10-30)

Animation sequences carry texture modifiers too: the 0x30-byte sequence
record's +0x28 (count, i16) and +0x2c (first index into the package's
0x58-byte texmod table, whose entries then have owner = the tree index),
+0x24 its repeat flag. DoAnimateTree 0x1129e8 and AnimateATreeFrame 0x112c04
call DoSeqTexMods 0x113208(tree, sequence, (int)(frame + 0.5) or the
reversed frame): per modifier, when count × divisor < frame and the
sequence repeats and count × divisor ≥ 2 the frame wraps modulo that,
then DoTexModSeqSub 0x110d10 — the routine DoTexModNode 0x1132d0 jumps to
for type-3 nodes (request_tree_texmods: frame swaps clamped to the last
frame, fades, scrolls). The client ran only type-3 nodes' modifiers; it now
builds each sequence's list in tree_texmods (whole tree, active while that
sequence plays) for effect trees (EffectHeaders, missiles, familiars,
enemy missiles, powerup trees) and item trees. Users on the disc (every
ANIM package scanned): WEAPONS 28 trees (EXPLOSION, EXPSMALL, HITCOL,
HITDIE, BLOODHIT/DIE, SPARKS, FIREHIT/DIE, ACIDDIE, LIGHTDIE, GENDEST,
CHESTDEST, POISONEXP1–3, EXPRING, MAGICFX, MP_LIGHT, FW_SHLD_ACTIVE,
L_SHLD_ACTIVE, ELECBREATHE, PHOENIX_FBALL, BOSS_BREATHE, LEVELUP_*), POWERUPS
10 (breath icons), the class SFX packages (VAL/WAR/WIZ powers), item
packages (IT, TRANS, FORCEF/FORCEF_S, boss-level coins) and monster
packages (projectiles, bombs, TREEHIT/DIE; ACI, DJINN, DRIDER and YETI
body sequences — character and critter trees are drawn by other paths
that do not run them yet: a coverage gap, unknowns.md).

Texture modifier alpha: materials_for chose Opaque / cut-out / blended
from a record's own level-0 texels. A target whose texels are all clear
(FBALL_EXP00, EXPLDSML_00, EXPLBIG_00, SUICIDE_RING00) read as a cut-out,
while its frames are soft (FBALL_EXP02…28: alpha up to 126 of 128), and the
0.5 test rejected every frame texel. A target now takes the most
transparent class of itself and its frames (records of the same package).

Evidence: a G1 red barrel (single-player 364, then 363 in a chain).
Reference: w7l1-barrel363-v2 (slot 6, player 0 written to (−2.8, 25.45,
−250.8) — WARPED — then 360 idle vblanks), Up 4 vblanks, cross held;
screenshots at 12…140 vblanks (private/runs/barrels/ref/shot-*.png): the
fireball from vb 34 to vb 86, red staves flying, then the scorch decal.
Trace trace-barrel363-v1 (--enemies, items): 364 hit at vb 22, +0xdc/+0xde
1 and +0xda 96 at 24, deleted at 120; 363 at 32/130 — the native sim
matches (game_run b363-detail). Native frames (RX 580): frozen-barrel363
(before: red light and staves only), frozen-barrel363fix2 (the fireball
grows, reddens and fades over ~24 updates). Original-presentation frozen
frames of TOWER, G1, J5, C2 and H4 stay identical to 1.0 (base-fx1).
Residual: after the fireball the settled break frames (BAREXPACTI F16–F24:
staves lying flat, height ≤ 0.46, radius ~4) draw bright red natively,
while the reference shows them dark against the scorch (unknowns.md).

DamageItem's leftovers: MBOX_NewObject(name, item node, node parent,
0x80800) → MBNewObject → MBNewNode copies the item node's local matrix
(0x1d4d10) under the item's parent, so the object outlives the item:
BAREXP0 (SCORCHED_EARTH, ±2.45 ground quad) and BARPOI0
(GREEN_BARREL_UNDERSPLAT) under exploded barrels, CHESTGEXP0 / CHESTSEXP0
(subtype 0x30) where a chest burst, ITEMEXP0 for an item a blast destroyed.
Game::item_debris lists them (presentation); gdl_play draws each at the
item's world matrix with node flags 0x80800 until the level ends (an item
on a moving floor would leave it in place: approximation).

## Client performance: material preparation (2026-10-24, night)

Reported with the M12 acceptance: the Sorceress's square attack (effect
trees 0x77/0x78 with a moving point light) slowed the frame rate. Two client
causes, both renderer bookkeeping, no change to what is drawn:
`Assets::get_mut` marks a material modified (re-prepared for the GPU) even
when no field changes — apply_material_requests and present_distance_fade
now compare first; and the eight point lights were part of every
GsMaterial's uniform, so a moving effect light re-prepared every material
each frame — they are now one storage buffer bound by every material
(binding 7; lib.rs `POINT_LIGHTS`, `write_point_lights`). Measured with the
client's DIAGNOSTIC frame log (GDL_FRAME_LOG) in scenarios perf-*-g1:
square attack 262 → 180 ms mean frame time on the private Xvfb, standing
126, cross throws 148 (m12-acceptance.md "Acceptance").
