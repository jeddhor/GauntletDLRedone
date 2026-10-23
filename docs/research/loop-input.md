# Loop, input and timing contract (M09)

Revision SLUS-20047. This document is the M09 deliverable: the loop map, the
input specification, the reference timing study and the replay schema. The
underlying evidence is in clock.md, player.md ("Pad buttons"),
gameplay.md ("Frame order", "Whole-update replay"), camera.md and
decisions/0002-random-source.md.

## Fields, frames and updates

- Field: one NTSC vblank (vb_counter 0x2a1c78 increments by one). The
  reference is stepped one field at a time (PCSX2 FrameAdvance, F10 in the
  private configuration; every step is verified by vb_counter).
- Update: one pass of main's loop. ClockOncePerFrame 0x102e68 measures the
  fields since the previous pass (vb_elapsed 0x20f91c), sets SPF 0x20f908 =
  vb_elapsed / 60 and advances AFrame 0x20f918 and Time 0x20f904; special
  paths give 0 (pause, loading) or the two-field fallback.
- Frame: the image the update draws; the GS dump of a frame holds one
  update's draws.

## Timing study (actual reference)

- Gameplay: every update of the 73 recorded traces (32,441 samples) has
  vb_elapsed 2 and SPF 1/30 except loading and pause: one long level-end
  frame (SPF 0.42 in trace-B6-goodwiz-v4, 0.62 in trace-G1-exit-v1, the next
  level loading inside game_main) followed by two one-field updates (SPF
  1/60), and SPF 0 while a modal scroll box pauses the Tower
  (trace-tower-v1, trace-tower-crystals-v1).
- Other modes (reference_warp.py --sample-clock, cadence-tally-tower-v1:
  the tally, shop, level and inventory panels, the select column, Done and
  the Tower start): game mode 0x4012 (tally/shop) and 0x4010 run one update
  per two fields with SPF 1/30; 0x400b (select) runs at the same cadence;
  0x400c (loading behind the select screen) holds AFrame with SPF 0.
- Native tick: gdl_play steps the simulation in Bevy's FixedUpdate at
  30 Hz with SPF 1/30 and vb_elapsed 2 (the measured cadence); rendering
  at any display rate redraws the latest simulated state. The original's long loading frames have
  no native counterpart (loading is not simulated time); the whole-update
  replays take SPF from each recorded update, so they include those frames.

## Loop map

main 0x1001e0, per pass: ClockOncePerFrame; srand(pbGetCPUTime() >> 3);
LoadVU1GameLogic (empty: jr ra); PlayerControls 0x105718 (ControlsUpdate →
ReadControls → the PlayerControl records); options; AudioQueUpdate;
game_main (by game mode); camera; the frame end. game_main's play path
(0x4010) runs world_update (DoWorldAnimation, SetupDynGrid, the good
wizard, spew items, quest items), ProcessItems, do_enemies, do_players
(PlayerMotion → DoPlayerAction → AnimateTree), the critters, ProcessEffects
and the auto exit; the level end and mode changes follow (exitflow.rs).
The native Game::step_with runs the same order; the whole-update replays
verify it per update (every part equal across 30+ traces; gameplay.md).

## Input specification

- Physical buttons → joyGetStatus entries (measured, player.md "Pad
  buttons"): d-pad 0..3, left stick 4..7, right stick 0xe..0x11, L1 8, L2
  9, square 0xa, circle 0xb, triangle 0xc, cross 0xd, R1 0x12, R2 0x13,
  start 0x14 (code), select 0x15; entries are (flags, value) with bit 0
  present and bit 1 analog.
- ReadControls (pad.rs) builds each port's level word: directions (stick
  over d-pad; mode 2 drives with the right stick), start 0x40000, select
  0x80000, L1 0x100000, L2 0x200000, R1 0x400000, R2 0x800000, circle
  0x1000000, cross 0x2000000, square 0x4000000, triangle 0x8000000, the
  d-pad copies 0x10000000..0x80000000; analog angle/magnitude through
  calc_analog_stick (atan2f, including its DOMAIN path at rest).
- PlayerControls applies the per-player remap (InitPlayerControls 0, 2, 1,
  1), edges (pressed this update and not the previous one), repeat edges
  (rep_speed), the lockout count (Enable/DisablePlayerControls) and the
  specials (CheckSpecials). Edges are per update, so a press shorter than
  two fields can be missed exactly as on the PS2.
- Input boundaries in controlled experiments: reference_trace.py and
  reference_warp.py hold keys over whole fields; each recorded update keeps
  the resulting PlayerControl words, which the replays feed back unchanged.
  The native client samples its keyboard/gamepads once per fixed update
  into joyGetStatus entries (gdl_play player_input), then runs the same
  ReadControls/PlayerControls code.

## Random numbers

newlib rand (0x1fb780) through pbRand/RandInt; the original reseeds from
CPU time every pass, so captured outcomes depend on host timing. Native
play seeds the same generator once per session (setup: the capture's
vb_counter) and continues it (decisions/0002). The replays solve each
update's seed from the recorded rand_next chain, so every consumer's draw
order is checked; RandItemIdx (RandomItemNum) is a separate counter
compared exactly.

## Replay schema

A trace (reference_trace.py, schema_version in each file) holds the launch
identities (disc, PINE slot, ini and state hashes), the fixture hash and
reload barrier, the input schedule ([{steps, keys}] over fields) and one
sample per field: step, observation (scalars, strings, players,
PlayerControl), player (player 0's record, control, window, StdCam,
options, Time, SPF, ATime) and enemies (the enemy array, rand_next,
BossIndex, SuicideBomberIndex, CurWorld, level data, the Items array,
WorldBase, plus effects, world animation and critters in later schemas).
game_run.py --frame-replay seeds each update from the sample before it and
compares the sample after it part by part (report: updates, *_updates_equal,
per-update diffs and the first divergence). WARPED captures (guest writes)
are named and labelled in the trace's source.

## Render-rate independence (2026-10-23, gdl_play)

tools/python/fps_independence.py runs gdl_play with the TEST harness:
GDL_TEST_AUTOSTART (a new default character, no menu time), G1, a scripted
port 0 (fps-independence-script-g1-v1: walking in four directions, turning,
throws, square and triangle presses) for 480 updates, and a render cap; the
client hashes its simulation records (players, enemies, items, effects,
rand state, Time) after every update. Reached render rates 4.84, 9.13 and
11.06 frames per second (caps 5, 12, none; about 6.2, 3.3 and 2.7 fixed
updates per rendered frame): all 480 hashes identical
(fps-independence-g1-v4). A run with caps 30, 60 and 144 (g1-v3) also
matched, but those caps did not bind on the Xvfb/llvmpipe renderer, whose
uncapped rate is about 11; rates of 60 and above need a hardware GPU and
were not measured. A first attempt (g1-v1/v2) entered the level through the
menus, whose wall-clock time changes Time and rand before the level, so its
hashes differed from the first update — the reason for the autostart.
