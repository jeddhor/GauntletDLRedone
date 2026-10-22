# Native gameplay simulation (M11–M13 work in progress)

Code: `crates/gdl_core/src/game/`. Records keep the original layouts (player
0x2b00, enemy 0x3b0, item 0x110, effect 0xf0); guest pointers become
documented indices (index + 1 where 0 means NULL). Presentation side effects
(audio, help text, FX, HUD notices) are emitted as `Event`s. Paths outside
the ported subsets return `Unsupported`, are logged per frame by system, and
never silently substitute behaviour.

## Frame order

`Game::step_with` follows game_main's 0x4010 path: world_update
(SetupDynGrid), do_items (ProcessItems), do_enemies, do_players (supplied
closure; the interim player driver), ProcessEffects.

## Ported systems (function addresses in each module header)

- Items: load path (verified 384/519 live records, items.md), ProcessItems
  cases for generators, placed enemies, chests, exits, triggers, wobjs, type 10
  breakables and falling rocks (ExtractYPR, rand_rot.216), wave exits idle.
- Pickups: PlayerItemCollision, GrabPowerup, keys/gold/food/potions/powerups,
  OpenChest.
- Enemies: generate_enemy and spawning, InitActions trees ("<PREFIX><level>",
  levels 4–7 use EnemyDesc letters A/B/S/F from ELF 0x22d858), do_ai with
  move_logic00/02/04/05/06/07/18 (and the logics below), blocked00/07, attacks, collisions,
  damage_enemy, damage_player. Other realms: move_logic14 0x13f320 (strafe,
  re-aim at π/4 + n·π/12 after four turns or a >π/2 miss), move_logic17
  0x13eba8 / move_logic23 0x13da78 (stationary, ranged action 0x18 within
  +0x314 and ±10 height; 23 waits on its +0x334 cooldown), move_logic30
  0x13cd40 (move_logic00 with a randomised attack period from level_data+0xc0
  × 90, action 0xc or 0x10 at level ≥ 2).
- Generator placement (generator field block at item +0xf0): place_logic12
  0x153ff8 (first two spawns 3.5 to either side of the +0x100 facing via
  trans_enemy 0x139888, then the spawn point; +0x330 = 1/2/4; full → +0xfa = 7,
  +0xf4 = 3), place_logic13 0x154318 (spawn chain through +0x348/+0x34c with the
  newest in +0xf5), place_logic14 0x154390 (alternating ±π/4, +0x330 = ±1).
  trans_enemy removes the spawn (+0xc8 = 0) when check_enemy_pos fails; its
  MBTreeSetFlags calls are presentation.
- move_logic29 0x13cf40 (hold an 8–18 band: +0x330 0 hold / 1 back off / 2
  close in at speed 0.8 with offset_step5 steps on +0x36c contacts; attacks
  0xc/0xe alternating via +0x33c, 0x10 at level 3+, on the +0x334 timer).
- Phasing enemies (type 0x18, enemy_ai_control 0x13b208): +0x2e8 alternates
  fade-out (positive) and fade-in (negative) periods of RandInt(60) + 60
  while +0xe0 is 0/3/4. enemy_mod_alpha 0x13b110 is the only reader of enemy
  tree alpha, so alpha is presentation (`enemy_alpha`).
- ActivePlayerMask 0x220154: SetPlayerVars 0x125ef8 ORs 1 << k for players
  whose +0xfc is 1 or 5 (sllv in the delay slot at 0x125f94); called from
  game_main, AddItemInstList and ProcessItems when NumInGamePlayers 0x20f98c
  is 0. Trigger flag 0x400 (0x1563f4) requires the touched mask to equal it
  and mirrors the result in the bound object's +0x10 bit 0x04000000.
