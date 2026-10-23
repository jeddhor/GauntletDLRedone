# Critters and bosses (M15)

Bosses and statue enemies (golems, gargoyles, generals) are "critters": a
data-driven engine separate from the ordinary enemy AI. Sources: SLUS_200.47
critter/boss family 0x191850–0x1a1fa8 (exports in private/reports/ghidra-critter,
-atree, -lgws, -lookat1). Status: transcription in progress; K2's General (a
golem-type kind 8, 210 updates) and B6's dragon (a boss, CritterBossAI, 250
updates) match the reference ("Reference comparison" below), as do all
eleven bosses through their activation, the chimera (A5) with its three
child heads included, and A1's gargoyle statue from its awakening through
the spawn (gameplay.md "Statues"); golem and general statues and Death
statues are not compared yet. Native port: gdl_core game/critters.rs (types,
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

## Reference comparison (2026-10-22, actual disc)

trace-k2-newgame-v5 (slot 7, the ship K2) records CritterInsts (16 × 0xb70,
delta-coded), NumCritterInsts, BossCritInst, CritterETypes, CritterNewID,
CritterCounter, CritterNActivePlayers, CritterPlayerNTargets, BossStartPos,
BossActivateTime, BossCamMode, EnemyRootNode and, per live instance, the +0x70
MB node (local matrix, +0x74 parent) and CritterGetNode's table (+0xc0, +0xbc
entries of 0x28 bytes, the MB node pointer first). The frame replay seeds each
root instance from its record: TYPE by CritterETypes, the native tree state as
CritterInitInst/InitGeo/InitColnodes derive it, the clock from the +0x84
AnimInfo, the pose evaluated at it with the previous update's look-at rebuilt
from +0x10c..+0x118, +0xdc and the collision/look-at nodes by the node table,
the node's parent as a WORLDS object by its +0x28 node pointer. Result
(replay-frame-k2-v10.json): critters 210/210, and every collision and look-at
node pointer agrees with the native node indices.

