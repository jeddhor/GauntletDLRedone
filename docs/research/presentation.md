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

Fixed through these comparisons (rendering.md, textures.md, hud.md):
materials and GS blend/test state, lightmaps and their CLUT, mip LOD
(TEX1 K from the part), multiplayer-only items drawn in single player,
statues, safe rocks, switches, GRU-style generators, Skorne's OANIM lower
body, E2's falling slabs, the inverted ProcessItemWobjs fade (the Tower's
podium and beams), point lights and the VU1 lighting formula, the frame's
PSMCT16S + dither output, texture modifiers, skins, chrome, particle
systems, effect trees.

Open (unknowns.md, rendering.md): the GS's per-draw integer truncation of
lighting (colours can differ by one or two steps); the Tower emblem's dark
texel-edge outlines (L1LINE154/131); critter broken-part models use the
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
- Movies: PSS video through the system ffmpeg, audio decoded natively
  (decisions/0003-movies.md).
