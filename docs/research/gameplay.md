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
blocked08/10/20, move_logic 03/08–13/15/16/19–22/24–29 except those listed, bosses, power/turbo melee (event bits 0xf0), dash attack 0x3e, turbo powers, shields, transporters' destinations,
wave exits (SaveStage), camera logic, save/load, co-op.
Realm sweep (level 1 of each realm, 900-update scripted wander, interim
player): B, D, E, H, I, K clean; A stops at DamageItem rock/wall/explosive
barrel, C at move_logic29 and the ActivePlayerMask all-players trigger, F at
the phasing enemy alpha (type 0x18), J at the ActivePlayerMask trigger.
