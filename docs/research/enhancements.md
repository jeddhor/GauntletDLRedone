# Enhancements and modern presentation (M19)

Everything here is NATIVE — not part of the original game — and changes
presentation only: the simulation keeps the original 30 Hz update and reads
none of these settings. `GDL_BASELINE=1` gives the original presentation
for fidelity comparisons (no blending, the frame's own shape, no MSAA, no
flash softening, no texture replacements).

## Settings

`settings.json` beside the saves (gdl_client::settings; missing fields take
their defaults, values are clamped, writes go to a temporary file renamed
into place). `GDL_SETTINGS=FILE` reads another file and, like
`GDL_BASELINE`, keeps the player's file from being written.

| Field | Default | Baseline | Effect |
| --- | --- | --- | --- |
| interpolate | true | false | 3D transforms blended between the last two updates at the display rate |
| vsync | true | (kept) | wait for the display's refresh |
| fps_cap | 0 | (kept) | frames per second at most (30..1000; 0 none) |
| frame_queue | 2 | (kept) | frames queued for presentation (1..3; wgpu's default 2): 3 may absorb a late frame at about one frame more latency, no benefit measured (performance.md) |
| widescreen | false | false | Hor+ 3D view in windows wider than 640:448 |
| msaa | 4 | 1 | samples (1, 2, 4) |
| fullscreen | false | (kept) | borderless fullscreen |
| reduce_flashes | false | false | brightening ambient specials at 30 % |
| texture_pack | none | none | replacement PNG directory (read at startup) |
| show_fps | false | (kept) | frame-rate counter |
| keys | {} | (kept) | player 1's key bindings (action → key names) |

## Settings overlay and keys

F10, or the options lists' NATIVE last entry "Display" (title Options,
the Tower's and a realm's Settings; added to the title_options,
tower_options and game_options records at load, drawn like their
entries; since 2026-10-28, when the playtest found no graphics options in
the menus), opens the settings overlay (gdl_client::enhance, drawn by gdl_play on
its own last camera in window pixels, outside the GS display decode): every
field above but the texture pack (frames queued as "lowest latency",
"default", "more buffering"), "Original presentation" (the baseline's
presentation, window and keys kept) and a key-binding page. The game waits
while it is open (simulate's waiting branch: audio polls, no update), and
keys still held when it closes are ignored by the game until released (so
the Enter that closed it does not press Start). Controllers work it too:
the d-pad chooses and changes, South (A/cross) changes, East or North
(B/circle, Y/triangle) goes back or closes; while it is open the menus
and the game ignore the controllers' buttons and stick (the pads stay
connected), and the presses it took are cleared for the frame. Closing
from "Display" leaves the options list where it was. F11 toggles fullscreen and
F3 the frame-rate counter anywhere. Changes are stored at once. The F-keys
F1, F2 and F9 keep their DEBUG uses.

Front end (NATIVE): Back on "Press Start" (Escape, Backspace, a
controller's North/triangle) quits the client — the PS2 game has no exit;
Back on the title menu returns to "Press Start", so two Backs from the
title quit. A controller's START also opens the title menu there
(do_titlescreen tests only the Start edge: new_start(−1) → and_edges
0x40000; the native front end also takes cross/Enter).

## Picture shape and widescreen

The PS2 frame (640×448) is placed in the window as the largest centred
rectangle of its shape (`frame_rect`); outside it the final pass draws
black. The 3D viewport is that rectangle's rows above the HUD inset, the
HUD's 512×384 blit screen is mapped onto it (the HUD camera's orthographic
extent widened by window/rectangle), and DisplayToLinear's Screen Adjust
offset and 16-bit dither grid are in frame units. A window of exactly the
frame's shape is all frame: at a 1280×896 client area the new layout is
pixel-identical to the previous build in G1, J5, C2 and H4 (frozen frames,
RX 580). Before 2026-10-28 the frame was stretched to any window shape.

With `widescreen`, a window wider than the frame shows the 3D view across
its full width at the frame's height: the vertical angle is the game's,
the horizontal tangent becomes tan_v × viewport aspect (Hor+;
Ps2Projection.wide), the frustum used for culling follows. The HUD stays
centred at the original shape; movies (the full-screen movie quad) show
bordered. Full-width blits of the flat TextureWhite (AAAWHITE: the trigger
camera's letterbox bars, fills) are stretched to the window's width;
textured blits stay in the frame. The end-of-stage fade and lightning act
on the scene's light, so they reach the widened sides.

Limits: the simulation's own visibility tests (MBWorldSphereVisible for
enemies, ItemVisible, PointVisible keeping players on screen) use the
original camera, so the widened margins are off screen to the game — an
enemy there may wait and an item may not animate until it enters the
original view; players are still kept within the original view. Checked:
per-update simulation hashes over 600 updates of a G1 walk are equal with
widescreen fullscreen at 1920×1080, blending, MSAA 4 and no vsync against
the baseline at 1280×896.

