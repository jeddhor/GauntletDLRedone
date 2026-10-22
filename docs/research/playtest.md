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

Controls: arrows/WASD move, Space/K throw, E/L use a potion (magic), F1
toggles the diagnostics block, stand still in the exit portal to leave the
level, F9 debug-skips to the next level. Death: do_players state 8 plays the
dying action, then kill_player — in a realm the player is out (keys
dropped, powerups cleared); with nobody left game_main goes to TowerWave and
the level-start state is restored (PlayerRestoreState); in the Tower the
player is restored at once. Message scrolls close with Enter/Esc (the
original's Triangle). Menus: arrows/WASD or D-pad,
Enter/Space or South to accept, Esc/Backspace or East to go back; Esc on the
"Press Start" screen exits. The HUD shows colour/class, level, experience,
save slot, health, gold, keys, potions, kills, active enemies and standing
generators; F1 adds the interim labels and any unported path hit.

Audio: original sound effects, announcer lines and level music play through
ALSA (research/audio.md); mixing is approximate. `--virtual` runs are silent.

Bosses (research/critters.md): the native critter engine (boss AI, grab and
fire cones, the chimera's heads, BOSSGEN, death, coin spew, the good wizard;
statue golems, gargoyles and generals) with PlayerCollideCritters; critters
are drawn from their skeletons without sequence blending (not modelled). The
HUD shows the boss meters.

Current state (2026-10-22): the simulation (do_players, do_enemies,
ProcessItems, ProcessEffects, world animation, the StdCam) is compared update
by update with reference traces of G1 combat and the Tower's new game — every
part equal on trace-fight-g1-v6, trace-combat-g1-v4, trace-varied-g1-v2 and
the Tower traces except sound items' audio handles (gameplay.md). The
paragraphs below up to the smoke tests describe the 2026-10-16..18 interim
build and are kept as history; the materials, item poses, enemy frames,
missiles and effects they call presentation-only have since been ported
(rendering.md). Still approximate: audio mixing (SPU curves), critter
sequence blending, the GS's per-draw integer truncation in lighting
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
