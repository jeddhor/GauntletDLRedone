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
package fallback for generators (no G1 generator needs it) and the placed
DEATH (kind 4) atree that SetItem's statue path takes from the enemy package.
No label occurs in both POWERUPS and an ITEMS package; realm I and level I5
share BRIDGEPAD (I5 is a boss level, so only item_anim2 is loaded there).
SetItem calls SetItemGeo only for kinds other than 4 and 13, kind 12 only
for subtype 2, and never for an instance whose flag byte +3 has bit 2;
items created later (AddItemSub: drops, spawned pickups) get their tree and
clock the same way. Tower trace check (trace-tower-v1, step 0): the items
without an AnimInfo (+0x80 = 0) are 95 BRIDGEPAD, 41 EXIT_PORTAL (8 more
have one) and 53 S_CARPETFLY in the reference, the same records native
leaves without a clock.

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
translation −t · rowᵢ from vsub 0 − t). With world-animated parents whose
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
driver's: AudioLOOP's queue id (negated while pending) or AudioFindTID's
one-bit track mask; native keeps 1 while playing (not modelled: the IOP
queue and track allocation).

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
