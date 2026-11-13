# Items: load path (AddItemInstList → SetItem → PlaceItem)

Source: SLUS_200.47 (profile ps2-slus-20047-fb47d40667df); instruction exports in
`private/reports/ghidra-items*/` and `ghidra-gameplay{1..4}/`. Native code:
`crates/gdl_core/src/items.rs`, CLI `gdl_tools item-records`, comparator
`tools/python/compare_items.py`.

## Load order (AddItemInstList 0x1531d0)

RandomItemNum = vb_counter; SetPlayerVars; MaxItems = instances + 500; NewItem
per WORLDS instance (60 B: def i16 +0, +2 player count, +3 flags, +4/+6 words,
+8 name[16], +0x18 position, +0x24 rotation, +0x30.. type parameters);
MaxLevelCoins = shown (1,1) items; ProcessItemWobjs; PlaceItem per slot;
MatchTransporters; LinkTriggers.

NewItem: matrix = identity, then WPitchMat3(rot.x), WYawMat3(rot.y),
WRollMat3(rot.z) (each skipped for |a| < 0x38d1b717; libm sinf/cosf; mul then
EE add/sub per row). CopyMat3 copies three quadwords, so record +0x4c receives
NewItem's uninitialised stack word (observed 1.0 or 0x01fffba0 in the fixture);
it is not a value to reproduce.

## Record (0x110 bytes)

Object block at +0x10 (matrix rows +0x10/+0x20/+0x30, translation +0x40,
attention position +0x50, collision position +0x60, render node +0x70,
offset flags +0x74), offsets +0xc8, +0xd4..+0xe5 common words, +0xe6 dyngrid
link, +0xe8 radius, +0xf0..+0x10f type-specific. Common/type fields are listed
in the SetItem port. Native pointers: +0x00 definition index; types 5/12 +0xf0
object index + 1; types 5/11 +0xf8 item slot + 1. Guest pointers (+0x70 render
node, +0x80 atree, +0x84/+0x88/+0x8c AnimInfo links, +0xc0/+0xc4) stay zero
natively; +0x74 (offset flags) and the AnimInfo scalars are values. The item
AnimInfo is at +0x84 in the enemy layout (+0x90 sequence count, +0x92
sequence, +0x94 frame count, +0x96 reset byte, +0x9c frame, +0xa4 start,
+0xb4 last time, +0xb8 loop word, +0xba mode; observed in the fixture's
Items array, 2026-10-22). export_live_items.py zeroes +0x70..+0xc7 — including
+0x74, which every live G1 record holds as 2 — so replays take those words
from the traced records instead.

## Item trees (InitItemInfoData 0x14fb68, SetItemGeo 0x151688)

