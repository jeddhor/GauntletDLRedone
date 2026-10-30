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
Adjust; Back on the "Press Start" screen (Escape, Backspace, gamepad
North) exits, and a gamepad's Start there opens the title menu. The HUD shows colour/class, level, experience,
save slot, health, gold, keys, potions, kills, active enemies and standing
generators; F1 adds the interim labels and any unported path hit. Native
keys since 2026-10-28 (enhancements.md): F10 the settings overlay (player
1's keys can be rebound there; the menus follow them), F11 fullscreen, F3
the frame-rate counter.

User playtest 2026-10-27 (run_play.py on the RX 580, a Green Wizard):
a pink triangle under the player's feet pointing along the movement — the
Wizard's DUMMY node object WIZ_GREDUMMY (an unfilled hand-glow
placeholder), drawn because player rigs ignored AtreeNodeInit's MB flags;
the name-entry letter spinner drawn under the panel's name — the select
screen's panels were not recomputed after init_player_select(0); and
stutter — material re-preparation and texture re-upload hitches plus
30 Hz motion. All three fixed (commit 635002c, performance.md), then M19
and M20 finished on the user's instruction.

User playtest 2026-10-28 (the M19/M20 build, RX 580): Start on the title
did not offer a character — the Tower came up with GAME OVER over its
intro scroll. The title's Start builds the Tower and opens the select
screen with player 1 active but in state 0 until do_player_select's first
update gives it state 2 (new_player). front_system's game-over check
("every player in state 0", once per rendered frame) ran in a rendered
frame before that update — after the load the first frame often carries
no 30 Hz step — and init_over cleared active_players; the select screen,
now with nobody choosing, counted as all ready and init_next_level went
into the Tower. The original calls init_over only from quit_game and the
level end (hud.md "Game over"), never in game mode 0x400b; the check now
skips the select screen. Reproducer: tools/python/title_start_check.py
(kwin virtual session on the RX 580, a virtual pad, audio into ALSA null):
3 of 8 trials kept the select screen with the earlier client (9c0b9b09…,
title-start-m20-before-audio: the select closed after four frames, the
Tower and GAME OVER followed; 4 of 6 with GDL_NO_AUDIO), 8 of 8 with the
fix (89bf7be5…, title-start-m20-after-audio); Load → the player's file →
the class choice followed on the fixed build. Not seen on the Xvfb, where
every software-rendered frame holds an update; present since the select
screen's port (the Oct 27 build had the same check), the faster M20
frames made it frequent.

Same playtest, two more reports. "The Screen page never pops up": in game
(the Tower's or a realm's Settings) choosing Screen resumed the game —
Menu::is_paused lacked the Screen and Control Style pages, so gdl_play
unpaused under them, nothing was drawn and the d-pad moved the player
(from the title it worked: nothing is paused there). Fixed; Screen is the
original Screen Adjust (the picture's position, as for a TV). The user
had found no graphics options in the menus (the F10 overlay was the only
way): the options lists now end with a NATIVE "Display" entry opening
that overlay, which takes controllers too (enhancements.md). And the
front end had no exit (only Escape on Press Start, undocumented in the
menus): Back on Press Start now quits. Checks: unit tests (every in-game
Settings entry keeps the game paused; Start/Back on Press Start; Display);
menu flows pause-screen-tower, pause-screen-realm, title-display,
pause-display-realm, press-start-exit (Xvfb); on the RX 580 a virtual
pad alone went Press Start (Start) → Options → Display → a setting changed
and stored → B closed it on the list → Y, Y, Y to the exit (code 0).

Next playtest (same day, widescreen fullscreen): (1) going from the
widescreen view to a 4:3 screen left the sides showing the level — the
3D view went on rendering behind the tally/shop, map, select screen and
inventory, and only movies switched to the bordered frame; now every
full-frame 2D screen does (enhancements.md; RX 580 at 1920×1080, G1's
exit tally: black sides). (2) "I was unable to pick up a potion … the
blue potion on the ground" in the first level (G1): reproduced natively —
chest 31 by the start, opened with a key, drops a random pick, here
POT_BLU, which could never be collected: SetItem's tail that gives chests
their NULL1 content node was missing, so OpenChest took the loose branch
and the content stayed linked to a deleted chest (player.md "Chests";
reference trace-G1-chest31-v3, replay-frame-g1chest31-v1). Fixed; the
scenario chest-g1 checks key → chest → potion. The ground potions with
+0xe0 3 or 4 (368, 369, 370) are hidden in single player as in the
original (w7l1-potion-v1); the shown ones (367, 371, 372) were always
collectable.

Next playtest (same day): (1) a "blinking selection cursor" at the bottom
of the game-over movie, "the one from the Start screen": the FAIL movie
plays as an attract entry while the client's page is the title menu, and
that menu record and its spinning arrow icon were drawn through the movie;
fixed (hud.md "Game over"; menu flow quit-game-movie, with a control run
on a build without the fix showing the arrow). (2) The map before G1 was
silent where the original says "You are now entering THE POISON FIELDS!":
AudioEnterNextStage takes the destination's lines after next_world's
LoadWorldData, and the second line is a stage field ResolveWorldData fills
at load ("S_<code>NAME"); the port used the finished level's tables and
the file's −1. Fixed (audio.md "Stage records", hud.md realm map; scenario
map-g1: S_ENTERING1G, S_G1NAME queued). Found on the way: the AUDS stage
record is LEVL +0x5a's — twelve levels (A2–A6, J4–J6, K1–K4) had another
level's music and bank — and the map dots after the Tower played S_WARN.
Reference: ref-stage-k2-v1 and ref-stage-g1-v1 equal the port's values.