Instance words that are guest pointers (not compared): +0x04 TYPE, +0x70 the
critter's MB node (MBNewNode(EnemyRootNode)), +0x80/+0xc0 the atree and its
node table, +0x84..+0x8c and +0xc4 AnimInfo links, +0xcc the tree root
(AtreeInit's first node, parented to +0x70), +0xd0 the shadow tree, +0xd4/+0xd8
/+0xdc/+0xe8 nodes, +0xe0 a move effect node (MBRemoveNode at a done
transition), +0xb60/+0xb64 the child chain, and each collision node's +0 NODE
record, +4 node and +8 reference node. +0xb54 is an Items pointer
(ProcessItems 0x155fe8; native slot + 1), +0xb5c a waypoint record
(FindClosestWaypoint's 0x23bd20 array, stride 0x80; native index + 1). The MB
nodes are not allocated in tree order (the stride-0x80 guess from one record
was wrong; the table is authoritative).

Stack residue: CritterGetSingleTargetPlayer copies its 0x30-byte target struct
into +0x140, and CritterCalcTarget never writes the struct's +0x14/+0x18/+0x1c/
+0x2c, so +0x154/+0x158/+0x15c/+0x16c hold whatever the stack had (0, 0,
0x80808080, 1.0 in the trace). No critter code reads them back (the 0x15c loads
in CritterDamage, CritterDamagePlayer and CritterGolemAI are the global
0x22015c, EnemyShrink); the replay skips them.

trace-b6-boss-v3 (slot 8, B6 reached through do_exit's wave skip): the dragon
— BossCritInst → instance 0, BossType 0x22, active, 2000 health — through its
moves and the player's two throws at it: critters 250/250 with no critter
change needed; the boss level's fixes were the camera-side ones (gameplay.md
"B6").

trace-g5-boss-v1 (G5 reached from slot 6 through G1's exit and skip_wave
0x10704, staged capture w7l5-boss-v1): the lich (BossType 0x29) from before
its activation through its first attacks: critters, camera and every other
part 250/250 once its attack effects followed their critter node (above).

The remaining boss levels (gameplay.md "The other boss levels"; staged
captures w<world>l<level>-boss-v1): the djinn (C5), wraith (J5), pboss (K5),
Skorne 1 (E2), Skorne 2 (F2) and Garm (H4) from their activation through
their first attacks, critters 250/250 with every other part; the drider (D5)
and yeti (I5) likewise in traces that walk (D5) or warp (I5) the player into
their activation range (TYPE +0xec: 55 and 60).

trace-A5-boss-v2 (the chimera, BossType 0x23): the root and its three heads
(instances 1..3, +0xb64 the root, +0xb60 the chain; TYPE = the root's file at
the pointer's 0x140-byte offset; the heads animate subtrees rooted at nodes 74,
167 and 89 of the root's 176-node tree and share its CritterGetNode table).
The replay seeds each head from its record and the root's table; 250/250 in
every part through the activation at step 281 after:
- CritterCopyAnim: AnimateATreeFrame(…, 1) runs AnimateNode after
  AnimateTreeFrameInterp, so DoAnimation's handshake leaves the head's reset
  byte at 2 (the port left 1).
- CritterLookAtPlayer with no move (a head after CritterCopyAnim sets +0x124
  to −1) passes a null move and still turns the +0xd4/+0xd8 nodes; the
  replay's look-at seed had skipped such instances.
- The chimera's floor (A5FLOOR#00 under A5ELEVATOR) composes through its
  static ancestors (collision.md).

Found by the comparison:
- ProcessCritter starts with GetWorldMat(+0x70 node → +0x10) and ends (every
  surviving path, LAB_00198ef0) with CopyMat4(+0x10 → node) and
  UnparentMatrix(node, node +0x74); CritterInitGeo ends the same way under
  EnemyRootNode. CritterCollideWorld's last FloorCollide hit re-parents the
  node: MBNodeSetParent(node, the object's node) when its +0x28 node exists and
  flag 0x1000 is set, else EnemyRootNode. A critter therefore rides a moving
  floor; K2's General stands on K2FLOOR_NODEA1 (object 1226) like the player.
  Native: Critter::node (the stored local) and Critter::floor; tree nodes
  compose through that local, the floor chain, the identity root and the
  translation-only root (tree_pose::node_world_chain), so mid-update node
  matrices use the node as the last UnparentMatrix stored it.
- CritterLookAtPlayer 0x19c600 (move types 0x10/0 return; death, MOVE +4 bit
  1, +0xb4c > 0 or +0xb4e > 0 relax towards zero; the target is
  players[+0x130] +0x60) and NodeLookAtPos 0x19c240(TYPE +0x60/+0x64 yaw
  limit, 0, TYPE +0x68/+0x6c pitch limit, TYPE +0x70/+0x74 pitch offset):
  GetYawPitch of the node's world row 2 and of the target from its world
  position, the accumulated yaw/pitch (+0x10c/+0x114 for +0xd4, +0x110/+0x118
  for +0xd8) approach by at most SPF·π/2 with one-step wraps, then ExtractPYR
  0x1d6790 of the node's local, each angle moved within its limit and
  CreatePYRMatrix 0x1d63f8 (3 × 3 rows, w zeros, +0x3c = 1, translation
  kept). The rebuilt head is what the next update's colnode pass reads.
- DoAnimation's reset handshake (a nonzero AnimInfo reset byte becomes 2) on
  critter clocks (step 42, a new WALK move).
- CritterNodePlayerCollide 0x19bcf4..0x19bd28: the knockback direction is
  t = a + b, then t + t (the decompiler prints a + b + a + b; one ulp in all
  three components at every hit).

## Native implementation notes (2026-10-17)

- Skeleton: crate::tree_pose evaluates DoAnimation locals at the clock's
  sequence/frame and composes them with GetWorldMat's parent order onto the
  instance matrix. Node scale and ADDA attachments (presentation) are not
  applied; NodeLookAtPos head turns are (2026-10-22); sequence blending does not
  occur (CritterAnimate's AnimateATree clears f12, the blend duration, at
  0x112a30 — 2026-10-22). Key gaps of 256+
  frames use a direct float reciprocal (sample_channels_long_gaps,
  HYPOTHESIS).
- Drawing (gdl_play present_critters, 2026-10-22): each tree node's object
  "<prefix><label>" (15 characters) unless the node has flag bit 0 or is an
  OANIM node (AtreeNodeInit's rule, as for item trees); an OANIM node (type
  2) draws one frame object of the critter's current sequence
  (DoObjAnimation: (int)(frame + 0.5) − start, the entry's objects from its
  first name's index on). Skorne's lower body (E2's robe, F2's lava column)
  is such a flipbook: BOSS1_RDY0F02.. 17 sequences, each entry's objects
  checked contiguous in MONSTERS/SKORNE1. Before, only named nodes were
  drawn and Skorne floated without them (scene-e2-v1 / ref-e2-active-v1).
- Effects parented to a critter node (CritterDoSfxSub's SfxSetParent: SFX
  flag 0x800 the +0xcc root's MB parent, 1 the +0xcc root, otherwise the
  +0xdc transition node, for SFX with flags 0x801 or an attaching caller)
  keep the SFX offset as their local translation and follow the node:
  ProcessEffects recomputes their world transform from it every update
  (since 2026-10-22, trace-g5-boss-v1; previously spawned at the world
  position and left there) with GetWorldMat's order (the local times each
  ancestor: the tree nodes, the +0x70 node, the floor chain, the roots).
  SfxSetPhysics' YawMat3(atan2f(vx, vz)) turns the node's local, so the
  local rows hold the critter's facing; a moved node is written back by
  CopyMat4(world, node) + UnparentMatrix(node, node +0x74), outermost
  ancestor first (trace-H4-boss-v1 step 484: the local translation advances
  along z and the rows carry InvertMat4's rounding, row 1 1.0000148). Particle SFXX are not simulated; texture-mod SFXX
  (flag 0x100) run CritterDoTexmodNode 0x193f88, a tail jump to SetSkinFX
  (SFXX +0x40 rate, +0xec, SFXX +8 texture, (int)(+0x3c × 30), +0x44
  repeats), whose skin CritterUpdateSkinfx then steps (trace-B6-bosskill-v1
  step 409: the dragon's LAVA00 death skin). CritterInitSfx resolves +8 of
  such SFXX by FindTexMod (the texmod record's +0x48) or MBOX_FindTexture
  (a texture handle: native stand-in, the replay skips critter +0xf8);
  every SFXX with a custom effect header gets a real effect slot as in the
  original (owner = critter ID | 0x1000).
- Custom effects (InitCustomEffectSub 0x179aa8) take EffectHeaders 0x61 + n,
  matched in the critter model, then weapon_anim, powerup_anim and the item
  packages.
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
