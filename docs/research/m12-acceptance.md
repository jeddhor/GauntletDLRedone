# M12 acceptance report — playable vertical slice

Revision SLUS-20047 (profile ps2-slus-20047-fb47d40667df). Labelled a
slice: the Tower hub and the Poison Fields' first level (G1) on actual disc
data, with the rest of the campaign loadable but outside this report.

Status: delivered for acceptance. Acceptance is the user's (charter): the
user's 2026-10-18 verdict on the interim build was "not playable" (missing
crystals and NPC, wrong lighting); the items in that verdict were fixed and
compared with the reference since (below). This report lists what to check.

## Build

- `python3 tools/python/run_play.py` (desktop) or `--virtual` (private
  Xvfb); `--import-card CARD.ps2` imports original card saves first.
- Client: `target/release/gdl_play` (Bevy), simulation `gdl_core::game`,
  configuration private/reports/play-config.json written by run_play.

## The slice, end to end

| Step | Evidence | Kind |
|---|---|---|
| Start: title, select screen, new character | playtest.md "M12 walkthrough" step 1 (client-m12a frames) | GUI run, actual disc |
| Tower: welcome scroll, fifteen crystals, UnlockLevel, Sumner and his speeches, NeedGargItems, the realm door, map, movie | walkthrough step 2; Tower whole-update replays (frame-tower r44: 170/170 except one sound-handle word) | GUI run + reference replay |
| G1 play: movement, combat, pickups, keys, generators, potions/magic, powerups | G1 replays r44 (fight, combat, varied, g1new, potion, xbow: every part equal except sound-handle words) | Reference replay |
| G1 exit, tally, shop, select | walkthrough step 3; frame-G1exit 186/186 in every part; scenario exit-g1 | GUI run + reference replay |
| Save, restart, recover | walkthrough steps 4–5; scenario save-reload (completion bytes, experience, gold, keys equal after a reload in a new client) | GUI run (scenario harness) |
| Failure: death | walkthrough step 6; scenario death-g1 (reward not kept, level-start state restored) | GUI run (scenario harness) |
| Lighting, materials | rendering.md (Tower vs reference at the same camera: render-tower-compare-v2; GS dumps); one open difference: the per-draw integer truncation of lighting | Reference comparison |

Scenario teleports and health writes are DEBUG shortcuts (labelled in
scenarios-m13-v2/v3); the exit, tally, save and load code they lead into is
the game's own. The user's own play is the acceptance test.

## User feedback (2026-10-18) and what changed

- "The crystals ... are missing": Tower items load from TOWER's item list,
  crystals hide/fade and are delivered by UnlockLevel (hud.md; the Tower
  replays include the crystal records).
- "So is the NPC": Sumner (GWIZ tree, SumnerAnimate, speeches with captions,
  hint pads) — hud.md "Sumner".
- "Lighting effects are all wrong or missing": DoLighting/ambient,
  world-light colours, ProcessItemWobjs fades (an inverted fade lit the
  Tower podium), point lights, the VU1 lighting formula, PSMCT16S/dither,
  lightmaps, texmods, mip LODs (rendering.md, textures.md).

## Risk estimate (updated 2026-10-23)

Animation, collision, content loading and behaviour capture scaled: 23
levels (the Tower, the first level of every realm but K, A1/G1 scenarios
and all eleven bosses) replay update by update against reference traces
with every part equal or a recorded first divergence; all 61 levels run the
scripted sweep without an unported path (content-coverage-v2). Remaining
risks: per-update ulp divergences in a few places (unknowns.md), the
presentation residue (lighting truncation, audio mixing), traversal of every
level by legitimate play (M14), the full critical path to the ending (M15)
and multi-player coverage (M16).

## What the user should check

1. `python3 tools/python/run_play.py`, create a character, collect the
   Tower's crystals, talk to Sumner's pad, enter the Poison Fields.
2. Play G1 to its exit, go through the tally and shop, save in the select
   column, quit.
3. Start again, load the character: the Tower should show the completed
   level and the delivered crystals.
4. Die in a level: back to the Tower with the level-start state.

## Acceptance (2026-10-24, night)

The user: "the M12 acceptance looks good now -- lighting issues are gone,
crystals and NPC are there, everything that I complained about at first
seems resolved." Two issues reported with it:

1. A green potion could not be picked up. Potions (item kind 1 subtype 4)
   are refused at MaxPotionsPerPlayer, 9 in every captured game state
   (private/reference/start-states/poison-fields-game-v3..v6); the
   pickup then raises help 3 and leaves the item (game/pickups.rs, ported
   from the instructions). Most likely the character held nine. Not yet
   checked in the reference with a full count.
2. The Sorceress's square attack (DMotion 0x10, action 99: effect trees
   0x77/0x78) slowed the frame rate. Reproduced on the private Xvfb with
   the client's DIAGNOSTIC frame log (GDL_FRAME_LOG; scenarios
   perf-square-g1, perf-cross-g1, perf-idle-g1, run perf1): mean frame
   time 262 ms against 156 (cross throws) and 135 (standing), the
   simulation itself as fast in all three (headless 2000 updates ≈ 2 s).
   Cause found in the client: Assets::get_mut marks a material modified
   whether or not it changes, and apply_material_requests and
   present_distance_fade took every overridden material mutably every
   frame, so all of them were re-prepared each frame; point lights
   (present_point_lights) re-prepare every material when an effect light
   moves. Fixed (2026-10-24 night): materials are taken mutably only for
   a change, and the eight point lights live in one storage buffer every
   GsMaterial binds (binding 7, lib.rs POINT_LIGHTS), written once per
   change. Frame times (run perf4, same scenarios): square 180 ms mean
   (p50 135) against 262 before, cross 148, standing 126; with the point
   lights switched off (GDL_NO_POINT_LIGHTS, run perf3) square was 172.
   The lights still apply: frozen frames at update 175 with the attack's
   light alive (scenario freeze-square-g1, GDL_FREEZE_AT=175) repeat
   exactly and differ from the same frame without point lights in
   226,918 pixels (the green glow on the ground around the Sorceress).
   These are software-rendered timings on the private Xvfb under a busy
   machine (llvmpipe); the share each cause takes on a GPU is not
   measured.

