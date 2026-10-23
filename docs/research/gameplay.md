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
  re-aim at π/4 + n·π/12 after four turns or a >π/2 miss), move_logic23
  0x13da78 for logics 0x11 and 0x17 (stationary, ranged action 0x18 within
  +0x314 and ±10 height after its +0x334 cooldown), move_logic30
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
0x1e/0x1f/3). All are ported. do_ai's jump table (0x2a7a30, read entry by
entry 2026-10-22) is not one function per logic: 0x10 and 0x1a run
move_logic16, 0x11 and 0x17 move_logic23, 0x19 kill_enemy; move_logic17
0x13eba8 and move_logic26 0x13d598 have no caller or pointer anywhere in the
ELF (the port had run 0x11 as move_logic17 and 0x1a as move_logic26;
trace-A1-statue-v2: a placed logic-0x11 grunt's +0x334 counts down by
vb_elapsed). Logics 8, 9, 0xb, 0xc, 0xd, 0x14, 0x15, 0x16
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
  collision uses the full matrix. Rotated nodes compose through GetWorldMat's
  MulMat4 chain (collision.md); no rotator has been traced yet.
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
  get_face_ang2), CurWorld 10 generator sounds 0x2c0024 /
  0x2c0025, Death's touch (do_enemy_attack type 0x1e, AddExp kind -2 =
  amount × LevelDeltaExp × 0.01, StartDeathFX) and dissolve, the time-stop
  and trigger-camera enemy paths of do_enemies, MaxStopTime.
- Tower hub (game/tower.rs, level L1, CurWorld 0xd): EnterTower opens the
  doors for delivered gargoyle items / crystals (ActivateSpecialTrigger) and
  HideExits disables exits to unreached levels (wave byte bit n−1) and to
  closed realms (WorldOpen: Temple/Hell/Battle need shards, Hell all runes).
  PlayersSetWaveFinished records each completed level. With the fixture save
  (G1 in progress) the tower offers the first level of A, B, C, D, G, I, J
  and K. Since ported (tower.rs, pselect.rs, hud.md): Sumner as a select-screen
  character, TowerCheckMessages' speeches, captions and cameras, the
  shard/rune display and the exit reveals. The client's
  campaign mode (`run_play.py --campaign`) starts in the tower; exits and the
  boss TowerWave pick the next level by (world, level).
