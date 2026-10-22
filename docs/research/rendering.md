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
  end_of_stage 2 after 2 s. gdl_core::game::lighting holds the state.
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
  material. HYPOTHESIS: no life fade. Every SfxSetLight caller is ported
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
  the init program sets 256 / 0, i.e. ITOF15 UVs / 128). The values the EE
  writes there for chrome draws are not traced (gdl_play adds the camera
  angles as the offset at scale 1).
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
