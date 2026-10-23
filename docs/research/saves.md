# Characters, saves and the front end (M17/M18)

Revision: SLUS-20047 (profiles/ps2/slus-20047.json). Addresses are EE
virtual addresses in SLUS_200.47. Exports: private/reports/ghidra-fe1,
ghidra-fe2, ghidra-save2, ghidra-plmodel, ghidra-plmodel2, ghidra-atts.

## Original save path

- `getSaveFileName` 0x107a80: `/BASLUS-20047GDL/BASLUS-20047save%04d`
  (slot + 1), `…GameOpts` (slot −2) and `…DirInfo` (slot −1).
- `saveSave` 0x109420 writes the slot file, then `GameOpts` (0x20 bytes from
  `GamePrefs` 0x21edb8). `saveLoad` 0x109630 reads at most the requested size.
  Files are raw: no header or checksum is added by these functions.
- `PlayerWriteSaveFile` 0x130b28: `player_store_in_save`, then `saveSave` of
  player +0x99c, 0x1074 bytes; +0x9a7 = 1. Its only caller is
  `do_player_select` state 0xe, reached from state 0xd (choose a file).
- In game, the Tower menu `tower_menu` 0x21d1b0 entry "Manage Character"
  (id 0xd) calls `init_player_select(1)` (DoOptions 0x10d4e0), whose first
  step is `player_store_in_save`; that is the in-game route to saving.
- "Quit Game" (0x25) sets `quit_game`; game_main → `init_over` 0x127e70:
  game mode 0x4014, every player `inactivate_player` → `clear_player(k, 1)`.
  Nothing is written: progress since the last save is lost.
- "Quit Level" (0x26, not in world 0xc) sets `quit_level`; game_main calls
  `abort_player` for all four players (`player_set_intower`).
- `PlayerLoadSaveFile` 0x130b88: `saveLoad` into +0x99c, `OptionsSetup` when
  the prefs were just loaded, `player_get_from_save(−1)`, +0x9a7 = 1, the
  region copied to the backup +0x1a28, +0x1910..+0x1a10 &= 0xf0, then
  `change_player(k, class)`.
- `PlayerSaveState` 0x130770 / `PlayerRestoreState` 0x130908 copy the region
  to/from the backup at level start (`init_next_level`: the Tower without the
  backup copy, the Underworld 0xc not at all) and on `kill_player`.

## Save region (player +0x99c .. +0x1a10)

| Offset | Meaning | Evidence |
| --- | --- | --- |
| +0x99c | name, 8 bytes ("PELE") | capture |
| +0x9a4 | class (i16) | player_store_in_save |
| +0x9a6 | colour (i8) | player_store_in_save / player_get_from_save |
| +0x9a7 | saved flag (0 at level start, 1 after load/write, 0xff new) | PlayerSaveState, new_player |
| +0x9aa | sum of ExpToLevel over the 16 class entries (i16) | player_store_in_save |
| +0x9ac + 0x18c | per class: exp, health (f32), 4 bonus attributes (f32) | save/load_player_atts, PlayerUpdateAtts |
| +0xcec + 0xb4c | per class record: potions, keys, shards, runes, wave/class bits (+0xc..+0x12), gargoyle items (+0x18), crystals (+0x1e), powerup flags (+0xa), gold (+0x30), powerups (+0x34..+0xb4) | store/get_from_save, tower.rs |
| +0x182c + 0xe c + w | completed-level bits per class and world | PlayersSetWaveFinished |
| +0x190c..+0x190f | PlayerControl option bytes (+0x2c, +0x30, +0x38, +0x34) | store/get_from_save |
| others | statistics (+0xb40 + 0x1c c …) and unknown bytes | kept byte for byte |

The per-class record ends exactly where the wave bytes begin (0xcec + 16 ×
0xb4 = 0x182c), so the powerup copy (+0x34..+0xb4) stays inside its class.

## Character records (gdl_core::game::profile)

Ported without the Sumner branches (player_sumner_desc is a runtime pointer):
`init_players`, `clear_player` (its class is `def_char_type(9)` — the loop
counter — so always 6, as the original does), `init_player_powerups`,
`set_player_default_atts`, `PlayerUpdateAtts` (attribute = min(base +
(level − 1) × 5, cap) + bonus, at most 999; PDAT pairs +0x28/+0x2c,
+0x38/+0x3c, +0x40/+0x44, +0x30/+0x34), `check_player_atts`,
`load_player_atts` (derived words computed before the level changes, as in
the original), `save_player_atts`, `player_store_in_save`,
`player_get_from_save`, `change_player`, `new_player`, `PlayerSaveState`,
`PlayerRestoreState` and the record part of `init_next_level` /
`load_player_geo` 0x130d40 / `load_player` 0x12f748 (`level_start`).