- Camera: superseded — the gameplay camera is the ported StdCam (camera.md),
  whose culling view gates placed-enemy spawns and item activation.
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
- With ProcessItems in the replay the generator spawns run natively, and
  the traces then exposed (all fixed 2026-10-22): InitAnim's AnimInfo
  mirrors at a spawn (+0x90 the tree's sequence count — type 13 levels
  1/2/3 hold 9/12/10, type 12 holds 7 — +0x92, +0x94 the frame count);
  InitAnimInfo 0x110750 zeroes the AnimInfo, so a spawn's previous sequence
  (+0xa0) is 0; init_enemy's GetObjWorldMat of SetEnemyObj's new node makes
  the record matrix the identity (a reused slot kept the dead enemy's rows);
  check_enemy_pos sets only the record's y to the floor — the probe it
  passes to EnemyCollidePlayer / EnemyCollideEnemy / ItemCollide keeps its
  own y (the port had floored it, so one spawn chose the opposite
  direction); and the ±0.0 matrix signs: do_enemies' state 1/7 opens with
  GetObjWorldMat (the record matrix from the enemy node, which
  UpdateObjWorldMat stored at the end of the previous update with
  UnparentMatrix's −0.0 → +0.0) and closes with UpdateObjWorldMat — unless
  enemy_ai_control returned nonzero (0x13b8f8 jumps to the common tail,
  skipping the skin effects too); an idle enemy (no do_enemy_move) keeps
  the node's +0.0. Game::enemy_node models the node (players already had
  player_node). Results: combat 270/270, fight 267/270
  (replay-items-combat-g1-v4.json, replay-items-fight-g1-v10.json; the
  three left are ProcessEffects' kills at 430/512 and the generator
  destruction's +0x2a4 link clearing at 474). Without ProcessItems the
  fight trace replays 253/270 (replay-enemies-fight-g1-v6.json),
  trace-enemies-g1-v4 179/180.

## Whole-update replay (2026-10-22, actual disc)

- Probe additions (reference_enemies.py): the Effects array (64 × 0xf0,
  delta-coded) with NumEffects, SfxId and each live slot's MB node (local
  matrix at node +0x00, scale vector +0x40), sfx_ene_bloodidx, the WORLDS
  world-animation entries (file base found below WorldBase; +0x6c, 16 bytes
  each) and the WORLDS object records (60 bytes, delta-coded). Traces
  trace-fight-g1-v4..v6 and trace-combat-g1-v3 re-recorded from the same
  fixture and schedules are byte-identical in their enemy samples.
- game_run.py --enemy-replay TRACE --frame-replay: each update starts from
  the before state — items, enemies, player 0 (as the player replay seeds
  it), the StdCam view, effects (records in the native convention, nodes
  as EffectNode: rows, translation, scale y with x/z as stretch), world
  animation entries and object words — and runs world_update's
  DoWorldAnimation, SetupDynGrid, ProcessItems, do_enemies, do_players and
  ProcessEffects with the rand seed solved over the chain; enemies, items,
  player 0, effects and the animation entries are compared afterwards.
  Effect words skipped: the node/tree/AnimInfo links, the tree's AnimInfo
  clock (+0x28..+0x57 except the loop word +0x50), +0xd4 streak and +0xe4
  hit-target node; the skin texture word (enemy +0x204, player +0x7c0) is
  a texture handle in the original.
- Results: trace-fight-g1-v6 (replay-frame-fight-g1-v7.json) enemies
  270/270, effects 270/270, world animation 270/270, player 0 269/270
  (+0x894 at step 194, open); trace-combat-g1-v3
  (replay-frame-combat-g1-v2.json) enemies 270/270, effects 270/270,
  player 0 268/270 (+0x894 at 194, +0x197c at 496). Throws, the missiles'
  flight, the two kills (state 8, death skin), kill credit and experience,
  and the generator's destruction all match.
- Fixed by these traces: SetSkinFX stores the texture and repeat words as
  integers (sw; ProcessSkinFX decrements with addiu); ProcessEffects'
  spin (PitchMat3(−x · SPF), YawMat3(y · SPF), RollMat3(z · SPF); the port
  had treated it as presentation) and CreateDirMatrix for spinless effects,
  whose "up" is Cam[0]'s row 2 (0x23e520, the camera's view direction) —
  the missile rows roll toward the camera; StartMissile's SfxSetPhysics
  YawMat3(atan2f) through libm and SfxSetStreak's record words (flag
  0x40000, +0xd8 the class record's +0x17c, +0xdc the radius);
  StartGenHitFx on a generator's level change (GenHitEffects /
  GenDieEffects: InitEffects registers each loaded enemy type's GENHIT /
  GENDIE trees first among the custom effects, 0x61.., so the client now
  shows the generator hit and death effects); AddEnemyHitCol's blood
  alternation needs sfx_ene_bloodidx as an input; and GauntWorldInit's
  world_update before AddItemInstList, whose DoWorldAnimation poses the
  moving floors that PlaceItem parents items on (items.md).
- Varied trace (trace-varied-g1-v2: the key pickup, throws that break a
  barrel, Square's class special 0x63, Circle + Up, Triangle; 175
  updates; pad keys K/J/I/L = Cross/Square/Triangle/Circle): enemies,
  player 0, effects and world animation all 175/175
  (replay-frame-varied-g1-v8.json). Found and fixed on the way:
  ResolvePlyrData 0x14d978 (LoadPlyrData's second call in load_player)
  was not ported — every class SFXX entry's +8 effect header and +0xc
  sound stayed the disc words, so class specials (SOR_PWRA1 …) started no
  effect and played no sound; now the custom effects are registered after
  WeapThrowFx (fixture: 117.. after 113..116) and the sounds resolved by
  name; the class SFX VU0 transforms (MulVecMat3 / MulVecMat4 in DoPlyrSfx,
  DoPlyrSfxSub and PlyrSfxDoDamageSub) use the VU adder; ProcessItems'
  opened-chest loop compares each enemy's +0x354 (find_closest_chest's
  slot) with the enemy index, as the original does; headless runs load
  TEXT/ENGLISH.ROM so NewHelp's logic (seen marks, help pause) runs, and
  the replays seed the help globals; player-parented effect nodes are
  compared by their local matrix.