## Frame rate and blending

The simulation steps at 30 Hz (Bevy's fixed clock); rendering runs at the
display's rate. `interpolate` blends each 3D transform between its last
two updates by the fixed clock's overstep (interpolate_transforms;
movements longer than 6 units in one update are cuts). Render-rate
independence of the simulation: equal per-update hashes at several render
rates (research/loop-input.md; again on 2026-10-28 at 60 and 30 fps).
Frozen-frame comparisons (GDL_FREEZE_AT) are independent of frame timing
since 2026-10-28 (rendering.md).

## Anti-aliasing

Bevy gives every camera 4× MSAA by default; every native render before
2026-10-28 used it. It is now the `msaa` setting (all window cameras),
default 4; the baseline renders without (the GS has none): in G1's frozen
frame 0.8 % of the pixels — edges — differ between 1× and 4×.

## Key bindings

gdl_client::controls: fourteen port-0 actions (the pad entries measured in
research/player.md), each with the client's original keys as defaults;
`keys` stores only actions that differ. Binding a key takes it off any
other action; an action left without keys gets its free defaults back. The
menus accept, besides their fixed keys (arrows, Enter, Escape, Backspace,
C), the bound keys of the directions, cross, triangle and circle. Player 2's
keys and gamepad buttons are fixed. GUI check (private/runs/client-m19-
bind1): with cross bound to N, holding N attacks and Space no longer does.

## Texture replacements

gdl_client::texture_pack: a texture's name is the first 16 hex digits of
SHA-256 over its width, height (u16 LE) and decoded level-0 RGBA with GS
alpha (0x80 opaque). `GDL_TEXTURE_DUMP=DIR` writes each decoded texture
once as DIR/<name>.png with the alpha doubled (G1 session: 2062 textures,
11 MB); a pack's PACK/<name>.png (any size, 8-bit RGB/RGBA/grey) replaces
it with the alpha halved back, one mip level. Lossless for alpha up to
0x80; the few textures with alpha above 0x80 (additive glows) clamp there.
World, model, item, effect, particle and HUD textures are covered;
lightmaps are not. Checked with a pack greying all 2062 textures (G1
frozen frame: textured surfaces grey, vertex-coloured panels and lightmap
tints kept).

## Accessibility

`reduce_flashes` shows a brightening SetAmbientSpecial (lightning, the
flashes of SetAmbientSpecial callers) at 30 % of its value; darkening
specials (S5's dark level) are unchanged. Key rebinding (above). Not
provided: HUD or text scaling (the original HUD's fixed 512×384 layout),
subtitles beyond the original captions.

## Diagnostics

F3's counter (mean frame rate and time over 60 frames, the worst of them);
GDL_FRAME_LOG (per frame: interval, entities, update, player action,
materials, visible meshes and materials, camera, Time delta, player body);
GDL_SCENE_DUMP; tools/python/perf_bench.py; the `profile` feature (Chrome
trace). See performance.md.

## Compatibility matrix (2026-10-28)

| Check | Result | Evidence |
| --- | --- | --- |
| Reference-trace replays (146) | all equal to r85 | tools/regress.sh r86 |
| Simulation with widescreen, fullscreen 1920×1080, blending, MSAA 4, no vsync | equal to the baseline, 600 updates | private/runs/m19-m20-evidence-v1/sim-hashes |
| Simulation at 60 vs 30 render fps | equal | rendering.md (frozen frames), loop-input.md |
| Original picture at 10:7 | pixel-identical to the previous build | G1 J5 C2 H4 frozen frames |
| Bordered and widescreen layouts at 1920×1080 | frame shape kept; Hor+ with centred HUD | private/runs/m19-m20-evidence-v1/layout |
| Window resize 1600×900, 800×900 | pillar- and letterboxed, frames continue | platform-checks-m20-v2 |
| Settings overlay, F3, binding | works; game held while open | client-m19-ovl1 |
| Texture pack | replacements drawn | G1 grey pack |
| Menu flows (GUI, 16) | all pass on the final client | menu-flows-m20-final3 |

Not provided: a render scale (the 3D renders at the window's own
resolution — 1920×1080, 2560×1440 or 4K are native high resolutions),
gamepad button remapping.