Next report (same day): (1) "pressing Start on the controller on Press
Start does nothing; A works". Not reproduced: gilrs maps the player's
controller (Microsoft Xbox Series S|X Controller, 045e:0b12, USB/xpad;
SDL mapping "Xbox Series Controller") Start → BTN_START, and on the RX 580
a virtual pad with that identity and those capabilities, START only,
reached the title menu from the SONY/attract screens (title_start_check
--attract --xbox, attract-xbox1/2: the client's own frames show the movie,
Press Start, then Start/Options; the window imports had been stale), under
the player's settings with WAYLAND_DISPLAY set as on the player's desktop (rx580-wl-copy; the client
has winit's X11 backend only, so it ran through Xwayland, as it does there) and after
the attract movies played out to Press Start (rx580-wl-natural). No
remapper is active (input-remapper has no controller preset; Steam was not
running). The player then found START working (same day; Steam not running, the ≡ Menu
button): closed without a found cause. (2) With
anti-aliasing on, the Display overlay washed the picture out: fixed
(enhancements.md "Anti-aliasing").

Playtest 2026-10-29 (widescreen): the screen before the realm map still
showed the level in the sides. That screen is the level end's
TRANSITION_SCREEN (the crossed weapons in lightning; world_update's
end-of-stage fade), drawn over play, which is still widened then, so the
2026-10-28 rule for full-frame screens never applied; it comes before the
tally at every level exit too. The sides now fade to black with it
(enhancements.md). Reproduced and checked on the RX 580 (kwin virtual
output 1600×1000, widescreen fullscreen, the TEST harness): the Tower's G
portal (as scenario map-g1) — before, the side columns stayed at the
Tower's brightness (mean about 42) under the opaque transition; after,
they fall to 0 over its two-second fade and the map follows bordered;
G1's exit: black sides from the transition into the tally. The frame's
centre is unchanged and the 1000 per-update hashes equal the previous
build's.

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

- ending-h4 (M15, scenarios-m15-v1.json, 2026-10-24; PASS): a new
  character in H4; at update 150 the DEBUG boss_kill action (labelled:
  Game::debug_damage_boss, lethal CritterDamage on Garm); then the game's
  own BossDeath, good wizard and auto exit, game_main's BossType 0x2c level
  end with /MPEG/victory.pss (frame 003: the movie), the shop and panels
  (the menu script's Done and back presses), the Tower (frame 004,
  experience 11038, level 9). The Sorceress's completed-level bytes gain
  0x08 for world 8 (H4); the Tower save (card file 0) loads in a second
  client with the same bytes. The reference comparison of the same path
  (Garm's death through the victory movie's start) is in bosses.md.

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

Rules added 2026-10-24 (batches b31–b34, single levels from their starts,
DEBUG health assist; each found in a stalled log, then checked by a
--teleport/--itemw/--watch experiment or a native render under
GDL_DEBUG_CAMERA):
- Triggers with +0xf4 bit 0x200 are skipped: ItemCollSub never lets a
  player touch them (another trigger's chain fires them). A4's BRIDGEPAD 231
  and G2's 408 had cost ~40 stuck events each run.
- Used triggers are revisited (three contacts, nearest the treasure first,
  reached only on contact) while a rune or legend item is out of reach, and
  a given-up treasure reopens after any trigger: B5's javelin lies past a
  ditch that trigger 279 bridges (B5ELEV1 rises 5 units, checked by
  standing on 279), and the way down is lift 275, already ridden — the bot
  now takes the javelin and exits (update 31108).
- A goal held riding for 1800 updates is given up (G1's ferry near
  (16, 41.4, −320) re-armed the riding hold for 26,000 updates).
- A generator attack that raised +0xfc no longer spends one of the three
  tries (A4's generator 128 broke over separate fights: 0.29, 0.5, 0.79,
  0.86); spent generators are soft obstacles.
- Graph: a floor under another with less than 3 units between their tops is
  no node (G2's stairs over its ground floor); the strict wall test uses a
  1.2 radius (the player's 1.5) and its failures cost 20× (with 0.3 and 4×
  the route left G2's building through the wall beside its stairs: a render
  from a pinned camera showed the railed staircase); after a failed drop
  every drop edge off the same object within 12 units is blocked; drops
  failing the wall test at the upper floor are no edges; barrels (kind 10,
  43–45) cost less than other obstacles.
- A given-up goal the player comes within 25 units of again is retried
  (G2's exit, given up behind barrels, lay 15 units away later; the bot had
  taken the secret exit beside it). G2 now ends through its regular exit
  (update 33095); G4 ends with 150,000 updates (81482, rune taken).
- Without the health assist the bot fires every update at enemies within
  ATTACK_RANGE and in windows at those within twice that; below 60 % health
  it seeks food (not BADMEAT). No-assist G1: three delays all died by update
  11714 before, two of three survived 60,000 updates after (health 308 and
  403 of 483). With the assist the old windows stay (the fights cost
  progress: b32 lost B5 and A3).
- campaign_chain.py --bot-tries N runs the bot side by side with different
  idle delays after the stage start (bot_delay=N; the simulation is
  deterministic, so a retry needs different input) and keeps the first that
  wins.
- Batches: b31 27 of 61 levels ended (60,000 updates), b33 32 (B3 D1 G3 I5
  K2 K4 gained, E2 lost), b34 34 (B1 E2 G2 gained, K4 lost; hard edges
  20×). Not ended by b34: A2 A4 C1–C4 D2–D4 E1 F1 G4 H1–H3 I1–I4 J1–J4 J6
  K1 K3 K4. The state search (e3: A2 C1–C4 D2 F1 G2 G4,
  600,000 updates each) ended none.
- Shot switches: kind-5 subtype-0x1f triggers are hit by missiles
  (WeaponCheckItemCollision), not walked on; the bot routes to a node
  within 9 units (not above it) or on a clear level line within 30, faces
  it and fires (D2's elevator call 405, 5 units above the floor).
- The key cap: at max_keys_per_player keys are not picked up, and one key
  short of it a keyring gives a single key and stays. A campaign carries
  keys between levels (nine by G4): the bot had stood beside six keys in G4
  and circled keyring 350 in K2.
- A waypoint the bot is stuck at five times is closed to routes until the
  idle reset; blockers count within their own radius (K2's barrels: 6); an
  assisted bot stuck five times within 2000 updates fires at range
  (enemies scale with the player's level).
- Reaching any trigger reopens every given-up goal; triggers are revisited
  after the exits also with no treasure left; switch-off pads are tried
  last (two contacts).
- Chained runs: the DEBUG assist's baseline is the larger of the start
  health and 80 % of max_health(level) (B6 began at 86 after the search's
  B5); the Tower shop's PlayersRestoreHealth is applied between levels
  (tower_shop=1; it refills from the class entry, which PlayerSaveState
  wrote, so health still carries over as in the game).
- Campaign chains t8–t14: L1, the G, B and A realms in one campaign (G2 by
  its secret exit to S7, B3 and B5 by the search, every rune and legend item
  of those realms taken, the G5 lich, B6 dragon and A5 chimera killed).
- puzzle.rs (plan=1, DEBUG): a breadth-first trigger/gate planner on state
  copies. Limited: G4's crossing is a bridge that cycles (floor 353 is
  absent at fixed settle times), G2 counted as solved at the start (its
  obstacle was the graph, not a trigger).
- K2 in the campaign was a native bug, not the bot (unknowns.md "Potion in
  K2"): GauntWorldInit's legend-item quest setup is unsigned (0x162f08
  sltiu 0x2b) and setup.rs compared signed, so a level without a boss
  started QuestItemState 1 for a player carrying the realm's legend item;
  a critter move then set +0x80c to 2, used the item up and held the
  player on code 1 (no attacks, no potions). Fixed; chain t16 then ended
  K2 (update 30,979, 4 assists), K3 and K4 with their rune and legend item.
- Exits: routes to anything but an exit stay out of every exit's reach
  (+0xe8 + 1.5). E1's way to pad 159 (the legend item's pedestal, object
  632) passed the exit portal; the bot left at update 245,365 without the
  item.
- The planner keeps branches without a gain to two presses in a row and
  ranks them below gains (K1 spent its 400 probes on orders of the same
  five pads); depth 10, 1200 probes.
- A foe within reach during a trigger wait is fought (the bot stood still
  on K2's lift pad 483 while the General beat it down); turbo (triangle,
  code 0x15, +0x800 >= 40) against a foe within reach every 40 updates.
- Legitimate play (no DEBUG assist): --shop-buy heal,potion buys in the
  Tower shop between chained levels with the character's own gold
  (Game::shop_buy, make_skiplist's refusals, then PlayerSaveState again;
  SHOP.WAD: SHP_CHERRY 20 health for 50 gold, SHP_drumstik 100 for 250,
  SHP_POTION 250); --loot makes gold, gems, food (when hurt) and potions
  (below the cap) within 25 units the bot's first goals. From the
  assisted chain's B6 end state (86 health, 1260 gold) the shop raised
  health to 586, but K2's General costs about 930 at level 24 (the
  assisted t16 run) and every no-assist try died on lift pad 483 by update
  1650: the assisted chain's economy (gold, health) is not one a player
  would arrive with. With turbo one of three tries got past the General
  and died at update 17,612 to a second critter (health 400).
- Treasure supports: fresh triggers bound to the object a left rune or
  legend item rests on (a floor query on a collision copy) or one of its
  parents come right after the treasures — D4's sword on object 735
  (trigger 355 raises it; the bot had stood on the ledge beside it 686
  times), E1's soul on 636 whose parent 632 is pad 159's, A3's rune on the
  elevator of 249 (still ends, b38).
- require=1 (the chain passes it to the bot tries): no exit goals while a
  rune stone or legend item is left (E1 exited at update 113,924 without
  the soul; pads 134/135 move the platform pad 159 stands on).
- Optional pickups (gold, food, potions, chests) are dropped after three
  stuck events (no-assist G2: 328 tries at one food item while health
  drained from 600 to 0). Chests with keys to spare are loot (legit G1:
  two of three tries ended at updates 20,661 and 21,675 against 29,252).
- Legitimate G2 (lg1, f69 retries): all die by update 28,000 — the bot's
  route to the exit takes longer than the health lasts (drain and hits,
  600 → about 100 at update 27,000). route_hints.py turns a winning run's
  reached triggers and treasures into hints for later runs.
- K1 (navdumps from teleported points, tools: components of the bot graph):
  the exit's region is reached from the ledge region above (−66, −32, −78)
  (14,654 graph nodes from there, the exit among them); bridge 1093
  (trigger 48) joins it to the western low area (y −34, x −208..−109); the
  mid region (y −8) above that area is fenced (an idle drop test at
  (−142.5, −8, −51.8) in four directions stops at the edge). The planner's
  frontier: [7, 38, 40, 41, 66, 8] (opens 9, 56, 57, the far west).
- Exit portals: the avoidance zone is the exit's reach + 3, no route for
  another goal enters it (a start inside included), and within reach + 2.5
  the bot steps away (b41: E1's player was taken by the exit at 4.33 units
  from its centre while fighting beside it).
- Chain t21 (binary c20, from t16's K4 end state): E1 ends at update
  63,780 with the soul taken (the first run to take it; 6 assists) and E2's
  Skorne 1 dies, level end at 29,611 → the Tower (12 assists).
- Trigger hints count as done when the bot reached them or their state
  changed since the hint began (a chain had set K1's far-west pads, so the
  hints were skipped without the bot going there).
- Treasure supports found: C3's bellows on object 854 (triggers 433, 434,
  435), I4's lamp on 1518 (trigger 527); H3's and J6's runes rest on
  objects no trigger is bound to (2748 → 2176; 933 → 811).
- K1's way (teleport checks, DEBUG): pad 9 is a lift (object 494 carries
  the player from (−213.7, −10, 19.6) to (−226.5, −35.8, 19.6), the far-west
  low region with triggers 10, 14, 39); pad 10 lifts object 320 from y
  −35.8 to −29.0 and 3.4 units east while stood on, level with the western
  low area (triggers 15, 19–32, 47–49; the rune RUNEA3 at (−192, −28.9,
  −44.2) on the platforms of pads 19–32); trigger 48's bridge 1093 joins
  that area to the ledge region of the exit. With the lock [7, 40, 41, 66]
  and bridge 8 to the far-west mid region (pads 9, 56, 57) that is the
  whole route; the rune's pads are the open part (planner from a teleport
  into the area).
- I4: the planner's plan is [527] (the lamp's platform 1518).
- K1, the last link (teleport checks): trigger 48 moves object 1093 from
  (−69.1, −30.7, −75.3) to (−63.8, −30.7, −65.2), away from the gap; pad 15
  is a ferry — object 1080 carries the player from x −113 to x −74.6 (z
  −77), beside the ledge region (−66, −32, −78), and back. K1's route:
  [7, 40, 41, 66] → 8 → (far-west mid) 9 ↓ → (far-west low) 10 ↑ →
  (western low area, the rune 192) → ferry 15 → the ledge region → exit.
- Traversal status (every bot batch and chain so far, 2026-10-24 night):
  49 of 61 levels ended alive with their rune and legend item, C3 ended
  without its bellows; not ended in the current reports: D3 D4 F1 H1 H2 H3
  I3 I4 J3 J6 K1 (D3 ended once in b22).
- Chain t21 continues: F1 ends at update 115,534 (52 assists; the first
  bot run to end F1) and F2's Skorne 2 dies, level end at 9,805.
- Route library (tools/bot-routes, README there): route_hints.py output
  per level, passed by game_run.py to every --bot and --explore run of the
  level; the state search's bot macros follow the rest of the route from
  the first hint not done in their state half of the time. G2, E1 and F1
  so far. Chains now take the first bot try to finish with a win.
- Legitimate chain lg2 (from lg1's G1 end state; no assist, shop, loot,
  route hints, route-guided search) replaces lg1 for G2 onward.
