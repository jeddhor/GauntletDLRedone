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
object index + 1; types 5/11 +0xf8 item slot + 1. Presentation pointers
(+0x70..+0xc7, type 2/13 +0xf8) stay zero natively.

World numbers come from worlddatas (EE 0x23deb0; 14 × 44 B, number +0, letter
+0xf): castle 1 … town 7 (G, Poison Fields) … tower 0xd. SetItem's generator
overrides apply to worlds 5 (temple) and 6 (hell), not to Poison Fields.
FindInfoIndex substitutes KEYRING for keys with count > 1 and LOW for type-3
generator descriptions.

Type-5 triggers bound to an object call AddItemWobj (item_wobjs, ≤ 0x95; inity
= the object's MB node +0x34, then y/starty/endy ×0.1, snd) and set object
+0x10 |= 0x100000 (|0x10000000 when 0x800), +0x14 trigger, +0x16/+0x17 = 0.

## Verification (fixture, synthetic only where stated)

`private/reports/items-compare-g1-b.json` against the user save's live Items
array (519 records): 519 slots, 0 definition mismatches, 0 unsupported paths,
384 records byte-exact over the compared ranges; world_has_gates 2/2; 16/19
item_wobjs exact; every object flag bit natively set is also set in RAM.

The fixture is mid-play, so remaining differences are reported, not masked:
generator counters/timers (+0xda, +0xf2..+0xfe), +0xd8 bit 0x4000 and +0xe1 on
items the game has since processed, sound ids of type 13 (AudioFindSound needs
the audio data parser), items on dynamic floors (dynamic objects are not in the
native collision world) or on animated nodes, and three wobjs whose objects have
world animations (node y 8.3038 vs loader 11.7344). A fresh-load capture would
remove the mid-play ambiguity; it has not been taken.

Hazard (type 8) active times use RandInt (decision 0002) and are excluded.