- DamageItem kind 10: rocks 0x29 (SetSafeRock 0x153438: stage from +0xe4
  against definition +0x44, model "<name><stage>"/+"L1"/+"L1ROOT" must exist
  — MBOX_FindObject_Sub, objcmp = strncmp 15 — else +0xf2 = -1), walls 0x2a
  (AudioExplodeWall: stage +0x12 world sound or 0x2e), explosive/poison
  barrels 0x2c/0x2d (StartExplosion 0x17e590 kinds 0x18/0x19: radius 12 /
  6.5, damage flags 0x421 / 0x800, damage level_data+0xdc × 30 / × 10; jump
  table 0x2ab980 verified; snd_355 0x23d4c0 / snd_351 0x23d488).
  The object-name set is a HYPOTHESIS: the labels of every package the
  runner loads (the original load list is not traced).
- Effects/missiles: FindEffectIdx/DeleteEffect/ZeroEffect, StartFXTree,
  ChangeEffect, StartMissile/PlayerStartMissile, CalcTargetDir,
  GetAttackDir, PlayerGetTarget, closest_enemy/closest_generator,
  MissileCollideEnemy, WeaponWall/ItemCollide, SfxSkipItem, DamageItem
  (generator levels), ProcessEffects missile mode and the area (blast) mode for
  enemies and items.
- PlayerStartMissile 0x18f9a8 (2026-10-22): the missile info is
  BossElecMissileInfo 0x24ee70 (+0x138 0x8000), BossAcidMissileInfo 0x24eea0
  (0x4000), BallistaMissileInfo 0x24ee40 (flags & 0x2100000 == 0x100000) or
  PlayerMissileInfo[class]; +0x138 0x400 throws from pojo_offset 0x24ef00.
  Trees: BOSSG_ELEC / BOSSG_ACID / PHOENIX_FBALL (effect flag 0x10000),
  SUPERARROW for the ballista, else the class throw tree. InitPlayerMissiles
  registers WeapThrowFx[player][type] = InitCustomEffect(SFX package,
  "WEAP_TW_<c>") for DmgTypeDesc 0x24de40 ("", RED, BLU, YEL, GRE); with the
  class info and a weapon powerup each missile gets StartFXSub(throw effect)
  linked at +0xe8 and parented to it (deleted with it when the missile is
  already gone), and the Wizard/Sorceress (classes 2/6) fly EffectHeaders[0]
  (NULLFX) under it. Headless: game-run sfx=… weapon=1:60
  (game-run-weapon-G1-v2.json: WeapThrowFx [−1, 109..112], the follower
  linked to its missile and deleted at the hit).
- Enemy missiles: shoot_missile, EnemyStartMissile (EnemyMissileInfo
  0x24de80, trees "<PREFIX>_ARROW/BOMB/FBALL" per InitEnemyMissiles; a missing
  tree returns no missile as Errorf does), MissileCollidePlayer and the
  missile/blast-vs-player paths of ProcessEffects; Random 0x1d6b00.
- SuicideExplosion (worlds 7/0xb): POISONEXP1 blast, radius 7.5, damage
  level_data+0xbc × 50, flags 0x2b (hits players, enemies, items).
- Melee: PlayerMotion's attack decision (target within reach + 1 → action
  0x27, small enemy → 0x52, else the throw 0x5c), DoPlayerAction's exit/entry
  event bits for attack actions, PlayerMotion's melee consumption,
  PlayerDamageEnemy 0x147070 and PlayerDamageItem 0x1472e8. Interim: the
  player stands still during a melee clip and the clip runs to its end.
- Magic: start_magic mode 0 → StartMagicFX (MagicEffects 0x2407a0) through the
  blast mode; DamageColor; randpottype (start state v6).
- Experience: AddExp, ModifyExp, LevelToExp, PlayerUpdateAtts attribute words.
  Owned-data check: the formula reproduces the save's four attribute words
  (+0x108..+0x114 = 200/150/600/400) from the class record and character bonus.
- Exits/progression: DoExit, player_collis_exit, all_players_go_to_same_level,
  anybody_playing, do_exit; the destination wave (world << 8 | level) is
  reported in `Game::level_done`. The Poison Fields exit in G1 leads to 0x701.
