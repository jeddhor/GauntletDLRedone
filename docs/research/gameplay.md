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
  move_logic00/02/04/05/06/07/18, blocked00/07, attacks, collisions,
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
- Hit-spark/visual effects are presentation; LoadPlyrData is not ported, so
  derived combat words (+0x128 damage, +0x12c speed) are not recomputed after
  a level-up.
- Later levels keep the carried player record; only get_player_pos's
  single-player placement is ported (no test_location spreading).

## Headless evidence (synthetic scenarios, not reference comparisons)

`tools/python/game_run.py` runs `gdl_tools game-run` with a scripted player.
Reports under `private/reports/game-run-g*-*.json`. With the scripted attack
schedule: G1, G2, G3 and G4 run 600–900 updates with no unsupported path (G1
logs only the type-30 enemy_load note); throws kill zombies and destroy a
generator; a debug potion override shows the blast growing to radius ≈ 20 and
killing nearby zombies and a generator.

## Not ported (known)

Critters (GENERAL/GOLEM/GAR/DEATH), shield reflection,
blocked08/10/20, move_logic 03/08–13/15/16/19–22/24–28 except those listed, bosses, power/turbo melee (event bits 0xf0), dash attack 0x3e, turbo powers, shields, transporters' destinations,
wave exits (SaveStage), camera logic, save/load, co-op.
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
