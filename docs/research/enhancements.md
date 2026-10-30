# Enhancements and modern presentation (M19)

Everything here is NATIVE — not part of the original game — and changes
presentation only: the simulation keeps the original 30 Hz update and reads
none of these settings. `GDL_BASELINE=1` gives the original presentation
for fidelity comparisons (no blending, the frame's own shape, no MSAA, no
flash softening, no texture replacements, the PlayStation button prompts).

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
| button_prompts | "auto" | "playstation" (GDL_BASELINE; the overlay's "Original presentation" keeps it) | the face-button prompts' style: "auto", "playstation", "xbox" (Button prompts, below) |
| keys | {} | (kept) | player 1's key bindings (action → key names) |
| texture_filter | "original" | "original" | "sharp": base textures at the GPU's level of detail, 16× anisotropic, over full mip chains (Graphics, below) |
| sharpen | 0 | 0 | contrast-adaptive sharpening of the 3D view, percent (0, 20 … 100) |
| full_colour | false | false | no 16-bit frame, dither or integer truncation, at the original's mean level |
| smooth_lightmaps | false | false | lightmaps sampled with a cubic B-spline |

## Graphics (post-1.0, 2026-10-30)

User request 2026-10-29 (after 1.0.0): sharper, higher-resolution-looking
textures and better lighting; the approved plan's first step (Checkpoint
A) is real-time filtering and the colour pipeline, no new art. Every
setting is presentation only and off in the original presentation; the
overlay's "Graphics..." page (F10 → Graphics...) shows them with a preset
row: Original (all off), Enhanced (sharp, sharpening 40 %, full colour,
smooth lightmaps), or Custom. New installs keep Original until the
Enhanced defaults are approved.