- Tower trace (trace-tower-v1: savestate slot 2, the new game in the
  Tower, recorded with reference_trace.py --slot 2; world L level 1, no
  items seed): enemies, player 0, effects and world animation 170/170
  (replay-frame-tower-v11.json). The first divergence (v4: player 0 at
  step 250, +0x54/+0x898) came from three faults, all fixed:
  ProcessItemWobjs' fade branch sets "moving" (s3) while the transparency
  changes (0x154af0 fading out below 0xf8, 0x154b30 fading in while
  non-zero), so a fading object gets +0x35 = 1, +0x10 |= 0x20000000 and
  +0x16 |= 0x10 — the port had moving = false, and the player stood on
  L1 object 2903 (child of fade object 2902) during the 16 updates the
  original excludes it (exclusion 1 & FloorCollide's mask 1). Collision
  reads the object words live, but native copied them into the collision
  world only after DoWorldAnimation; ProcessItemWobjs (at its end), the
  rotator and the Tower trigger chain now sync the objects they write.
  PlayerCollideFloor's two re-queries pass player_floor_col (0x2341a0)
  and WorldCollide fills the struct only on a hit, so PlayerNewFloor
  sees the re-query's object and normal; the port kept the first query's.
  At step 252 that first query ties: static L1FLOOR2#1 (3260, grid pass
  first) and the dynamic 2903 above it both give the edge metric
  1.4521765 (10000 × the squared edge distance; WorldObjCollide's
  compare is strict, c.lt.s at 0x11dd84/0x11ddc0), and the re-query by
  distance picks 2903. The node matrices of 2902/2903 read from the
  savestate are the native ones bit for bit. With the same change,
  WorldObjGetAllFlags and the wall/floor flag tests read the run-time
  +0x10 words (the moving-floor gate, 0x0c000000 with 0x20000000, can
  now fire) and the floor normal of a matrix object is turned by its world
  matrix (WorldVector, as WorldCollide's tail does). G1 effects of the
  same fixes: trace-fight-g1-v6 player 0 270/270 (the step-194 +0x894
  diff is gone; replay-frame-fight-g1-v10.json), trace-combat-g1-v3
  269/270 (+0x197c at 496 open; replay-frame-combat-g1-v4.json), player
  replays combat 270/270 (replay-player-combat-g1-v16.json) and fight
  267/270 (kill credit from enemy deaths, which the player replay does not
  run; replay-player-fight-g1-v11.json).
- Items in the whole-update replays (2026-10-22, report field
  differing_item_slots counts every update; item_diffs keeps 60 per
  update): trace-fight-g1-v6 270/270 and trace-varied-g1-v2 175/175 for
  every item word (replay-frame-fight-g1-r11, replay-frame-varied-g1-r14),
  the Tower 92/170 with only item 160's +0xfe audio handle left
  (replay-frame-tower-r11, run with --load-players 0). Fixed on the way
  (items.md): DoAnimation's reset-byte handshake for item trees with a
  type-1 node; AddItemInstList's ProcessItemWobjs before PlaceItem;
  WorldDynCollide's cached CreateDynobjGrid poses; the load-time
  world_update gate for a select-screen entry; GetWorldMat and the recursive
  UnparentMatrix as chains over the nodes' locals and the world root nodes;
  kind 13's FindWorldAnimNode node and its anchor; SetItemGeo's
  InitAnimInfo + AnimateATree start of the clock. world_update now returns at
  once while paused or in help (DoWorldAnimation, SetupDynGrid and
  CreateDynobjGrid first, then the boss, quest, stage-start, lightning and
  end-of-stage parts), sets start_enemyinfo every update (start_of_stage == 0
  or LEVL +0x48), and do_auto_exit runs before it (game_main).
- Tower crystal pickup (trace-tower-crystals-v1: slot 2, idle 30, Up 70,
  Left 130 — blocked near x 6.6 —, Up 100, Cross 10; 400 vblanks): the
  reference grabs crystal 63 (GEMORANGE) at step 64 (+0xd8 |= 0x100) and
  deletes it at 72. Native whole-update replay
  (replay-frame-tower-crystals-v1.json, --load-players 0): enemies, player
  0, effects and world animation 192/192; items 126/192, the rest the sound
  items 160/161's +0xfe audio handles. Reference screenshots of the pickup
  (private/reports/ref-tower-crystal-up40/60/120.png: Up held from the
  savestate): the 1/15 counter at once, the "Crystal" got-it panel over
  player 1's box by 120 vblanks, Sumner on his pedestal. Several quick
  pickups stack do_got_it panels in the reference too (three "Crystal"
  labels at 300 vblanks, two at 350: ref-tower-crystals-190/250/300/350-v1
  from slot 2 with Left 60, Up 50, Up+Left 40, Up), as the native client
  draws them.
- GUI walkthrough (2026-10-22, gdl_play on the private Xvfb, keys through
  xdotool on the client's own window): title → select (New, the default
  name, Sorceress) → the Tower's WelcomeMessage pages (triangle) → the
  crystal camera → all fifteen crystals (UnlockLevel: "You now have enough
  Crystals to enter the Forsaken Province", the set marked −1) → the "15"
  door → the level circle (portal I open) → the realm map → the level movie
  → G1 combat (kills, experience, "Collect potion before using magic").
  Reference frames at matching points: the new game's start, the podium
  (ref-tower-route-podium-v2), Sumner's hint pad (the menu opens after
  SumnerHintsActivate's 2 s and does not reopen while the player stays on
  the pad: ref-tower-hint-open-v1, ref-tower-hint-reopen-v2; the native
  reopens only after the player leaves and returns). Unexplained: in one
  earlier session one crystal could not be collected (fourteen counted, the
  glow left in place); a second session collected all fifteen with the F2
  item dump (DEBUG) and did not reproduce it.
- trace-combat-g1-v4: trace-combat-g1-v3's schedule re-recorded with the
  current probe (world animations, objects, help globals); all 541 enemy
  and player samples are byte-identical to v3. Whole-update replay
  (replay-frame-combat-g1-r16.json): enemies, items, player 0, effects and
  world animation 270/270. The old +0x197c diff at step 496 was a replay
  artifact: +0x197e is HELP_SEEN + 0x6e (PlayerMotion's NewHelp(0x6e) at a
  full turbo meter with 15 visible enemies); the first solve attempt showed
  it, and the retry kept the help box state while the player record was
  reseeded, so NewHelp's priority test refused it. The replay now restores
  the help state per attempt when a trace lacks the help globals.
- A1 (the Castle's Courtyard, a new game; slot 4 from reference_warp.py,
  trace-a1-newgame-v2: the stage start, Up, Up+Left, two throws; 540
  vblanks, RuneDispTimer now probed): whole-update replay
  (replay-frame-a1-v6.json) 270/270 in every part — enemies, items, player
  0, effects, world animation. Fixed on the way: the replay seeds cam_wait
  and start_of_stage from the before sample (main's camera ends the stage
  start after game_main) and recomputes start_enemyinfo; init_next_level's
  RuneDispTimer = 300 (outside world 0xc) and the rune pickup's were missing
  (ShowRuneStones then marks player +0x93e bit 2 and show_crystals waits);
  newly attached enemy trees take InitAnimInfo's +0xac 1.0 and +0xbc node
  count; InitAnim(START) only after a successful generate_enemy placement
  (LAB_001397cc; a failed spawn keeps sequence 0); ProcessItems' animated
  item timers use GetActiveTime with the game's rand (the item world had its
  own generator, shifting every later draw) and a base of (frames × period
  scale + 0.5) << 1 (the port multiplied by 1.0) — the A1 traps (SPIKES,
  TENTACLE) and the LOW food item.