Class data: PDATA/<TAG>.WAD (LoadPlyrData 0x14d9f0) holds SFXX, DAMG and
PDAT chunks; the PDAT chunk is 0x180 bytes in all sixteen files. Model
packages: `players/<desc>/<colour><level/10>0` for classes with multiple
models, class clips `players/<base desc>/anim`, effects
`players/<base desc>/sfx<colour>` (load_player_model_sub 0x12f120,
load_player_model 0x12f378). Colours: player_color 0x220e00 {yel, blu, red,
gre}. Thrown missile tree: `<PlayerMissileDesc>_THROW<c>` with c =
PlayerMissileDesc[class][4 + level/10]; '0' takes the tree from the model
package, otherwise from the SFX package (InitPlayerMissiles 0x18efe0).

### Actual-data checks (profile-check)

`gdl_tools profile-check GAME_START.json SLUS_200.47 SOR/ANIM PDATA×16`
rebuilds the captured player 0 from its save region (PlayerLoadSaveFile path,
level_start for G1, PlayerSaveState) and lists every differing word against
the captured record (output: private/reports/profile-check-pele-v1.txt):

- Equal: the whole save region except +0x9a7's backup copy, class/colour,
  attributes +0x108..+0x114, derived words +0x118..+0x12c, gold, keys,
  potions, shards, runes, controller options [0, 2, 1, 1] (also equal to the
  captured PlayerControl words), the 149-entry action map at +0x1e8, the
  name "SOR_YEL" at +0x698, the class-record fields +0x814/+0x820/+0x828/
  +0x82c and the 1.0 scales.
