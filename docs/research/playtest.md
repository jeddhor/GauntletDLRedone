# Interim native playtest (not the M12 slice)

`python3 tools/python/run_play.py` launches `gdl_play` on the current desktop
(`--virtual` uses the authenticated Xvfb session). It writes
`private/reports/play-config.json` with every package resolved by verified
archive identity, then runs the native game simulation (research/gameplay.md)
for the Yellow Sorceress in The Poison Fields, levels G1–G4.

Controls: arrows/WASD move, Space/K throw, E/L use a potion (magic), stand
still in the exit portal to leave the level, Enter continues after death
(interim: restores the start-state health), F9 debug-skips to the next level,
Esc quits. The HUD shows health, gold, keys, potions, level, experience,
kills, active enemies, standing generators and the first log note or
unported path.

Start: G1 uses the state exported from the preserved save
(`export_game_state.py` v6 + `export_start_state.py` v2). Later levels carry the
player record and enter at the level's start locator. The G5 boss level is not
ported; leaving G4 ends the session with a message.

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
no audio.

Smoke tests on Xvfb (llvmpipe, 2026-10-16): G1 launch with enemies swarming;
throws (7 missiles, 6 kills); key pickup; F9 transitions to G2, G3 and G4 with
geometry, generators and items rendered.
