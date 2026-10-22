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
upstream description, checked only structurally: the private formatted copy
`private/reference/config/PCSX2/memcards/Mcd001.ps2` parses (stride 528,
empty root). The user's own `~/.config/PCSX2/memcards/Mcd001.ps2` and
`Mcd002.ps2` are unformatted (all 0xff / identical hashes) and hold no saves,
so no original save file has been decoded; the playable character comes from
the savestate capture instead. Both cards were only read.

## Native saves (gdl_core::save)

- One JSON envelope per character, `save%04d.json` (the original's slot
  numbering): format "gdl-native-save", schema 1, revision SLUS-20047, slot,
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
ignored, raw migration, schema/revision refusal, options round trip; the
profile round trip new character → play changes → region → reload.

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
characters and Sumner, name entry rules of the original, DirInfo.
