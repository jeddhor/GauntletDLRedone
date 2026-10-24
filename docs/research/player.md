# Record-based player simulation (do_players, PlayerMotion, DoPlayerAction)

Revision: SLUS-20047 (profiles/ps2/slus-20047.json). Exports:
private/reports/ghidra-player-animation (DoPlayerAction), ghidra-player-motion-
events (do_players, PlayerMotion), ghidra-player2, ghidra-controls2/3,
ghidra-gameplay8, ghidra-pmotion1..4 (2026-10-19).

## Modules

| Module | Original functions |
|---|---|
| game/pad.rs | ControlsUpdate 0x104130, ReadControls 0x104b60, PlayerControls 0x105718, CheckSpecials 0x104878, ClearPlayerControl, InitPlayerControls, get_dir/get_button/get_analog_dir/calc_analog_stick/dir_edge |
| game/paction.rs | DoPlayerAction 0x177c28 (both jump tables read from the ELF: 0x2aad40 ×0x96, 0x2aafa0 ×0x66) |
| game/pcontact.rs | GetPlayerDMotion 0x1448e0, combo_player 0x144688, PlayerCollideItems, VerifyItemCol, ModifyPlayerDpos, PlayerCollidePlayers, PushPlayer, PlayerCollideEnemies, PlayerCollideCritters, PlayerWorldDamage, PlayerNewFloor, PlayerCheckMovingFloor, OtherPlayerOnOtherMovingObject, DoTransporter, player_collis_trans, PlayerKnockback, PlayerCheckFloor, DoAutoExit, PlayerUsePowerup |
| game/pmotion.rs | PlayerMotion 0x1480a8 (the reused stages: player_collision's PlayerCollideWalls/Floor, camera.rs NewCamLimitPlayerDpos) |
| game/doplayers.rs | do_players 0x133190, DoPlayerStart, do_weakening, GetObjWorldMat / UpdatePlayerWorldMat (UpdateObjWorldMat, UnparentMatrix, UpdateObjCol/AttnOffset) |

## Layouts (instruction-derived)

- PlayerControl is at 0x2114f0 (symbol, 0x3c × 4): +0 lockout, +4 levels,
  +8 edges, +0xc repeat edges, +0x10 special cooldown, +0x14/+0x18 special,
  +0x1c/+0x20 left angle/magnitude, +0x24/+0x28 right, +0x2c mode (= the
  ReadControls remap set; ControlsUpdate: 2 drives with the right stick;
  PlayerControls: 3 aims with it), +0x30/+0x34/+0x38 (InitPlayerControls 2,1,1;
  PlayerMotion reads +0x34 auto-aim and +0x38 enemy blocking).
- remap 0x2112a8: 10 entries × 4 sets × (mask, require); entry i sets level
  bit 8+i. rep_speed 0x2107b8 (14 words); SpecialData 0x210880 (13 × 200).
- AnimInfo at player +0x94: +0x0e sequence (+0xa2), +0x10 clip frame count
  (InitAnim; +0xa4), +0x12 reset byte (+0xa6: InitAnim 1, DoAnimation turns
  nonzero into 2 when the tree is evaluated, CalcAnimInfo 2 → 0), +0x13
  option bits, +0x28 playback scale (+0xbc), +0x34 loop word (+0xc8), +0x36
  mode bits (+0xca).
- PlayerMotion fields: +0x830/+0x838 push velocity this update (do_players
  decays +0x83c into it, ×0.667), +0x848 knockback velocity, +0x860 request,
  +0x8a8/+0x8ac/+0x8b0/+0x8b4 damage, flags, previous flags, direction
  (PlayerKnockback), +0x8c8 attack type, +0x8cc/+0x8d0 held/pressed attack
  buttons, +0x8d4 attack start time, +0x8d8 animation event word, +0x8dc
  relative aim, +0x8e0 combo counter, +0x8e4 target flags, +0x8e8 turbo cost,
  +0x914/+0x918/+0x91c transporter, +0x928 exit counter, +0x92c "It"
  timer, +0x978 camera limit result, +0x964/+0x968/+0x96c/+0x970 speed,
  turn, control, +0x970 scales (DoPlayerAction).
- Native links: +0x690/+0x694 partner, +0x880 target item, +0x884
  exit/transporter item, +0x888 grabbed item, +0x89c floor object, +0x8a0
  wall object are index + 1 (0 = null). +0x6b4 (load_player's
  AtreeFindMbidxNode of "<+0x698>DUMMY": tree node index + 1 of the
  character's tree 0, when the model package has that object — every
  class's tree holds a "DUMMY" node) feeds the team moves; +0x6b8 (swing
  tree) stays 0.

## The node round trip (observed)

do_players' first loop runs GetObjWorldMat for every active player: the
record matrix +0x20..+0x5c is reset from the player's node, then +0x70/+0x60
are recomputed. UpdatePlayerWorldMat (end of do_players) writes the node only
when status +0x93c bit 1 is set, which PlayerMotion sets for |d|² > 0.0001 or
a facing change > 0.0001. trace-player-g1-v2 shows the consequence: a step
blocked against a wall to 0.0013 units snaps back every update (node x stays
12.255360 while the record shows 12.256683). The node's −0.0 entries are
+0.0 (UnparentMatrix's product). HYPOTHESIS: under PlayerRootNode,
GetWorldMat returns the local matrix unchanged (consistent with all 150
updates); moving-floor parents use the translation-only UnparentMatrix form.

## Verification (actual disc)

- private/reports/trace-player-g1-v2.json: 300 vblanks on the Poison Fields
  fixture (walk Up, throw, idle, walk Right, throw while walking, walk Down
  into a wall, throw, idle), probe tools/python/reference_player.py (player 0
  record, PlayerControl, window, StdCamera, node, 26 globals; addresses from
  the ELF symbol table).
- private/reports/replay-player-g1-v4.json: `game_run.py --replay`, each of
  the 150 game updates starting from the reference's record/controls/node
  and globals, through the native do_players: **150/150 updates equal** over
  every u32 of the 0x2b00-byte record except guest-pointer words (22) and the
  HUD words +0x878/+0x87c (get_screen_pos) and +0x2ae0 (draw_power_meter,
  found by an instruction scan of the ELF).
- Earlier: trace-player-g1-v1 (no node capture) differed in 5 updates; that
  first divergence is what established the node model above.
- Not covered by the trace: melee (no enemy in reach), magic/potions,
  powerups, knockback, transporters, moving floors, team moves, enemies.

## Class effects (game/psfx.rs)

PlyrSfxDoDamage/PlyrSfxDoDamageSub/DoPlyrSfx/DoPlyrSfxSub port the class
special attacks (actions 0x23/0x25/0x3c/0x54/0x56/0x57/0x58/0x5a/99/0x7b)
and the team-move contact effects from the PDATA WAD's DAMG and SFXX
chunks; process_effect applies their damage. HYPOTHESIS: effects parented
to the player's node follow the record matrix (the animated child node's
pose is not applied). Not trace-checked yet.

## Pad buttons (measured 2026-10-22)

Reference: slot 2 (tower-newgame-v1), each PCSX2 Pad1 binding held 6
vblanks with release gaps (reference_trace.py --player; private/reports/
trace-buttons-a-v1.json, trace-buttons-b-v1.json). PlayerControl +4 of
player 0 while held (level bit from ControlsUpdate, plus the remap row of
the default set):

| Button (PCSX2 key) | Levels | joyGetStatus entry | Game use |
| --- | --- | --- | --- |
| cross (K) | 0x2000200 | 0xd | attack (DMotion 0xf); menu accept 0x2000000 |
| circle (L) | 0x1000100 | 0xb | magic (DMotion 0x18); menus return 2 |
| triangle (I) | 0x8000800 | 0xc | turbo (edge 0x800 → 0x15); menu back, scroll close 0x8000000 |
| square (J) | 0x4000400 | 0xa | DMotion 0x10 |
| L1 (Q) | 0x102000 | 8 | edge 0x2000 with turbo ≥ 5 → 7 |
| L2 (1) | 0x220000 | 9 | 0x20000: team move (combo_player) |
| R1 (E) | 0x404000 | 0x12 | 0x4000: strafe with the left stick |
| R2 (3) | 0x801000 | 0x13 | edge 0x1000 → 2 |
| select (BackSpace) | 0x80000 | 0x15 | |
| L3 (2), R3 (4) | 0 | — | |

Start (entry 0x14, bit 0x40000) is from ControlsUpdate only (pressing it
opens the pause menu). gdl_play's gamepad buttons and its keyboard (K/L/I/J
as PCSX2's bindings) follow this table; the earlier HYPOTHESIS mapping had
triangle and square swapped and every shoulder button wrong.

Control resets (ELF callers): EnablePlayerControls 0x105e50 =
ResetPlayerControls (disable_player_controls 0, every PlayerControl and
the repeat counters cleared) + ClearAllPlayerControls(4) (lockout: four
PlayerControls passes see nothing); DisablePlayerControls 0x105e20 the
same with the flag 1 (Sumner's speech starts, TriggerCameraActivate;
Enable: SumnerSpeechEnd, TriggerCamUpdate). start_optmenu clears with 2,
GeneralMenu with 1, ServeFireScroll with 2 per frame. The pause menu
(DoOptions) reads PlayerControl edges itself; gdl_play keeps PlayerControls
running while paused so a closing press is an old level on resume.

## Numeric domain

EE FPU and VU arithmetic flush subnormal results to signed zero and read
subnormal operands as zero (the reference's DenormalsAreZero sets DAZ and
FTZ; upstream claim from the pinned PCSX2 source). A knockback velocity
decaying by ×0.667 per update reaches that range after a few hundred
updates.

## Legend items (boss endings)

GauntWorldInit sets +0x80c = 1 and QuestItemState 1 for the first active
player holding the world's legend item (PlayerHasLegendItem, class word
+0xcf4). world_update's machine (quest_item_update) moves 2 → 3 after 1 or
3 s, 4 → 5, and 5 → 6 after 29 s; do_players (2/3) spends the item
(PlayerUseLegendItem, +0x80c 2); PlayerMotion shows the weapon glow
(QuestWeaponSfx), the combo effect, forces the use action (99 / 0x6b /
0x73 by BossType) and on its event fires the finale per BossType (effects,
CritterDamage, homing SfxSetHitTarget at the boss node); ProcessEffects'
boss hit while 2/3 sets the per-boss timers. Exercised with the debug
options legend=1 quest=N in all boss arenas (no unported path; not
trace-checked). HYPOTHESIS: BossType 0x23's target is the lowest-index
first child under the chained critter's +0xd4 node.

