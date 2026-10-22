# Critters and bosses (M15)

Bosses and statue enemies (golems, gargoyles, generals) are "critters": a
data-driven engine separate from the ordinary enemy AI. Sources: SLUS_200.47
critter/boss family 0x191850–0x1a1fa8 (exports in private/reports/ghidra-critter,
-atree, -lgws). Status: transcription in progress; nothing here has been
compared with the reference yet. Native port: gdl_core game/critters.rs (types,
loading, instances, pose), critter_ai.rs (per-frame AI), critter_hits.rs
(attacks, damage, hit tests); tree_pose.rs evaluates the skeleton.

## Files and loading

`CRITTER/*.WAD` (18 files) are MB containers read by CritterInitHeader
0x191978 (tags SFXX, DAMG, MOVE, PTRN, NODE, DESC, TYPE, ADDA; strides from the
consumers: TYPE 0x140, DESC 0x30, ADDA 0x30, MOVE 0x90, PTRN/NODE/DAMG/SFXX
0x50). `gdl_tools critter-inspect` parses all 18 from the disc
(private/reports/critters/*.json). Unused bytes hold authoring-tool text
residue; it is retained, not interpreted.

LoadGauntWorldStart 0x163ec0 loads one file per critter enemy type in the
level's EnemyType slots (GetEnemyTypes) and clears that slot:

| EnemyType | File |
|---|---|
| 0x1d | golemI.wad (CurWorld 9), golemF.wad (CurWorld 6), else golem.wad |
| 0x20 | gar_<EnemyData name+0x10>.wad |
| 0x21 | general.wad |
| 0x22–0x2c | dragon, chimera, djinn, drider, pboss, yeti, lich (0x29), wraith (0x28), skorne1, skorne2, garm |

(0x28 → wraith.wad and 0x29 → lich.wad: the string table order differs from the
case order; addresses 0x2a9628/0x2a9618.) CritterLoadAllTypes 0x192498 /
CritterAllocType 0x192360 bind each TYPE to its DESC (+0x50) and register it
in CritterETypes[desc kind][TYPE +0x52]. GetCritterDesc 0x192168 names the
model package: kind 7 "monsters/<desc>_<EnemyData name>", kinds 3/8
"monsters/<desc>/<world_data+4>", else "monsters/<desc>" (the lich:
MONSTERS/LICH). The animation tree is "<DESC +0x10><TYPE name>" (lich: "LICH").

DESC kinds (DESC +0x20) select the per-frame AI in ProcessCritter 0x198c28:
4 boss (CritterBossAI 0x197c50), 3/7/8 CritterGolemAI 0x198598, 0/1/2/5/6 only
animate. Disc files: kind 4 = the eleven bosses, 3 golems, 7 gargoyles, 8
general.

## Records (fields used so far)

TYPE (0x140): +0 name[16]; +0x20/+0x30/+0x40 node names → node indices
+0x56/+0x58/+0x5a; +0x50 DESC index; +0x52 CritterETypes slot; +0x5c flags
(0x1 shadow, 0x10 node parent, 0x80 floor-triggered activation, 0x1000 no
0x800 tree flag, 0x10000 set when a move type is 0x30–0x39); +0xa0..+0xa8
home position (used when +0xa4 < 999); +0xb0 floor offset; +0xb4 attention
height; +0xc0 collision offset (vec4); +0xe4 health factor (× level_data
+0xac); +0xec activation distance; +0xf4/+0xf6 sfx; +0x110/+0x112 move
count/first; +0x114/+0x116 pattern count/first; +0x118/+0x11a colnode
count/first; +0x11c child TYPE index (chain); +0x11e alias TYPE.

MOVE (0x90): +0 move type (0 init, 0x10 start, 0x11 death, 0x20 ready, 0x21
alternative ready, 0x22 roar, 0x23 block, 0x30–0x39 translate, 0x40–0x42
reactions, 0x7f–0xef attacks, 0xf0 pattern marker); +4 flags (0x2 child-count
gate, 0x4 disabled, 0x10 needs node); +8 interrupt priority; +0xc sequence
index (from +0x20 name); +0xe node index (from +0x30 name); +0x40/+0x44 action
frame windows, +0x48/+0x4a DAMG indices, +0x4c interval, +0x50/+0x52 frame
limits, +0x54 next move, +0x56 interrupt mode (0, 0x14, 0x28, 0x3c, 0x50,
0x5a), +0x58/+0x5c sfx indices and frames, +0x60 target query, +0x80 repeat
delay, +0x84 translate speed (CritterTranslate; also gates waypoint moves in CritterLookForTranslate), +0x88 turn rate, +0x8c hold time.

PTRN (0x50): +0x10 flags (0x2 child gate, 0x1000 disabled), +0x14 delay,
+0x20 eight move indices (−1 terminated), +0x30 target query.

Critter instances (CritterEmptyInst 0x193ab0): up to 16 × 0xb70 bytes;
children chained through +0xb60, parent at +0xb64. Health +0x4f4 = TYPE +0xe4
× level_data +0xac. Move state: +0x124 current, +0x126 next, +0x128 pattern,
+0x12a pattern candidate, +0x12c pattern step, +0x130/+0x132 target player,
+0x134 grabbed player; per-move last-use times +0x25c[n], per-pattern
+0x35c[n]. Animation: AnimInfo at +0x84 (AnimDone = byte +0x36, the low byte of
the clock's mode word), sequence +0x92, frame +0x9c.

## Boss flow

AddLocatorInstList calls AddBoss 0x19da88 for the boss locator:
BossStartPos = locator +0x30, BossCritInst = CritterNewInst(4, 0, locator).
CritterBossAI activates the boss two seconds after it is first processed
(TYPE +0x5c bit 0x80 instead waits for the floor object's byte +0x16 bit
0x10), subject to TYPE +0xec player distance. Per frame: target selection,
critical/block/attack/translate/ready move search, CritterAnimate, death
timing (move 0x11 → BossIsDead, fade, CritterDelInst), then
CritterDoTransition/DoActions/Translate/Rotate/LookAtPlayer and floor snap.

## Native implementation notes (2026-10-17)

- Skeleton: crate::tree_pose evaluates DoAnimation locals at the clock's
  sequence/frame and composes them with GetWorldMat's parent order onto the
  instance matrix. Sequence blending, node scale, NodeLookAtPos head turns
  and ADDA attachments (presentation) are not applied. Key gaps of 256+
  frames use a direct float reciprocal (sample_channels_long_gaps,
  HYPOTHESIS).
- Effects parented to a node are spawned at its world position and do not
  follow it. Visual-only SFXX (particles, texture mods) are not simulated;
  every SFXX with a custom effect header gets a real effect slot as in the
  original (owner = critter ID | 0x1000).
- Custom effects (InitCustomEffectSub 0x179aa8) take EffectHeaders 0x61 + n,
  matched in the critter model, then weapon_anim, powerup_anim (item anims
  are not searched yet).
- CritterDamagedPlayer from ProcessEffects receives whatever f12 holds after
  damage_player (no reload in the instructions); the effect damage is used.
- CritterNewID starts at 1 (its initial data value is not read).
- Player targeting: PlayerGetTarget range 200 with a boss; critter targets
  (index | 0x10000) via CritterGetTarget(Sub); melee through PlayerDamageEnemy's
  critter branch.

Ported since: CritterNodeEnemyCollide, EnemyCollideEnemy's critter test
(kind 2, NODE flag 8), ItemLineCollide 0x157708, PlaceEffectOnFloor, effect
end AddItem(3, 0, "BOSSGEN") (the lich's spit lands a boss generator),
BossGenerateEnemy (acts only for Garm 0x2c), BossSpewCoins + StartSpewItem +
ProcessSpewItems (spew_tab 0x25d1a8; passes 0/1 rotate the unscaled vector
but still draw three Random values), acosf (kernel 0x1e9980), modify_boss_ene
sound names, BossType from level_data +0x44 (LoadWorldData), and the
completion: BossDeath (PlayerGiveShard(p, GetWorldOrder(CurWorld)) → +0x1a24
bit for players in state 1; the health meters' blits hidden) →
StartGoodWizard → DoGoodWizard (game/goodwiz.rs, 2026-10-21): a 5 s wait
(10 s for 0x2a/0x2c); the WIZARD tree of the level's item package
(item_anim2 = items/level<code>) at calc_wizard_pos ((BossStartPos + the
players in state 1/8) / (n + 1), the first player's y doubled, 3 up;
(0, −12, 6) for 0x2a/0x2b), turned toward a player by calc_good_wiz_attn
(atan2f, at most SPF·π/2 a frame) and faded in by 4 a frame; the boss's
GameText speech 0x93..0xa3 (CaptionText(−1, message, line, frame / 2,
0x10), each line held 0x3c vblanks once revealed, the phase ending 0.5 s
after the last line) with AudioGoodWizard(boss, 0 / all_rune_stones); the
rune phrase 0x9b..0x9e by the realm's runes found (worldseltab +0x10 bits,
+0x14 count) or Skorne's 0xa0/0xa1 (hide_rune_stones: all twelve rune
bits), 1 s after its last line; the closing line (shards 0x3fe and runes
0xfff → 7, shards 0x1fe → 6, else 5); auto_exit_timer WizDelayGoldLeft 600
(WizDelayNoGold 120 without gold and without all rune stones; past state 8
without gold at most 120); AudioStillPlaying holds state 9; TextureExit
skins at EXIT_SKIN_TIME 35 → do_auto_exit → auto_exit_flag → game_main's
exit (do_players done and the voice queue empty) to TowerWave 0xd00.
Headless (actual disc data, GDL_DEBUG boss-kill): A5, B6, C5, D5, E2, F2,
G5, H4, J5, K5 run the sequence to level_done 0xd00
(private/reports/game-run-<level>-goodwiz-v1.json); I5's boss does not
activate on the scripted walk. Native GUI A5: the wizard and the captions
"You have defeated the fearsome Chimera…", "You have not found any
runestones from this realm." shown.

Also ported: grab/release (CritterGrabPlayer, CritterReleasePlayer, the
grabbed-player pin), fire cones (DoDamage type 4 → CritterFirePlayerCollide
and the DamageFX effect), and the effect orientation in CritterDoDamageFX:
YawMat3 0x1d5388 by DAMG +0x14, then WPitchMat3 0x1d5228 by DAMG +0x1c
(each row's y/z rotated; no-op below |a| 0x38d1b717, sinf first).

Child critters (chimera heads) are ported: CritterNewInst follows TYPE
+0x11c through the same file's types (CHIMERA: root 0 → EAGLE 1 → LION 2 →
SNAKE 3), chaining +0xb60 and pointing +0xb64 at the root. CritterInitChild
0x193900 copies the root's animation instance (+0x80..+0xc4) and roots the
child's at its TYPE +0x10 node of the root's tree (AtreeFindNodeIdx, 16
characters), detached from the root's animation tree by
AtreeNodeSetParent(node, 0, 0, 0); the MB parent is kept, so world matrices
still compose through the body. Native model: children share the root's
pose; each instance animates only the nodes whose nearest child root is its
own (AnimateNode walks the detached subtree). ProcessCritter processes
children only through the root (CritterUpdateChild 0x198b78: root matrix,
attention and collision centre; the position follows +0xdc), counts live
children (+0x487) and kills a root once every child's health is gone.
CritterBossAI: children's GetTargetPlayers/SetDifficulty,
CritterResolveMultipleTargets 0x19b060, child selection
(CritterChildGetPattern 0x196a78 while the root runs a pattern, else
CritterChildCriticalMove 0x195fd0 → LookForAttack → LookForReady; busy
children make the root pick a type-1 move), CritterCopyAnim 0x195a20 for
idle children (AnimateTreeFrameInterp: clock offsets +0xe sequence, +0x14
blend, +0x18 frame, +0x20 start, +0x24 blend end, +0x30 last time, from
CalcAnimInfo), and the children's transition/actions/translate/rotate.
CritterDamage: a child's damage also comes off the root; a root's damage is
split over its active children at half rate. CritterNoHit tests the family,
CritterMoveGetTarget falls back to the root, CritterKill on a root sets the
children's health to 1.0, CritterActivate activates them, and MoveDone /
LookForCriticalMove reset the children's patterns. CritterDoSfxSub parents
by flag: 0x800 the MB parent of +0xcc (a root's own matrix, a child's tree
parent), 1 the animation root, else +0xdc.
CritterChildCriticalMove reads the current-move record with +0x124 = -1
(the preceding MOVE record of the shared chunk); this is the instruction
behaviour, kept as is.

Statue critters are ported. ProcessItems 0x155eb0 holds statue items
(0x1d golem, 0x1e death, 0x20 gargoyle) until DamageItem sets +0xf8 bit 0,
then marks +0xd8 bit 0 / +0xde = 1 for one update and spawns once +0xda < 1.
0x20/0x1d/0x21 spawn CritterNewInst(7/3/8, 0, item matrix) (+0xb58 = item
+0xfc × level +0xb4, +0xb54 = linked item); other types, or no instance,
use generate_enemy. Files: golem/golemF/golemI.wad, gar_<ENMY +0x10>.wad,
general.wad; models per GetCritterDesc: kinds 3/8 "monsters/<desc>/<world_data
+4>" (MONSTERS/GOLEM/LEVELG), kind 7 "monsters/<desc>_<ENMY +0x10>"
(MONSTERS/GAR_SERP); trees GOLEM1, GAR1, GENERAL1. Generals (kind 8) start on
FindClosestWaypoint(10, position) (game/lookouts.rs). CritterGolemAI
0x198598: CritterGetSingleTargetPlayer 0x19b260, activation without range,
Block → Attack → Translate → Ready selection (Ready only while a trigger
camera runs, none under StopTime), death move → CritterDropItem 0x197778
(linked item or "GARG<name>" in StartBagFX 0x17f2f8) and CritterDelInst.
CritterDoKnockback 0x1989e8 for non-bosses. CritterTranslate for non-bosses
collides (game/critter_move.rs): world walls/floor with CritterWorldDamage,
items (CritterColItem codes), players (pushed via +0x848), enemies (golems
damage enemies with half height ≤ 2) and critters (CritterLineCollide mode 1
skipping itself). Waypoints: CritterMoveTargetPos follows NextWaypoint
links, CritterRotate turns to the lookout. CritterLineCollide's t0 mode is
LineCylinderCollide's directional flag (EnemyCollideEnemy and
PlayerCollideCritters pass 2, ProcessEffects 0, CritterCollideCritters 1);
the port previously passed non-directional for mode 2.

Headless check (G1, DEBUG teleport to (-2, 11, -80)): general #88 spawns
(health 150), patrols lookouts 0 ↔ 1 and attacks the player; no unsupported
path. Not checked against a reference instance (the fixture has none live).

Not ported yet: safe rocks beyond setup, legend items against bosses, PlayerCollideCritters (players walk
through critters), boss camera and health meter (presentation).

Headless check (G5, DEBUG boss_health=10, potions 3): the lich dies, spews
coins, the good-wizard sequence runs and the level ends with destination
0xd00 and no unsupported path.

Headless check (G5, 2000 updates, scripted approach and throws): the lich
activates when the player is within 39 units, plays START, then READY and
attacks; throws reduce its health (4500 → 4240) and its stomps/swings reduce
the player's. Non-boss levels are unchanged (G1/A1/C4 sweep values identical).

Headless check (B6 dragon, 1400 updates, scripted approach and throws): fire
cones and pitched effects run; the dragon takes 2000 → 1987 and the player
483 → 129, with no unsupported path. C5/F2/K5 also run their fire cones;
A5 (chimera) still stops at setup on child critters.
Headless check (A5 chimera, scripted approach and throws, DEBUG
boss_health=10 for every instance, potions 3): the heads activate with the
body, pick their own moves, damage the player and die separately; the body
dies (with the DEBUG health a head hit also empties the body), the good
wizard runs and the level ends with destination 0xd00, with no unsupported
path. Without the override the chimera (4500 body, 900/1125/900 heads at
these multipliers) damages the player 483 → 31 in 1400 updates. B6, G5,
G1, A1, C4, C5, F2 and K5 runs are identical to the previous build.