- B1 (the Mountain's first level; slot 3, trace-b1-newgame-v2: Up,
  Up+Right, two throws, Left; 500 vblanks, critters probed — B1 has none):
  whole-update replay (replay-frame-b1-v9.json) 250/250 in every part. The
  first divergences and their causes, in order:
  - step 276, enemy 4's floor link: EnemyCollideFloor's second FloorCollide
    (at position + move, when the step exceeds speed × 0.1) reassigns the
    object the rest of the function reads and returns (0x143120: iVar2);
    the port returned the first query's B1RAMP#6 at the seam with
    B1FLOOR00#04.
  - step 278, the poison barrel's blast and the KEY: SfxSkipItem's kinds 1
    and 2 (and 2/0x2b) end in the shared tail 0x17b1dc, which skips an
    indestructible item (+0xe3 = −1) unless the damage flags have 0x600.
  - step 366, the barrel's end (+0xdc 1 → 2, deleted, help 0x2d): a replay
    artifact — the first solve attempt deleted the barrel and the retry
    lost its clock. The replay now restores the native item side state
    (floor parent, sound node/anchor, tree, clock, geometry flag, first empty
    slot) per attempt; DeleteItem keeps the record's +0x84 AnimInfo as the
    clock left it.
  - step 402, the melee kill of enemy 1 (+4 experience): PlayerMotion
    passes uStack_118 (sp+0x128: PlayerCollideEnemies' contact, then the
    targeting stage's result) and piStack_114 into both PlayerGetTarget
    calls (0x14988c, 0x14bb1c); the port passed −1, so the targeting
    stage's generator item won the melee query.
  - step 404, +0x3a4 of the dying enemy: do_enemies' state 8 runs
    DoEnemyAction only with a tree (+0x80) and not for BossType, then sets
    +0x3a4 = 0 when +0xe0 ≠ +0xe4, else −1 (0x13bad4..0x13bb10).
- K2 (the ship — SKY's LEVL record 0 —; slot 7, trace-k2-newgame-v5: the
  stage start, Down, two throws, Up; 420 vblanks with CritterInsts, the
  critter globals and each live critter's node, parent and node table):
  whole-update replay (replay-frame-k2-v10.json) enemies, player 0,
  effects, world animation and critters 210/210; items 206/210, the four
  S_BARROLL loop sounds' +0xfe (the audio driver's first AudioLOOP return
  is negative, 0xffdc, then a handle; native stores 1). The General
  (CritterETypes[8][0]) stands on the deck; the player's PlayerGetTarget
  found it (+0x8e4 bit 0x10), which the replay could not show before
  critters were seeded. Fixed (research/critters.md): ProcessCritter's
  GetWorldMat of the +0x70 node and its closing CopyMat4 + UnparentMatrix,
  CritterCollideWorld's floor re-parenting, tree nodes composed through the
  node and the floor chain, CritterLookAtPlayer/NodeLookAtPos, the reset
  handshake on critter clocks, and CritterNodePlayerCollide's knockback
  direction (t = a + b, then t + t at 0x19bd14 — the decompiler shows
  a + b + a + b).
- G1 from its new-game capture (slot 6, trace-g1-newgame-v1: the stage
  start, Up, Up+Left, two throws, Left; 500 vblanks): whole-update replay
  (replay-frame-g1new-v2.json) 250/250 in every part. Step 364 was a
  replay ambiguity, not a port error: the solved seed fixes the number of
  rand calls, not their order. Two generators spawned into slots 8 and 9
  (format_brain, then RandInt(8) for the first gendir direction, each);
  with the native's 5-call seed the first spawn faced away from player 0
  (orient_spawn: the generator's +0x100 plus the direction's +0x26c), its
  find_closest_player cone test failed and move_logic_wander's
  format_brain drew the fifth call — a self-consistent solution. The
  reference's 4-call seed puts the spawn at gendir 4 (π/4), it sees the
  player and keeps logic 7. The replay now tries neighbouring call counts
  when a solved update still differs and keeps the closest
  (results "alt_seed").
- B6 (the dragon's arena, a boss level; slot 8 — reached from B1 by
  writing player 0 onto the exit and skip_wave = 0x10205, do_exit's wave
  skip; trace-b6-boss-v3: idle, Up, two throws at the dragon, Left, Up; 500
  vblanks with the window eye and Cam[0] probed): whole-update replay
  (replay-frame-b6-v7.json) 250/250 in every part — enemies, items, player
  0, effects, world animation and the dragon (CritterBossAI, BossCritInst,
  BossType 0x22). Fixed on the way:
  - main's camera dispatch sets newcam_mode 0 when BossCritInst is set (2
    under StdCamUpdate); ItemVisible tests the camera target's distance
    only in modes 1 and 2, so with a boss the fire vents (FLAMEV) stay
    visible (Game::newcam_mode; the native had 2 always). The replay now
    culls with the traced MB window matrix (rows +0x70..+0x9c, eye +0xa0),
    which the boss camera writes.
  - ProcessEffects' CreateDirMatrix up is row 2 of Cam[0]'s matrix
    (0x23e520 = Cam + 0x30), which CamUpdate copies from the StdCam and the
    boss camera leaves ((0, −sin 1, −cos 1) in B6); the port used the
    window row, equal under StdCam only.
  - StartFXTree (0x17d7b0) writes +0x50 (the AnimInfo loop word) only on
    its positive-time and zero-frame paths; otherwise InitAnim's value, the
    first sequence's repeat flag, stays (the dragon's header 102 loops).
  - With the camera state probed (trace-b6-boss-v4) the frame replay also
    runs main's camera dispatch after each update: the boss camera 250/250
    after porting CameraLimitPlayerDpos's boss branch (camera.md "Boss
    camera"). The same stage on G1's new game (trace-g1-newgame-v4, with
    world_update's stage-start block) checks the StdCam through the
    stage-start zoom: 250/250 in every part.