- Level start: WORLDS locator list (header +0x58 count, +0x5c offset, 0x1c
  bytes; AddLocatorInstList kind 7 = player start, yaw at +0x14); WldBtm =
  world min y − 4.5 (GauntWorldInit); enemy_load derived per level from
  GetEnemyTypes (LoadEnemy loop) — G1 derivation equals the captured table for
  types 12/13; type 30 (DEATH) differs only because no package is supplied.

## Interim pieces (labelled; not fidelity evidence)

- Player driver (players.rs): PlayerSim movement plus a presentation clip
  clock; library return flags approximated; record sync without offset
  rotation; PlayerGetTarget origin = +0x70.
- Magic button triggers use_potion directly (the original raises event 0x20000
  from the magic action's animation).
- Hit-spark/visual effects are presentation; the derived combat words
  (+0x118..+0x12c) follow the attributes each update (PlayerProcessPowerups'
  check_player_atts), so a level-up takes effect at once.
- Later levels keep the carried player record; player 1 is placed at the
  start locator (place_player, get_player_pos's single path); other players
  through load_player_geo → get_player_pos(k, 1) (player.md).

## Headless evidence (synthetic scenarios, not reference comparisons)

`tools/python/game_run.py` runs `gdl_tools game-run` with a scripted player.
Reports under `private/reports/game-run-g*-*.json`. With the scripted attack
schedule: G1, G2, G3 and G4 run 600–900 updates with no unsupported path (G1
logs only the type-30 enemy_load note); throws kill zombies and destroy a
generator; a debug potion override shows the blast growing to radius ≈ 20 and
killing nearby zombies and a generator.

## Enemy move logics (2026-10-22)

do_enemies saves LOGIC in +0x326 before the AI and afterwards stores the
logic that ran in +0x328 and restores LOGIC from +0x326 (0x13bbd4), so a
switch inside do_ai (e.g. 15 → 0, 27 → 0, 28 → 7, any → 0x18) lasts one
update; format_brain runs when the logic differs from the one that ran last.
Newly ported from instructions:

- set_suicide_bomber_index 0x139920 (called by do_enemies after FWRDVAL): the
  first live logic-0x12 enemy near the camera with action 4 at +0xe0/+0xe4;
  unchanged when none. Without it found_suicide_bomber never fired.
- move_logic24 0x13d8b0: run at 2× directly away from SuicideBomberIndex,
  adding the next offset_step5 step (≤ 8, +0x338) on each +0x36c contact.
- move_logic27 0x13d4e8 (type 0x1f via enemy_fix_ai type 31): stand until a
  target exists, then +0x314 = 1e6 and move_logic00.
- move_logic28 0x13d310 (C3 generators): hold, face the target and attack
  (0xc/0xe alternating on +0x33c, 0x10 at level ≥ 3, on the +0x334 timer)
  near the camera within +0x314 and ±10 height; within 6 → logic 7.
- move_logic31 0x13cbc0 (type 0x1b, the GRM raised by the boss, via
  enemy_fix_ai type 27): xlate31 0x13cb10 lunge on ATTACK1 (+0x2e0 = target
  within 7.5), 30–59-vblank rest after attacks, half-speed creep, then
  ATTACK1 within 10 or ATTACK3.
- move_logic15 0x13efe0 (placed enemies with AI 15, e.g. A2/B1 and the 0x21
  fallbacks): patrol the lookout chain (stage 0 nearest lookout with a next
  link, stage 1 follow +0x70 on arrival |dy| < 4, planar < 1); a target
  within 0.8 × +0x314 (+0x28c and planar) → logic 0.
- move_logic19 0x13e4e8 (golem enemies): AnimInfo +0x84 disabled (+0xa2 = 0)
  until a target in range is near the camera (+0x330, AudioGolemBorn), then
  WALK stages with a 30–119-vblank walk, stomps at WALK frames 10/25 and
  AudioGolemSwing at ATTACK1/2/3 frame 2 (+0x3a0 = previous frame).
- move_logic01 0x141f48 with get_target_ang 0x13c2d8 (not assignable:
  init_enemy_vars maps AI 1 → 0 and 10 → 7).

Reachability (private/reports/enemy-logic-scan-v1.json, all 51 levels):
generator AI values 0/2/4/7/0xe/0x1c/0x1d/0x1e, placed-enemy AI values
0/3/7/0xf/0x10/0x11/0x12/0x13/0x17/0x1a/0x1b, plus enemy_fix_ai,
enemy_logic_num and the cathedral/hell tables (7/0x11/0x12/0x13/0x17/0x1b/
0x1e/0x1f/3). All are ported. Logics 8, 9, 0xb, 0xc, 0xd, 0x14, 0x15, 0x16
and blocked08/10/20 are selected by no disc data and stay unported (do_ai
reports Unsupported); the golem enemy path cannot run on this disc because
MONSTERS/GOLEM is absent (generate_enemy −5), so golems are critters.

Synthetic spawns on the actual disc (game-run spawn=, DEBUG):
game-run-spawn-G1-l15/l27/l28/l31-v1.json and -bomber-v1.json — no
unsupported paths; 15/27 hand over to the logic-0 chase, 28 holds and
attacks, 31 creeps and attacks, and a GRU beside a level-6 bomber ran logic
24 for 40 updates while the bomber ran, then returned to 7. Not compared
with a reference.

## Not ported (known)

Unreachable move logics above. (The front-end fixed camera of modes
0x8009/0x400b/0x4012 only moves a view hidden under full-screen blits —
hud.md.)
- move_logic16 0x13ecd8 (ranged 0x18 attacks on the +0x334 timer; inside
  0.6 × range it backs off with action 0x16 at speed 0.8 until beyond 0.8 ×
  range), AudioDemonFireBall 0x15fcb8.
- Rotators (item type 12, ProcessItems 0x1566c0): the bound object turns by
  vb_elapsed × +0xf4 per update via YawMat3 0x1d5388 on its node matrix
  (subtype 0 continuously unless LockCamReq and object flag 4; subtype 2 once
  triggered until +0xfc reaches ±+0xf8, then +0xd8 |= 0x1000; subtype 1 is a
  co-op camera target, presentation). The object's flag 1 is cleared so
  collision uses the full matrix. Native composition of rotated nodes with
  their parents (rows × parent rows) is not verified against GetWorldMat.
  AudioRotator 0x160020 (snd1_370 0x23d568 loop, snd2_371 0x23d5a0 stop).
- Lookouts (game/lookouts.rs): kind-8/10 locators, at most 0x14, +0x30
  position, +0x70 next (locator +2), +0x72 parameter (locator +1). Actual
  reference check: the native G1 table equals the fixture's live Lookouts
  0x23bd20 (6 records, NumLookouts 0x23c720).
- Statue flags for SetItem: golem/death statue anims are enemy_anim[0x1d] /
  [0x1e] (0x233598 +0x74/+0x78: the enemy package of that type is loaded);
  critter_3/7_loaded are CritterETypes[3/7][0] (registered before items).
- Ported for the level sweep: SuicideExplosion outside worlds 7/0xb (effect
  0x16 + 0x1c flame; every explosion also starts 0x50 when that header
  exists, StartFXMissile returning -1 otherwise), rune / legend item /
  scroll / crystal / gargoyle-item pickups (PlayerHasRune, PlayerGiveRune,
  PlayerGiveLegendItem, PlayerGiveCrystal, PlayerGiveGargItem, StartGemFX;
  ScrollMessageBox is a UI event), random chest content (RandItemIdx),
  shot switch chains (DamageItem kind 5 subtype 0x1f), move_logic03 (flee,
  get_face_ang2) and move_logic26, CurWorld 10 generator sounds 0x2c0024 /
  0x2c0025, Death's touch (do_enemy_attack type 0x1e, AddExp kind -2 =
  amount × LevelDeltaExp × 0.01, StartDeathFX) and dissolve, the time-stop
  and trigger-camera enemy paths of do_enemies, MaxStopTime.
- Tower hub (game/tower.rs, level L1, CurWorld 0xd): EnterTower opens the
  doors for delivered gargoyle items / crystals (ActivateSpecialTrigger) and
  HideExits disables exits to unreached levels (wave byte bit n−1) and to
  closed realms (WorldOpen: Temple/Hell/Battle need shards, Hell all runes).
  PlayersSetWaveFinished records each completed level. With the fixture save
  (G1 in progress) the tower offers the first level of A, B, C, D, G, I, J
  and K. Not modelled: Sumner (secret character), TowerCheckMessages
  speeches/window cameras, shard/rune display, exit model swaps. The client's
  campaign mode (`run_play.py --campaign`) starts in the tower; exits and the
  boss TowerWave pick the next level by (world, level).
- Camera (HYPOTHESIS, do_camera not ported): the simulation camera follows
  player 0's rest pose after every update; it gates placed-enemy spawns and
  item activation. It was previously fixed at the level start, so placed
  enemies spawned only near the start.
- ResolveWorldData 0x161738 rewrites each LEVL record once (+6 0 → 1) and
  clears flag bit 0: zero fields +0xac..+0xdc take +0xa8 (+0xa8 and +0xc4
  default 1.0), +0xa8/+0xb0/+0xb4/+0xd0/+0xd4/+0xdc are multiplied by
  diff_mul_tab.15 0x23e168 [difficulty word 0x21edcc] (0.667/1.0/1.5) and
  +0xb8/+0xc0/+0xc8/+0xd8 become 1 / (factor × value). Native setup uses the
  resolved copy (level::resolved_level). Actual reference check: the
  fixture's live G1 record (level_data 0x23e158 → 0xfb51e4, difficulty 1)
  equals the resolved TOWN.WAD record over +0xa8..+0xdc (+0xb8 2.0, +0xc8
  2.0, +0xd8 1.3333) and +6 = 1. FOREST.WAD stores zero multipliers, so its
  levels run at 1.0 (previously generators got max 0 and the drider health 0).
- Level records: LoadWorldData 0x161b98 sets level_data = LEVL[wave & 0xff];
  the level directory is the record's code (GetLevelDesc 0x162750,
  "levels/level%s"). Records are not in level-number order (CASTLE: A1, A6,
  A2, A3, A4, A5), so tools select the record by code.

