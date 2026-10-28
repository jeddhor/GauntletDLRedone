# Performance and platform QA (M20)

## Machine and method

AMD Radeon RX 580 (RADV POLARIS10, Mesa 26.0.x), AMD Ryzen Threadripper
1950X, Linux 7.0.0-38-generic, Bevy 0.18.1 on Vulkan. Measurements run the
release client on the real GPU in an invisible `kwin_wayland --virtual
--xwayland` output (the private Xvfb cannot present a hardware swapchain);
scenes start the DEBUG test character in a level (GDL_TEST_AUTOSTART,
GDL_DEBUG_LEVEL) with scripted input, and the client's frame log records
every frame (tools/python/perf_bench.py; tools/python/soak_test.py for long
co-op sessions; the `profile` feature for Chrome traces of per-system time).
Frame times below are the main loop's frame-to-frame intervals; with vsync
they include the wait for the display.

## Targets

On the machine above, with representative worst-case content (J5's boss
arena, F1's generators, H4's Garm, four players in co-op, widescreen
1920×1080 with 4× MSAA):

1. 60 Hz with vsync: no stutter — p99 frame interval ≤ 25 ms and no frame
   over 33.4 ms after the level start.
2. Headroom: uncapped mean frame time ≤ 12 ms and p99 ≤ 16.7 ms at
   1280×896 (the frame's shape), and p99 ≤ 16.7 ms at 1920×1080 widescreen
   with 4× MSAA.
3. Level load (process start to the first update) ≤ 2 s; peak resident
   memory ≤ 1 GB.
4. A 30-minute four-player session: no stall, no growth of resident memory
   beyond 10 % after the first five minutes, the frame-time statistics of
   the last window within those of the first.
5. Motion: with blending, the shown camera's per-frame step within 10 % of
   its local mean (median) while walking.

## Hotspots found and fixed

| Date | Cause | Effect |
| --- | --- | --- |
| 2026-10-24 | `Assets::get_mut` re-prepared unchanged materials; point lights in every material uniform | G1 square attack 262 → 180 ms on the Xvfb (rendering.md "Client performance") |
| 2026-10-27 | present_scene_light took every material mutably when the count changed (70 ms re-prepare twice a second); texmods re-uploaded unchanged frames; apply_material_requests cloned every mesh's material each frame | the periodic hitches the playtest reported gone (commit 635002c) |
| 2026-10-27 | the 3D scene moved only at 30 Hz | presentation blending at the display rate (enhancements.md) |
| 2026-10-27 | one bind group per material: no batching (submit 4.8 ms in J5) | bindless GsMaterial: G1 uncapped 10.2 → 8.5 ms mean, J5 16.8 → 11.6 ms (submit 1.1 ms) |
| 2026-10-28 | present_critters recomputed each part's parent chain every frame (3.4 ms in J5) | posed once per update, per node: J5 uncapped 11.6 → 8.6 ms mean, p95 15.7 → 13.1 ms |
| 2026-10-28 | harness clocks (frozen captures) dropped steps; texture search in HashMap order | frozen frames independent of frame timing (rendering.md) |

## Results

### Scenes (perf_bench.py, 2026-10-28)

Scenes as perf_bench.py defines them: the DEBUG test character at the
level's start (the Tower with its welcome scrolls closed), 900 updates, the
first 60 frames excluded; G1 walks up from update 150. kwin output
1280×896 (client area 1280×868, the frame bordered) unless stated.
bench-uncapped.json is {"vsync": false}; bench-wide1080.json adds
fullscreen, widescreen and MSAA 4. Binary 853c3d2d… queued three frames
by default (a4d978dc… two, wgpu's default); the release keeps two
(Long sessions below), so a4d978dc's tables describe it, and 853c3d2d's
vsync table shows the three-frame variant.

Default settings: vsync, blending, MSAA 4, three frames queued (perf-bench-m20-final3-vsync; binary 853c3d2df7177493, window 1280×896, env defaults):

| Scene | Frames | Mean | p95 | p99 | Max | >33.4 ms | Load to first update | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tower | 2055 | 16.67 | 20.72 | 22.07 | 27.4 | 0 | 579.4 ms | 543 MB |
| g1-walk | 1711 | 16.67 | 20.93 | 22.11 | 25.07 | 0 | 574.0 ms | 531 MB |
| c2 | 1709 | 16.67 | 23.52 | 24.38 | 24.95 | 0 | 620.9 ms | 561 MB |
| f1 | 1711 | 16.67 | 20.47 | 21.21 | 32.4 | 0 | 595.7 ms | 530 MB |
| i1 | 1711 | 16.67 | 22.1 | 23.11 | 26.86 | 0 | 619.9 ms | 552 MB |
| j5 | 1710 | 16.67 | 21.69 | 22.87 | 25.26 | 0 | 700.9 ms | 579 MB |
| h4 | 1716 | 16.67 | 19.44 | 20.97 | 23.88 | 0 | 466.8 ms | 470 MB |

Uncapped (vsync off), otherwise defaults (perf-bench-m20-final2-uncapped; binary a4d978dc56fa3707, window 1280×896, settings bench-uncapped.json):

| Scene | Frames | Mean | p95 | p99 | Max | >33.4 ms | Load to first update | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tower | 3222 | 10.72 | 14.26 | 16.24 | 18.91 | 0 | 561.4 ms | 546 MB |
| g1-walk | 3309 | 8.74 | 11.34 | 12.57 | 15.29 | 0 | 563.3 ms | 543 MB |
| c2 | 2912 | 9.9 | 13.19 | 14.13 | 25.98 | 0 | 636.7 ms | 562 MB |
| f1 | 4407 | 6.58 | 9.42 | 10.54 | 15.58 | 0 | 570.2 ms | 539 MB |
| i1 | 2897 | 9.96 | 12.67 | 14.0 | 17.51 | 0 | 605.0 ms | 557 MB |
| j5 | 3285 | 8.78 | 13.26 | 14.24 | 30.51 | 0 | 666.9 ms | 566 MB |
| h4 | 3125 | 9.26 | 12.53 | 13.62 | 18.21 | 0 | 472.6 ms | 469 MB |

Widescreen fullscreen 1920×1080, MSAA 4, uncapped (perf-bench-m20-final2-wide1080; binary a4d978dc56fa3707, window 1920×1080, settings bench-wide1080.json):

| Scene | Frames | Mean | p95 | p99 | Max | >33.4 ms | Load to first update | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| j5 | 3341 | 8.61 | 13.25 | 14.39 | 18.03 | 0 | 702.6 ms | 579 MB |
| f1 | 4384 | 6.62 | 9.58 | 10.56 | 18.34 | 0 | 575.0 ms | 538 MB |
| h4 | 3016 | 9.58 | 12.76 | 13.92 | 16.89 | 0 | 470.0 ms | 471 MB |
| c2 | 2780 | 10.36 | 14.1 | 16.92 | 23.59 | 0 | 624.3 ms | 564 MB |
| g1-walk | 3258 | 8.87 | 11.6 | 12.74 | 17.17 | 0 | 584.9 ms | 542 MB |

### Long four-player sessions (soak_test.py)

G1, four players on virtual pads with seeded random input (seed 20),
default settings, 1280×896 client area, 30 minutes each:

| Run | Binary | Frame log | Memory (10 s → 5 min → 30 min) | Updates per 5 min | p99 per window | Frames over 33.4 ms | Present intervals over 33.4 ms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| soak-m20-coop4-30min | 238683ae (before the audio fix) | project disk | 478.7 → 494.5 → 499.9 MB | 9000 | 22.9–23.9 ms | 9 of 107,867 (max 42.3) | 1 (max 40.4) |
| soak-m20-coop4-30min-v2 | a4d978dc (two frames queued) | tmpfs | 562.5 → 580.3 → 583.6 MB | 9000 | 23.2–23.5 ms | 14 of 107,827 (max 51.3) | 2 of 107,827 (max 44.7; p99.9 21.7 ms) |
| soak-m20-coop4-30min-v3 | 853c3d2d (three frames queued) | tmpfs | 560.1 → 577.3 → 580.7 MB | 9000 | 22.9–23.5 ms | 17 of 107,892 (max 41.5) | 8 of 107,892 (max 39.9; p99.9 21.2 ms) |

No stall in any run (exactly 9000 updates per window), entities steady
(9070–9160), memory +0.6 % after the first five minutes (the audio banks
add about 85 MB from the start). "Frames" are the main loop's end-of-frame
intervals; "present intervals" are Time's delta, which with pipelined
rendering follows the render thread's presents. Diagnosis (GDL_SLOW_STEP,
Game::step_phases; 10-minute runs soak-m20-phasediag and soak-m20-latency3):
about once in two minutes a simulation update takes 8–13 ms instead of the
usual few, spread over ProcessItems (≈2.6 ms), do_enemies (3.6–6.7 ms) and
do_players (1–2 ms) rather than one phase; and with two frames queued some
long intervals came from the present path (the virtual kwin output's
vblank). Three frames queued gave a clean 10-minute run (max present
interval 25.9 ms) but 8 long present intervals in 30 minutes against 2 with
two queued, so the release keeps two (the setting stays). None occur
uncapped. The help box (NewHelp, 0.34 ms) and the HUD were
measured and are not the cause.

### Targets

| Target | Result |
| --- | --- |
| 1. Vsync: p99 ≤ 25 ms, no frame over 33.4 ms | Scenes: p99 ≤ 24.4 ms in all; frames over 33.4 ms: none with three queued; with two (the release) one in J5 per run, at a different update each time (437, 461) with a long present interval and nothing spawned. Not met in the 30-minute four-player session: 2 of 107,827 present intervals over 33.4 ms (max 44.7 ms) with two queued, 8 with three (open: the 8–13 ms simulation updates, the virtual output's present path) |
| 2. Uncapped mean ≤ 12 ms, p99 ≤ 16.7 ms at 1280×896; p99 ≤ 16.7 ms at 1920×1080 widescreen MSAA 4 | Met at 1280×896 (mean ≤ 10.7 ms, p99 ≤ 16.2 ms). At 1920×1080: met in J5, F1, H4, G1 (p99 ≤ 14.4 ms); C2 p99 16.92 ms (0.2 ms over) |
| 3. Load ≤ 2 s; peak memory ≤ 1 GB | Met: 0.45–0.70 s to the first update; peak 470–593 MB |
| 4. 30-minute four-player session stable | Met: no stall, memory +0.6 % after five minutes, last window's statistics within the first's |
| 5. Motion within 10 % (median) | Met: camera 4.1 %, body 4.5 % (Frame pacing) |

## Frame pacing

With vsync the main loop's end-of-frame intervals alternate about 14 and
19 ms around 16.67 ms (an update frame does more work than the frame
between updates), while Time's delta — what the blending and the fixed
clock read — stays within 15–18 ms. Walking up in G1 with blending, the
shown camera moves within 4.1 % (median) of its local per-frame step and
the player's body within 4.5 % (private/runs/m20-pace; the alternation of
the body step follows Time's). The Mesa overlay layer (present timing)
could not write its output file in this setup ("Bad address" for any
path), so presentation times themselves are not measured.

## Platform matrix (2026-10-28)

| Check | Linux (this machine) | Windows |
| --- | --- | --- |
| Build | release, Clippy clean | x86_64-pc-windows-gnu cross-build (toolchain.md) |
| Runs, renders on the RX 580 | yes | under Wine 10.0 (winevulkan/RADV) |
| Simulation identical | reference | per-update hashes equal over 600 updates (Wine) |
| Keyboard | GUI flows, overlay, bindings | not tested |
| Gamepad use, unplug, replug | virtual uinput pad: pass (platform-checks-m20-v2) | not tested |
| Focus loss with a key held | player stops, frames continue (Xvfb) | not tested |
| Resize, pillar/letterbox | pass | not tested |
| Fullscreen on/off | pass (settings path F11 uses) | not tested |
| Full disk | saves and settings keep the previous file (unit test, platform check) | not tested |
| Audio | ALSA | cpal/WASAPI built, not tested |
| Long co-op session | see Results | not tested |
| Title Start → select screen at the real GPU's rate | 8/8 (title-start-m20-after-audio; 3/8 before the fix) | not tested |

The last row was added after the user's 2026-10-28 playtest: on the RX
580, Start on the title often went straight to the Tower with GAME OVER
(playtest.md). The software-rendered Xvfb steps at least one update in
every frame, so the menu flows there could not show it;
tools/python/title_start_check.py runs it in the kwin virtual session on
the real GPU with a virtual pad and audio into ALSA's null device.

Not tested anywhere: a physical controller unplugged mid-game (the
virtual pad covers the client path; M16's ControllerMessageBox was
checked with the TEST disconnect action), audio device changes, a real
Windows installation.

## Release identity

Release candidates are built with `cargo build --release --locked`
(Linux) and the cross-build command in toolchain.md (Windows). 2026-10-28
(source 730d114): target/release/gdl_play SHA-256 9c0b9b0951b3066e…
(identical from a second target directory), gdl_tools f9614eed8bb7ec74…,
gdl_play.exe 05c34013d3dfa56d…; the Windows build under Wine and the Linux
release give identical per-update simulation hashes over a 600-update G1
walk (private/runs/m19-m20-evidence-v1/final). GUI menu flows 16/16 on the
final client (menu-flows-m20-final3). The
binaries embed the source path in panic locations; the SHA-256 of each
tested binary is in its report (perf-bench-*, platform-checks-*, soak-*,
private/runs/m19-m20-evidence-v1/binaries.sha256). Running needs only the
user's disc extraction and the configuration run_play.py writes; no DEBUG
switch or developer path is required (the TEST and DEBUG environment
switches are off unless set).

2026-10-28, after the playtest fix (the select screen's game-over race):
target/release/gdl_play SHA-256 89bf7be5a0e3244b… (the same from two
builds); rebuilt from 730d114's source the previous client reproduced
9c0b9b0951b3066e… exactly. GUI menu flows 16/16 on it
(menu-flows-m20-selectfix1). Only gdl_play changed; gdl_tools and the
Windows build were not rebuilt.
Then (the menu fixes: in-game Screen Adjust, the Display entry,
controllers in the overlay, Back on Press Start): gdl_play
dd2cc2f9e4294391…; menu flows 21/21 (menu-flows-m20-menus2),
title_start_check 6/6 (title-start-m20-menus2).
Then (widescreen 2D screens bordered; chests' NULL1 content node, core
and replay convention): gdl_play c91768cd1e84076a…, gdl_tools
b66b73cb42e60b99…; regression r87 equal to r86 for all 146 replays, menu
flows 21/21 (menu-flows-m20-chest1), title_start_check 4/4
(title-start-m20-chest1), scenarios chest-g1 and death-g1 (scenarios-chest1).
