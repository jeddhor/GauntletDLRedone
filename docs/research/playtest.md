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

Bot rules added 2026-10-23 (b11/b12), each from a diagnosed failure (maps:
floors by height, reachable nodes, items and trigger bindings; game_run
`watch=`/`probe=`/`itemw=` samples of object matrices, floor probes and item
words):
- Exit portals: DoExit counts only while the stick is released, so the bot
  stands still on a portal (+0x884 contact; A1 stood on its exit forever).
- Ramps: edges steeper than STEP_UP are kept when the midpoint floor lies
  near the mean height (A1's stair to the raised platform, 2.2 per cell).
- Gates: closed (not entered) while no key is held; edges pressed against a
  gate without a key are blocked only until a key is held.
- Column sampling sweeps down in 8-unit pieces: a long radius-0.3 sweep from
  far above misses some dynamic floor objects that short sweeps find (E1's
  doorway, object 825; the player's own floor test is a short sweep, so
  play is unaffected — noted, not investigated further).
- The graph is regrown when collision objects moved (runtime flags, node
  heights and matrices; at most every 900 updates — rebuilds dominated the
  large levels' run time, b12 timing out 15 levels at 3,600 s): E1's doors open when
  the player walks within pad 109's reach (+0xfc 10 + radius), as in the
  reference (trace-E1-newgame-v1: the doors' world animation starts at
  vblank 233, item 109 +0xe2 = 1 with the player 11 units away).
- Stand-on triggers (+0xf4 0x100, lifts): the goal is a node on the bound
  object (or a child), where the bot then stays.
- When nothing is reachable, the blocked edges (guesses from stuck events)
  are forgotten and the plan retried.
- Stuck also covers oscillation: no new goal while the player stays within
  2 units for 180 updates (b13's A2 alternated between two points 0.9
  apart every 15 updates from update 5,733 to the end, never 0.5 short of
  the 45-update check point).
- Bosses: inside 8 units the bot steps back with attack released (held
  attack kept the player in its attack actions; from b12 the denser floor
  sampling let it reach H4's node nearest Garm, 5.8 units away, where Garm
  took no damage for 48,000 updates). H4 ends again (t15: update 35,892).
  The attack range is 18, or the foe goal node's distance + 4 when no
  reachable node is closer (C5's ring around the djinn's pit): batch t17
  ends all ten reachable boss levels (bosses.md).
- Idle rebuilds: with nothing to do the graph is regrown every 900 updates
  even without moved objects (K4's ferry left the player where the last
  build had not reached).
- Lifts: a ride holds input only once the floor has moved 1.5 units from
  where the ride began (K4's raft bobs 0.63 units every ~90 updates and had
  held the bot for 21,000 updates); the report's `holds` counts the updates
  held per reason (stage/camera wait, exit portal, riding). K4 then reaches
  22 goals and 12,375 graph nodes (t20); E1 ends at update 24,471 in 30,000.
- Labelled preconditions: `--pexp N` gives player 0 the experience of level
  N through ModifyExp (PlayerUpdateAtts raises the attributes); route hints
  (tools/bot-routes/<W><L>.json: items, points, waits; GDL_BOT_HINTS=0
  ignores them).

Found by the bot and fixed in the game logic: J4 (and any level starting on
a moving platform) left the player behind (player.md "Level start on a
moving floor").

Rules added for b26 (2026-10-23), from the b10–b23 logs:
- D1's walkway (object 1114, y 21.3) borders ground 9 units below (1108);
  the graph's drop edges pass its sweeps, but the player cannot step off
  (b10: 1,474 stuck events at one corner). The reference behaves the same
  (trace-D1-ledge-v1 from the WARPED capture w4l1-ledge-v1, player 0 at
  (39.8, 21.4, −78.5): Left, which points at the drop, slides along +z to
  (39.4, 20.1, −70.7) as natively), so it is the graph, not the game. A
  stuck drop edge now blocks every drop edge within 3 units heading the
  same way.
- Stuck beside a generator (kind 3) the bot attacks it like a barrel or
  chest (only when stuck within 3.5 units; b7 had made generators goals).
- With no goal for 900 updates (H3 and D1 idled ~40,000 updates with every
  goal given up) the bot forgets given-up goals, gate blocks, used triggers
  and explored columns and regrows the graph.


## State search (M14/M15 tooling, 2026-10-24)

crates/gdl_tools/src/explore.rs (game-run explore=BUDGET [nav=1|2]
cells=N seed=S; game_run.py --explore --nav --cells): a Go-Explore search
over native Game clones (Game derives Clone; collision objects, triangles,
grid and item definitions are Arc-shared and the run-time node matrices
sparse, about 1.6 MB per A1 state). Cells are player 0's 4 × 3 × 4 box,
keys held and a progress count (keys picked up, gates opened, triggers
set); each iteration restores a cell and plays a macro — random d-pad
presses, a graph-guided d-pad walk (bot.rs's Nav, stuck edges learnt as
blocked), or (nav=2) the traversal bot itself on the shared graph with a
random first route hint, filed every 150 updates. Inputs go through the
ordinary pad path (ReadControls → PlayerControls → do_players); the DEBUG
health assist is the bot's rule (restore below 40 % of the start health),
applied identically by the search and by the replay (assist=1). The result
is a pad script — pads= runs for digital pads, padfile= JSON with the
analog stick for bot macros — which game-run replays from a fresh load at
the end of the same run: A1's guided search (x5) ended its replay at
exactly the searched state's position (57.248, −7.023, 18.911), so the
clones carry the whole simulation state. Like the bot, a level the search
does not finish is not evidence that it cannot be finished.

Diagnosing a level the search does not end (2026-10-24): the report's
frontier (the best cell's place, keys, and the progress items done and
left), game-run navdump= (navdump_at=N for a later state) with
tools/python/level_map.py (floors by height, the graph reachable from the
player, exits, gates, keys, triggers, legend item, the search's timeline),
and short scripted checks (--teleport, --watch objects, --itemw items,
--probe floors, --sample-every 1). Findings so far:
- I5: a native bug (item culling under the boss camera, items.md) kept
  the raft's pad untouchable; fixed — the level ends from its start.
- D2: gates were opened but not counted (an opened gate keeps its record
  with +0xdc set); the progress count now includes them.
- F1: after pad 94 drops its bridge (object 324, y 29.7 → 9.7, one-shot:
  object tf 0x2) the graph reaches pad 95, but the bot's route west hugs a
  wall the player slides along. Compared with the reference (WARPED
  warp-f1-bridge-v1: player written onto the pad, 300 idle vblanks, Left
  held): the same positions — (−45.0, 10.6, 40.4) after 60 steps, the slide
  from x −52 to −55 at z ≈ 36 — and enemies crowding the bridge (health
  124). Native collision agrees there; the stall is the bot's path choice.
- A3's rune stone (item 251) lies at the bottom of a framed 6×6 pit
  (floor y 1.1 in a floor at y 8); the reference player walking into it
  slides along its rim (WARPED warp-a3-runepit-v2), as native's. The pit's
  floor is the elevator A3ELEV3 (node 1003), which trigger 249 on the far
  side of the level raises 7 units to the surrounding floor, rune and all.
  The bot now visits keys and triggers before the exits while a rune or
  legend item is left: it presses 249, takes the rune and exits (update
  17793).
- Deaths: a level also ends when the player dies (TowerWave); the tools
  count a level as won only when it ends without the player dying (state
  8 or 0xb). The no-assist chain t5 had counted a death in G1.

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
Followers walk the leader's trail (its positions every 10 updates) and
join it on triggers (the leader waits until they stand within 3 units:
pads for every player, lifts). Matrix c4: as above, and B2 with four
players does not end — one follower stays at a climb the leader made (the
camera then holds the others): a bot limitation, not a game state.
Matrix c8 (2026-10-23, current bot): followers now walk the leader's graph
to the exit portal the leader waits on (state 4) and stand still on it
(DoExit counts only then), stepping aside when another player blocks the
way; a follower holding a key heads for a closed gate the keyless leader
stands at. B2 with four players ends (update 5202, all four exited),
with two (4601), S1 with four (794), B6 with four (12437, the auto exit,
all four in state 4). B2 with three players stalls in this build with the
leader (nine keys) stuck short of gate 77 on a step — the leader's own
route, a bot limitation; c2's three-player B2 ended (3596).

Two-pad reference (2026-10-23): the private reference ini gives Pad 2 a
DualShock 2 on free keys (Y/B/V/N directions, M cross, U square, O
triangle, P circle, Z start, X select, R L1, C R1; reference_warp and
reference_trace accept them; PCSX2 ejects a pad a state was saved without
and re-inserts the configured one after its eject delay). From the Tower
new game (slot 2) pad 2's START opens player 2's select column (New/Load),
New → name "A" (up, right to END, cross) → Sorceress → the Tower reloads
with two players (captures tower-2p-select-v1, tower-2p-v2; tower-2p-v1 is
a mistaken early stop, the start state). Native placement of player 2 by
load_player_geo → get_player_pos (game_run --players 2 in the Tower) is
bit-exact with the reference: (3.1742401, −0.2734375, −27.781271), yaw
−3.1031971. trace-tower-2p-v1 (292 vblanks: idle, both players moving
apart, attacks; player1 record now captured by reference_player) replayed
update by update (replay-frame-tower2p-v6): enemies, both player records,
effects and world animation 146/146, items 145/146 (sound item 160's
+0xfc, the audio driver's track word, as in the one-player Tower replay),
camera 145/146 (the two-player stage-start zoom's step x differs by 45 ulp
at its first update). Replay harness fixes found on the way: NumInGamePlayers
is seeded from the traced player states (the camera's multi-player branch),
do_players' first block (welcome check, TowerCheckMessages, whose
SumnerLevelSpeechStart writes the class experience copies) runs before
do_players, and an update's camera is compared with the last sample of its
AFrame (the vblank pause can fall between game_main and StdCamUpdate).

Two players in a realm level: joining is Tower-only (pad 2's START in B1
does nothing, as natively). From tower-2p-v2 both players are written onto
the Poison Fields portal (reference_warp --pos/--pos2, labelled guest
writes; with one player at the portal the game waits in state 4 for the
other — capture w7l0-2p-v1 is that Tower state despite its name) and G1
loads (w7l0-2p-v6, after the realm movie; a state saved during the movie,
mode 0x400e, cannot be frame-stepped after reloading). trace-G1-2p-v1 (332
vblanks: walking, attacks, both players) replayed with the level loaded for
two players (game_run --vplayers 2: NumVirtualPlayers at load) —
replay-frame-G1-2p-v3: items, world animation and critters 166/166,
enemies 165/166, effects 165/166, players 157/166, camera 157/166. Found
and fixed: SetNumPlayers' NumVirtualPlayers (0x20f954 when set, else the
active count) was never assigned natively, so multiplayer-only items
(ShowItem's player count) stayed hidden for a second player. First
divergence of v3 (kept as the reproducer): step 201, player 2's position by
0.003 units while both players attack touching each other. Cause: the
harness — player 2's node was not traced, so the record's matrix stood in
for it. trace-G1-2p-v2 (the same schedule from w7l0-2p-v6, recorded with
node1 under the multitap configuration) replays 166/166 in every part
(replay-frame-G1-2p-v4: enemies, items, both players, effects, world
animation, camera, critters).

Multitap reference, players 3 and 4 (2026-10-23): launch_reference.py
--multitap puts a multitap in port 1 ([Pad] MultitapPort1) with DualShock 2s
in its slots C and D ([Pad4] Home/End/Delete/PageDown, comma cross, slash
square, backslash triangle, PageUp circle, semicolon start; [Pad5] 5/6/7/8,
9 cross, 0 square, minus triangle, equal circle, bracketleft start); pad 2
stays in port 2. The game's own multitap layer (mtap_open 0x103888,
MtapOpenPort 0x103940, serve_mtap 0x103d90: both ports polled, joy index =
port × 4 + slot) configures the pads (PCSX2 log "DS2 Config Finished -
P1/S2, P1/S3"; mtap_status[0] = 2). Slot B (joy 1) cannot join while pad 2
plays: ReadControls reads joy_remap[k] or port k for k = 0..3 — ports 0, 4,
2, 3 — so joy 1's Start never makes an edge (probed: plr_levels/plr_edges
stay 0 for it; joy 2's Start shows 0x40000). A join needs the stage start
to have ended (start_of_stage 0 near vblank 200 after the reload; game_main
tests it before check_active_players). From tower-2p-v2: slot C's Start at
vblank 260 → select column 3 → New, name "A", Sorceress, confirm →
tower-3p-v1; then slot D → tower-4p-v1 (captures tower-3p-select-v1..v3,
tower-4p-select-v1; reports join-pad3-v1, join-pad4-v1..v3,
join-pad5-v1/v2). Placement: players 3 and 4 at (−0.825737476, −0.2734375,
−27.794530869) and (−0.812478065, −0.2734375, −31.794507980), yaw
−3.1031971 — native load_player_geo → get_player_pos (game_run --players 4
in the Tower) bit-exact for all four. trace-tower-4p-v2 (292 vblanks from
tower-4p-v1's first update: the stage start, all four walking, attacks;
reference_player now records player1..player3 and their nodes node1..node3)
replayed update by update (replay-frame-tower4p-v3): every player record
146/146, enemies, effects, world animation and critters 146/146, camera
145/146 (a 2-ulp step at 179), items 144/146 (the first update replaces 60
items' load-time +0x4c pointer by 1.0, which native sets at load; item 161's
audio track word +0xfc). Found on the way: PlayerDoWeapTrail 0x14f428 was
not ported (the first update sets +0x7d8 −1; weapon trails were never
drawn), now in game::weaptrail with the client drawing the copies; native
records now carry load_player's node markers (+0x80, +0x6b8 = 1), so
DoPlayerAction's held-weapon hide/show (MBTreeSetFlags(+0x6b8, 2)) reaches
the client. The original's assign_controller only maps port & 3 == 0 to
player port / 4 when neither port has a multitap (mtap_status ≠ 2); the
native port omits that condition (no native multitap; recorded in
unknowns.md).

Four players in G1 (2026-10-23): from tower-4p-v1 all four records are
written onto the Poison Fields portal (reference_warp --pos/--pos2/--pos3/
--pos4, labelled guest writes) and G1 loads (w7l0-4p-v1, warp-G1-4p-v1).
Placement: (24.375, 0, 2.5), (26.20339, 0, 6.0576653), (22.645727, 0,
7.886057), (20.817335, 0, 4.3283916), yaw −2.391101 — native bit-exact
when player 0 is placed as init_next_level does (game_run --place0:
the start locator, then load_player_geo; game_run otherwise keeps the
Poison Fields fixture's position for player 0). trace-G1-4p-v1 (332
vblanks: all four walking in three directions, two rounds of attacks)
replays 166/166 in every part (replay-frame-G1-4p-v1: enemies, items, all
four players, effects, world animation, camera, critters; --vplayers 4).

Device loss (GUI, scenarios-m16-v2 controller-reconnect): port 1 reports
no device in G1; after joyGetStatus's 60 polls player 1 is lost, the game
holds under ControllerMessageBox's CONTROLLERMISSING text, and START on the
port reclaims the player (joy_remap back to port 0, controller_lost 0).