- G5 (the lich's crypt; staged capture w7l5-boss-v1 from slot 6 through
  G1's exit with skip_wave 0x10704; trace-g5-boss-v1: idle, Up toward the
  lich, two throws, Up, Left; the boss activates at ~step 200): whole-update
  replay (replay-frame-g5-v2.json) 250/250 in every part, camera included.
  The one divergence was the lich's attack effect (header 99) at step 166:
  SFX attached to a critter node keep their offset as a local translation
  and follow the node (critters.md); the port had spawned them at the world
  position.
- The other boss levels (2026-10-22, actual disc). Each was reached from slot
  3 (B1's new game) by writing player 0 onto B1's exit and skip_wave =
  0x10000 + the boss wave (C5 0x304, D5 0x404, I5 0x904, J5 0xa05, K5 0xb04,
  E2 0x501, F2 0x601, H4 0x803; reference_warp.py --until-wave stops when
  that wave is in play) and saved through the staging slot 9 as
  w<world>l<level>-boss-v1.p2s. trace-<level>-boss-v1 (E2: v2, which adds
  DoRockFly/DidRockFly): 500 vblanks, idle, Up, two throws, Up, Left, idle.
  Whole-update replays (replay-frame-<level>-r29.json) are 250/250 in every
  part, camera included, for the djinn (C5, 0x24, active from step 170), the
  wraith (J5, 0x28, from 162), the pboss (K5, 0x26, from 93), Skorne 1 (E2,
  0x2a, from 75), Skorne 2 (F2, 0x2b, from 39) and Garm (H4, 0x2c, from 64).
  In D5 (drider, 0x25) and I5 (yeti, 0x27) BossActive stays 0 for the whole
  trace, so only the boss before activation is compared: D5 250/250, I5
  250/250 apart from items 249/250 (S_LWATERFALY's +0xfe audio handle, as
  in the Tower and K2). Activation traces: trace-D5-boss-v2 (Up+Right
  toward the drider, active from step 145) and trace-I5-boss-v2 (from the
  warped w9l5-boss-v2, 62 units from the yeti, active from step 83): both
  250/250 in every part (replay-frame-<level>-r31.json). A5 (the chimera,
  0x23, three heads as child instances; trace-A5-boss-v2, active from step
  281): 250/250 in every part, critters and camera included. Fixed on the
  way:
  - PlayerMotion passes one vector as both `to` and `out` to its first
    PlayerCollideItems (0x148924..0x148930), so each push-out moves the point
    the following items are tested at (E2: Skorne's rocks under the +50 grid
    radius of BossType 0x2a/0x2b).
  - Falling rocks (kind 10, subtypes 0x28/0x31/0x33/0x34/0x35) call
    UpdateObjWorldMat only: the node takes the record matrix and +0x50/+0x60
    follow at the next update's GetObjWorldMat, an update behind the rock.
  - E2's rock fall is gated by DoRockFly/DidRockFly, now probed and seeded.
  - SfxSetPhysics' YawMat3 turns an effect node's local matrix; under a
    critter node that is the local rows, and ProcessEffects writes a moved
    node back with CopyMat4 + UnparentMatrix(node, node +0x74) (H4 step
    484: Garm's header 102 flies along its local z under Garm's tree root;
    critters.md).
  - Object nodes compose through every ancestor's node (CreateWorldNode
    parents each node under its parent object's, with the file position),
    and a parented wobj's node takes its item-driven y (A5: the elevator
    under A5GEOMETRY's y −0.0703; the chimera's floor was 9/128 high).
  - NewWorldObject clears 0x2/0x8/0x10/0x20 on objects with 0x100, and the
    static grid test reads the live +0x10 (I5: a thrown weapon passed
    through I5#35; collision.md).
  - The heads: CritterCopyAnim's AnimateATreeFrame runs AnimateNode, whose
    DoAnimation handshake leaves the reset byte 2; CritterLookAtPlayer turns
    a head's nodes with a null move (the replay's look-at seed skipped
    heads, which CritterCopyAnim leaves at move −1).
  - reference_enemies.py reads node tables of up to 256 entries (the
    chimera's tree has 176; its heads share the root's table).