Level sweep (every level of realms A–K, 600-update scripted wander, interim
player, 2026-10-16): no unsupported path outside the boss arenas; G5 stops at
the ActivateNearestEnemy boss trigger. The boss arenas (A5, B6, C5, D5, E2,
F2, G5, H4, I5, J5, K5) have no generators. Fireworks (CurWorld 10 explosive barrels, StartExplosion 0x17e810 →
StartFirework 0x17e4d8): three kind-0 effects with random size, speed and
direction; at expiry (flag 0x10000000) the MBPsysFirework burst is
presentation but its RandInt/Random draws are kept. Destroyed or shot
potions (DamageItem / PlayerDamagedItem) call start_magic(0.8, player or -1);
player -1 uses power × 20 and size 0.8 without potion audio. Not ported:
SetPlayerVars' other writes, StartMagicHealFX for players above level 74.

## Powerups and the shop (game::powerups)

PlayerProcessPowerups 0x12d040 runs every update for players in states 1/4
(called from drive_player before motion): the combat words +0x118..+0x12c
are re-derived from the attributes (so level-ups and bought bonuses take
effect, and the interim motion uses +0x124), the flag words +0x130 (weapon
element, longest-lasting element wins), +0x134 (shields) and +0x138 (powerup
bits) are rebuilt from the eight slots +0x144 + 16k {time, kind, amount,
flags}, timers count down by SPF outside the Tower (3 × SPF while a boss is
active, frozen otherwise on boss levels), kind 7 adds speed, kind 8 magic,
kind 9 flag 0x80000 refills the turbo meter +0x800 to at most 100, and the
words are clamped to the class ranges. Before this port the flag words were
never rebuilt, so picked-up shields, shots and amulets had no effect.

