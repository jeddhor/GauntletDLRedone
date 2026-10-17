# Interim native playtest (not the M12 slice)

`python3 tools/python/run_play.py` launches `gdl_play` on the current desktop
(`--virtual` uses the authenticated Xvfb session). It writes
`private/reports/play-config.json` with every package resolved by verified
archive identity, then runs the native game simulation (research/gameplay.md)
for the Yellow Sorceress in The Poison Fields, levels G1–G4.

`--realm A`…`K` plays that realm's levels instead (level-code order with the
boss arena last; `--level A3` starts later, `--level G5` goes straight to the
lich). The realm's
WDATA, every monster package its levels load, and per level both the level's
own ITEMS package and the realm's (HYPOTHESIS) are configured. Exits follow
the destination wave to the matching LEVL record; defeating the boss ends the
session with "COMPLETE" (the hub is not ported), any other realm's wave ends it
too. The HUD names other realms by their WDATA file
(e.g. "Castle realm (A)").

Campaign: `python3 tools/python/run_play.py --campaign` starts in the Tower
(hub) with every realm configured; realm portals open per the save's
progress, a defeated boss returns to the Tower. Packages load per level.

Controls: Enter starts from the title prompt; arrows/WASD move, Space/K
throw, E/L use a potion (magic), P pauses, F1 toggles the diagnostics block,
stand still in the exit portal to leave the level, Enter continues after death
(interim: restores the start-state health), F9 debug-skips to the next level,
Esc quits. The HUD shows level, experience, health, gold, keys, potions,
kills, active enemies and standing generators; F1 adds the interim labels and
any unported path hit.

Audio: original sound effects, announcer lines and level music play through
ALSA (research/audio.md); mixing is approximate. `--virtual` runs are silent.

Bosses (research/critters.md): the boss is simulated by the native critter
engine and drawn from its skeleton (no sequence blending); the HUD shows its
health. Only the lich (G5) has been run end to end; other bosses may stop at an
unported path (grab, fire cones, the chimera's heads), shown as SIMULATION
STOPPED. Players currently walk through critters.

Start: G1 uses the state exported from the preserved save
(`export_game_state.py` v6 + `export_start_state.py` v2). Every other level
(and every level of another realm) carries that player record and enters at
the level's start locator. Boss levels are not ported; an exit to one ends the
session with a message.

Native and reference-compared components: controls, motion code/run timer,
request, facing, static/dynamic collision and position (movement.md,
collision.md); item load records (items.md); attribute formula (gameplay.md).
Everything else in gameplay.md is a transcription without a reference
comparison yet.

Presentation only (labelled on screen): unlit materials with alpha cutout,
rest-state follow camera, item models at tree rest pose (camera-facing "CF"
nodes omitted), enemy per-frame objects chosen by matching object stems to
sequences by frame count (the OANIM channel is not decoded), missiles drawn
with the SFXYEL WEAP_TW_Y rest pose, hit sparks and magic blasts as spheres,
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