- Statues (2026-10-22, actual disc). w1l1-statue-v1.p2s: slot 4 (A1)
  with player 0 written beside the gargoyle statue (item 134 at (−75, 0.1,
  32.7); written to (−75, 0, 42.7), a WARPED capture, 250 vblanks);
  trace-A1-statue-v2: Down+Left past the statue, two throws, idle. The
  contact sets +0xf8 bit 0 (step 57), the statue marks +0xd8 bit 0 and asks
  for clip 1 (+0xde, step 59), the awakening clip's switch sets +0xda = 170
  (step 61), and once it runs out (step 231) CritterNewInst(7) spawns the
  gargoyle and the item is deleted. Whole-update replay
  (replay-frame-A1statue-r32.json): 250/250 in every part, the spawned
  gargoyle included. Fixed on the way:
  - SetItem gives statues their tree through SetItemGeo (GAR_STATUE in the
    model of CritterTypeLoaded(7, 0); GOL_STATUE in enemy_anim[0x1d] or the
    golem critter's model; DEATHSTATUE1/2 in enemy_anim[0x1e]) — the port
    had none, so the gargoyle spawned at once (items.md).
  - CritterInitGeo's AtreeInit (InitAnimInfo + InitAnim(0): +0x90 the clip
    count, +0xa0 0, +0xac 1.0, +0xbc the node count) and
    CritterAddHealthMeter's GMETER tree (TYPE +0x5c bit 0x800; AnimInfo at
    +0x48c; its links and RED_FILLE node are guest pointers) for spawned
    critters; +0x6c is MulVec4Mat3's VU0 register residue (dest xyz).
  - do_ai's dispatch (above) and init_enemy's final AnimateATree(+0x80, 0,
    2) (a placed grunt without a START sequence ends its spawn update with
    reset byte 0).
  - DamageItem's generator stage change: ItemGenDesc ("GEN_<prefix><level>")
    then AtreeMatchAnyHeader → ReplaceItemAtreeHeader, else
    ItemFindMBObjectL1 → MBSetObject, else DeleteItem(item, 0): a destroyed
    LOW generator has no GEN_LOW0, so it is deleted before +0xd8 &= 0xfffe
    (0xfffe in the record; the port kept the generator).
  - Critter files load in EnemyType slot order (LoadGauntWorldStart), which
    orders their custom effect headers (A1: golem, general, gargoyle; the
    tools sorted them).
  - The replay rebuilds pbCalcMatrices for the seeded view (MBWorldToScreen
    reads +0x2d0, which its lazy refresh leaves; camera.md) and seeds enemy
    clocks for the lettered trees (EnemyDesc levels 4..7: A1's placed
    logic-23 grunt).
  Golem statue (w1l1-golem-v2.p2s, player 0 written to (121.29, 25.86,
  98.69) beside item 71; trace-A1-golem-v2, a four-direction walk): contact
  at step 53, the golem (CritterNewInst(3), GOL_STATUE from the golem
  critter's model — MONSTERS/GOLEM is absent) at step 117, CritterGolemAI
  after: 240/240 in every part with no further change. Statue items carry a
  player count (+0xe0; +0xe1 1 hidden): A1's golem 67 and the General 103 by
  the start need 2 and 4 players, so single-player traces never see them.
  Death statue 50 (A1): fenced off from both sides tried (w1l1-death-v1/v2,
  trace-A1-death-v1/v2, 260/260 each without activating it).
  - PlayerGetTarget's gate test (the +0x880 item of kind 7) after a target
    found by the search reads fStack_f0, which only a given target writes:
    stack residue left at PlayerMotion's sp − 0xf0, usually by
    PlayerCollideFloor's FloorCollide (its saved ra, a denormal, and the
    zero upper half). HYPOTHESIS: the world origin as the target position
    (trace-A1-death-v1 steps 105..113 match; the port had stopped with
    Unsupported, cutting player 0's update short near gates).
- A boss death (2026-10-22). w2l6-bosskill-v1.p2s: slot 8 (B6) with the
  dragon's health written to 10 (reference_warp.py --critter-health; a
  WARPED capture); trace-B6-bosskill-v1: the B6 walk and throws. The first
  hit kills it (step 203, state 1), its death move's DAMG type-9 action
  spews five COIN_SILVER and the arena blast at step 409 (with the LAVA00
  death skin). Whole-update replay (replay-frame-B6kill-r34.json): 250/250
  in every part. Fixed on the way: InitEffects' per-level headers
  (rendering.md), CritterDoTexmodNode's skin, CritterInitSfx's texture
  words; the replay now runs world_update's DoGoodWizard / ProcessSpewItems
  and QuestItemState parts, restores the spew list per retry and drops items
  a retry appended. The rest of the completion in chained captures
  (w2l6-bosskill-v2/v3/v4, each the previous capture after the same
  inputs; the state at each link equals the trace's last sample):
  trace-B6-goodwiz-v2 (BossDeath's StartGoodWizard at step 43, GoodWizard
  1 → 2 → 3 (300 vblanks) → 4 → 5), -v3 (the speech: 5 → 6 → 7 → 8, then 10
  with auto_exit_timer = 600, coins being left), -v4 (the timer to
  EXIT_SKIN_TIME, state 11 and the players' exit skins at 451, game_main's
  level end at 485, mode 0x4012): 300/300, 300/300 and 242/242 in every
  part, the good wizard's state included (the replay ends at the mode
  change; game_main's level end is outside it). Fixed on the way:
  BossDeath's BOSSKEY effect (InitCustomEffect from item_anim2,
  StartFXSub(0, key, position, 0x80000040), SfxSetMorph(30, key, BOSSKEY2));
  the replay seeds the good wizard, the spew list (SpewItems), items created
  before the trace, the freed boss instance BossCamBossCalc still reads, and
  runs main's AudioQueUpdate (vb_counter seeded) and game_main's
  do_auto_exit per update, restoring the custom effects and the voice queue
  per retry.