## Team moves (2026-10-22)

game/teammove.rs (from instructions): combo_player 0x144688 (turbo ≥ 50,
the initiator's +0x6b4 node, the partner within 5 and 45° ahead on a floor
object, 3 in height) on PlayerControl level 0x20000 (remap row 9: raw
0x200000 = joyGetStatus entry 9, R2) gives GetPlayerDMotion code 0x16 and
+0x694; PlayerMotion 0x14c448 links the pair once the action is 0x58/0x59
(both +0x690, the partner's +0x93c |= 0x10, the turbo cost +0x8e8 spent)
and runs the class switch (jump table 0x2a7c20): warrior carries the
partner on its DUMMY node until frame 30 of 0x58 then throws it (status
0x40, +0x1d2 = 0xf0 until the unlink); valkyrie/archer turn the partner
away (PlayerFacePlayer 0x144548 with π) and ride its DUMMY node during
0x58/0x59 (released with the kept position); dwarf rides like them on 0x58
(status 0x80, partner +0x8d4 = Time, +0x1d2 = 0xf0) and lets go after the
throw timer; wizard/knight/sorceress/jester carry the partner during
0x58/0x59 (the jester releases at once). PlayerSetParent 0x136c58 /
PlayerUnsetParent 0x136d70: the node becomes identity under the other
player's DUMMY node (GetObjWorldMat → MulMat4(node world, node)) and
returns to PlayerRootNode with the position (kept at +0xf0 when restoring).
The DUMMY node's world is the other player's record matrix under its tree
pose at the clock's sequence/frame (HYPOTHESIS: the original reads the
matrix the last MBEndFrame left; no blend). Headless (actual disc, two
Sorceresses in G1, game-run front=1 turbo=100, R2):
game-run-team-G1-sor-v1.json — the partner plays 0x92 parented for the
90 frames of 0x58 and is released with its kept position; no unported
path. Other classes not exercised; no reference comparison.

