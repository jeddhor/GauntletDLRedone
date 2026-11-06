# Presentation comparison report (M17)

Revision SLUS-20047. What the native client draws and plays against the
original, by layer; the evidence files are private (private/reports).

## Scenes

tools/python/scene_compare.py LEVEL SOURCE TAG puts the reference frame
(a PCSX2 slot or staged capture, reference_screenshot.py) beside gdl_play at
the reference's camera (reference_camera.py → GDL_DEBUG_CAMERA):
scene-<tag>-ref.png / -native.png and the side-by-side scene-<tag>.png.

| Set | Levels | Kind |
|---|---|---|
| The hub | the Tower (render-tower-compare-v2, GS dump draw by draw) | actual reference, same camera |
| First levels | A1, B1, C1, D1, E1, F1, G1, H1, I1, J1, K2 | actual reference (warped captures labelled), same camera |
| Boss arenas | A5, B6, C5, D5, E2, F2, G5, H4, I5, J5, K5 | actual reference, same camera |
| Features | A1 statue, golem, Death; G1 mip levels | actual reference, same camera |
| Every other level (2026-10-24) | A2 A3 A4 A6 B2 B3 B4 B5 C2 C3 C4 D2 D3 D4 G2 G3 G4 H2 H3 I2 I3 I4 J2 J3 J4 J6 K1 K3 K4 S1–S9 (scene-<level>-v1; B2/D3/I2 from their warped start captures, the rest from lvl-<L>-play-v1) | actual reference, same camera |

Fixed through these comparisons (rendering.md, textures.md, hud.md):
materials and GS blend/test state, lightmaps and their CLUT, mip LOD
(TEX1 K from the part), multiplayer-only items drawn in single player,
statues, safe rocks, switches, GRU-style generators, Skorne's OANIM lower
body, E2's falling slabs, the inverted ProcessItemWobjs fade (the Tower's
podium and beams), point lights and the VU1 lighting formula, the frame's
PSMCT16S + dither output, texture modifiers, skins, chrome, particle
systems, effect trees.

With these every one of the 61 levels has a reference frame beside the
native one at the same camera. The 2026-10-24 set shows no missing model,
material or lighting layer; its differences are the moment (trap spikes up
or down, spinning gems edge-on or face-on, caustic and tile phases, enemy
counts and health after the native's 12 idle seconds). The pale-green
player silhouette in the native B4, C3 and J6 frames is the original's hit
flash, checked against a reference GS dump (rendering.md "Hit flash"); the
Tower's crystal brightness is phase spread (rendering.md "GS dump software
replay").

Open (unknowns.md, rendering.md): the GS's per-draw integer truncation of
lighting (colours can differ by one or two steps); critter broken-part models use the
"<prefix>D<label>" name (HYPOTHESIS). Remaining differences in the frames
are timing (animation phase, enemy positions), not content.

## User interface

- Player panels, scroll boxes, help boxes, pickup notices, the select
  screen, inventory, shop, tally, map, Sumner's speeches and captions, boss
  meters, thermometer, compass, title, option menus, credits and the attract
  sequence are drawn from the ELF records and packages (hud.md); the panels
  were checked against the Tower capture.
- Menu flows (playtest.md "Menu flow tests"): five GUI flows pass
  (menu-flows-m17-v1): the title and every Options entry, a new character,
  the Tower and realm pause menus.
- Native additions, labelled: the save recovery notice (saves.md) and the
  controller message box's layout (the scroll box's, with the original
  CONTROLLERMISSING text).

## Audio and video

- Event → asset bindings: av-catalog-v2 (audio.md "Audiovisual catalog"):
  every ELF cue ID, music stream, world sound and level movie resolves to
  disc assets. Event selection and timing come from the ported Audio*
  functions, checked by the whole-update replays.
- SPU volume/pan: ported from instructions (audio.md "DCS voices"):
  AudioAng's pans from Cam[0], volSFX/volMusic/mono from the options,
  DCS voice allocation, ducking and voiceSetVolPan with the disc's pan
  table. Approximate: the SPU envelope, reverb, the hardware mix and the
  movie soundtrack level.
- Movies: PSS video and audio decoded natively (decisions/0003-movies.md,
  0007-native-movie-video.md; "Movies" below).

## Movies

Decoder: gdl_formats::mpeg2 (decisions/0007-native-movie-video.md, from
2026-11-05; FFmpeg as a child process before). Evidence, actual disc:

- Stream survey (every sequence, extension and picture header of the 36
  MPEG/*.PSS and MPEG2/*.PSS video streams): one sequence each, 640×480,
  frame_rate_code 5 (30 fps), aspect 4:3, no loaded quantiser matrices;
  sequence extension profile/level 0x48 (Main@Main), progressive_sequence 1,
  4:2:0; one GOP header per 11–12 pictures; every picture coding extension
  intra_dc_precision 0, frame picture, frame_pred_frame_dct 1, no
  concealment vectors, linear quantiser scale, intra_vlc_format 1, zigzag
  scan, progressive_frame 1, no repeat_first_field; 11 956 pictures (I, P
  and B). No quant-matrix extension.
- movie_check.py all-c: the decoder's pictures (SHA-256 of the Y, Cb and Cr
  planes, display order) equal FFmpeg 8.0.1's with its C code
  (`-cpuflags 0 -fps_mode passthrough`, yuv420p) for all 11 956 pictures of
  the 36 movies; all-simd: equal to FFmpeg's default (SIMD) decoding as
  well; wine-a: the Windows build of gdl_tools under Wine gives the same.
  The old client command (`-vf scale=640:480 -pix_fmt rgba`, no fps option)
  gives the same frame counts (OPENING 1349, FSCROLL 617, MOVIEA1 125), so
  the pacing is unchanged. Release build speed: 290–330 pictures a second
  with hashing (OPENING 1349 in 4.3 s).
- mpeg2_tables_check.py (private/runs/mpeg2-tables): every VLC table agrees
  with PCSX2's lookup tables (reference source 0f71e668, docs/toolchain.md:
  IPU/mpeg2_vlc.h with IPU_MultiISA.cpp's indexing) on all 65 536 16-bit patterns; the zigzag and alternate scans
  and the non-linear quantiser scale are equal. Codes only PCSX2 gives
  values to: 12 invalid motion codes and B.14's 12/13-bit codes of the ten
  pairs B.15 shortens; the decoder alone accepts macroblock_stuffing (as
  FFmpeg does).
- Colour: PCSX2's IPU CSC model (upstream claim) against the old FFmpeg
  RGBA, 12 frames each of OPENING, VICTORY and TITLE2: mean 0.74–0.81 of 255
  per channel, 99.8–100 % of values within 2, at most 19 (a few edge
  pixels; FFmpeg's chroma placement differs).
- In the client (movie_play_check.py, Xvfb/lavapipe at 640×448, PATH
  without FFmpeg): the attract sequence plays MIDWAY (5.96 s; 186 pictures
  = 6.2 s) and OPENING (44.89 s; 44.97 s) to their ends with changing,
  non-black frames (linux-e); the Windows build under Wine the same (6.01 s,
  44.83 s; wine-d). 1.3.0 without FFmpeg, same check: every movie "none"
  (did not start) — the owner's Windows report reproduced (old130-noffmpeg).
  At lavapipe's full-size frame rate (about 4 a second) the client's clock,
  which takes at most 0.25 s a frame, lengthens movies; 1.3.0 with FFmpeg
  behaves the same there (old130-ffmpeg).