### The G1 exit (2026-10-22/23)

Capture w7l1-exit-v1 (slot 9): poison-fields-original with player 0 WARPED
onto G1's exit node (warp-G1-exit-v1; pos write, labelled). trace-G1-exit-v1
records 480 vblanks without input from it: player_collis_exit, DoExit,
do_exit's countdown (+0x1ca from 0x32), the sink, the spin and SetSkinFX's
exit skin, through ExitState. The whole-update replay (`--frame-replay`)
matches 186/186 updates in every part (player, items, effects, world
animation, critters, camera, wizard; replay-frame-G1exit-v5, kept in the
regression set since r41). Fixed on the way: the replay maps the player's
item links +0x880/+0x884/+0x888 from guest pointers to slots; do_exit's
sink (0, −0.12, 0) × vb into +0x50..+0x58 and its spin YawMat3(SPF ×
0x4116cbe5) of +0x20 with +0x93c bit 0; the item AnimInfo loop word
(+0xb8) kept in step with the native clock (process_exit, process_trigger,
attach_item_anim). Not modelled: do_exit's del_target 0x168f78 on state 5.
Two repeats of the same warped exit through the tally to the Tower
(baseline-transition-a/b-v1, equal logs)
are in reference-baseline.md.

### Magic, powerups and transporters (2026-10-23)