## Unported (Unsupported when reached)

(2026-10-22 review.) The paths this list named earlier are ported: the
thunder hammer / breath / familiar events (PlayerMotion 0x2000000,
0x1000000, 0x10000000), the select/join states through the select screen,
inactivate_player, setup_player_display, PlayerProcessFamiliar/Scale,
update_player_milestone with AddLocatorInstList's kind-5 milestones, the
team moves. What remains Unsupported in the player code are guards for
states the disc does not reach or that would read garbage in the original
(a sequence without clip timing, QuestItemState 2 without BossCritInst, a
team-move wall bounce without a normal, DAMG/SFXX chain cycles, effects
parented to effects, get_player_pos beside a player without a floor
record, the debug menu's Next/PrevWorldLevel). Floor parents with rotation
use MulMat4 with InvertMat4's rigid inverse. PlayerGetTarget's gate test
after a searched target, which reads stack residue, is no longer Unsupported:
it takes the world origin (HYPOTHESIS, gameplay.md "Statues").

## Power melee and magic modes (2026-10-21)

- PlayerMotion's melee consumption: event bits 0xf0 (the turbo melee)
  triple the damage, add flag 0x20 and take the turbo cost +0x8e8 from
  +0x800 (then clear it) before the usual reach test.
- start_magic 0x134408 modes: 0 the potion blast (StartMagicFX: ScaleFX
  min(power/32, 1), SfxSetLight 1.5 power, light_color[mclr_idx[c]]); 1
  turbo defence (StartShieldFX 0x17eec8: StartFXMissile(3,
  ShieldEffects[c], 0x13a), physics radius and blast = power/4, damage 25
  k, ScaleFX(f, 1, f) with f = power/12 in [0.33, 1], parented to the
  player); 2/3 attack + magic (StartThrowMagicFX 0x17ec80: StartFXSub(0.667,
  ThrowMagicEffects[c], 2 × forward + 4 up, 0x20010e), gravity 100, radius
  0.5, velocity ((short)+0x930 × 1.5 + 5) × (fwd.x·0.707, 0.707,
  fwd.z·0.707), spin (10π, 0, 0) (0x240830), SfxSetHit(MagicEffects[c],
  sound colour + 7), damage 40 k, blast 0.75 power, +0xb0 min(0.75
  power/32, 1)). Players past level 0x4a also get StartMagicHealFX
  (header 0x33, parented) except in mode 1. Checked headlessly (G1 game
  runs with --potions: turbo + magic reaches mode 1, attack + magic modes
  2 and 3; no unsupported path); not yet compared with a capture.

## Chests and explosions (2026-10-21)

- OpenChest 0x1588a8: a random chest (subtype 0x30) holding gold becomes
  the gold chest — its definition is replaced by chest_gold_info (the last
  kind-2 subtype-0x2f definition; the CHESTSG tree swap is presentation)
  and +0xf4 takes the gold amount. A Death statue from a chest whose
  +0x100 ≠ 0 gets +0xf2 = 2 and +0xf3 = enemy_logic_num[0x1e] (the
  `lbu` at 0x22d618). The content-node branch (+0xf8) is unreachable: no
  store writes a node into a chest's +0xf8.
- DamageItem 0x15a620 on chests: a potion blast (flag 0x200) on a chest
  holding Death writes the chest DEFINITION's subtype to 1 (shared data),
  swaps the content to APPLE, plays AudioDeathDies and sets +0xf2 =
  damage × 3; a blast (flag 0x400, ≥ 5) opens the time-bomb chest (0x2c:
  +0xde = +0xdc = 2) or releases a Death and destroys the chest (FX 0x1f,
  0x21).
- The time-bomb chest (ProcessItems, +0xdc == 2, subtype 0x2c):
  StartExplosion(level +0xdc × 50, 0x1d), AudioBarrelExplode, DeleteItem,
  NewHelp 0x89. StartExplosion now covers every kind (0x17, 0x18, 0x19 with
  its 0x1a/0x1b morph after 4 s, 0x1d = header 0x16 plus effect 0x1d,
  default) with the ScaleFX x/z stretch.
- ProcessEffects' morph chain (flag 0x4000): ChangeEffect(+0xc0), end =
  +0x74 or the new tree's length, then +0xc2 or the end of the chain
  (0x4000 and the blast cleared); loops.
- damage_enemy: a 0x80000-shield hit on a drained Death (+0x21a == 2)
  calls AddExp(player, 1, −2).
- Checked: the 51-level pad sweep (sw21 for B1 after the morph port; B1's
  poison cloud now persists and the scripted player ends with less health).

## Healing magic (damage flag 0x800000, 2026-10-21)

start_magic sets damage flag 0x800000 for casters past level 0x18. With it:
- damage_enemy / CritterDamage call do_heal_players 0x132620(damage
  (an enemy's clamped to its health), player, target matrix): past level
  0x4a the caster heals damage × ((level − 0x4b) × 0.016 + 0.1), other
  players in state 1 within the caster's +0x120 (x/z fqdist, from the
  instructions — Ghidra drops the arguments) half that; NewHelp 0x93,
  StartHealFx(0x32).
- ProcessEffects calls HealItem 0x15b010 on each item the blast reaches
  (before DamageItem): by the caster's +8 & 3 and level it turns gold piles
  and gold chests to silver (25) or gold (50) (CHESTG1/CHESTG3,
  TREAS_SILVER/TREAS_GOLD), spoiled food to APPLE (25) or CHICKEN (50),
  revives kind-8 items (bits 1: +0xda 600; at 50 the "<name>_D" tree or
  deletion) and kind-10 items (bits 3: +0xf4 = 4; at 50 DamageItem(10000));
  heal FX 0x2f/0x30/0x31 and help 0x8b–0x92.
- ReplaceItemAtree now keeps an item-slot → tree map (Game::item_trees)
  that gdl_play draws (it also shows the existing BADMEAT/GAPPLE/TREAS_JUNK
  swaps, which were previously dropped events).
- Checked headlessly: G1 game runs with --plevel 30/80 and magic, turbo +
  magic and attack + magic: no unsupported path; NewHelp 0x93 at level 80.

## Enemy and boss paths (2026-10-21)

- The enemies array starts at 0x22d960: Ghidra's DAT_ names in kill_enemy
  and generate_single are offset by 0x10 (kill_enemy's `lwc1 f0,0x40(s1)`
  reads the position at +0x40).
- kill_enemy 0x139c58: a gargoyle (type 0x20) without a carried item drops
  AddItem(1, 0x10, "GARG" + LevelEnemyDesc(0x20) upper-cased) at +0x40
  (visible, +0x100 = 0x3c for kind 1, placed); type 0x1b (GRM) fires
  enemy_death_fx → StartEnemyDeathFX (effect 0x58 toward the target or first
  active player along CreateDirMatrix(dir, 0) — YPR with half the pitch —
  at 20, radius 3, damage 50 flags 0x100020, morph to 0x59 after 3 s);
  every kill deletes the +0x1f4 effect (Death's StartDeathFX).
- Shield reflection (player +0x134 & 0x1020000) and piercing hit effects
  on players in ProcessEffects (see the code comments).
- generate_single 0x153e70 (logic-15 generators): one enemy per generator,
  +0x2ec = 2, +0x324 = 0xf, the spawn heading as generate_now's.
- do_enemy_attack type 0x1f: the "It" enemy tags its target (+0x298): the
  old It's +0x92c cleared, It = target with +0x92c = 1, NewHelp 0x32, the
  tagger dies (the swap on contact, targeting and HUD icon were ported).
- DoWorldEffects 0x1535c0 (SFXX flag 0x20, player and critter effects):
  boss 0x2a counts DoRockFly; 0x29 / 0x2c hide G5BIGDIRT / H4NSFFXL_PURPLE.
  ProcessItems launches subtype-0x33 rocks when DoRockFly changed (modes
  1/2/3: up 20+R(10), 30+R(15), 50+R(100) with x/z 10 × the direction from
  RockFlyCenter (0, 0, 0)) and copies DoRockFly to DidRockFly at the end.
- BossGenerateEnemy 0x19dbe0: Garm raises two GRM (0x1b, level 3) at ±π/4
  (genang.15) off the effect forward, 3 units out.
- SkorneSpewItems: Skorne spews boss1_items (BGNTR/BMASK/BHORN/BGNTL_IC,
  kind 1 subtype 9, +0xf0 = 0x4000/0x2000/0x1000/0x8000, +0xf8 = 240.0)
  instead of coins.
- Checked: 51-level pad sweep sw22 clean.

## Treasure rooms (world 12, 2026-10-21)

- run_play now configures world 12 (SECRET.WAD, S1–S9; previously
  unreachable natively).
- A secret exit (item type 9 subtype 0x32) touched while skip_wave < 3:
  +0xd8 bit 0, skip_wave = +0xf0 + 3, SaveStage 0x1914b8(item, first
  touching player, its +0x50) and init_got_it; skip_wave > 2 makes everyone
  leave (someone_wants_to_exit → DoAutoExit).
- do_exit destinations by skip_wave: 2 the current wave, 3..11 0xc00 |
  (skip − 3), 12 the Tower, 13..0xffff recorded_wave, ≥ 0x10000 skip −
  0x10000; ±1 (Next/PrevWorldLevel) only from the debug menu (set_wave /
  set_advance have no code callers) and stay unsupported.
- SaveStage keeps recorded_wave, secret_pos/leader, ≤ 0x400 item
  (status, kind) pairs (the exit, empty slots and open chests as gone) and
  the item world objects' +0x16/+0x17; init_next_level with skip_wave
  13..0xffff runs LoadStage 0x1916a0 (leader at secret_pos, statuses back,
  changed types or spent generators gone, trigger bytes back) and skips
  PlayerSaveState. gdl_play carries the record through the level load
  (Game::stage_save). HYPOTHESIS: the camera returns settled on the
  player (SaveAllRecords' camera copy is not kept).
- Timed levels (LEVL +0 flag 4): init_wave_timer / do_wave_timer /
  modify_sandglass — WaveTimer = LEVL +0xc + 0.99 counting down outside
  the stage start, pause and help; per-second AudioSecretTimer ticks,
  AudioTimeRunsOut at 8, AudioCountDown below 6; at 0 skip_wave = 0xd
  (back to recorded_wave). The sandglass blits TIMER, TIMER_SAND (two
  cropped halves), SAND_ANIM.
- Treasure-room gold: PlayersGiveSecretCoin 0x177690 (+0x908 coins,
  +0x900 = secret_char[level] + 0x200, +0x904 = 60; at MaxLevelCoins the
  unlock bit 1 << (char − 8) in +0x9a8, then AudioSecretChar, the AllCoins
  scroll and WaveTimer = 1, WaveComplete = 1); AudioPickupGold's
  bronze/silver/gold voices by value.
- Checked headlessly: S1–S9 load and run (sandglass expiry auto-exits).
  Played natively (gdl_play, GDL_DEBUG_LEVEL=A6 with the DEBUG
  GDL_DEBUG_TELEPORT onto A6's SECRET_ICON): the exit leads to the
  treasure room, the sandglass drains, and its expiry returns to A6 at the
  exit. Not compared with the reference.

## Continue and game over (2026-10-21)

- do_players state 0xb with +0x2ad4 = 1 (the continue prompt) in game mode
  0x4010: Back → inactivate_player 0x1319a0 (+0x2ac4/+0xfc/+0x2ad4/
  +0x2ad8 = 0, controls_remove_active_player 0x107268, +0x1cc = 0xb4, keys
  and powerups gone, the panel re-set; clear_player(k, 1) by the host); X
  clears +0x2ad4. State 0's +0x1cc timer re-runs setup_player_display.
- With no active player left game_main calls init_over: gdl_play returns
  to the title. In single player the wave ends on the frame the player
  reaches 0xb (do_players reports it done), so the prompt only matters
  while others play.

## Level end (game_main, 2026-10-21)

game::exitflow ports game_main 0x125280's level-end branch and next_world
0x1250f0. The level ends once do_players reports every player done and
AudioStillPlaying is clear (`Game::exit_ready`). Leaving a realm level by
its exit (skip_wave 0) goes to TowerWave → init_shop(0): the tally, the
shop and the final stats, then the Tower (shop_setup's LoadTowerAndSelect
loads it; init_next_level enters it). The exit item's destination is kept
in the player's +0x808 but does not choose the next level here. From the
Tower, the portal taken (do_exit → +0x808) leads through EndTower,
next_world (the largest +0x808 of the players not in state 0/2) and the
map (init_mapscreen(0x78, 0)) to that level. Secret exits (skip_wave 3..11)
go through the map to world 12; the stage return (skip_wave 13..0xffff)
to recorded_wave (map with skip); world 12 and levels (5, 0) / (6, 0) go
on to the next level index unless a player recorded a destination; boss
types 0x2b / 0x2c play their movie first (init_gamemovie, not ported: the
client enters the Tower). PlayersSetWaveFinished(PrevWorld, PrevLevel)
runs unless every player is dead. Exit destinations on the disc (game-run
`exits`): A1 → 0x105, A2 → −1, A6 → 0x102 and the secret 0xc02; the
Tower has one portal per realm level, the first shown and the rest hidden
by HideExits until reached. Every LEVL record has +4 bit 0 set, so
NextWorldLevel's step in next_world never applies (guarded). GUI checks:
A1's exit → Stats → Shop → class stats → inventory → the Tower (welcome,
Sumner); the Tower's 0x100 portal → A1. The client's earlier shortcut
(exit → the map of the exit's destination) is gone; F9 keeps it as a
DEBUG skip.

## Players joining (2026-10-21)

check_active_players (and_edges with allow_assignments: assign_controller
binds an unassigned port that pressed Start to a free player slot) runs in
the select screen and, in game_main, only in the Tower; realm levels take
no new players. A selected player (state 3) is activated by do_players
(activate_player 0x1310e0: state 1, the others' destination +0x808, a 5 s
kind-9 powerup outside the stage start, every enemy's +0x340 cleared, at
the end of a stage straight to state 5); advance_ok holds the level end
while only selecting players remain. The client loads each player's
character (load_player_model: players/<class>/<colour><tier>0,
players/<class>/<colour> for classes 8..16, players/<class>/<code> for a
Hidden descriptor or Sumner's "sum"; clips and SFX of class & 7) and the
game gets each class's clips and ClassData. The old co-op join (a copy of
player 1) is gone.

## Versus targeting and damage (GamePrefs mode 2)

PlayerGetTarget 0x146990: in mode 2, after enemies/critters/generators, any
other player in state 1 inside the 0.707 cone (3-D dot with the facing)
whose distance less its +0x828 radius is shorter wins the distance; `out`
keeps the last examined player's direction and the target stays −1. With a
target and a kind-7 item at +0x880, ItemCollSub from the origin to 2 × the
radius towards the given target's position refuses the target (target −1,
range); the target position is only set for a target passed in (a target
found by closest_enemy would read uninitialised stack: Unsupported).
PlayerMotion's melee with no target and no item in mode 2 calls
PlayerDamagePlayer 0x1473e8: the nearest other player in state 1 in the
cone within the attacker's radius + 2 (less its radius) takes
damage_player(damage, k, 2, flags, dir), dir being the loop's last
direction. Ported from instructions; not exercised (no versus run).

## Player placement (get_player_pos 0x14cef8)

init_next_level sets game_mode 0x400c, then for each player in play
load_player_geo 0x130d40 → get_player_pos(p, 1), the yaw/pitch/roll matrix
of +0xd8, +0x86c = yaw, +0x88c = y, +0x854 = position and
UpdatePlayerWorldMat(p, 1). A join in the Tower goes through the select
screen, whose Done runs init_next_level again, so every placement of a new
player is a level-start one. get_player_pos(p, mode):

- Stage start (0x400c or start_of_stage): the first earlier player in play
  (state 1/4, +0x93c bit 0x20 clear) gives +0x88c (its y), its matrix and
  +0xd8; the try position is its +0x70 plus (new_offset_start[p] −
  new_offset_start[k]) × (its radius + 0.5), rotated by StartTransmitter
  +0x24 (x' = dx·cos + dz·sin, z' = −dx·sin + dz·cos). new_offset_start
  0x234100 = (0,0), (0,2), (−2,2), (−2,0). Mode 1 stops here on failure;
  mode 2 keeps the copied matrix. In 0x400c a success takes the start yaw
  (CopyMat3 of YawMat3(PlayerStartYaw)).
- Otherwise (the PlayerMotion respawn, mode 0): from RandInt(4) on, another
  player in state 1/4/8 with a tree; its floor is refreshed (FloorCollide
  0.5, 4, −10, 0x17 → +0x89c, +0x898 flags, +0x88c), its rotation and
  +0xd8 copied, position = its +0x50 (+0xf0 while grabbed), then the 16
  new_offset 0x234120 probes × (radius + 0.5) about its +0x70. Success
  parents the node like that player's and copies +0x89c. All failing: its
  matrix; with an enemy/critter blocking, that position and damage_enemy
  9999 / CritterDamage 99999 (player −2); else FloorPos at its position
  and Errorf "Can't find start near other player".
- No other player: PlayerStartPos (test_location: FloorPos within 3), the
  16 probes, then PlayerInitStartPos[0]; FloorPos y, the start yaw.

Level start on a moving floor (2026-10-23, J4): the reference capture
w10l4-start-v1 (warped from slot 3 through do_exit's wave skip, labelled)
starts the player on platform object 43 with +0x89c = 0 and +0x100 = 5 (do_players'
previous state, left by the exit), and trace-J4-start-v1 shows the platform
carrying the player (x −20.5 → −69.4, then down). With +0x100 ≠ 1 the first
PlayerMotion takes PlayerCollideFloor's moving path, so PlayerNewFloor
registers the floor and PlayerCheckMovingFloor parents the player to it.
Native: place_player clears +0x89c as get_player_pos does (it had stored
the floor object, so the first motion never ran PlayerNewFloor), and
game_run's fixture-based level starts set +0x100 = 5 (the fixture is
mid-play, +0x100 = 1); the client's records keep the previous level's value.
Native now rides the platform as the reference does.

try_location 0x14cbd8(other, p, pos, out, blocker, 1): FloorPos(WldBtm,
r/2) must hit — the same object when other stands on a 0x1000 (moving)
object — within 6 of other's +0x88c, and within 3 unless other's +0x898
holds 0x38; +0x50 and UpdateObjOffsets; outside 0x400c and without
LockCamReq/BossCameraMode, get_screen_pos(+0x60) must fall inside
(LLIM+30, RLIM−30) × (DLIM+40, ULIM−20); y += +0x820; then
PlayerWallCollide(1.0) from other's +0x70, PlayerCollidePlayers (first),
ItemCollide → +0x880 (PlayerItemCollision refusing) and
PlayerCollideEnemies (the blocker returned). get_screen_pos 0x173ea0 is
MBWindowProject 0x1d1278 with Cam[0]'s matrix 0x23e500, which CamUpdate
copies from the StdCam matrix (both captures: equal); see camera.md.

Evidence: actual-disc headless runs (gdl_tools game-run players=4) in G1,
A1 and the Tower place players 2–4 in the square at (radius + 0.5) × 2 = 4
units, turned by the start yaw (game-run-mp-{G1,A1,L1}-v1.json); a drop
below the world bottom (fall=200, 3 players) respawns player 1 beside
another at a new_offset probe (game-run-mp-fall-G1-v1.json). Not compared
with a reference multi-player run.

## The familiar (PlayerProcessFamiliar 0x12cc18, 2026-10-23)

From level 30 (0x1e) the player carries FamiliarTree[player][0] (FAMILIAR1
of the SFX package, InitPlayerMissiles 0x18efe0), from level 80 [1]
(FAMILIAR2); below 30 the tree is deleted. ReplaceTree attaches it under
the player's node (+0x80, flag 0x800) at PDAT +0x164..+0x16c (× 1.2 at
level 99), written to the tree's +0x10 and its root node's +0x30..+0x38.
In game mode 0x4010 it is hidden (MBTreeSetFlags 2) while +0x138 bit 0x80
is set; otherwise shown and animated (AnimateATree(tree, +0x8d8 bit
0x10000000, that ? 2 : 0)). Each SFX package's FAMILIAR trees are one OANIM
node with READY (20 frames, repeat 1) and ATTACK (9) sequences. The
simulation keeps +0x720 as a marker (powerups.rs); gdl_play draws the
tree (present_familiars; READY looping on ATime is a HYPOTHESIS). Native
frames at levels 35 and 85 in G1: runs client-fam5-69700 / -280200. No
reference capture.

