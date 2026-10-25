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
    bridge 1054, the mover-place loop above), lg10 at 26,561–104,955.


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