WARPED captures from the fixture and A1's new game (reference_warp.py
--pos; warp-*-v1 reports), traced with reference_trace.py and replayed
update by update:

- Potion and magic (w7l1-potion-v2 / trace-G1-potion-v2: onto POT_YEL 367,
  pick it up, Circle): PICK, the cast (action 0x73 → 0x74), the potion
  spent, the blast and its hits — 80/80 for player, effects, enemies,
  world animation (items 79/80: sound item 391's audio handle +0xfe, the
  known residue). w7l1-potion-v1 (POT_YEL 368) shows a multiplayer-only
  potion (+0xe1 = 1) is not collected in single player. Fixed on the way:
  event 0x10000's StartMagicPlayerFX 0x17ee18 (StartFXSub(0, 0x22, zero
  vector, 0, 0x880), parented to the player's +0x6a4 node) is a real
  Effects slot, not presentation; start_magic's PlaceEffectOnFloor after
  StartMagicFX; AddItemHitCol 0x17dcf8 takes its header from the damage
  colour (0x240710 / 0x240728 by flags & 0xf) instead of fixed 2/3.
- Weapon powerup (w7l1-xbow-v1 / trace-G1-xbow-v1: onto XBOWICON 398, then
  two throws): the pickup and the powered throws 90/90 in every part
  (items 89/90: sound item 375's handle).
- Transporter (w1l1-trans-v1 / trace-A1-trans-v1: onto A1's TRANS pad at
  (−70.8, 0.2, 94.8)): DoTransporter carries the player to the linked pad
  (−64.2, 0, 94.7), and the arrival kills the enemy in the way. 180 updates:
  player, items, effects, world animation, critters and camera 180/180,
  enemies 179/180. Fixed: the arrival kill's damage is 9999.0 (0x461c3c00 at
  0x149314), not 10000. Left (first divergence, unknowns.md "Spawn
  direction wall test"): step 304, a generator spawn in slot 23 takes
  direction 1 natively where the original takes 7 — the native
  EnemyWallCollide hits A1FENCE#60 (triangle 789; the end point lies
  0.94 from the plane, inside the triangle) where the original's does not.