- Rules added 2026-10-24 (late; binaries f86–f91):
  - Islands count for the "by the island" triggers only up to 3000 nodes
    (H3 695, J6 130, D4 35). G1's rune island had 14,451 nodes — from it
    the rune reaches most of the level and the start is the cut-off part —
    so every trigger was "by the island" and b45 pressed them in turn for
    150,000 updates; b49 (f89) takes the rune and exits at 24,332 (G2 G3 G4
    A3 K4 unchanged; E1 from a fresh start still not, as in b33–b42).
  - Breakable walls (kind 10 subtype 0x2a: AudioExplodeWall, then
    DeleteItem) have their origin raised in the wall: the blocker and
    obstacle height window reaches 5 units above the player for them (H3's
    wall 8 stands 3.76 above the floor that leads to the rune RUNEE1).
  - Long attacks on a blocker count per visit: 3000 updates after the last
    try the count starts again (J6's barrel 567, health 5, breaks at once
    from a teleport beside it; b44 had spent its tries on earlier passes).
  - A trigger's contact forgets the edges learnt as blocked within 30 units
    of the trigger or its object (D4: pad 55 raises the pit floor 333 —
    wobj, 5.5 − 10 → 5.5 — beside trigger 355, which raises the sword's
    pedestal 735; the edges into the pit, blocked while it was down, kept
    355 out of reach for 130,000 updates after pad 55 in b47). Natively
    the pit's rim holds the player at x −78.5 while the floor is down (wall
    item 12 destroyed; a teleport check, not compared with the reference).
  - Death (enemy type 0x1e: weapons take one point, a potion's blast
    dispels it; death_touch drains health) is no fight target: within 6
    units the bot uses a potion (one per 150 updates) or steps away. In
    lg2's G2 try 2 health fell 99 → 25 while the bot sought food beside
    G2's DEATH statue by the secret exit (−176, 11.9, −200); it died at
    update 18,886 (the enemies doing the damage are not in the report).
    Hazards (kind 8: spikes) are soft obstacles. lg3 (with this rule): all
    six G2 tries still died (17,870–22,286), no Death met.
  - Platforms: a ride ends 10 updates after the platform stops (was 30)
    and the bot does not fight for 120 updates after it. J6's rune room
    (y 116.7, x 15–29, z 84–102: the rune, a LOW generator, a GENERAL
    statue, chests, barrels) is reached only by lift 694 (pad 31 at (20.8,
    82.2, 77.0), 81.2 → 115.9 and back); it stays level with the room for
    about 30 updates. From a teleport onto the pad the bot now walks into
    the room (f93; before, it fought from the lift and rode it down).
  - Without the assist, generators (kind 3, armour ≥ 0) within 9 units
    come first while no foe is within 2 units; one whose health (+0xe4)
    has not dropped in 300 updates of fire is left. lg3's tries died
    fighting the spawns of four generators (GRU 232, LOW 201 and 204, KNI
    200) by (−184, 0, −165) on G2's route, health 383 → 0 in 1800 updates.
  - Drops: PlayerCollideFloor (0x145218, native player_collision.rs)
    refuses a move whose floor lies more than 3 units from the current
    floor (4 on the same object), and one with no floor within the
    player's height + 3 below; the player stops at the edge. The graph
    took drops of up to 12 units (DROP), none of which a walking player
    can take: D4's pit rim, H3's ledge 8.7 above trigger 504 (the bot's
    only way to it in b43, given up after six stuck events; 504 lowers
    bridge 51 between lift 287's top and the exit plateau), D1's walkway.
    DROP is now 4; deeper descents only on a ramp, as climbs (f95). b53
    (f95): D4 ends for the first time with its sword (pads 342 → 55 → 355,
    the sword at update 25,286, exit at 36,377; route hints in
    tools/bot-routes/D4.json). Regression b54 (f95, 150,000 updates):
    G1 G3 A3 B2 B5 D1 D2 K2 K4 I1 end (G1 17,662 against 24,332, I1
    63,874 against 121,907); A1 held by the Death rule (fixed in f98);
    C1 not: the bot sat in the water pit by (−20.5, −12.1, 50.8), 1.3
    under the surface, for 135,000 updates (528 assists; C1's earlier
    win took 143,254 updates and 410 assists).
  - Death: stepping away from it within 6 units (f91–f97) held A1's bot by
    the fenced Death statue for the whole of b54 (3200 stuck events); now
    it is only no fight target, with a potion within 4 units (f98: A1 past
    that point again).
  - Steps: climbs of 2–3 units are edges when the player-like wall test
    one unit up passes (a lower face, or a step object, +0x10 & 0x38,
    which PlayerMotion hands to the floor stage); PlayerCollideFloor's
    limit is 3 (4 on one object) (f99). It changed nothing in D3: there
    the bridge pieces raised by pads 50–47 stand at 70, 71.9, 73.4 and 74.7
    (navdump after the pads), the graph already joined them, and the bot
    stuck 162 times stepping 71.9 → 73.4 toward pad 354 — a step the graph
    passes and the player does not take.
  - Walk check (f100): every graph edge with more than 1 unit of height
    change is walked with the game's own static displacement
    (player_collision::player_displacement — PlayerCollideWalls and
    PlayerCollideFloor, 96/96 updates byte-equal with the reference in G1)
    from a standing start, 0.25 per step, radius +0x828 and height +0x82c
    of player 0; an edge the walk does not finish is no edge. Withdrawn
    the same night: b56 (f100) — A3 and K2, which end otherwise, did not
    (1289 and 2652 stuck events; the standing-start walk refuses steps the
    moving player takes), and none of D3 I4 J3 J6 K1 ended. The function
    stays for diagnostics, unused.
  - b53 (f95): I4 takes its lamp (legend item 540) but does not reach the
    exit; H3 takes its rune standalone (and ends in chain t26).
  - Without the assist the start delay (0–541 updates per try) is spent
    fighting foes within reach in place (f96). lg4 (generators first, f94):
    four of six G2 tries died by update 28,450 — try 6 lost 123 health
    idling out its delay and was down to 88 by update 5400.
  - State search without the assist (f97): a cell keeps the healthier of
    two states (by over 25 health), else the earlier; cells are chosen
    with weight × health/600 (at least 0.05). lg2's no-assist search had
    run 129 iterations without a survivor.
- Level structures found (teleport checks and wobj tables, 2026-10-24
  late):
  - H3: the exit (1, 20.7, −118.8) stands on a plateau. Pads 479 (on the
    lift, stand-on), 497 (top, on) and 498 (bottom, off) drive lift 287
    (y 0.6 → 18.6 at (9.7, −78.6)); trigger 504 at (−107.7, 7.5, −64.6)
    lowers bridge 51 (y 24 → 18, (−6.4, −97.6)) between the lift's top and
    the plateau. b47 took the rune (wall 8 broken) but never reached 504.
  - J6: the rune room (y 116.7) by lift 694 (pad 31, see above).
  - C3: trigger 434 at (−38.2, 30.1, 60.0) fires 433 and 435 by its chain
    and raises the bellows' platform 854 from y 22.8 to 31.6 within ~150
    updates (hint run, f101, watch 854); the bellows (47.1, 31.7, 33.8) is
    then level with a region at y 30 (x 35–68, z 21–57) that the graph
    from the start never joins (a trench at y 19 runs beside it); its way
    in is not found yet.
  - J3: trigger 502 stands 1.4 behind the static wall 1512 (object flags
    2) from the floor the bot reaches; graph nodes on that side no longer
    count as within reach of an item (line test, f102).
  - D4: pad 55 (y 16, (−81, −164)) raises the pit floor 333 to the
    corridor's level; trigger 355 on it raises the sword's pedestal 735 at
    (−192.8, 11.9 → 17.9, −102.4).
  - K1's rune area: pads 19–30 switch objects 1278, 1280 and 1281 on
    (flags 0xa: 20/26, 21/24/28, 23/27) or off (0x9: 19/22/25, 30, 29),
    TRIGGER2 47 raises 1046 beside the rune; with every object switched on
    (k1-h4, f82) a dip 4 units deep stays between the bot and the rune
    (−192.0, −28.9, −44.2). Eight on/off combinations through route hints
    (f91) did not bring the rune within reach either; open.
 Chain lg1 plays from a new game with --no-assist
  --loot --shop-buy heal,potion.
- 2026-10-25 runs (c29 = f102):
  - Standalone searches s1 (400,000 updates each, assisted) for the five
    levels the bot does not end: none ended (progress D3 11, I4 18, J3 8,
    J6 5, K1 10; I4 took its lamp, 540).
  - lg7 (no assist, G2 from lg1's G1 end, 600,000-update search): the six
    bot tries died (state 0xb, by updates 6,098–22,195) and the search
    reached progress 22 (lg6: 23). Legitimate G2 stays blocked by the
    character's strength at that point, not by the route.
  - t33 (I2 onward from t31's I1 end): the six I2 tries and the search
    (progress 11) left rune 505. Cause: the route hints (from b37, before
    DROP was 4) list trigger 485 on the ground (y 3.6) after bridge pad
    476; b37 got there on lift 47, which the pad sets moving between the
    walkway (13.1) and the ground (4.8 at its lowest): the old graph held
    the 9.6-unit drop from the walkway and the bot happened to ride the
    lift. With DROP 4 the graph built at the pad (i2dbg1-nav, update 8731)
    has no node within 4 units of 485; hint 9 was never reachable and
    every later hint was skipped in turn after 60 stuck events (the bot
    reached the ground only when a rebuild caught the lift at the bottom,
    update 59,741).
  - Where legitimate G2 loses its health (lg7 tries 1, 2, 4, 5; 150-update
    windows): 773–894 health lost a try, two thirds of it in windows with
    stuck events, by enemies (KNI generators 123/125 near the start, 140/229
    by (−70, 0, −5), LOW 220 at (−102, 10.3, −98.5)) and by SPIKES (36–146 a
    try, e.g. (−64, 0, −5)); no health drain without enemies (drain-g2-v1:
    600 for 3600 idle updates with --no-enemies). The character enters G2
    at level 4 with no potions and 250 gold (five SHP_CHERRY bought).
  - Game difficulty: the Game Options menu's Difficulty (Easy / Medium /
    Hard, difficulty_options → GamePrefs +0x14, the 0x21edcc index of
    ResolveWorldData's diff_mul_tab 0.667 / 1.0 / 1.5) is an ordinary
    setting; the fixture plays Medium. game_run.py --difficulty N writes it
    into the start state as gdl_play does at each level start
    (diffcheck-easy-v1: G2's multipliers +0xcc..+0xdc 0.75 0.667 0.5 2.0
    0.333 against Medium's 0.75 1.0 0.75 1.333 0.5). Enemy health (+0xac)
    and damage (+0xbc) are not among the scaled fields; enemy speed (+0xb0,
    fwrdval) and aggression (+0xb4) are (× 0.667 on Easy).
- Rules added 2026-10-25 (bot.rs; tuning, not game data):
  - Ride edges (Nav::ride): the bot samples every object that can move (a
    run-time y, rotation or animation matrix on it or an ancestor) every 5
    updates; one whose translation y has spanned at least 1 unit and has
    reversed direction is a mover. Edges between floors on different
    objects, one of them a mover, are ride edges when at some point of the
    travel the far floor is within 3 below (PlayerCollideFloor) or 2 above:
    the bot waits at the near end (nodes on movers followed where their
    floors are now) until the step fits, at most 600 updates, then blocks
    that edge and every ride edge onto the same object within 4 units.
    Mover nodes are kept by object in the blocked-edge memory, whatever
    height a rebuild caught them at. First tries (c30, no reversal test,
    height-keyed places): I2 reached trigger 485 by lift 47 at update
    10,289 (hint 9; t33 never), but objects that rose once and stayed
    held the bot in ride waits (I2 i2r1: 208–229 timeouts a try; G1 in b60:
    183, the level no longer ended).
  - A ride edge onto a mover that has not moved for 300 updates times out at
    once (K1's lift by (−71, 74), moving only while stood on, 31 units
    below its ledge: waited for 16 times in k1rune-c38-v1).
  - The goal-node line test (f102) ignores the item's own bound object
    (+0xf0): I2's trigger 495 sits on door 262, which it opens (bound
    object 262; door 262's edge moves from x 6.6 to x 6–9.5 later and
    closes the way west to trigger 496 and the rune), so it had no target
    node and hints 11–20 were skipped (i2r2, c38; i2dbg4's explain at
    update 16,000).
  - Checks on c38: G1 J4 C1 end (b61), H1 H2 J2 end at 300,000 (b63), J6
    ends (b62); E1 F1 I3 do not (E1 identically on c33 and c38: 4,406
    stuck events at (0, 0, 80.3)).
  - Trigger target nodes: a node nearer than the trigger's contact distance
    (the player's radius + the definition's reach +0xc, +0xfc when set,
    doubled for subtype 0x1b; less 0.3) is a target even when the line
    test fails — item_coll_sub does not test walls. I2's trigger 495
    (TRIGGER2, box, reach 0.7) stands behind static wall 1192 from the
    floor the bot reaches, 1.1 units from its node (explain_items in
    i2dbg5); b37 fired it from that side. J3's 502 (same definition) stays
    excluded: its nearest node is 2.2 away. With it (c44) I2 ends from
    t31's I1 end in all three tries with the rune: updates 59,136
    (delay 61), 76,299 and 84,836 (i2r4).
  - A trigger hint whose state an earlier touch left set counts only on
    contact (+0xe2): I3's hint 8, trigger 533 (it raises the rune's
    platform 566 about 6 units, after which the rune is taken from y −2.6;
    b48 took it 2,640 updates after the touch), counted as reached from
    184 nodes away in b65 (c41) and the rune stayed down.
  - b64 (c39, start delays 0/149/397, 300,000 updates): C3 ends with its
    bellows in two of three (128,846 and 269,205: lift 741, y 30 ↔ 45,
    then the y-40 walkways to the region by platform 854); D3 J3 K1 I4 do
    not (I4 takes its lamp in two).
  - D3 (d3dbg1, c44, update 80,000; item dump d3items-v1): two exits —
    138 EXIT_PORTAL at (−67.8, 27.4, −34.2), on a floor region (y 20–78)
    that no floor within 12 units of height adjoins, and 398 SECRET_ICON
    at (35.6, 52.5, 62.7) (b22 left by it, wave 0xc01). The secret region
    (y 52–80) lies 9–12 above the bridge-pad area (y 67–70) by objects 463
    (wobj: 10 up; triggers 365, 366 and 358 bound to it, 366 and 358 on
    it) and 5.4 above a floor at (15.8, 47, 65.3), beside pad 360 on object
    624 (wobj: 5 up; pad 353 too). The bot reached 366 by its state from
    32 units away (update 10,773) and never rode 463; no graph node near
    360 in that run. Not ended (b62–b66).
  - I4: door 1261 (flags 0x80103002, wobj 5 down) closes the south side of
    the ledge at y −62 by (−40, −62, 82), above the exit 181 at (−84.2,
    −77.1, 75.0); trigger 534 at (−8.1, −41.7, 192.0) is bound to it, in
    the region lift pad 523 (24.8, −31.6, 86.2) takes the bot down to.
    No run has touched 534; route hints tools/bot-routes/I4.json (b64b's
    trigger order with 534 after 523, hand-made) were skipped in turn
    before the bot got there (b67, c44).
  - Striking triggers behind walls (PlayerMotion, pmotion.rs's event stage):
    an attack (action event & 0xfe: melee 2, throws 4) with the stick
    released and no item in contact (+0x880 0) runs PlayerCollideItems at
    the player's position + 2 radii along the facing, walls not tested.
    I4's trigger 534 lies in a three-node strip behind wall 2644, which
    the game's own displacement does not walk into from the floor beside
    it (walk_reaches false for all three; growing the graph from the
    strip at the start reaches the rest of the region only outward, the
    wall one-sided). The bot now takes target nodes within 2 radii + the
    trigger's contact distance − 0.5 when none is within touch, and at
    such a path end faces the trigger for 6 updates and attacks with the
    stick released for 18, at most 600 updates (strike_reach, c47). Check
    (i4strike-v1, DEBUG teleport to (−6.8, −41.9, 189.8), hint [534]): the
    strike began at 2.3 units (update 123) and 534's state went 0 → 2 at
    update 125; the bot then took lift 524.
  - Which attacks probe: PlayerMotion's attack code 0xf makes a melee swing
    (0x27…, event 2) only with a target within the player's radius + 1,
    otherwise the throw 0x5c (and 0x5f/0x61; 0x5e/0x60/0x62 moving), whose
    events (bit 1, or 0x100) fail the probe's & 0xfe test. In J3 (b69, c47)
    the strike at trigger 502 from 2.86 units threw for 900 updates and the
    state stayed 0 (j3strike-v2: +0xd8 0x4010). The strike now uses the
    turbo attack (triangle edge, a melee) when +0x800 ≥ 40 (it refills by
    2 × spf an update), else attacks with the stick released (a swing when
    a foe comes within reach); 900 updates at most (c49). Replay j3strike-v4
    (delay 263): the turbo swing at update 31,992 did not reach it; a melee
    swing with a foe near at 32,070–32,082 did — 502's state 2, reached at
    update 32,080.
  - J3 standalone on c49 (b70, delays 0/263/397): delay 263 ends at update
    123,052 (502 reached at 32,080 and again at 38,060; exit 125); delay
    397 struck at 502 from (64, 21, 147) for over 360 stuck events without
    contact; a turbo swing facing it from 3.15 units (j3strike-v4, update
    31,992, action 0x56) did not touch it either — whether +0x880 held
    another item then is not checked.
  - D3's bridge pieces against the reference (WARPED: player 0 written onto
    pads 50/366 at (−129.9, 59.3, −5.2) in w4l3-start-v1, the reference
    relaunched with the multitap the capture was made with — without it
    the game holds on "A multitap(PS2) has been removed"; then Down held,
    ref-d3climb-down-v5): the player rides object 463 from 59.3 to 69.05,
    steps up to 70.2 and stops at (−133.4, 70.2, −13.9) — native with
    d-pad mask 8 from the same teleport (d3climb-m-8) stops at the same
    point. Chained warps (captures w4l3-climb-b/c/d-v1) are INVALID: the
    player stood on a moving object, and reference_warp.py writes the
    node's local translation as world (positions doubled, (−261, 140,
    −17)). Natively the pads ride on the pieces (50 lifts pad 49 to 71.3,
    49 lifts 48 to 73.0, 48 lifts 47 to 74.5, 47 lifts 354 to 75.8;
    d3pads-v1), and a moving player climbs the whole staircase: holds of
    mask a (2|8) then 6 (2|4) from the teleport press 49, 48, 47 and 354
    and reach (−104.5, 76.3, −12.9) (d3dir2-6, crossing 71.9 → 73.4 near
    x −122 heading +x). The bot stalls there because it arrives at each
    step slowing for its waypoint. The b22-era binary climbed it
    (d3-b22bin-v1, secret exit 0xc01 at update 55,840); the native
    dyngrid (d360160) added the pieces to the wall test since.
  - Run-ups (c52): the steering target for a step up (over a unit) lies
    past the step's top, and a stuck step up is run at again three times
    (15 updates' back-off) before its edge is blocked. b73 (c52): D3 ends
    in both tries — delay 0 by the secret exit (wave 0xc01) at update
    32,207, delay 149 by the regular exit at 43,203 (pads 50 49 48 47 and
    354 by updates 12,245 / 13,667; 10 and 17 run-ups).
  - J3 in the chain (t36, t37, and six more tries from t35's J2 end with
    delays 700–1777, c49–c54): none touch 502. Its strip lies between two
    reachable floors (z 147.75 and 152.25), walk_reaches false from both
    (j3dbg2). The Sorceress (the tools' class) probes only with a melee
    swing, which needs a target (foe, critter, generator) within her
    radius + 1 (attack_request; PlayerGetTarget): her throws (0x5c/0x5f/
    0x61, 0x5e/0x60/0x62 moving) and her turbo attack (0x56) raise no probe
    event (& 0xfe), square (99) none for class 6 below +0x80c 2. b70-263's
    touch came from (67.0, 145.5), 5.0 units off, when a foe was near.
    Reference (WARPED j3-strike-ref-v1 from lvl-J3-play-v1: player 0
    written to (64.87, 20.88, 147.11), four cross presses with no
    direction): 502's +0xde stays 0, its +0xd8 goes 0x10 → 0x4010 as
    natively (j3strike-v2). INCONCLUSIVE as to facing: the poke went to
    +0xdc, but the probe's direction is atan2(+0x40, +0x48), the matrix
    row PlayerMotion resets from the player's node, so the player's facing
    in that test is unknown. J3's four switches 499–502 are
    all behind walls (TRIGGER2, reach 0.7). The strike now has a lure
    phase after its 900 updates (c57/c58): back off toward 3.8 units, face
    the trigger, attack only with a foe within 2.5 units inside 25° of it.
    Chain t38 (c54, from t35's J2 end; J3 skipped, labelled DEBUG): J6
    (120,030), J4 (65,322) and J5's boss (7,756) end.
  - K1 (c54, hints [38 7 40 41 66 8 9 10 19 30 29 192 48 15]): the rune's
    floor 1279 is fenced by the switchable objects (1280's face at y −28
    by (−192, −46.2) in k1dbg3; translations 1278 −25.6, 1280 −26.4, 1281
    −18.9); the bot had pressed only the 'on' pads (26 21 27 20 28 23).
    With trigger 38 first (it sits at the bottom of stand-on lift 7, which
    goes back down once left) and the 'off' pads 19, 30, 29: delay 149
    takes the rune at update 51,873 and exits at 97,949; delays 0 and 263
    do not (hint 5, pad 8, unreachable in time; the rune's barriers up).
    On c56 with the same hints (tools/bot-routes/K1.json; the route_hints.py
    list made from the win, 50 entries, did not win in b76k): delays 149
    and 397 end (94,708 and 89,420), 0 and 263 do not.
  - Strike nodes are a last resort for ordinary goals (c56): a pass over
    every goal list with plain target nodes first, then only the items
    that gained a strike node. C1 (bisect c41/c44/c47/c49: ends on c44 at
    134,752, not from c47 on) had followed trigger 342's new strike node
    into the water pit by (−20, −12, 52) and never left it; on c56 C1 ends
    at 80,278, D3 at 52,432 and I4 at 174,576 (b76). Hints keep strike
    nodes.
  - Full campaign chain tfull1 (c56, from the Tower of a new game, levels
    in the order the gates allow — bosses.md "The critical path"; --require,
    no legend-item precondition, DEBUG health assist): L1, G1–G5, B1–B6,
    A1 A6 A2 A3 A4 A5, K1–K5 (K1 on its sixth try at update 291,502), D1–D5,
    C1–C5, I1–I5, J1, J2 — 40 levels in one carried campaign — then J3
    stopped it (six tries and the search; the Sorceress's strike). Twelve
    more J3 tries from its J2 end did not touch 502 (none of 12, then six
    with the lure). The tail from tfull1's J2 end with J3 skipped (tskip1,
    labelled DEBUG: J3 not played; it holds no rune) ends J6, J4, J5, E1,
    E2 (Skorne 1), F1, F2 (Skorne 2), H1, H2, H3 (the rune, sixth try,
    update 280,050) and H4 — Garm, the ending — at update 6,848 (boss auto
    exit). One carried campaign from a new game to the ending: 51 of the
    52 non-secret levels played in the gated order with every rune stone
    and legend item taken, no legend-item precondition; DEBUG health
    assist throughout, J3 skipped.
  - Ride waits are timed per edge (waiting and crossing together, 1200
    updates at most): J6's bot held at one bobbing floor from update
    30,000 to 300,000 in b74 (c52), the per-wait timer restarting each time
    the step came within range.
  - Chain t35 (c47, from t34's I3 end): I4 (29,560), I5 (boss), J1, J2 end;
    J3 not (search progress 9); t36 (c49) runs J3 onward.
  - Without the assist only (legitimate play): stuck with a foe within 10
    units the bot fights in place for up to 600 updates; hazards (kind 8)
    and statues (kind 4, within reach + 1.5; contact or a hit wakes them)
    cost 150 to enter instead of 25 (closing statues shut lg11's try 2 in
    for 155,000 updates); generators up to 15 units along a clear line
    are fired at; foes within 3.2 units are stepped away from every other
    20 updates (knights strike from about 3); explosive and poison barrels
    are not broken on the way; nodes inside a live poison cloud (an effect
    with damage flags 0x800: StartExplosion 0x19, blast 6.5) are closed
    and the bot walks out of one; food with a negative amount is no loot.
  - Report `hurt` entries (DIAGNOSTIC): each loss of 3 or more health with
    the foes within 30 units (critters as type −1, within 40) and hazards
    within 5. hurt-g2-easy-v2 (Easy, no assist, died at update 35,265):
    790 lost — knights at about 3 units 382, statue creatures 290 (hits of
    29–39 from 5–9 units), hazards 71; lg11 try 1 lost 230 to 4.1-point
    ticks every 31 updates with nothing near (a poison cloud).
  - Legitimate G2 tries (no assist, from lg1's G1 end): Medium lg8 died at
    6,044–23,820; Easy lg9 at 29,320–73,375 (one alive at 200,000 on
    bridge 1054, the mover-place loop above), lg10 at 26,561–104,955,
    lg13 (all the rules above) at 13,947–138,991 (losses: knights within 6
    units 190–433, hazards 98–213, statue creatures 47–303 a try). Chain
    lg14 (Easy, six tries then a 600,000-update search) did not end G2:
    the search reached progress 7 in 256 iterations and 93 bot macros
    (lg7 on Medium, before these rules: 22, 299 and 176).
  - Kiting (c55, no assist: always back away from foes within 4 units
    unless held by stuck events): lg16's six Easy G2 tries still died
    (24,185–110,686), knights within 6 units 312–497 a try. G2 holds 114
    generators (KNI ×72, LOW ×29); the tries destroyed 12. Four players
    (lg17, --players 4: copies of player 0, labelled DEBUG; the followers'
    bot steps): no wipe in three tries (followers at 267–625 health; the
    leader alive at 288 in one) but no exit by update 200,000 — every
    hint skipped and the leader stuck by (−46, 0, −89) for thousands of
    updates (the shared camera holds the players together).
  - Enemies do not scale with the player's level in the ported code (the
    enemy keeps PlayersAverageLevel at +0x384, read by nothing ported);
    damage dealt to enemies does (× (1 + 0.1 × (level − LEVL +0x9c)) above
    the level's target, − 1 % a level below), and max health is 500 + 100
    a level. Replaying G1 to level up (campaign_chain.py now takes a level
    twice, e.g. G1 G1 G2; a completed level is entered again from the
    Tower in the game): lg15's six G1 replay tries (level 4, Easy) all
    died (20,902–78,210). A/B from the same state (ab-g1-*): the build
    before these rules (c29) dies too (Medium 25,282 and 22,992), as do
    both builds from the fixture's own G1 start (level 1: c29 at 24,907,
    c44 at 64,386); lg1's four G1 exits (f94-era build, 2026-10-24) were
    the lucky end of a wide spread, not a better bot.
- 2026-10-26 runs (c60–c63; J3 tries from tfull1's J2 end, delays 0, 61,
  149, 263, 397 and 541, 300,000 updates each; DEBUG health assist):
  - What the Sorceress's strike at 502 needs (instructions read, not a
    reference test): the probe (PlayerCollideItems at the position + 2
    radii along the facing, radius +0x828 1.5) touches a box-shape-3
    TRIGGER2 within radius + reach (0.7) of its +0x60 and within radius +
    half extent (+0x14/+0x18, 0.5) along its rows +0x10/+0x30 — for 502
    the probe point within 2.0 of (64.66, 149.97) on both axes, so from
    the bot's spot (64.7, 147.1) a facing within about 41° of the trigger.
    A melee swing (the only Sorceress attack whose events pass the probe's
    & 0xfe) needs a target: PlayerMotion's targeting stage tests the
    enemies (closest_enemy: +0x70 to the enemy's +0x60 less its width
    +0x24c, state 1 or 6, inside the cone 0.707 + distance × 0.293/30 —
    target_cone_mode 0: 0.5) along `want`, the stick's direction, and
    swings while that distance is under radius + 1 + extra (1 while attack
    is held); the swing does not turn the player (+0x8dc picks the side
    swings 0x2c/0x30 or the back swings 0x34/0x38), and the probe follows
    the facing (atan2 of +0x40/+0x48). Triggers are not melee targets
    (DamageItem kind 5 acts only on subtype 0x1f).
  - c60 (lure 4.5 units off, attack with a foe within 3.5 units inside
    60° of the trigger): no melee request (0x27) in any strike window of
    the six tries (tfj3e), only throws (0x5c).
  - c61 (melee_facing: a facing whose cone holds a foe within reach and
    whose probe lands on the trigger, the turn by stick taps): no swing in
    six tries (tfj3f). c62 diagnostics (tfj3g, foes within 4 units every
    60 updates of a strike): type-24 foes 3.9 units off (width 2.0, so
    1.9 by the game's measure, within reach) held still for hundreds of
    updates at 112°–178° from the trigger's direction — behind the bot,
    out of any facing that also probes the trigger.
  - c63: with the facing on the trigger and a foe within reach at any
    angle, the stick flicks at the foe for one update with attack pressed
    (the targeting cone follows the stick), then is released for 30
    updates with attack held (a facing turn is at most spf × 5π × +0x968
    an update, about 30°, inside the probe's window). Batch tfj3h: five
    of six tries touch 502 at the first flick or within a few (updates
    9,827–87,292) and end J3 by the regular exit (item 125, exit state 5)
    at updates 36,192 (delay 397), 41,613, 46,620, 101,600 and 108,379,
    with 1–2 health assists (b70-263: 86); delay 61 never struck. The
    chain's tail is rerun from tfj3h-d397's end (tfull2). Regression
    b78 (c64, the eleven levels whose traversal wins logged strikes, from
    their own starts, delay 0): A2 A4 C2 D3 F1 G2 G4 I2 I4 end at the same
    updates as b77; J3 and K1 do not end, as in b77 (their v8 wins came
    from other delays; J3 here swung twice at 502 without touching it).
- Hidden generators (bot fix, c63): the no-assist generator targets
  included generators with +0xe1 2 (player-count items waiting to show
  until off screen); ProcessItems, closest_generator and ItemCollSub skip
  them, and one beside the bot stays hidden. wg1g: the Warrior stood at
  G1's generator 170 (+0xe1 2) attacking — PlayerGetTarget on a clone
  found no target, every attack a throw. Generator targets now need +0xe1
  0 (item goals already did).
- Another class for legitimate play (tooling, c63): game_run.py
  --new-character CLS[:COLOUR] (with --next-level; campaign_chain.py
  --new-character for the first level) makes player 0 a new character as
  the select screen does (profile::new_character; class data from the
  class's PDAT as gdl_play's class_data_of) and loads that class's clips,
  SFX<colour> and PDATA; later chained runs take the class from the start
  state's player 0 (+0xc class, +4 colour). Level-1 derived words
  (+0x118 melee, +0x11c armour, +0x120 throw): Warrior 14 / 1.5 / 10.4,
  Valkyrie 9.5 / 2.25 / 15.2, Wizard 8.75 / 0.75 / 22.4, Archer 8.75 /
  1.0 / 15.2, Dwarf 14 / 1.75 / 10.4, Knight 10.25 / 2.25 / 14.0, Jester
  8 / 1.0 / 16.4, Sorceress 8 / 0.75 / 22.4; health 500. Armour is taken
  off each hit (ModifyDamage: d − armour).
  - lgw1 (Warrior, new game, no assist, --loot --shop-buy heal,potion
    --require): L1 ends (update 30,183); all six G1 tries die by update
    8,085 (247 throws to 33 swings: the kiting rules keep knights 3–4 units
    off). Melee tactics for a class whose melee beats its throw (no
    assist: no stepping away; attack within reach by the game's measure,
    walking at the foe otherwise; generators fought only with no foe within
    5 units, struck from 3): wg1c–wg1i still die in G1 (4,965–12,180;
    knights within 3–4 units 400–600 of 600–760 health lost; levels 2–3).
  - The Sorceress, legitimate G2 from lg1's G1 end with the generator fix
    (Medium, sg2a): five of six die (18,135–23,625); the sixth stands
    unhurt at (−90, 0.9, −135) from update 10,000 to 200,000 (a route
    problem, not a fight).
  - Easy (Game Options difficulty 0), sg2e: five of six die (20,871–
    41,629; 850–960 health lost a try). Knights within 6 units cost under
    100 a try; hits of 19 or more (the KNI statues' creatures: 29–39 from
    6–20 units, plus 6-point ticks; 277 and 450 health) 420–540, hazards
    174–293. Statues wake on contact or a hit (gameplay.md "Statues"), and
    sg2e-d541 passed statues 113 and 114 (KNI) at 1.0 units just before
    its first burst (statue 57, a DEATH statue, at 1.1).
  - Contact avoidance (c64, no assist): the stick direction is turned by
    the least of ±30°, ±60°, ±90°, ±120° that keeps the points 0.5, 1.0
    and 1.5 units ahead out of contact (the player's radius + the item's
    reach + 0.4) with every dormant statue (kind 4, +0xf8 and +0xd8 bit 0
    clear) and every live hazard (kind 8 in state 2 or 4 with +0xd8 bit
    0); route costs already kept the paths off them, this covers stepping
    away from foes, back-offs and corner cutting. sg2f (Easy, no assist,
    from lg1's G1 end, the shop's cherries bought with the character's
    gold): two of six end G2 by its regular exit — delay 541 at update
    27,949 (health 560) and delay 61 at 143,117 — with no health assist;
    three die (17,229–39,789), one is alive at 200,000. The first
    legitimate G2 ends. Chain lg20 (a new game, Easy, no assist, --loot
    --shop-buy heal,potion --require, the critical path L1 … H4 with J3)
    ended L1 and G1 (level 3, no gold) and stopped at G2: six tries died
    (17,190–70,366), the search reached progress 20. lg1's character had
    entered G2 at level 5 with 400 gold (eight cherries: health 660).
  - Chain lg21 (no assist, Easy, from sg2f-d541's end: the legitimate
    campaign so far is lg1's L1 and G1 on Medium, then sg2f-d541's G2):
    G3 (legend item), G4 (rune), G5 (the lich killed at update 7,155 with
    19 health left, auto exit), B1 (rune), B2, B3, B4, B5 (legend item),
    B6 (the dragon killed at 3,045), A1, A6, A2 (legend item), A3 (rune) —
    13 levels, no health assist; A4 stopped it: three tries died, three
    were held to update 300,000, two of them beside the GENERAL statue 54
    by (−18.8, 0, −2.6) (516 stuck events; the way to goal 262 passes
    within its contact), and the search reached progress 14.
  - Contact allowed (c66): three stuck events within contact + 1.5 units
    of a dormant statue let keep_off pass it (logged `contact_ok`); the
    bot then wakes and fights it. lg22 (A4 onward from lg21's A3 end)
    repeated lg21's A4 update for update: statue 54 (and key 179 beside
    it) are four-player items (+0xe0 4, +0xe1 2 and 1, no collision), so
    nothing blocked the bot there; it stood throwing at a critter 6.25
    units off at (−18.8, 0, −8.9) that never lost health (a4dbg3, attack
    branch tags from GDL_BOT_TAG_FROM). Rule (c67): a fight target fired at
    for 600 updates (gaps under 100) without its health dropping is no
    target for 3000 updates (logged `foe_skip`). a4t (six A4 tries from
    lg21's A3 end): delay 263 ends A4 by its regular exit (wave 0x104) at
    update 142,182 with 1,284 health and no assist; five die (18,615–
    45,025). Chain lg23 runs A5 onward from a4t-d263's end.
  - Followers (co-op runs) take the DEBUG health assist only when the
    leader has it.
  - Assisted campaign tail with the real J3 win (tfull2, from tfj3h-d397's
    J3 end, bot binary c64, DEBUG health assist counted): J6 (update
    103,015), J4, J5 (boss), E1, E2 (Skorne 1), F1, F2 (Skorne 2), H1, H2
    end in order; H3 took its rune in two tries but did not exit within
    300,000 updates. tf2h3 (six H3 tries, 600,000 updates): delays 541 and
    61 exit by wave 0x803 (state 5) at updates 340,673 and 400,632 with
    rune 545 taken (3 assists each); the other four are alive at 600,000
    without the rune. tfull3: H4 (Garm) from tf2h3-d541's
    end — Garm killed and the auto exit (wave 0xd00, state 1) at update
    6,848, one assist. With tfull1 (new game … J2) and tfj3h, every
    non-secret level is ended in campaign order with its rune and legend
    item, DEBUG assist included.
- 2026-10-26 (night): the Tower's realm gates were not checked by the
  chains (lg1–lg23, tfull1–3 load each level directly).
  - WorldOpen (tower.rs) opens an ordinary realm when a player holds the
    full count of its WorldCrystal set (class record +0xd0a + 2n at
    MAX_CRYSTALS[n], or −1 once delivered). From the ELF tables (game-run
    report fields world_crystal, max_crystals): G set 1 (15, the Tower's
    own crystals), B set 2 (100), A set 3 (125), K set 4 (150), D set 5
    (175), C set 6 (200), I set 7 (225), J set 8 (250); Temple, Hell and
    Battle open on shards (and Hell on runes).
  - Crystals per level (kind 1 subtype 0xf, +0xf4 the set; survey of all
    61 levels at load from a4t-d263's end, one player — game-run report
    field crystals_at_load, private scratch): set 2 in G1–G4 (40, 35, 35,
    15: 125); set 3 in G (44) and B1–B5 (96): 140; set 4: 25 + 21 + A's
    114 = 160; set 5: 5 + 7 + 28 + K1–K4's 140 = 180; set 6: 8 + 15 + 30
    + D1–D4's 150 = 203; set 7: 5 + 20 + 60 + C1–C4's 140 = 225; set 8:
    5 + 13 + 41 + I1–I4's 191 = 250. I and J need every crystal of their
    set in one play of each level; PlayerGiveCrystal counts every pickup up
    to the maximum, so replays make up the rest. Boss levels, J, E, F, H
    and S hold none.
  - The legitimate chain was past every gate: lg21 entered B1 with 60 of
    the 100 set-2 crystals and A4's end held 60/57/82 of sets 2–4 (the
    probe's realms_open: only G and S). The assisted chain likewise
    (tfull2's H2 end: 55/54/70/63/91/60/74). Those runs show the levels
    can be traversed; they are not a legitimate campaign.
  - Legend items serve another realm's boss: the pickup sets class-word
    bit +0xf4 (PlayerGiveLegendItem) and GauntWorldInit tests bit
    CurWorld. Bits seen: G3 → 9 (I5), B5 → 11 (K5), A2 → 2 (B6), C3 → 4
    (D5), D4 → 1 (A5), I4 → 3 (C5), J2 → 10 (J5); the bit is cleared at
    its boss (tfull1: bit 9 at I5). Crystals open K and D before A5 is
    fought, so D4's scimitar can be carried to the chimera.
  - A5 without the scimitar (lg23, six no-assist tries, Easy): every try
    died by update 4,667; the chimera (4500 body, heads 900/1125/900) lost
    at most 758. CritterDamage scales a boss's damage by 1, 1, 0.5, 0.3,
    0.2 for 0–4 active critters outside QuestItemState 1–4 (0.2 with the
    body and three heads), and a head's damage is also taken from the
    body. The bot never used its boss branch: the heads (the nearest
    foes) set the attack range to 18 and the body stood 18.1 from the
    nearest node; it threw at the heads, mostly 10–20 units above it.
  - Bot (c69): seek_crystals=1 (game_run.py --seek-crystals) makes every
    crystal of an unfilled set a goal before the other lists (optional
    after three stuck events, like gold and food). campaign_chain.py
    --gates probes each level for two updates from the carried state and
    requires its realm in realms_open; a closed crystal gate is filled by
    replays (bot, --seek-crystals, no --require) of the played
    --replay-pool level with the most crystals of that set, at most
    --max-replays per gate; a closed shard gate stops the chain.
  - Chain lg24 (no assist, Easy, loot, crystals, gates; from lg21's G4
    end — L1 G1 G2 G3 G4 legitimate so far) runs the bosses after the
    realm that holds their legend item: B1–B5, A1 A6 A2 A3 A4, B6, K1–K4,
    G5, K5, D1–D4, A5, C1–C4, D5, I1–I4, C5, I5, J1 J2 J3 J6 J4 J5, E1
    E2, F1 F2, H1–H4. Its first gate: B1 closed (set 2 at 60), G1
    replayed — 38 of its 40 crystals taken (60 → 98) and the exit at
    update 26,776 —, B1 still closed, G1 again.
  - A5 tests (no assist, Easy, 60,000 updates, six delays; scratch
    reports a5N/a5L/a5M/a5D in private/reports): from a4t-d263's A4 end
    (level 28, 3200 health) without the scimitar (c70: boss range from
    BossCritInst) every try dies by 4,503 with the body at 3,108–3,995;
    with the DEBUG legend item (a5L, c70) every try dies by 6,559 with
    the body at 1,967–3,198 (QuestItemState 1, 2, 3, 5, 6 by update 450;
    the same head ends at 0 in every try). Bot c71: in the boss branch
    the turbo attack every 40 updates at +0x800 ≥ 40, and the weakest
    child within 6 units of the player's height as the target (a5M: body
    at 1,775–2,844 at death). Damage taken is spread over the whole fight
    area (8–15 per 15-update sample from z −20 to +5). From tfull1's D4
    end (level 43, 4380 health, the scimitar carried — the legitimate
    order's strength at A5; assisted chain's state, capability check
    only), c71: five of six tries kill the chimera (auto exit, state 1,
    updates 7,195–9,063, 280–701 health left); one dies at 7,011.
  - lg24 was stopped in its second G1 replay and restarted as lg25 on
    c71 from lg24's G1 replay end (the same arguments).
  - Completion record: game-run's end states never ran game_main's level
    end, so no chain recorded a completion (every wave byte 0 through
    tfull3's H4 end). Since c76 the end state of a level that ended (a
    player through do_exit or the boss's auto exit — level_done, which
    stops the traversal about 20 updates before do_players reports done —
    or do_players done and the exit test) runs game_main_level_end:
    PlayersSetWaveFinished (wave bit, the realm-progress hint words) and
    the destination (end state field level_end_flow). Check: B2 from
    lg25's B1 end ends at update 8,812 as before; its end state has B's
    wave byte 0x2 and the probe opens B3's portal (B2's stays closed: B1
    was played before the change). The hint words and B's other bits feed
    only presentation (Sumner's hint menu, the rune thermometer; hints.rs,
    thermometer.rs), so earlier chains' play is unchanged. A normal exit
    returns to the Tower (the shop) whatever the exit's destination; only
    E1, F1 and world 12 go on by the map.
  - Portals (the Tower's 49 EXIT_PORTAL items, +0xf0 the wave; HideExits:
    level n needs bit n−1 of the world's wave byte, E/F the realm, H4 all
    runes); game-run reports level_open at load and campaign_chain --gates
    stops on a closed portal in an open realm. LEVL record order is the
    portal order: A1 A6 A2 A3 A4 A5, J1 J2 J3 J6 J4 J5 and K2 K3 K4 K1 K5
    — every chain so far played K1 first, which the Tower does not open
    before K4.
  - Completion persistence tooling: gdl_tools state-save (an end state's
    player 0 save region through profile::save_region with gdl_play's
    controls, into a native save slot; DEBUG tooling standing in for the
    walk from the level end to the Tower) and completion_check.py (gdl_play
    loads the slot into the Tower — GDL_TEST_LOAD_SLOT —, dumps, saves
    from the Tower, a second run reloads; checks the completed-level bytes,
    runes, shards and gold against the end state and across the save).
    Check on lvl-B2-c76's end (completion-b2test1/b2test2): PASS — B's
    wave byte 0x02, runes 392, gold 20 loaded, saved and reloaded equal.
    Found on the way: gdl_play's TEST progress dump took the class from
    +0 (the player index) instead of +0xc, so every scenario's gold was
    class 0's (fixed; save-reload passes again on the fixed client,
    scenarios-m13gold1). run_play.py takes GDL_PLAY for another client
    build.
  - No-assist probes of C1 and F1 (from the assisted chains' D5 and E2
    ends): F1 — 181 HEL generators — drains 0.37 health an update
    (types 2/20/24/25 at 4.4–7.2 units; 8,100 of 7,900 + food by update
    22,000–28,000 in every try), where the assisted win needed about
    52,000 updates; the worst stretch is by generator 170 (−105, 13,
    −114). Firing every update out to 10 units (c77, GDL_BOT_RANGED_FAR,
    an experiment switch) changed nothing (f1x: all die by 28,205).
    Powerups (kind 1 subtypes 5–9; c79) are loot now — taken always but
    a second size potion — yet few lie within LOOT_RANGE of the route
    (f1p: the same deaths); the Tower shop's powerups last 15–45 s
    (shop_buy). C1: a critter hit the bot for 8–43 from 10–19 units, out
    of its 10-unit fire range, for 6,000 updates (305 → 268 health, the
    player 3,400). c78: a losing exchange (900 updates of fire, the foe
    down under a fifth, the player 800 or more) skips that foe for 6,000
    updates; c80: hurt within 60 updates with no foe within 10, a live
    critter within 20 units is the ranged target.
  - C1 again (six tries each, all die): c80 retaliation (c1r, deaths
    9,258–27,354), without crystal seeking (c1n, 11,575–35,573: crystals
    are not the cause, though c1r-d149 lost 2,700 in a pit trying for
    crystals 388/389 between GRU generators 148 and 234). 61 % of the
    damage lands while the player stands (moved under 0.5 in a 15-update
    sample); tags (c1n-d61-tag) put most of it in the generator branch,
    which fought generators until a foe came within 2 units while type-7
    creatures struck from 2.3–3.7. c81: the guard is 4 units (5 after a
    hit within 30 updates) for a ranged class — c1g: still six deaths
    (16,861–30,519). The assisted chain took 4–6 assists on every C
    level (Medium).
  - lg27 (c76) ended L1 and G1 (level 3, 29 health, no gold after G1:
    the Tower's refill gives 500 for G2); all six G2 tries died (26,339–
    60,472) and the search ran — stopped. campaign_chain --grind N: when
    every bot try at a level dies, the last level ended (not a boss
    level) is played again for experience and gold (a replay, before the
    search), at most N times per level. Chain lg28 (c81, --grind 4) runs
    G2 onward from lg27's G1 end with the same plan.
  - c82: a crystal is skipped after one stuck event (C1's tries were
    stuck three times at each of ten crystals in its pits; c1g) and
    crystals are goals only with half the level-start health or more.
    C1 (c1h): the rune taken in three of six tries and the deaths later
    (17,408–57,941), one try alive past update 60,000; C1's food for one
    player is scarce (its 16 food records include multi-player ones).
  - c83: an optional goal item stood beside (1.5 units) for 300 updates
    and still there is skipped (logged `skip_unpicked`; c1h-d263 stood
    0.5 units from BOOSTICON 78 from update 21,163 to 36,913, firing at
    foes behind a fence), and without the assist a foe fired at for 300
    updates (was 600) without losing health is skipped. C1 (c1i, 150,000
    updates): delay 397 takes the rune and exits (wave 0x301) at update
    44,133 with 667 health — the first no-assist C1 end; delay 61 alive
    at 150,000 (2,761 health, no rune); four die (20,018–88,696).
  - lg28 (c81): G2 (try 4, 30,225), G3 (legend item, the sixth try), G4,
    then B1 directly (set 2 full from one play of G1–G4 with crystal
    seeking), B2–B5, the A gate filled by one B4 replay, A1 A6 A2
    (iceaxe) A3 A4, B6 (the dragon killed at 4,186 with the iceaxe), the
    K gate by two A1 replays, K2 K3 K4 (goodbook), G5 (the lich at 5,493
    with it) — no assist, every portal and realm gate passed.
  - lg28 was stopped in K1 and continued as lg29 (c83) from its G5
    end. K1 (the portal order's fourth K level): five tries alive at
    300,000 without the rune, one dead (77,346). Four of them did hints
    0–10 and 12–13 but skipped hint 11 (the rune, 192) — the hint's stuck
    count carried the earlier hints' events (bot5 skipped it 180 updates
    after hint 10) — and timed out 84–88 times on one ride edge by the
    rune ((−189.75, −29, −47.25) → (−189.75, −33.2, −45.75)); delay 541
    skipped hints 2–4 (loot and crystal goals while hint 2's target had
    no path) and timed out 442 times on lift (−69.75, −33.7, 74.25).
    lg29 was stopped in its K4 grind replay. c84: the hint stuck count
    starts again with each hint.
  - K1 on c84 (k1a, 150,000 updates): delay 263 takes the rune and exits
    at update 89,383 (1,082 health) — the first no-assist K1 end; three
    tries skip the rune hint again (ride timeouts by it), one dies, one
    idles at the lift. lg30 (c84, from lg28's G5 end, --grind 2): K1 (the
    same delay-263 win), K5 (the boss at 4,714 with B5's javelin), the D
    gate filled by two K4 replays, D1, D2 (rune).
  - H3 without the assist (h3n, c83, 600,000 updates, from tfull2's H2
    end): both tries alive (6,023 and 4,139 health) without the rune; of
    H3's 183 route hints (36 items, then goto waypoints) they skipped
    every goto hint after 60 stuck events (one per ~10,000 updates), which
    the assisted win had done 15–25 updates apart. c85: no contact
    avoidance (keep_off) while a goto hint is the goal — the statues and
    hazards beside H3's route turned the stick off its waypoints.
    campaign_chain's BOT_DELAYS run to twelve (--bot-tries up to 12).
  - lg30 went on: D3, D4 (scimitar), A5 (the chimera killed at update
    7,539 on the first try, the scimitar carried — legitimate order),
    the C gate by four D2 replays (set 6: 125 → 200, about 25 of D2's 38
    a replay), then C1: all six tries died, twice, around two D2 grind
    replays (level 45–46 against C1's target 47).
  - LEVL +0x9c is each level's target player level (gdl_tools levl-list):
    G 1–15, B 12–20, A 19–30, K 28–40, D 0, C 47–55, I 54–60, J 62–70, E
    73–74, F 75–76, H 77–80. A player's damage to enemies and generators
    is scaled by 1 − 0.01 per level below it and 1 + 0.1 per level above
    (EnemyDamage, DamageItem); max health is 500 + 100 per level. Enemy
    damage does not follow the player's level (the enemy's +0x384 copy of
    PlayersAverageLevel is not read in the port). A D2 replay gives about
    3,000 experience (one level at 45), so grinding is slow.
  - C1 tactics: c86 retaliation also against enemies within 12 units and
    8 of height (type-7s struck from 2–6 units above the pit floor), the
    generator branch yielding to them; c87 danger rule (8 % of the start
    health lost in 300 updates within 6 units: the optional goal and
    every optional item within 10 units skipped); c88 nest mode on the
    danger rule — 600 updates of generators first whatever foes are near
    (the guard had kept the bot firing at the spawns of GRU generators
    148 and 234, standing at (−35.5, −12, 23.4) for 1,000 updates, c1k)
    and a potion against a crowd. From lg30's D2-6 end (c1l, c88): delay
    263 takes the rune and exits (0x301) at update 46,179 (588 health).
    lg30 stopped; lg31 (c88, --bot-tries 12) runs C1 onward from there.
  - lg31: C1 by try 4 (delay 263, update 46,179, rune taken; end state
    campaign-lg31-C1-bot4-end.json). C2: all twelve tries died (updates
    7,677–25,063), the C1 grind replay lost all twelve too, and lg31 was
    stopped. C2's damage: type-7 creatures dealt 54,028 of 64,116, mostly
    from 3–4 units at the bot's height with 20–30 health each, 63 % of
    it taken standing still (character level 49, 5,260 health, 440 gold,
    no potions). Experiment switches (c89, labelled EXPERIMENT in bot.rs,
    off unless set): GDL_BOT_GEN_GUARD=N (the ranged class's generator
    guard) and GDL_BOT_CROWD_POTION=1 (a potion when three foes are within
    6 units); every variant died. c90: a foe with at most twice the
    throw's damage (+0x120) is shot, not kited — C2 4 of 12 (delays 61,
    683, 811, 1193; updates 119,161–123,841) and F1 2 of 6 from tfull2's
    E2 end (f1r, c91: delays 0 and 263 at 105,334 and 28,541, to F2;
    every earlier no-assist F1 try had died by 28,205).
  - Every c90 C2 win left by SECRET_ICON 516 (kind 9 subtype 0x32, wave
    0xc00, S1), as the assisted chains t29 and tfull1 had. A secret exit
    sets do_exit's destination from skip_wave 3..=11 and SaveStage records
    C2; game_main's level end runs PlayersSetWaveFinished and goes by the
    map to the treasure room, whose timer (skip_wave 0xd) returns to C2 by
    LoadStage — the regular exit is still to be reached. A chain step ended
    by one skips the room and the return, so c91 no_secret=1 (game_run.py
    --no-secret; campaign_chain passes it to every bot run and does not
    count a try that ends in world 12) keeps the bot off secret exits
    (their nodes closed). No win of lg27–lg31 had ended in world 12.
  - Why C2's regular exit 187 (234.5, 21.3, −29) was never a goal: its
    platform is reached up stairs of 1.25-unit risers (object 1340, flags
    0xa: a step object) from y 10 to 21.3, and the graph (c2x-d683's dump
    at update 60,000) held the stairs' edges downward only. A column centre
    0.16 short of a riser samples the lower step, and the level edge onto
    it, swept 1.5 above at radius 0.3, touched the riser's top corner
    (0.297). The player climbs them: c2-step-m4 (DEBUG teleport onto the
    step, Up held) walks from y 13.75 to the exit platform (239.2, 21.19).
    c92: a permissive-sweep hit on a step object (+0x10 & 0x38) passes
    when the player-like test does (edge_ok, which already ignores step
    objects); the graph grown from (190, 10, −28) reaches the exit
    (c2z-c92; c91 stops at x 207.8).
  - lg32 (c92 from lg31's C1 end): all twelve C2 tries died (updates
    22,787–39,128). c92 had also joined C2's west pit floor (object 1181
    at 5.7, under a step object: 35,559 more edges from (−104, 9, −74);
    explain_walks: the game's displacement walks 5,093 of the 5,234 flat
    ones), and with it 343 climbs onto the rim (objects 1196 and 1201 at
    6.6–8.7) that it does not walk; one blocked a stuck event, the tries
    spent 10,000 updates there (try 7: key 353 the goal from 8,104 to
    18,634). Most of the damage: type-7 creatures (GRU generators' spawns,
    15–45 health, EnemyMissile t 7) within 5 units, 29,434 of about
    65,000 over the twelve tries; 12,312 from beyond 10 units. c94: a climb
    (over a unit) not taken after its run-ups blocks every climb from the
    same object onto the same object within 12 units (the drop rule's
    counterpart). lg32 stopped; lg33 (c94) repeated lg32's twelve deaths
    update for update: the stuck edges were not climbs but rim object
    1196's drops of 1.9 onto the pit floor, and only four such gateway
    edges (from the nodes c91 reaches; c2p dumps) let c92 into the pit.
    c95: the step-object rule only on edges that do not drop (b no lower
    than a − 0.5); the pit graph is c91's again (5,007 nodes), the stairs
    still reach the exit, and the start graph differs from c91's by three
    edges. Nav::explain also reports the permissive sweep's hit (loose).
    lg33 stopped; lg34 (c95) runs C2 onward: C2 by try 7 (delay 683)
    through the regular exit 187 (wave 0x302, update 32,272, 544 health,
    no assist; level end Shop), C3 by try 11 (rune and the bellows, update
    161,678), C4 by try 11 (41,970), D5 (the bellows carried: the boss
    killed, auto exit at 8,600, first try). The I gate needs set 7 at 225:
    B holds 5, C 140, D 60, K 20 (the probes' crystals_at_load), and the
    character held 107 after D5 — the bot took 13 in C2, 20 in C3 and 6 in
    C4 (C4: 19 of its 46 crystals made goals, 18 skipped after one stuck
    event, half the samples under half health). Replays of C2 (36 shown):
    the first +21 (try 8, update 23,057), then +16 and +12 (144, 156):
    each skipped 34–42 of its 44 crystal goals after one stuck event (the
    c82 limit). c96: crystal_stuck=N (game_run.py --crystal-stuck; default
    1) and campaign_chain --replay-crystal-stuck N for crystal replays only
    (a death there costs one try of twelve). lg34's fourth replay +15
    (171), then lg34 was stopped; lg35 (c96, --replay-crystal-stuck 3,
    --max-replays 16) from that end: C2 replays +15, +29, +10 → set 7 at
    225 (character level 61), the I realm open. Then I1 (try 4, update
    41,493), I2 (try 10, 46,062, rune), I3 (try 2, 167,585, rune), I4
    (try 2, 36,133, the lamp), C5 (the lamp carried: the boss killed, auto
    exit at 8,011, first try) and I5 (the yeti killed, auto exit at
    11,829, try 2), all without the assist. The J gate needs set 8 at
    250: the I levels hold 48/44/45/54, and the character held 129 after
    I5 (level 66); the I4 replays +41, +28, +38, +14 → 250 (level 68).
    J1 (try 8), J2 (try 12, the lantern), J3 (try 3), J6 (try 6, rune), J4
    (try 3, rune). J5: all twelve tries died with the wraith at
    1,593–2,889 of 5,000 (updates 3,418–5,175). The wraith (fixed at (0,
    −11.9, −35)) did all the damage, hits of 97.5–157.5 at 9–39 units (most
    of the fight at 12, the closest node), from many of its moves (hurt-log
    boss_move, j5m-d1061: moves 7–11, 13, 17, 18, 20, 22); the legend window
    (QuestItemState 1–3) took the first ~1,000, then 0.6 an update against
    the player's 2.2. Neither a sideways step when hurt (c97 EXPERIMENT
    GDL_BOT_BOSS_STRAFE=1, j5s: all dead, 1,135–2,782) nor two strength
    upgrades bought instead of potions (j5f: all dead, 1,756–2,828)
    changed it. The assisted wins (tfull2, level 70, four health assists)
    took ~7,700 updates, about 14,600 damage. J5's target level is 70
    (LEVL +0x9c; J1–J4 62–67): above it the player's damage gains 10 % a
    level and health 100, so the chain grinds J4 (target 67). The first J4
    grind (85,920 updates) took the character from 68 to 73, but J5 again
    killed all twelve (1,707–2,982, no potions: 300 gold): CritterDamage
    applies no level scaling to bosses (kind 4), and the throw damage
    +0x120 grows little (30.92 at 72, 31.04 at 73). DEBUG powerup probes
    (--powerup reflect/invulnerability) gave the same runs update for
    update: the powerup does not survive the level entry. The distance:
    c99 GDL_BOT_BOSS_KEEP=R (back away inside R, no walk back to the fight
    node), six tries each from the grind's end — 22: none, 26: two (boss
    killed, 47 and 117 health left), 30: one, 34: one. c100: the wraith
    (BossType 0x28) is fought from 26 units without the assist. lg35
    stopped; lg36 (c100, --grind 4) runs J5 onward from the grind's end:
    J5 (try 3, the wraith killed, auto exit at 7,970), E1 (try 10, 20,855,
    the soul; level end by the map to E2), E2 (Skorne 1 killed, auto exit
    at 11,369, first try), F1 (try 2, 25,857, to F2), F2 (Skorne 2 killed,
    first try, update 5,208; level end Movie(0x2b), the "garm" movie), H1
    (try 4, 23,957), H2 (try 7, 48,422) — all without the assist. At H2's
    end: level 80, 21,510 gold, runes 0xfff (H3's is the thirteenth),
    shards 2046. The H3 wins of the c85 probes came at 393,292 and 419,376
    updates, past lg36's --bot-first 300000, so lg36 was stopped at H3 and
    lg37 (c100, --bot-first 600000) runs H3 and H4 from H2's end.
  - lg37: H3 by try 7 (delay 683, update 320,546, the thirteenth rune),
    H4 by try 11 (Garm 5,625 → dead, auto exit at update 7,303, 3,799
    health; level end Movie(0x2c), "victory"). **The legitimate campaign
    reaches the ending**: lg27 (from a new game: after L1 level 1, no
    completion bits, runes, shards or gold) → lg28 → lg30 → lg31 → lg34 →
    lg35 → lg36 → lg37, each chain from the previous one's end state
    (chain logs private/reports/campaign-lgNN-log.txt), no DEBUG assist.
    The H4 end (campaign-lg37-H4-bot11-end.json): level 82, runes 0x1fff,
    shards 4094, 17,820 gold, completion bits 003f3f1f1f03031f0f1f3f1f0000
    (every non-secret level). completion_check.py on it: state-save into
    slot 1 of private/runs/completion-lg37h4b/saves, the client (target-dev
    gdl_play) loads it in the Tower, saves it and reloads it — world,
    completion bits, runes, shards and gold equal throughout: PASS
    (private/reports/completion-lg37h4b.json; the screenshot shows PELE,
    level 82, 17,820 gold, 3,799 health). A first run (completion-lg37h4)
    failed before any comparison: the private Xvfb's authority file under
    /tmp had been removed (the session was 11 days old); a new session on
    :192 replaced it (the old record kept as
    private/reference/xvfb-session-191-stale.json).
  - H3 without the assist (c85, from tfull2's H2 end, 600,000 updates):
    delays 61 and 541 take the rune and exit to H4 (updates 419,376 and
    393,292; 163 and 167 hints done); delay 0 runs out of updates.
  - Chain lg26 (c75) was stopped in L1 (its end states would not have
    recorded B1). Chain lg27 (c76, a new game, no assist, Easy, loot,
    crystals, gates with portals): L1 G1–G4 B1–B5 A1 A6 A2 A3 A4 B6 K2
    K3 K4 G5 K1 K5 D1–D4 A5 C1–C4 D5 I1–I4 C5 I5 J1 J2 J3 J6 J4 J5 E1 E2
    F1 F2 H1–H4. lg25 (c71, no completion record, K1 before K2) goes on
    as a reconnaissance chain only.
  - Feasibility probes (one no-assist try each from the assisted chains'
    end states, c71, capability checks only): F2's Skorne 2 and H4's
    Garm die (the level ends alive: 2,119 and 5,768 health left); E2's
    Skorne 1 (6000) stood in its pit 15.4 units under the floor at
    (−0.1, −25.4, −19.6) and the bot, knocked back past the one closest
    node, explored for 4,000 updates with Skorne at 2,171, then died.
    Bot c72: on boss levels every node within 3 units of the closest
    node's foe distance is a foe goal (e2t: two of six kill Skorne, the
    rest die with it at 119–347). c73: a potion every 300 updates in the
    boss branch without the assist (the potion rule never ran at a boss);
    the magic blast barely touches Skorne (e2u: 12–18 potions a try, two
    of six win). c74: back to the fight node in alternate windows when
    knocked more than 2 units past it — E2's tries had taken 8.6–10.6 a
    sample and dealt 12.4–12.8 from 24–28 units, but 17–20 against
    2.3–3.7 from 32–36 units, a quarter of the fight (e2v: two of six;
    a5v, A5 at A4 strength with the DEBUG item: one of six, the first).

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

Resolved 2026-10-27: with entry 0 (Settings) selected in the Tower's pause
menu, pause-tower's frame had shown Sumner's unlock scroll and the trace no
page change — the flow dismissed four of the Tower's scrolls, and the
unlock scroll still up took the accept. pause-settings (six dismissals)
opens Settings (tower_options: Audio, Compass, Controls, Screen) and backs
out to the pause menu: menu-flows-m17-v26 PASS (actual disc, the client on
the private display).

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

## Playtest after 1.0 (user, 2026-10-30)

- "An elevator in the Poison Fields with a rainbow texture and a green
  switch beside it; I pressed every button at point-blank range": the first
  area (G1), the user on the floor beside it. That is G1ELEV1 (object 1083,
  wobj inity 19.8, starty −8: the platform with the animated pinwheel), at
  the bottom from the load. Its triggers (item dump at load, lever-g1a):
  BRIDGEPAD 418 riding on it (flags 0x50c: every active player on the
  bound object, toggling with the +0x100 timer), TRIGGER3 428 on the floor
  beside it (flags 0x9: a touch clears byte_16 bit 0x20 — the lift comes
  down) and TRIGGER3 429 at the top (flags 0xa: a touch sets it — the lift
  goes up). So the floor post only calls the lift down; with the lift
  already down a touch changes nothing. Headless (game_run, lever-g1d-*):
  walking onto the platform starts it at once and it reaches the top 2 s
  later (−8 → 0 at 4 units/s); walking to the floor post touches it (+0xe2 1)
  with the lift staying down. Client (RX 580, frozen-lift-g1b): the walk
  onto the platform carries the player up beside the upper deck; the floor
  post's top shows red once the lift is up. The trigger code is
  instruction-derived (item_runtime process_trigger, process_item_wobjs)
  and its elevator case replays equal against trace-J4-combat-v2
  (J4ELEV1); this lift itself was not captured in the reference.
  "All switches might be suspect": kind-5 subtype-0x1f levers ("TRIGGERS",
  G2 item 537 → G2BAR_WALL, G4 511–514; none in G1) take a hit
  (DamageItem sets +0xe2 0xf: G4 512 by a melee swing, lever-g4c-atk) and
  a touch; G4 512 raises G4ELEV12 in the headless run. The legitimate bot
  campaign to the ending (M15) uses pads, triggers and shot switches.
- "Back (Y) on the Save/Load menu before loading a character leads to a
  broken avatar selection; Escape does not close it": Back on the select
  screen's New/Load menu with nobody else in is sel_leave → psel_abort_req,
  and do_player_select then calls init_titlescreen 0x1841a0 (0x189b94..
  0x189ba4; after the final level instead EndTower, inactivate_player for
  each and init_attract_mode(0x8000), 0x189b5c..0x189b84). The client had
  kept an interim message there, and the empty game went on into the
  Tower with no player (v1.0: "UNSUPPORTED camera: front-end fixed
  camera"). Now the session ends and the Press Start page follows (or the
  attract sequence after the ending). GUI flow select-back (menu_flows.py;
  the select reads player 1's keys, triangle I): select-back4 passes
  (Press Start, then Start reaches the select screen again); the same
  flow on the 1.0 binary fails (select-back-v100). All 23 menu flows pass
  on the fixed build (menu-flows-fix1-all); title_start_check fix1 6/6
  and fix1-attract-xbox 2/2 on the RX 580.