- Sharp texture filtering (gdl_client::texfilter). The GS samples a
  record's level 0 bilinearly, or for records with mips the level
  log2(1/Q) + K (textures.md "Mip selection"), chosen for 448 lines.
  Sharp samples every base texture with the GPU's own level of detail at
  the window's resolution and 16× anisotropy, over the disc's levels
  continued to 1×1 by generated ones (2×2 box in linear light, colour
  weighted by alpha; cut-outs — texels either rejected by the GS test,
  AREF 1, or at least 0x80 — stay binary in every level, binarised at the
  threshold that keeps level 0's coverage, ties in 8×8 Bayer order). LOD
  bias −0.5 (NATIVE choice; GDL_SHARP_BIAS overrides, TEST): frozen
  frames at 1920×1080 with biases 0, −1 and −2 (runs frozen-e-sharp0/m1/m2)
  show C2's distant cliff sharper than the GS formula already at 0; near
  the player textures are magnified and every choice samples level 0 (only
  replacement textures sharpen those). Texture modifiers' frames keep
  their own levels and their targets only follow the sampler: frame steps
  re-upload the target, and with generated chains that cost about 3 ms on
  the frames that stepped (perf-bench gfx-p1-sharp against
  gfx-p1-sharp-nochain; anisotropy 1 or 4 made no difference,
  GDL_SHARP_ANISO, TEST). Switching back restores the decoded levels byte
  for byte (texfilter tests).
- Sharpening: Bevy's contrast-adaptive sharpening (CAS) on the 3D camera
  only — the HUD and 2D screens are drawn after it. With MSAA every window
  camera shares one multisampled texture and the next camera's resolve
  put the unsharpened picture back; Bevy 0.18.1 extracts
  Camera::msaa_writeback but this build has no node for it (see
  "Anti-aliasing"), so gdl_client::writeback draws the 3D camera's result
  back into the multisampled texture before the first 2D camera's pass
  (its msaa_writeback set to Always while sharpening is on). A first
  version with an extracted marker component and a per-frame prepare
  system changed the draw order of equal-depth cut-out faces: G1's
  original-presentation frame differed in 3,491 spear-edge pixels with
  every effect off (scene dumps identical; first observation kept in
  private/runs/frozen-base-p1h); the node now adds no per-frame system and
  the frame is identical again. The anti-aliasing plugin group is not
  built: its TAA orders itself after bevy_post_process's motion blur
  (absent); FXAA (inactive) and CAS are added singly, and gdl_scene and the
  texture laboratory disable the group too.
- Full colour: the frame is held in floating point without the PSMCT16S
  output, its dither and the integer truncations of the multiplies (GS
  output stage, rendering.md), at the original's mean level: DIMX holds
  each offset −4..3 twice, so over a 4×4 cell the stored value of an
  integer x averages max(x − 4, 0) (248 at most) and the continuous value
  goes 4.5 down (DisplayToLinear mean_level; the base write before the
  lightmap pass likewise), each integer product 0.5. Additive draws add
  their mean step (−4.5) but not below 0 (the GS's darkening by faint
  texels is left out). Mean RGB over the 3D area (frozen frames, 1920×1080):
  Tower 33.8/26.6/10.5 against the original's 33.9/26.7/10.5, G1
  20.1/22.8/9.3 against 20.3/23.0/9.6, C2 96.5/88.1/47.7 against
  96.4/88.1/47.8; without the shift it was 7–9 brighter (run frozen-e-full).
  The final pass converts the HUD too, so it loses the grain as well.
- Smooth lightmaps: the 4-bit lightmaps sampled as a cubic B-spline (four
  bilinear taps) instead of bilinearly; the lightmap texels are unchanged.
  Baked shading on walls and floors loses its texel steps (the Tower's
  walls and tree, frozen-e-lm2).
- Tried and dropped: alpha to coverage for cut-outs ("smooth cut-outs").
  The original's alpha test passes almost everything above 0 (AREF 1), so
  the cut-out edge lies in the dark bilinear fringe; partial coverage there
  stippled it into jagged dark outlines (G1's spearmen, frozen-e-a2c), worse
  than the original's edges.

Switching live (the overlay, or GDL_TEST_ACTIONS "settings" at update
150): texfilter::apply switches every registered texture, texture
modifiers copy their current frame in again (texfilter::generation), and
every material is prepared again two frames later — a material keeps the
texture views and samplers it was prepared with (its bindless slots;
Bevy 0.18.1 orders no material preparation after the images'), and
without that a live switch left speckled anisotropic/isotropic surfaces
(frozen-sd-sharp-live: 207,548 pixels against a fresh start). Against a
fresh start in the new mode a live switch now leaves 6,026 (G1) and 20,655
(C2) pixels, mean 0.03 and 0.12 of 255, in tied translucent layers (G1's
red additive glows, a glow over a C2 pillar) while scene dumps — every
drawn mesh's texels, levels, sampler and material parameters — are
identical: the re-prepared materials take new slots, which reorders ties;
the residual moved with the re-preparation schedule (immediately, +2, +1/+3/+6
frames; frozen-live3/4/5-*). Sharpening, full colour and smooth lightmaps
switch off pixel-identically (frozen-live-off-*).

Checks (RX 580, 2026-10-30; private/runs/gfx-ref holds the binaries:
gdl_play-d100e5f is the 1.0 sources built here, gdl_play-cpA this
checkpoint, SHA-256 c3bf9a7d…7b51b): tools/python/frozen_frames.py (frozen
frames of the Tower, G1, J5, C2 and H4 in a virtual kwin output, own HOME
and saves): the original presentation is pixel-identical to the 1.0 build
in all five (frozen-cpa-base against frozen-base-ref); 600 G1 updates with
every enhancement on at 1920×1080 have the same per-update simulation
hashes as the original presentation and the 1.0 build
(private/runs/gfx-simhash-cpA); uncapped frame times at 1920×1080
widescreen with MSAA 4 (perf_bench.py gfx-p1j-cur / gfx-p1j-enh; mean /
p99 ms): Tower 11.76 / 18.41 → 10.95 / 16.93, G1 walk 8.90 / 12.61 →
8.79 / 12.50, C2 9.99 / 14.82 → 10.29 / 17.17, J5 9.39 / 19.88 → 8.93 /
14.69 (within run-to-run spread); peak memory +8..17 MB (the generated
levels); title_start_check gfx-cpa1 6/6, and with Enhanced settings and
the Xbox pad through the attract sequence to the overlay
(gfx-cpa1-enh-overlay) 2/2, the overlay listing "Graphics...". Comparison
sheets: private/runs/gfx-checkpoint-a (current settings above, Enhanced
below; 2× crops near the player and in the distance). The 146
reference-trace replays (tools/regress.sh r89) equal r88 (no simulation
code changed). The Windows build (x86_64-pc-windows-gnu, gdl_play-cpA.exe)
under Wine on the RX 580 with Enhanced settings: the same 600 G1 update
hashes as Linux (private/runs/gfx-wine-cpA).

## Settings overlay and keys

F10, or the options lists' NATIVE last entry "Display" (title Options,
the Tower's and a realm's Settings; added to the title_options,
tower_options and game_options records at load, drawn like their
entries; since 2026-10-28, when the playtest found no graphics options in
the menus), opens the settings overlay (gdl_client::enhance, drawn by gdl_play on
its own last camera in window pixels, outside the GS display decode): every
field above but the texture pack (frames queued as "lowest latency",
"default", "more buffering"; the graphics fields on its "Graphics..."
page), "Original presentation" (the baseline's
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
centred at the original shape. Full-frame 2D screens show bordered with
black sides: movies (the full-screen movie quad), the tally and shop, the
realm map, the select screen, the full-screen inventory and the front end
(title, menus, attract screens) — the 3D view keeps rendering the level
behind them, which in the widened sides showed frozen beside the 4:3
screen until 2026-10-28 (playtest; checked on the RX 580 at 1920×1080:
G1's exit tally with black sides, play still widened). The level end's
TRANSITION_SCREEN (world_update's end-of-stage fade, two seconds before the
tally or the realm map) is drawn while play is still widened: the sides
are covered by black fills at the transition's alpha, so they darken as it
fades in and are black once it is opaque (playtest 2026-10-29: the Tower
showed beside it before the map; checked on the RX 580 at 1600×1000, the
Tower's G portal and G1's exit: side columns from the scene's brightness to
0 over the fade, the frame's centre unchanged, per-update hashes equal the
previous build's). Full-width blits of the flat TextureWhite (AAAWHITE: the trigger
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

The overlay's camera (order 3, after the HUD camera's DisplayToLinear)
clears the target's textures to transparent and is composited onto the
window with premultiplied blending. Until 2026-10-28 it loaded them: Bevy
0.18.1 shares a target's textures per (target, HDR, MSAA) and performs no
MSAA writeback (Camera::msaa_writeback is extracted, never consumed), so
with MSAA it loaded the multisampled texture as the HUD camera left it —
display values, before DisplayToLinear — and its resolve wrote those back
as linear: the whole picture washed out while the overlay (or the F3
counter) was up (playtest). A non-MSAA overlay camera is not possible: the
2D depth texture is shared per target and a differing sample count fails
wgpu validation. The same missing writeback dropped any post-processing
of the 3D camera under MSAA; gdl_client::writeback (2026-10-30, "Graphics")
performs it for the first 2D camera while sharpening is on. Checks: title-display at MSAA 4 and 1 (Xvfb: overlay
frames equal within the setting's own text and edges, mean 0.6/255;
closed-menu frames pixel-identical to the previous build); on the RX 580
under the player's settings (fullscreen 1920×1080 through Xwayland, widescreen,
MSAA 4 vs 1, title_start_check --overlay): mean difference 0.5/255; F3's
counter drawn, the rest unchanged.

## Key bindings

gdl_client::controls: fourteen port-0 actions (the pad entries measured in
research/player.md), each with the client's original keys as defaults;
`keys` stores only actions that differ. Binding a key takes it off any
other action; an action left without keys gets its free defaults back. The
menus accept, besides their fixed keys (arrows, Enter, Escape, Backspace,
C), the bound keys of the directions, cross, triangle and circle. Player 2's
keys and gamepad buttons are fixed. GUI check (private/runs/client-m19-
bind1): with cross bound to N, holding N attacks and Space no longer does.

## Button prompts

User request 2026-10-29: with an Xbox controller, triangle as "back" is
hard to remember. The original's prompts are separate MBOX textures, not
part of the screens: BUTTON_X, BUTTON_O, BUTTON_SQ, BUTTON_TRI (32×32
format 66: an opaque black disc of 30 texels, rim alpha-blended, the
symbol in antialiased strokes about 2.5 texels wide, 18 texels across;
blue cross, orange circle, pink square, green triangle) and the d-pad
arrows BUTTON_U/D/L/R. The game's text never names a button: strings
leave a gap ("Press     when done.", "Press    to select.") that a button
blit fills, and ENGLISH.ROM's control texts name actions (MAGIC, TURBO).
So every prompt is one of these textures: the scroll box, the option
menus' button rows, the select screens, Screen Adjust's arrows, the
inventory's Continue.

gdl_client::glyphs (presentation only; gdl_play's present_hud resolves a
blit's texture): when a face-button texture is decoded, an Xbox variant is
made from it — the opaque texels (disc and symbol) redrawn black, the rim
kept, and the letter of the controller button that presses the same pad
entry drawn over it (player_input: South → cross: A green, East → circle:
B red, West → square: X blue, North → triangle: Y yellow; letters 16
texels high in strokes 2.5 wide, 4×4 samples a texel). The d-pad arrows
stay. `button_prompts` "auto" uses the controller whose button was pressed
last (else the first connected one): Sony's (USB vendor 0x054c, or a name
naming a PlayStation pad) keep the original prompts, every other
controller gets the letters; no controller (keyboard): the original.
Not changed: the Controls page's picture (opt_ctlblit: a PS2 pad; its
labels sit on the same positions of an Xbox pad); Nintendo-layout pads get
the Xbox letters by position (KNOWN_ISSUES).

Checks (RX 580, kwin virtual 1600×1000; private/runs/title-start-glyphs-*,
scratch frames): "xbox" — the Tower's welcome scroll reads "Press (Y) when
done."; "auto" with the virtual Xbox Series pad (045e:0b12, as the
player's) — the title Options menu's row "(Y) Back (A) Select" and the
select screen's "(A) Select (Y) Back"; "auto" without a controller — the
original triangle. 1000 per-update simulation hashes with "xbox" equal
the previous build's (Tower → G portal run).

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
GDL_SOUND_LOG (per sound request: ID, ROM name, volume, pan, priority —
cue checks without listening, e.g. scenario map-g1); GDL_SCENE_DUMP; tools/python/perf_bench.py; the `profile` feature (Chrome
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
| Xbox button prompts | letters on scroll, menus, select; original without a pad; hashes equal | title-start-glyphs-auto-xbox1, title-start-glyphs-select-xbox1 |
| Graphics (2026-10-30): original presentation | pixel-identical to the 1.0 build (Tower, G1, J5, C2, H4) | frozen-base-p1j vs frozen-base-ref |
| Graphics: every enhancement on, 600 G1 updates | simulation hashes equal to the original presentation and 1.0 | gfx-simhash-p1j |
| Graphics: frame time, 1920×1080, MSAA 4 | Enhanced within run-to-run spread of the current settings | perf-bench-gfx-p1j-cur / -enh |
| Graphics: reference-trace replays (146) | all equal to r88 | tools/regress.sh r89 |

Not provided: a render scale (the 3D renders at the window's own
resolution — 1920×1080, 2560×1440 or 4K are native high resolutions),
gamepad button remapping.