- Different (runtime, set during play or by unported presentation code):
  position/matrices, animation tree state +0x90..+0xd8, health (483 vs the
  stored 500), experience (80 vs the stored 1), +0x800/+0x804 (100.0 vs 0 —
  set by an unidentified path after clear_player), +0x7b8..+0x7d0, +0x878
  (320 vs load_player_geo's 256), +0x890/+0x898/+0x8d4/+0x8e4/+0x8f0/+0x8fc,
  +0x93c, +0x974..+0x980, the backup's statistics and the select-screen words
  +0x2ac4..+0x2ae8.
- The SOR PDAT equals the captured class record except the pointer words
  +4/+8/+0x24.

## Memory cards (gdl_formats::memcard)

Read-only reader for PCSX2 .ps2 images (superblock, two-level indirect FAT,
512-byte entries; ECC spare area skipped, not checked). The layout is an
upstream description, checked structurally and against cards the original
wrote. The user's own `Mcd001.ps2`/`Mcd002.ps2` are unformatted and were only
read (hashes before the card experiments:
private/reports/user-cards-before-scratch.sha256).

### A card the original wrote (2026-10-22, actual reference)

`launch_reference.py --scratch-card1 scratch-save-v1.ps2` boots the reference
with a new scratch card in slot 1 (slot 2 disabled). PCSX2 creates the
missing file as an unformatted 8 MB card; the game offers to format it at
boot ("Yes" → "Format succeeded"). A new Sorceress "JAKE" was created and
saved through Tower Menu → Manage Character → Save (reference_driver.py run
private/runs/reference-save1/). Captures: card-scratch-created-v1.ps2 (just
formatted), card-scratch-jake-v1.ps2 (after the save); extraction
private/reports/card-scratch-jake-v1/ (memcard-inspect).

- `/BASLUS-20047GDL` holds icon.sys, view.ico, copy.ico, delete.ico,
  `BASLUS-20047DirInfo` (0x80 bytes), `BASLUS-20047save0001` … `save0008`
  (0x1074 bytes each; an unused file is all zero) and
  `BASLUS-20047GameOpts` (0x20 bytes).
- save%04d is the raw save region (player +0x99c..+0x1a10) with no header or
  checksum, as saveSave's instructions say; the card copy carries +0x9a7
  as it was before PlayerWriteSaveFile set it.
- DirInfo: eight 16-byte entries {u32 level sum (+0x9aa), u32 class,
  name[8]}; unused entries are `ffffffff ffffffff "Empty"`. JAKE's entry is
  {16, 6, "JAKE"}.
- GameOpts: 80 00 00 00 80 00 00 00 01 00 … 01 (sfx 128, music 128, stereo
  1, 0, 0, 1, 0, 0) — the same block the native client uses as its default.

### Loading it back (actual reference vs native)

- `gdl_tools profile-load-check REGION LIVE_PLAYER SLUS_200.47 PDATA×16`
  runs the native PlayerLoadSaveFile path (profile::load_character) on the
  card's save0001 and compares with the original's player 0 record dumped
  right after its own load (reference_driver.py `dump`): the save region,
  the live attribute words +0x1a10..+0x1a28 and the backup copy
  +0x1a28..+0x2a9c are equal; only runtime words (matrices, animation, the
  select screen's +0x2ac4..+0x2af8) differ.
- Repeated: Tower → Manage Character → Quit → title → Start → Load → slot 1
  → JAKE → Done, twice in one boot. The second load
  (dump-player0-loaded3.bin) equals the first over the region, attributes
  and backup (0 differing bytes) and also passes profile-load-check.
- Quit with the saved flag +0x9a7 = 0 (cleared by PlayerSaveState at the
  Tower's level start) shows "Character Not Saved. Quit Anyway?" Yes/No with
  No selected; the native select screen has the same prompt
  (pselect.rs). Quit writes nothing: the card file's timestamp did not
  change across both quits.
- The Tower and Manage Character menus keep their cursor between openings.
- New character: `gdl_tools profile-new-check JAKE 6 0 save0001 …` builds
  the region with profile::new_character + save_region; it differs from the
  card only at +0x9a7 (card 0: PlayerSaveState at the Tower's start; native
  0xff, new_player), +0xa3c (class 6 experience: card 1, native 0 — the
  original reached 1 between creation and the save; source not identified,
  unknowns.md) and +0xbee/+0xbef (+0xbec below).
- +0xbec (f32) counts up while the character is in play (844 at the save,
  15248 four minutes later); HYPOTHESIS: a play-time counter. Kept byte for
  byte.

### Import (gdl_core::save::import_card)

`SaveStore::import_card` / `gdl_tools card-import CARD.ps2 SAVE_DIR
NEW_REPORT.json` / `run_play.py --import-card CARD.ps2`: reads DirInfo,
save0001..0008 and GameOpts without writing the card. Each non-empty region
with a valid class becomes a native slot (its own number when free, else the
next free slot), source "imported:card:<card sha256>:save%04d"; all-zero
files are reported empty; wrong sizes and classless regions are refused; a
region already in the store is not imported twice; DirInfo is compared with
each region (name bytes, class, level sum) and reported, not trusted;
GameOpts goes to options.json only when the store has none. The PS2 import
gate (M18) needs checksum and progression semantics: the files carry no
checksum, and the imported region loads into the same record the original
builds (above). Actual-data run: card-scratch-jake-v1.ps2 → slot 1, DirInfo
agrees, seven empty files, GameOpts imported
(private/reports/card-import-jake-v1.json; the card's SHA-256 is unchanged),
a second import adds nothing. In gdl_play (private/runs/client-cardjake):
title → Start → Load → slot 1 → JAKE (first row, as on the PS2) →
Sorceress level 1, 200/400/150/600 → Done → the Tower.

## Native saves (gdl_core::save)

- One JSON envelope per character, `save%04d.json` (the original's slot
  numbering; the select screen's card files 0..7 are save0001..save0008 —
  interim builds before 2026-10-23 used save0000..save0007, and
  `migrate_slot_zero` moves a save0000 to the first free card slot at start,
  keeping the old files as migrated-save0000.*): format "gdl-native-save", schema 1, revision SLUS-20047, slot,
  source ("new", "imported:capture:<sha256>", "migrated:raw:<sha256>"),
  sequence, the 0x1074-byte region (hex) and its SHA-256.
- Writes: `.tmp` written and flushed, the old file renamed to `.bak`, `.tmp`
  renamed into place, directory flushed. Loads verify format, schema,
  revision and checksum, then fall back to a complete `.tmp`, then `.bak`,
  reporting the recovery.
- Schema 0 = a raw original save file (0x1074 bytes, what a card holds);
  `import_raw` migrates it into a new slot. Newer schemas and other
  revisions are refused, not reinterpreted.
- `options.json`: the GameOpts block. Default when absent: the captured
  session's GamePrefs (sfx 128, music 128, stereo 1, 0, 0, 1, 0, 0) — the
  ELF image holds zeros there.
- Default directory: `$XDG_DATA_HOME/gdl-native/SLUS-20047/saves`
  (run_play.py `--save-dir` overrides).

Tests (synthetic): store/backup/sequence, a corrupted primary recovers from
the backup, an interrupted write recovers from `.tmp`, a truncated `.tmp` is
ignored, raw migration, schema/revision refusal, options round trip, the
slot-0 migration, card import (one character, empties, re-import, an
occupied number, a disagreeing DirInfo, refused sizes/classes, a card
without the game's directory, on memcard::synthetic_image cards); the
profile round trip new character → play changes → region → reload.

Recovery in the client (2026-10-23): SaveStore records each slot it loaded
from `.bak` or a leftover `.tmp` once per session (`take_recoveries`); gdl_play
opens a message box in the original scroll-box style — "NOTICE (native) …
slot N was damaged. It was restored from its backup copy / an interrupted
save." — for any load path (the original select screen's Load through
NativeCard, or a direct start), and the status line names the file and the
reason. The original has no such message (its text ROM holds only the card
space/format messages), so the text is labelled native.

Client scenarios (tools/python/scenarios.py, GUI on the private Xvfb, report
private/reports/scenarios-m18-v2.json; actual disc): two saves of card file 0
in one session (in G1 before the exit, then in the Tower after it), then the
save directory is damaged and a new client loads slot 1:
- recover-corrupt: one hex digit of the primary's region flipped (checksum
  fails) → the earlier save from `.bak` (G1 not completed), notice shown.
- recover-interrupt: the primary renamed to `.tmp` — the state a crash leaves
  between write_atomic's two renames (`.bak` = the old save, `.tmp` = the
  complete new one) → the new save (G1 completed), notice shown.
Both pass.

Profile ownership in co-op (2026-10-23, scenarios-m18-v4 coop-ownership,
GUI, actual disc): card files 0 and 1 hold one character with health 500
and 123; player 1 starts from file 0 and player 2 joins on the second
keyboard set (U start, R cross): Load → memory card slot 1 → the file list
shows file 0 unavailable — verify_vmu_file_ok refuses a file another active
player owns (+0x2af4) — and selects file 1; player 2 then plays file 1's
character (health 123, port 2) while player 1 keeps file 0. Fixed for it: a
native slot load (the TEST autostart, the native save list) now records
player 1's card file as PlayerLoadSaveFile does; before, the file showed as
free to player 2.

## Front end (gdl_client::menu, gdl_play)

Original menu tables (ELF): title "Press Start" (0x2abed0) → title_menu
0x21ce30 {Start, Options}; title_options 0x21cff0 {Audio, Game Options,
Compass, Controls, Screen}; tower_menu {Settings, Manage Character, Shop,
Inventory, Quit Game}; game_menu {Settings, Quit Level}; tower_options
{Audio, Compass, Controls, Screen}; hint_menu {General Hints, Guardians,
Items of Legend, Runestones}. Text ROMs (gdl_formats::textrom; ENGLISH.ROM 194 entries, SCROLL_E.ROM
57, HINTS_E.ROM 45; dumps private/reports/text-rom-*-v1.txt): MB containers
with FONT names (20-byte records), TEXT lines, TOFF line offsets, STRS
{line count, first line, font, scale} per entry, entry names in DEFS via
SDEF offsets, and LIST/LOFF/LDEF named lists. The class and colour names
and "SELECT A CHARACTER" / "TO BUILD A CHARACTER" / "ENTER INITIALS" come
from ENGLISH.ROM. In game, NewHelp 0x18e710 ids go through helptab 0x24bf08
(256 × 0x1c: +0x10 entry, +0x14 line or −1 for all lines; the capture holds
the same values, num_helps 256) to ENGLISH.ROM lines (e.g. help 0 →
USEMAGIC, 0x22 → LEVELUP); scroll items (ScrollMessageBox) show
SCROLL_E.ROM entry n (HYPOTHESIS: the runtime ScrollText array, 0x44-byte
records, follows the ROM's entry order) in a box that pauses the game until
a button is pressed. PRESENTATION: hints show for 3 s and an id repeats
after 20 s at the earliest (help_displayed/help_interval not ported).

Hint menu: Sumner's hint pad (HintMenu event) opens hint_menu's four
categories, mapped to HINTS_E.ROM lists GENERAL_HINTS, BOSS_HINTS,
LEGEND_HINTS and RUNE_HINTS; every hint is paged in list order (NATIVE:
next_general/boss/legend/rune_hint's progress-based choice is ported: hud.md).

Title: TITLE00–03 assembled into the 512×384 title picture behind the
menu (alpha forced opaque). NATIVE presentation (labelled): text menus instead of the SELECT/TITLE
scenes, the save list, the import entry, "Save Character" in Manage
Character, the quit confirmation, volume steps. Not ported: Shop, Inventory,
Compass, Controls, Screen, Game Options, hint menus, credits, the secret
characters and Sumner, name entry rules of the original, writing DirInfo
(the native card view derives names, classes and level sums from the
regions; DirInfo is read on import).
