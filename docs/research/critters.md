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

Not ported yet: child critters (chimera heads), CritterGolemAI, grab/release
(PlayerSetGrabbed), BossSpewCoins and spew items, fire cones
(CritterFirePlayerCollide, ItemLineCollide), safe rocks beyond setup,
CritterNodeEnemyCollide, legend items against bosses, PlayerCollideCritters
(players walk through critters), boss camera, health meter, death → exit.

Headless check (G5, 2000 updates, scripted approach and throws): the lich
activates when the player is within 39 units, plays START, then READY and
attacks; throws reduce its health (4500 → 4240) and its stomps/swings reduce
the player's. Non-boss levels are unchanged (G1/A1/C4 sweep values identical).