Shop (do_shopping 0x18a898): ITEM records of SHPDATA/SHOP.WAD (31; entry 0
leaves), kinds 1 key, 3 potion (random colour 1..4), 5–8 +10 bonus to
fight/speed/armor/magic (PlayerInc* + the +0x98c..+0x998 shop counters),
0x11 heal by the record's amount, the rest PlayerAddPowerup with the
original amounts/times/flags. make_skiplist's loop reads one record past
the table in the original; the native table stops at its end.

Death (kill_player 0x131a98): see research/saves.md and playtest.md.

## Enemy replay against the reference (2026-10-22, actual disc)

- Probe: reference_trace.py --player --enemies (tools/python/reference_enemies.py):
  per vblank the 25 enemy records (`enemies`, 0x3b0 each), newlib's rand
  state (_impure_ptr → +0x58), BossIndex, SuicideBomberIndex, frame_main,
  Items, WorldBase, WorldInfo and the generator item records the enemies
  link to. Trace private/reports/trace-enemies-g1-v4.json: the slot-1
  fixture, 120 vblanks idle, 120 walking Up, 120 idle (180 game updates;
  all 25 slots live: type 12 logics 2/4, type 13 logics 5/6/7).
- Replay: game_run.py --enemy-replay TRACE [--items-seed LIVE_ITEMS] (gdl_tools
  game-run enemy_replay=): each update starts from the reference's records
  (AnimInfo +0x84 → the native clock; the tree's level from the InitActions
  table at +0xe8; generator +0x2a4 / floor +0x2ac pointers → slot / object
  + 1), player 0, controls, globals, frame_main and generator records;
  SetupDynGrid rebuilt; visibility from the reference's +0x2ee/+0x2f0. main
  reseeds rand every loop (srand(pbGetCPUTime() >> 3)), so the seed is
  solved by inverting the LCG over the native call count (solved in every
  update). Live items: tools/python/export_live_items.py
  (private/reports/live-items-g1-fixture-v1.json).
