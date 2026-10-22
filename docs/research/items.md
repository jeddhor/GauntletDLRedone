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
current matrix (parented_matrix) before the offsets; the +0xf8
world-animation node of kind 13 is not modelled (unknowns.md "Item world
matrices"). GauntWorldInit calls world_update (0x163338) right before
AddItemInstList, so the items are placed on the posed floors: native runs
that DoWorldAnimation in load_items' before-instances step (item 307's
local relative to object 821 then matches the fixture node's to 2e-5; 21
items parented natively, 23 in the fixture — item 308 is parented to
object 227 natively but not in the fixture, open). The same +0x28 reading corrected PlayerCheckMovingFloor and
OtherPlayerOnOtherMovingObject, which had required a +0x18 parent.

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