LoadItems 0x14f7d0 (GauntWorldInit, LoadTowerAndSelect; InitItems clears both
at every LoadGauntWorldStart): item_anim = items/<WorldItemDesc> (world_data
+4, "levelG" in the fixture) only while BossType < 0, item_anim2 =
items/level<LevelItemDesc> (level_data +8, "G1") when the file exists.
InitItemInfoData sets every definition's +0x4c = AtreeMatchAnyHeader(+0x28
name, kind == 3) 0x14fa60: item_anim, powerup_anim (POWERUPS), item_anim2,
then for generators the 45 enemy_anim packages. SetItemGeo: +0x4c set →
AtreeInit (+0x80) and AnimateATree(+0xde, 2); otherwise the static object
(ItemFindMBObjectL1). Fixture: KEY's tree 0xdd7c30 and CHERRY's 0xdd9d74
lie in POWERUPS (powerup_anim 0xdbce40), EXIT_PORTAL's and the IT
generator's in item_anim 0x1909940; GRU/LOW generators and S_CROWS have none.
Native (2026-10-22): the item clocks follow that order (realm package,
POWERUPS, the level's own); before, POWERUPS was not searched, so keys, gems,
food, treasure and scrolls had no clock and never animated (the renderer
already fell back to POWERUPS for their models). Not modelled: the enemy
package fallback for generators (no G1 generator needs it). Statues (kind 4,
2026-10-22): SetItem's statue path calls SetItemGeo with
AtreeMatch(package, name, 1) — GAR_STATUE in the model of CritterTypeLoaded(7,
0), GOL_STATUE in enemy_anim[0x1d] (else CritterTypeLoaded(3, 0)'s model),
DEATHSTATUE1 (levels 0/1) or DEATHSTATUE2 (2/3) in enemy_anim[0x1e]; native
ItemWorld::statue_tree names it and setup adds those trees to the item tree
library (trace-A1-statue-v2: the gargoyle statue's awakening clip times its
+0xda countdown). Generator stage changes (DamageItem kind 3) swap to the
tree or model "GEN_<prefix><level>" (ItemGenDesc 0x151308) and delete the
item when neither exists (gameplay.md "Statues").
No label occurs in both POWERUPS and an ITEMS package; realm I and level I5
share BRIDGEPAD (I5 is a boss level, so only item_anim2 is loaded there).
SetItem calls SetItemGeo only for kinds other than 4 (statues aside) and 13, kind 12 only
for subtype 2, and never for an instance whose flag byte +3 has bit 2;
items created later (AddItemSub: drops, spawned pickups) get their tree and
clock the same way. Tower trace check (trace-tower-v1, step 0): the items
without an AnimInfo (+0x80 = 0) are 95 BRIDGEPAD, 41 EXIT_PORTAL (8 more
have one) and 53 S_CARPETFLY in the reference, the same records native
leaves without a clock.

SetItemGeo's tree branch (0x1516f0..0x151778, 2026-11-12): +0xdc = 0,
AtreeInit, then `sh 1, 0xb8` — the AnimInfo's loop word is 1 whatever the
clip's repeat word (InitAnim had stored the clip header's +0x24) — and for
+0xd8 bit 0 +0xde = ((+0xd8 >> 2) ^ 1) & 1, then AnimateATree(+0xde, 2).
POWERUPS' KEY, KEYRING and SCROLL clips have repeat 0, so without the write
they turned once and stopped (playtest 2026-11-12: "key rings not
rotating"; a key ring beside the player stood at frame 0 or 29 for 120
updates). Reference trace-G1-chest31-v3: +0xb8 = 1 in every item record at
step 0; after the fix all 295 G1 item records with a tree match it in +0xb8,
+0xdc and +0xde on loading, and the replays D2combat / G3combat, whose
dropped BOOSTICON / SCROLL differed at +0xb8 in r95, now match on all 300
updates (regress r96: 146 replays, the other 144 equal to r95). Hazards
keep 0: their first AnimateATree restarts clip 0, whose InitAnim replaces
the 1 (also so in the reference).

Fire traps (FLAMEH, ITEMS_G): no animation channels; the flame is the tree's
type-4 node, a particle system. AnimateNode 0x112720 switches such a node
when the AnimInfo's option bits (+0x13; item +0x97) have bit 4: flag
0x200000 (emission off) in sequence 0, cleared in any other. SetItem's tail
(0x1529e0) sets that bit for kind 8 (hazards). The client emitted the flame
whenever the trap was on screen, so traps looked lit while off (playtest:
"fire traps stuck on"); gdl_play follow_tree_psys now applies the rule
(frames g1-flame-shots3: trap 110 shows its flame only in states 2/3).

## Player-count items (+0xe0, +0xe1)

SetItem stores +0xe0 (the record's player count; 10 + n = exactly n) and
+0xe1 = !show_item 0x151620 (NumVirtualPlayers, or 2 in attract modes,
against +0xe0). ProcessItems keeps +0xe1 as a state — 0 shown, 1 hidden, 2
waiting to show until off screen (keys, or with 0x20f954/0x20f958 at once)
— and sets the item tree's hide flag (MBTreeSetFlags 2) whenever +0xe1
becomes 1, clearing it when +0xe1 returns to 0; its main body skips records
with +0xe1 ≠ 0. B1 in single player: 61 of 127 items hidden (+0xe0 2, 3, 4
and 12: the extra chests, keys and barrels; trace-b1-newgame-v2). gdl_play
draws an item only while +0xe1 is 0 (it had drawn the multiplayer chests,
which could not be opened; scene comparison private/reports/scene-b1-v2-*).

## Item geometry names (SetItem → SetItemGeo, 2026-10-22)

SetItem 0x151878 picks the geometry name: the instance's own (+8, strncpy
16) when it has one, else the definition's +0x28; generators take
ItemGenDesc ("GEN_<prefix><level>", AtreeMatchAnyHeader(desc, 1) for a
tree: item_anim, powerup_anim, item_anim2, then enemy_anim[0..0x2c] —
MONSTERS/WAR's GEN_WAR0..3 are the only generator trees there, used in J3
and J4; gameplay.md "Level-start sweep"); rocks
(kind 10 type 0x29) "%s%d" of the definition name and +0xf2 (the stage, so
SAFEROCK3 at full health; SetSafeRock restages, −1 hides). SetItemGeo uses
the definition's tree (+0x4c) when there is one, else ItemFindMBObjectL1
0x151238: the name, then +"L1", then +"L1ROOT" (MBOX_FindObject_Sub). Every
kind gets geometry except 4 (statues aside), 13, 12 other than subtype 2
and instances with flag bit 2 (ItemWorld::geo); triggers (kind 5) draw
their tree (TRIGGER2 switches in every realm package) or model (I5's
BRIDGEPADL1). Models may live in the level's own table: E2's falling rocks
(kind 10 type 0x28, instances E2ROCKFALL#00..#20 in LEVELS/LEVELE2) are the
slabs covering Skorne's pit, falling from step 271 of trace-E2-boss-v2 (as
the reference frames ref-e2-rocks-260/290-v1 show); gdl_play's item models
search the level's table last (unknowns.md). gdl_play had drawn no
triggers, statues or rocks, and only
generators whose model has no "L1" suffix (GEN_RAT1 but not GEN_GRU1L1);
scene comparisons at reference cameras (private/reports/scene-*-ref.png /
-native.png; K5's three SAFEROCK vats, A1's GRU generator arches and
statues).

## Per-update matrices (ProcessItems loop 2, PlaceItem)

ProcessItems' second loop calls GetObjWorldMat(+0x10, +0xc8, +0xc8) 0x103450
for every item not 0xffff / 0x8100: with a render node (+0x70) the record
matrix becomes the node's world matrix (GetWorldMat) and UpdateObjOffsets
recomputes +0x60 and +0x50 from +0x40, +0xc8 and +0x74; without one, identity.
PlaceItem 0x153068 parents the item node to its floor object's node when the
object has a node (+0x28) and flag 0x1000 (MBNodeSetParent), then
UpdateObjWorldMat makes the matrix local — so items on moving floors follow
them. Kind 13 sound items keep FindWorldAnimNode(10.0, position) at +0xf8;
S_CROWS' +0x40 moves with the flock while +0x50/+0x60 stay at the anchor.
Fixture: 23 live G1 items have nodes parented to flag-0x1000 objects (node
+0x74 = the object's +0x28), several of them root objects without a +0x18
parent (229, 420, 817); every one of the 1,360 objects has its +0x28 node.
Native (2026-10-22): place_item records the floor parent and the node matrix
relative to it (doplayers::unparent_matrix, shared with UpdatePlayerWorldMat),
ProcessItems' second loop rebuilds the record matrix from the parent's
current matrix before the offsets. GauntWorldInit calls world_update (0x163338) right before
AddItemInstList, so the items are placed on the posed floors: native runs
that DoWorldAnimation in load_items' before-instances step (item 307's
local relative to object 821 then matches the fixture node's to 2e-5). The
same +0x28 reading corrected PlayerCheckMovingFloor and
OtherPlayerOnOtherMovingObject, which had required a +0x18 parent.

Matrix chains (2026-10-22, from the savestates' nodes): GetObjWorldMat is
GetWorldMat(record +0x70 node), which multiplies the node's local by each
ancestor's local in turn (MulMat4 0x1d55a0, VU0 multiply/accumulate) up to
the world root nodes: an identity with +0x60 0x1000 (MulMat4: −0.0 becomes
+0.0) under a translation-only identity (+0x60 4, add.s); unparented items
hang from a translation-only identity of their own. UpdateObjWorldMat's
UnparentMatrix 0x1d0620 is recursive — outermost level first, each level
inverting its own local (InvertMat4 0x1d4e80: transposed rotation,
translation −t · rowᵢ from vsub 0 − t and vmul by each input row, then the
sums x = (p0 + p1) + p2, y = (p1 + p0) + p2 and z = (p2 + p0) + p1 —
0x1d4ef4..0x1d4f08; native had z as (p0 + p1) + p2, an ulp off under K4's
tilted floors and I1's ice floes, corrected 2026-10-24). With world-animated parents whose
rotation carries scale (G1's 387: [1][1] = 0x3f80003e) the transpose is not
an exact inverse, so inverting the composed parent world instead drifts by
~2e-3 at a 300-unit translation: native now follows both chains
(collision::chain_world, unparent_chain) and its item node locals equal the
fixture's bit for bit (items 52, 307, 398, 420). Kind 13 sound items
(S_CROWS, S_CARPETFLY): SetItem stores FindWorldAnimNode(10.0, position)
0x11cd38 at +0xf8 (the world-animation object, over the entries with data,
whose GetWorldMat translation is strictly nearest below 10; FPU distance and
SQRT.S), and ProcessItems case 0xd copies that node's GetWorldMat into the
record matrix before the distance test; the render node stays where SetItem
(or PlaceItem, which skips definitions with +10 bit 0) put it, so loop 2
rebuilds the record — and +0x50/+0x60 — from that anchor first. SetItemGeo
starts a tree's clock at once: AtreeInitSub's InitAnimInfo 0x110750 (zeroed
AnimInfo, reset byte 1, +0x28/+0x2c 1.0 — record +0xac/+0xb0 — and
InitAnim(0) with the library present), +0x38 (record +0xbc) the tree's node
count, then AnimateATree(+0xde, 2). +0xfe of a sound item is the audio
driver's (ProcessItems 0x156d2c..0x156dac around DoItemSound 0x15d2b0): the
track mask AudioFindTID found, or −tid while the IOP has not answered the
loop's request — AudioLOOP → AudioTrack4 returns AudioTempTrackSet's tid
(tid.165 0x2bd000 + 1, 1 after 0x3fff; the temp track's +0x14) and
AudioFindTID ORs the bits of the twelve audioTrackData tracks holding it
(none until AudioTrackSet, the IOP's reply, has run). trace-B5-combat-v2's
item 257: 0, then −84 for one update, then 2. Native (2026-10-24) stores −tid
for the update that starts the loop and a mask from the next (bit 0: the
IOP's track choice is not modelled, HYPOTHESIS; the comparison takes the
sign), and keeps a known track playing (AudioFindSoundTracks not modelled).

Load order (2026-10-22, from the savestates' node +0x74 links): AddItemInstList
runs ProcessItemWobjs between the NewItem loop and PlaceItem, so root elevators
(0x1000 without 0x1000000, read live from node +0x30) already sit at inity + y
when items are placed — G1ELEV3 (227, inity 17.42, starty −8) is at 9.42, and
item 308 (instance y 27.34) lands on the ledge at 27.27, unparented, as in
the fixture. Matrix objects (0x1000000) are tested by WorldDynCollide at the
pose CreateDynobjGrid cached in world_update (collision.md), i.e. before that
ProcessItemWobjs: the Tower's L1FLOOR_LINE10 (2874, child of L1ELEV02 2873,
starty −19) is still at its rest −13.27, so the BRIDGEPAD triggers 141 and
147 (instance y −10.66, −6.25) are parented to it with locals (−2.1484, 0.1,
−8.2344) and (−0.3047, 0.1, −8.9609) — the savestate's node locals — and
148 (−27.83) finds no floor (−27.73). Native follows both: 23 parented G1
items as in the fixture, and the Tower's three as in its savestate. The
load-time ProcessItemWobjs uses OnWobj 0 (inferred: nobody stands on the new
level yet) and the start state's SPF, start_of_stage and vb_elapsed (the
load-time values are not captured; they matter only for an elevator away
from its target and the fade transparency). The load-time world_update's
DoWorldAnimation is gated as at run time (NumInGamePlayers or game mode
0x8000, the trigger-camera delay, not paused or in help); GauntWorldInit
runs it before AddItemInstList's SetPlayerVars, so a level entered from the
select screen (the Tower of a new or loaded game: init_next_level after
LoadTowerAndSelect) does not animate — the Tower's items on object 1851
(67, 210, 211) have the savestate's locals only without that step, G1's 307
only with it. LevelInputs::load_in_game_players carries it (gdl_play: 0 for
the Tower from start_session; game_run.py --load-players 0).

AnimateATree calls DoAnimateTree(…, 1), whose AnimateNode passes every
type-1 tree node (AtreeNodeInit's +0x2c) to DoAnimation; DoAnimation
(0x10fd10..0x10fd2c) turns a nonzero AnimInfo reset byte (+0x12; item
+0x96) into 2, and CalcAnimInfo clears 2 on the next update. Native applies
it to item clocks whose tree has a type-1 node (631 of the 679 item trees in
the 27 item packages; none has a type-1 node with +0x34 < 0, the null-cache
case). Before, the loop wraps of the Tower's crystals and lights left +0x96
at 1 (reference 2). Enemy clocks stay at 1 in every reference sample (their
trees reach no DoAnimation in the update) and native leaves them.

World numbers come from worlddatas (EE 0x23deb0; 14 × 44 B, number +0, letter
+0xf): castle 1 … town 7 (G, Poison Fields) … tower 0xd. SetItem's generator
overrides apply to worlds 5 (temple) and 6 (hell), not to Poison Fields.
FindInfoIndex substitutes KEYRING for keys with count > 1 and LOW for type-3
generator descriptions.

Type-5 triggers bound to an object call AddItemWobj (item_wobjs, ≤ 0x95; inity
= the object's MB node +0x34, then y/starty/endy ×0.1, snd) and set object
+0x10 |= 0x100000 (|0x10000000 when 0x800), +0x14 trigger, +0x16/+0x17 = 0.

## Verification (fixture, synthetic only where stated)

`private/reports/items-compare-g1-c.json` against the user save's live Items
array (519 records): 519 slots, 0 definition mismatches, 0 unsupported paths,
393 records byte-exact over the compared ranges (384 in -b, before SetItem
used the ResolveWorldData multipliers; no new mismatch groups); world_has_gates 2/2; 16/19
item_wobjs exact; every object flag bit natively set is also set in RAM.

The fixture is mid-play, so remaining differences are reported, not masked:
generator counters/timers (+0xda, +0xf2..+0xfe), +0xd8 bit 0x4000 and +0xe1 on
items the game has since processed, sound ids of type 13 (AudioFindSound needs
the audio data parser), items on dynamic floors (dynamic objects are not in the
native collision world) or on animated nodes, and three wobjs whose objects have
world animations (node y 8.3038 vs loader 11.7344). A fresh-load capture would
remove the mid-play ambiguity; it has not been taken.

Hazard (type 8) active times use RandInt (decision 0002) and are excluded.

## Items carried from the start (DoEnemyPickupItems, 2026-10-23)

GauntWorldInit runs DoEnemyPickupItems 0x15a4d8 after SetupDynGrid and
CreateDynobjGrid: every kind-4 item gets +0x102 = EnemyPickupItem
0x15a340(+0xf0 enemy type, +0x40 position). For types 0x20 (gargoyle),
0x21 (general) and 0x1d — the critter generators — it walks the item grid
(StartItemGrid(−1.0) / NextGridItem order) for an active (+0xd8 ≠ −1, no
0x8100 bits), shown (+0xe1 0) kind-1 item whose horizontal fqdist beats
the best so far (from 2.0) with a height difference under 3.0; that item
is carried — +0xe1 = 10, its tree hidden (MBTreeSetFlags 2) — and its slot
returned (else −1). The generator's critter later drops it (+0xb54 →
StartBagFX at death; item_runtime). Ported in gdl_core at the end of
world init (instructions read); before this the pickup stayed visible
and collectable under the statue. Check against the reference: the G1
fixture (live-items-g1-fixture-v1, poison-fields-original) holds generator
76 → item 416 and 88 → 239 with both items at +0xe1 10; the native load
computes exactly [[76, 416], [88, 239]]. game-run's report now lists the
links (carried_at_load): 38 of the 61 levels have such pairs (scratch
scan). The whole-update replays seed their items from captures, so they
were not affected by the missing port.


## D3, I2 and B2 level starts (2026-10-23)

Three WARPED captures reached through do_exit's wave skip from slot 3
(player 0 written onto B1's exit; reference_warp.py): w4l3-start-v1 (D3),
w9l2-start-v1 (I2), w2l2-start-v1 (B2), plus w4l3-leaf-v1 (D3 with player 0
written to (31, 47.4, −64), under three falling leaves). Traces
trace-D3-start-v1, trace-D3-leaf-v1, trace-I2-start-v1 and
trace-B2-start-v1 (idle) replay through the whole update (game_run.py
--frame-replay): items, players, effects, critters, world animation and the
wizard equal in every update (D3 120/120 and 250/250, I2 120/120, B2
120/120); the camera is 119/120 in D3 (the stage-start zoom step, unknowns
"Stage-start zoom step"). In the leaf trace the three kind-10 subtype-0x31
items (radius 15, height 20, definition +0xa bit 0: not placed) take +0xd8
bit 0 at step 190, once the stage start ends, fall through the floor and
are deleted (+0xd8 0xffff) at steps 438–442, every update equal; native
plays AudioLeafFall for them (sound.rs). D3's kind 12 subtype 0 item (324)
and I2's kind 12 items stay passive in the idle traces and equal.

Load comparisons (compare_items.py → compare-items-D3-start-v3,
-I2-start-v2, -B2-start-v2): 464/464, 599/599 and 198/198 slots, no
definition mismatch. The remaining differences are play-time or post-load
words the load-only `item-records` tool does not run: items on D3's and
I2's rising platforms (y lower by the wobj's starty −9/−10/−20/−40 in the
reference; the native game run's GetObjWorldMat puts them there, e.g. D3
item 119 at y 55.31 from update 0), DoEnemyPickupItems' +0xe1/+0x102,
LinkTriggerToCam's +0x102 on kind-5 triggers, kind 13 sound words, generator
timers, play bits of +0xd8, and WorldPsysActivate's 0x800000 on particle
objects (unknowns "Object flags at load").

RandomItemNum: ProcessItems 0x154f50 adds 1 every update, so a capture taken
U updates after the load holds vb_counter_at_load + 0x1b7 × calls + U. The
comparator had subtracted only the RandItemIdx calls; B2's two random items
(119 from definition 54's six choices, 149 from definition 100's four)
then disagreed for 119 (native CHERRY, reference BANANNA). With
--updates-since-load U the derived seed is final − 0x1b7 × calls − U; U = 5
for B2 (and 6 for D3/I2) is vb_counter − final + 0x1b7 × calls, which holds
when vb_counter advanced by 2 per update and not otherwise since
AddItemInstList (HYPOTHESIS; it makes both B2 picks equal). RandItemIdx's
index is the item slot ((record − Items) × 0xf0f0f0f1 >> 4, the inverse of
17 for 0x110-byte records); do_see_thru calls it with advance 0.

Unparented item translations: GetWorldMat of an item node without a floor
parent goes through the item's own translation-only root (add.s), so a −0.0
instance coordinate becomes +0.0 (I2 item 257, a generator at instance x
−0.0, holds +0.0 in w9l2-start-v1). ProcessItems' matrix step now applies
that add to the record translation (item_runtime.rs); before, native kept
−0.0 for the whole level. D3's BRIDGEPAD 353 (matrix [1][0] −0.0 at load)
is floor-parented and already became +0.0 through chain_world.

## Item culling under the boss and trigger cameras (2026-10-24)

ProcessItems sets +0xd8 bit 0x4000 from ItemVisible 0x153ad0 (outside the
stage start): MBWorldSphereVisible(+0xe8, +0x50) against the MB window's
camera, then, while StdCam ≠ 0 and newcam_mode is 1 or 2, the squared
distance from StdCam +0xa4 (its target) against ITEM_ACTIVE_DIST
(instructions read, ghidra-items5). ItemCollSub tests an item only with
+0xd8 & 0x4040, so a culled trigger, gate or key cannot be touched. The
window camera is whichever camera last ran MBCameraUpdate — StdCam, the
boss camera or a trigger camera. Native built the culling view from the
window's rows but the StdCam record's eye, which the boss camera never
updates: in I5 the raft's pad (trigger 40, flags 0x50a, bound object 111)
stayed culled from the bot's approach, so the raft never left and the
yeti's arena was unreachable on foot (bosses.md: "I5's on-foot route to
the arena is still not shown"). sync_view now takes the window's eye
(pb eye, written by every MBCameraUpdate); with it the traversal bot walks
from the start onto the pad, rides the raft (~180 updates, about 12.8 units
south) and enters the arena (game-run, bot route hint to item 40).
