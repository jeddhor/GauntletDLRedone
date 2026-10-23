# Interim native build (not the M12 slice)

`python3 tools/python/run_play.py` launches `gdl_play` on the current desktop
(`--virtual` uses the authenticated Xvfb session). It writes
`private/reports/play-config.json` (the Tower and every realm, shared and
level packages resolved by verified archive identity) and runs the native
game simulation (research/gameplay.md). Character packages (model per level
tier, class clips, SFX package, PDATA) are resolved by the client from the
same manifests, each file's decoded SHA-256 checked on load.

Front end (research/saves.md): "Press Start" → Start / Options. Start opens
SELECT A CHARACTER: saved characters, "Create a new character" (eight base
classes, four colours, up to seven letters/digits) and, until done once,
"Import PELE (PS2 savestate capture)" — the save region of the character in
the preserved savestate, copied into a new native slot (the capture is only
read). Every game starts in the Tower. Options follow the original menu
records: Audio (music and sound volume, options.json), Game Options
(difficulty, multiplayer mode), Compass, Controls (style, vibration, auto
aim, auto attack) and Screen (Screen Adjust).

In game, Esc / P / gamepad Start opens the original pause menu: in the Tower
Settings, Manage Character, Shop, Inventory, Quit Game; in a realm Settings,
Quit Level. Shop lists SHPDATA/SHOP.WAD's items with their prices ("-" marks
items make_skiplist excludes: unaffordable, full key ring/potions, full
health, attribute at 999); buying follows do_shopping (keys, potions,
powerups, +10 attribute bonuses, healing). Manage Character → Save Character writes the character to its
slot (player_store_in_save; the previous version is kept as `.bak`). Quit
Level returns to the Tower; Quit Game returns to the title without saving
(as the original, progress since the last save is lost). Saves live in
`$XDG_DATA_HOME/gdl-native/SLUS-20047/saves` unless `--save-dir` is given.

Realm portals in the Tower open per the character's progress (EnterTower,
HideExits); a defeated boss returns to the Tower. Packages load per level.

Co-op (M16 in progress): player 2 joins by pressing R (keyboard T/F/G/H
move, R throw, Y potion) or South on gamepad 2; gamepads 1–4 drive players
1–4 (D-pad or left stick, South throw, East potion). Joining goes through the
select screen (init_player_select(1)) with the player's own character and is
placed by load_player_geo / get_player_pos; the camera is the original StdCam
(research/camera.md).