- Result (private/reports/replay-enemies-g1-v7.json): **179/180 updates
  equal** over every u32 of all 25 records except the presentation pointers
  +0x70/+0x80/+0x84/+0x88/+0x8c/+0xc0/+0xc4/+0x1f0. The other update (step
  182) is a generator spawning into slot 1 during do_items, outside
  do_enemies.
- Found and fixed on the way: DoEnemyItemCollide's +0x340 delay uses
  find_closest_item's fourth output — the nearest item of any kind that
  passes the filters — not the nearest blocking item (the port had 30
  where the reference had 15..19). Replay inputs found necessary:
  frame_main (find_closest_player's (frame_main & 7) stagger) and the
  generator records (generator 131's +0xd8 gained 0x4000 at step 148, after
  which it blocks enemies through check_vacancy).
- Combat-approach trace (private/reports/trace-combat-g1-v1.json, 540
  vblanks: walk Up 120, Up+Left 260 onto the zombies' platform, then throws;
  the native route was planned headlessly first and the reference ended
  within 0.3 of it). Enemy replay: 195/270 updates equal
  (replay-enemies-combat-g1-v1.json; the rest: generator spawns in
  do_items, the +0x2a0 item link now mapped, AnimInfo +0x94 mirrors, ±0.0
  signs in yaw matrices and one logic-07 stuck-counter divergence of
  enemy 6 — open). Player replay (--replay with the enemies, items and
  generator records the trace carries, +0x880 mapped, +0x93e cleared as
  SetPlayerVars does): 244/270 equal (replay-player-combat-g1-v8.json; was
  82). Fixed on the way: show_crystals' +0x904 timer uses the EE adder
  (sub_guarded_rz; plain subtraction was one ulp off), and headless game-run
  now runs init_next_level's player displays (hud_init without textures) so
  WritePlayerInfo keeps its record words. Then: PlayerMotion's auto-aim
  facing test `param_1[0x232] - 1U < 10` is unsigned (an attack type of 0
  fails it); the port compared signed, so an idle player near a target kept
  re-aiming — 268/270 equal (replay-player-combat-g1-v9.json). Open: +0x894
  (the floor-slope pitch) at step 194 and +0x197c at step 496 (written in
  the update a generator spawned; likely do_items). The 150-update walking
  trace still replays 150/150.
- Enemy replay on the combat-approach trace with the +0x2a0 item link
  mapped: 225/270 updates equal (replay-enemies-combat-g1-v2.json); most of
  the rest are generator spawns (do_items). The step-352 enemy-6 case
  (native check_vacancy blocked on generator 127 where the reference walks
  on) was a replay input, not a port difference: see the next item.
- Whole Items array in the probe (2026-10-22): reference_enemies.py now
  also keeps NumItems and every item record, delta-coded (first capture
  complete, later only changed slots). The trace re-recorded from the same
  fixture and schedule is byte-identical in all 541 enemy samples
  (trace-fight-g1-v3 vs v2, trace-combat-g1-v2 vs v1), so the reference run
  is deterministic. The replays seed every item from the before state with
  the after state's +0xd8 0x4000 bit — do_items' per-frame visible flag
  (item_visible), written before do_enemies and do_players. Root cause of
  the vacancy case: ItemCollide returns the *first* hit in grid order
  (nearest = 0), and slot 426 (kind 5, subtype 0x18, +0xfc reach 5) is
  ahead of generator 127 in that cell's chain; in the reference it was
  visible (+0xd8 0x4010; the fixture seed had 0x0010, which ItemCollSub's
  0x4040 test skips), so ItemCollide returned it and EnemyItemCollision
  (kind 5: no block) left the step vacant. Same for trace-fight-g1-v2
  step 532, enemy 13.
- Fixed by the traces: InitAnim's AnimInfo +0x10 (enemy +0x94) is the new
  sequence's frame count; animate_enemy did not write it on a switch
  (0xc → 0xf on seq 6 → 1 at fight-v3 steps 250/264; do_enemy_anim's
  0x18..0x1a timing reads it).
- Results with full items: enemy replay 250/270 (combat,
  replay-enemies-combat-g1-v3.json) and 247/270 (fight,
  replay-enemies-fight-g1-v4.json); player replay 268/270 (combat,
  replay-player-combat-g1-v10.json) and 266/270 (fight,
  replay-player-fight-g1-v5.json). The player replay seeds an enemy killed
  in the update (state 1 → 8) from its before record and, with full items,
  drops the after-state generator overlay: both kills and the generator's
  destruction come from ProcessEffects after do_players (with them the
  auto-aim words +0x8dc/+0x8e4 at fight steps 430/474/512 match). Every
  remaining enemy difference is a do_items spawn, a hit applied outside
  do_enemies, a link those write (+0x29c, +0x2a4) or a ±0.0 sign in the
  yaw matrix (+0x14/+0x28: the reference gets +0.0 in updates where
  +0x2f0 goes 1 → 0; CreateYPRMatrix gives −0.0 there unless the roll it
  reads is −0.0 — open, no gameplay effect). The remaining player words
  are ProcessEffects' kill credit (+0x8f0 combo, +0xbd4/+0xbe4 kill
  counts, +0x1a1c experience), +0x894 at step 194 and +0x197c at step
  496.
- ProcessItems replay (game_run.py --enemy-replay TRACE --items-replay):
  each update seeds every item from the before state (item clocks from the
  +0x84 AnimInfo), the view from the before state's StdCam record
  (Game::replay_sync_view: MBCameraUpdate rows, eye, target), runs
  ProcessItems (native visibility) then do_enemies with the rand seed solved
  over both, and compares the whole Items array (guest pointers and the
  +0xe6 link skipped). trace-fight-g1-v3: every seed solved; enemies 248/270
  (replay-items-fight-g1-v4.json). Items: 248/270 updates equal once four
  records are set aside (key 307 on a moving floor and the three S_CROWS
  sound items; unknowns.md "Item world matrices"), the rest being
  generator 127's damage by the player (ProcessEffects, steps 430/474) and
  the AnimInfo reset byte +0x96 (1 -> 2 when DoAnimation evaluates a drawn
  tree; presentation-dependent). Found: keys, gems, food, treasure and
  scrolls had no native clock (their trees are in POWERUPS; items.md "Item
  trees"); 79/270 before the fix.
