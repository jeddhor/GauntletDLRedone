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
  wall object are index + 1 (0 = null). +0x6b4/+0x6b8 (team-move node,
  swing tree) stay 0: the paths that need them are Unsupported.

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

## Unported (Unsupported when reached)

Team moves (0x58..0x5a, 0x88..0x93, PlayerSetParent/
PlayerFacePlayer), thunder
hammer / breath / familiar events (0x2000000, 0x1000000, 0x10000000 with
+0x138 0x80), PlayerDamagePlayer (versus), get_player_pos beside another player and at
the stage start, players in states 2/3 (selecting/joining), inactivate_player,
setup_player_display, PlayerProcessFamiliar/Scale for boss state 5, rotating
Milestones: AddLocatorInstList's kind-5 locators (FloorPos drop + 0.5),
update_player_milestone's +0x950 history. Floor parents with rotation use
MulMat4 with InvertMat4's rigid inverse.