Controls (PS2 buttons as measured, player.md "Pad buttons"; keyboard is a
native choice with K/L/I/J as the reference's PCSX2 bindings): arrows/WASD
move, Space/K cross (attack), E/L circle (magic), Shift/I triangle (turbo;
closes message scrolls), Q/J square, Z L1, X L2 (team move), Ctrl R1
(strafe), V R2, Enter start, Tab select; gamepads by their own buttons. F1
toggles the diagnostics block, stand still in the exit portal to leave the
level, F9 debug-skips to the next level. Death: do_players state 8 plays the
dying action, then kill_player — in a realm the player is out (keys
dropped, powerups cleared); with nobody left game_main goes to TowerWave and
the level-start state is restored (PlayerRestoreState); in the Tower the
player is restored at once. Message scrolls turn their pages on triangle
only, as in the original. Menus: arrows/WASD or D-pad, cross
(Enter/Space/K, gamepad South) to accept, triangle (Esc/Backspace/I/Shift,
gamepad North) to go back, circle (C/L/E, gamepad East) centres Screen
Adjust; Escape on the "Press Start" screen exits. The HUD shows colour/class, level, experience,
save slot, health, gold, keys, potions, kills, active enemies and standing
generators; F1 adds the interim labels and any unported path hit.

M12 walkthrough (2026-10-22, gdl_play on the private Xvfb through
tools/python/client_driver.py; frames in private/runs/client-m12a,
client-m12b, client-ex1..4, client-die4):
1. Start: title → Start → select: New, the default name, Sorceress → the
   Tower's WelcomeMessage (triangle turns the pages) and the crystal camera.
2. Play: all fifteen Tower crystals (counter, got-it panels, gem effects),
   UnlockLevel ("You now have enough Crystals to enter the Forsaken
   Province"), Sumner's hint pad, the gargoyle door's NeedGargItems message
   (repeats every 10 s of game time while the player stays on it, as
   TowerNeedGargItemsMsg arms its timer after the box), the realm door, the
   level circle (portal I open), the realm map, the level movie, G1: combat
   (kills, experience, a generator destroyed, "Collect potion before using
   magic"), a key pickup.
3. Finish: G1's exit portal (also reached by the warped reference capture
   ref-g1-exit-stand-v1: the same tally) → the tally (Stats), the shop, the
   level panel, the inventory, the select column; G2 entered by the debug
   F9 and left by Shift+F9 to the Tower.
4. Save: the select column → Save → slot 1 → "No Gauntlet save data ...
   Create 285KB Save File?" → Yes → the first file → Done → the Tower with
   portals I–III open.
5. Restart and recover: a new client on a copy of the save directory →
   Start → Load → slot 1 → JUSTIN (health 296 as saved) → Done → the
   Tower: the crystal set delivered, the floor crystals gone, portals I–III
   open.
6. Failure: dying in G1 (no input among the enemies) → the "In Tower"
   panel → TowerWave's skipped shop → init_next_level → the Tower at the
   realm's level circle with the level-start state restored (health 500,
   the level's experience gone).
Fixed on the way: triangle closes scrolls (measured buttons), the shop's
player panels (SetPlayerVars per mode), the all-dead loop between the
skipped shop and the select screen. Not compared with the reference beyond
the frames named here.

GUI checks: tools/python/client_driver.py drives gdl_play on the private
Xvfb (serve/send of key-hold steps, frames of the client's own window,
private/runs/client-TAG/); F2 (DEBUG) logs player 0's position and crystal
counts and the nearby items; F1 shows the position.
tools/python/scene_compare.py LEVEL SOURCE TAG puts a reference frame (slot
or staged capture) beside gdl_play at the reference's camera
(private/reports/scene-TAG.png); the 2026-10-22 set covers the first level
of every realm, the eleven boss arenas and A1's statues.

Audio: original sound effects, announcer lines and level music play through
ALSA (research/audio.md); mixing is approximate. `--virtual` runs are silent.

Bosses (research/critters.md): the native critter engine (boss AI, grab and
fire cones, the chimera's heads, BOSSGEN, death, coin spew, the good wizard;
statue golems, gargoyles and generals) with PlayerCollideCritters; critters
are drawn from their skeletons (AnimateATree passes blend 0, so critter
sequences never blend in the original either). The HUD shows the boss
meters.

Current state (2026-10-22): the simulation (do_players, do_enemies,
ProcessItems, ProcessEffects, world animation, the StdCam) is compared update
by update with reference traces of G1 combat and the Tower's new game — every
part equal on trace-fight-g1-v6, trace-combat-g1-v4, trace-varied-g1-v2 and
the Tower traces except sound items' audio handles (gameplay.md). The
paragraphs below up to the smoke tests describe the 2026-10-16..18 interim
build and are kept as history; the materials, item poses, enemy frames,
missiles and effects they call presentation-only have since been ported
(rendering.md). Still approximate: audio mixing (SPU curves), the GS's
per-draw integer truncation in lighting
(rendering.md "Tower lighting comparison"). Smoke test 2026-10-22 (Xvfb):
attract → title → Load → the Tower, the welcome scroll pages, crystals
collected with their pickup effects and the crystal counter.

Start: player records are built natively from the save region
(game::profile: PlayerLoadSaveFile / new_player + change_player, then the
level-start record steps); the session globals, culling plane constants and the
player-simulation template still come from the state exported from the
preserved save (`export_game_state.py` v6 + `export_start_state.py` v2,
INTERIM). Every level enters at its start locator.

Native and reference-compared components: controls, motion code/run timer,
request, facing, static/dynamic collision and position (movement.md,
collision.md); item load records (items.md); attribute formula (gameplay.md);
the gameplay camera (camera.md).
Everything else in gameplay.md is a transcription without a reference
comparison yet.

Presentation only (labelled on screen): unlit materials with alpha cutout,
item models at tree rest pose (camera-facing "CF"
nodes omitted), enemy per-frame objects chosen by matching object stems to
sequences through their OANIM entries, missiles drawn
with the character's `<desc>_THROW<n>` tree at rest pose (textures that live
in another package show magenta, e.g. the Sorceress's WND_THROW1), hit sparks
and magic blasts as spheres,
moving world objects (elevators' item-driven y, rotators) are drawn by
displacing each node relative to the nearest moved object among itself and its
ancestors (rest matrix → current matrix); visual rotation not yet observed in
a run.

Smoke tests on Xvfb (llvmpipe, 2026-10-16): G1 launch with enemies swarming;
throws (7 missiles, 6 kills); key pickup; F9 transitions to G2, G3 and G4 with
geometry, generators and items rendered.

Realm smoke test on Xvfb (2026-10-17): `--realm A` starts in A1 with the
castle's enemies, chests and keys rendered; F9 carries the player into A2
(LEVL record 2) at its start locator; `--realm G` still starts G1 from the save.

Boss smoke test on Xvfb (2026-10-17): `--level G5` shows the lich animating in
its arena with the boss health line; headless runs cover activation, attacks,
BOSSGEN generators, death, coin spew, the good-wizard timing and the exit.

Front-end smoke test on Xvfb (2026-10-18, scratch save directory): Press
Start → Start → Import PELE (slot 1, status line) → PELE enters the Tower
(level 1, 500 health, HUD "YELLOW SORCERESS … slot 1") → Esc → Manage
Character → Save Character ("saved to slot 1 (version 2)", `.bak` written) →
Quit Game → Create a new character (Warrior, Red, "BRAVE") → slot 2 enters
the Tower with the warrior model → F9 into Castle A1 (enemies engage, death,
continue) → Esc → Quit Level → back in the Tower.

Shop and death smoke test on Xvfb (2026-10-18): Tower → Esc → Shop shows the
31-entry table with prices; buying a potion took 250 gold and added a potion.
Castle A1 → killed by the castle's enemies → "YOU DIED - back to the Tower"
→ the Tower with the level-start health and gold.

## Persistent scenario suite (M13, 2026-10-23)

tools/python/scenarios.py runs gdl_play on the private Xvfb with the TEST
harness (research/loop-input.md): GDL_TEST_AUTOSTART (a new default
character, or GDL_TEST_LOAD_SLOT for a stored one), GDL_DEBUG_LEVEL, no
in-level input unless scripted, GDL_MENU_SCRIPT for the tally, shop, select
column and message boxes (port 0 by ticks outside play), and
GDL_TEST_ACTIONS: teleports and health writes as labelled DEBUG shortcuts,
the select screen's PlayerWriteSaveFile as a save action, and progress
dumps (experience, the class's gold, keys, potions, shards, runes, the
sixteen classes' completed-level bytes, the save region's SHA-256).
Report private/reports/scenarios-m13-v2.json and -m13-v3.json; all pass:

- death-g1: the key beside G1's start is picked up (keys 1), then the
  player dies (health 0.5 → do_players' death); TowerWave restores the
  level-start state: in the Tower (world 13), alive, keys 0, experience and
  the completion bytes unchanged — the reward is not kept or duplicated.
- exit-g1: G1's exit (teleported onto it) → tally, shop, level and
  inventory panels, Done → the Tower; the Sorceress's completion byte for
  world 7 gains bit 0 (the level is recorded as finished).
- save-reload: the same exit, a save to card file 0 in the Tower, then a
  second client loading save0001: in the Tower with the same completion
  bytes, experience, gold and keys.

## Traversal bot (M14/M15/M16 tooling, in development 2026-10-23)

`game_run.py --pads 0:1 --bot` (gdl_tools bot.rs) drives port 0 through the
ordinary pad path, so every step is the native PlayerMotion's. It samples a
navigation graph from the level's live collision (FloorCollide columns on a
1.5-unit grid; edges with a permissive wall sweep, those failing a
player-like test at a higher cost; blocked edges kept by place), picks goals
(the exit, else the nearest key or unused switch-on trigger, else a live
critter, else the nearest unvisited column), waits on triggers while their
platforms move, idles during the stage start, attacks enemies, critters,
barrels and chests in 12-update windows, and restores health below 40 %
(DEBUG assist, counted in the report). Followers (co-op runs, players=N)
follow player 1 and attack. Reports: the bot section of the game-run JSON
(goals reached, stuck events, the level end and its wave).

Batches (30,000 updates per level, all 61 levels, scratch reports): b2
ended the nine treasure rooms (their timers) and B2; b9 (2026-10-23, the
current rules: regular exits before secret ones, triggers counted on the
game's contact, foes sought only on boss levels, the legend item held on
boss levels as a labelled precondition) ends 21 of 61: S1–S9, B2, B5, A6,
the Tower (its first realm door → wave 0x700), and the bosses of A5, B6,
D5, F2, G5, H4, J5 and K5; a 60,000-update run ends E2 (Skorne 1 dead at
33,000). The closest approach to an exit in the others ranges from 0.19
(A1) to 1.0 of the start distance. Tried and dropped (worse batches):
escape sweeps when stuck (b6), generators as attack targets (b7). The bot
found one unported path (CritterDamage's breakable collision nodes, now
ported: critters.md). Levels the bot does not finish are not traversal
evidence either way.


## Menu flow tests (M17, 2026-10-23)

tools/python/menu_flows.py runs gdl_play on the private Xvfb through
client_driver.py, sends key holds to the client's window and records
GDL_UI_TRACE (gdl_play `ui_trace`: one line per change of the screen, the
front-end page or the in-game modal state — select screen, shop, map,
movie, message box, flyby — with the world and level). A flow passes when
its patterns match trace lines in order. Report
private/reports/menu-flows-m17-v1.json (actual disc), all five pass:

- title-options: Press Start → Title → Options → Audio (the original audio
  sliders) → Options → Title.
- options-all: every Options entry opened and closed: Audio, Game Options
  (gameoption_options), Compass (compass_options), Controls
  (controls_options), Screen Adjust; the list wraps after five.
- new-character: Title → Start → the in-game select screen (cross = Space;
  in play Return is Start) → a new default character → the Tower with its
  welcome scroll.
- pause-tower: the Tower's pause menu — Manage Character (the select
  screen), Shop, Inventory (with its closing swirl), Quit Game (the confirm
  page) — each opened and backed out of.
- pause-realm: G1 (TEST autostart) → pause → Quit Level → the Tower.

Open: with entry 0 (Settings) selected in the Tower's pause menu, the frame
shows Sumner's unlock scroll and the trace no page change; not yet explained.

## Co-op matrix (M16, 2026-10-23)

Headless (game_run.py --pads 0:1 --bot --players N; actual disc): player 1
is the traversal bot, players 2..N follow it along the bot's graph and fight
(their pads are present; health assists counted). Matrix c2 (scratch
reports bots/c2-<level>-p<N>.json):

| Level | 1 player | 2 players | 3 players | 4 players |
|---|---|---|---|---|
| S1 (treasure room, timer) | ends | ends | ends | ends |
| B2 (ordinary, exit portal) | exit | exit | exit | see below |
| B6 (dragon; legend item held) | boss dead, exit | boss dead, exit | boss dead, exit | boss dead, exit |

Every run is free of unported paths; in each ending run all players reach
the end state (5 = exited through the portal, 4 = left with the auto exit).
B2 with four players: run c2 again after the follower fix (the first four-
player run ended before the exit). Not compared with a reference: the
reference has one pad configured.

Device loss (GUI, scenarios-m16-v2 controller-reconnect): port 1 reports
no device in G1; after joyGetStatus's 60 polls player 1 is lost, the game
holds under ControllerMessageBox's CONTROLLERMISSING text, and START on the
port reclaims the player (joy_remap back to port 0, controller_lost 0).
