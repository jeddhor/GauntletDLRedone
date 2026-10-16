# Interim native playtest (not the M12 slice)

`python3 tools/python/run_play.py` launches `gdl_play` on the current desktop
(`--virtual` uses the authenticated Xvfb session). It walks the Yellow Sorceress
through The Poison Fields with original level/character geometry, textures and
animation clips, driven by the native simulation at 30 Hz.

Inputs are resolved by verified archive identity: LEVELS/levelG1 OBJECTS/WORLDS/
TEXDEF/TEXTURES, PLAYERS/SOR/YEL00 OBJECTS/ANIM/TEXDEF/TEXTURES, PLAYERS/SOR/ANIM/
ANIM.PS2, the profile-qualified SLUS_200.47 and a private start state from
`tools/python/export_start_state.py` (copied from the preserved save).

Native and reference-compared components:
- Keyboard → the game's level words (observed Up 0x40000030, Down 0x800000c0,
  Left 0x10000003, Right 0x2000000c; attack 0x02000200) → JoyAng/JoyMag built by
  a native InitJoyAng from the ELF's `magval`/`angidx`: 98/98 runtime table words.
- Motion code/run timer, request, facing turn, static wall/floor collision and
  position (see movement.md, collision.md).
- Closed loop from the start state with original per-update controls
  (compare_closed_loop.py): positions 96/96; facing 92/96 (four 1-ulp
  differences at turn updates from the interim matrix rebuild).

Interim, not fidelity evidence (shown in the window):
- Start state copied from the save (spawn rules unrecovered).
- Requested action from observed code pairs (1→0, 8→17, 13→19, 15→92).
- Player matrix rebuilt as rotation(facing) × captured scale (CreateYPRMatrix
  and the 1.0000074 scale source unrecovered).
- Animation: native choose() transitions and original clip timing, but a
  presentation clock without CalcAnimInfo state, blending or events; action→clip
  map captured from the runtime table at player+0x1e8.
- Camera: StdCamera rest state (target = position + attention offset +0x810,
  forward basis and distance 24.0 from StdCamera, FOV from MboxWinPtr+0x1c); no
  camera logic, smoothing or collision.
- Presentation: x mirrored for Bevy (the original camera basis satisfies
  right × up = forward); the screenshot comparison confirms orientation.
  Materials are unlit with a per-texture alpha cutout at half the texture's own
  maximum alpha; GS/TEXA alpha, lightmaps and fog remain M05 work.
- Not present: enemies, generators, items/keys, barrels, dynamic objects,
  projectiles, damage/health, HUD, audio, exits, saves.

Smoke tests performed on Xvfb (llvmpipe): launch, READY idle, Up → RUN1, Up+Right
stopping at the cliff edge, attack → THROW1S; screenshots compared visually with
the fixture screenshot for layout/orientation only.
