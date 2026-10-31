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
| texture_filter | "sharp" | "original" | "sharp": base textures at the GPU's level of detail, 16× anisotropic, over full mip chains (Graphics, below) |
| sharpen | 40 | 0 | contrast-adaptive sharpening of the 3D view, percent (0, 20 … 100) |
| full_colour | true | false | no 16-bit frame, dither or integer truncation, at the original's mean level |
| smooth_lightmaps | true | false | lightmaps sampled with a cubic B-spline |

## Graphics (post-1.0, 2026-10-30)

User request 2026-10-29 (after 1.0.0): sharper, higher-resolution-looking
textures and better lighting; the approved plan's first step (Checkpoint
A) is real-time filtering and the colour pipeline, no new art. Every
setting is presentation only and off in the original presentation; the
overlay's "Graphics..." page (F10 → Graphics...) shows them with a preset
row: Original (all off), Enhanced (sharp, sharpening 40 %, full colour,
smooth lightmaps; since 2026-10-31 also the lighting effects of
"Lighting" below, until the Checkpoint C playtest decides), or Custom.
Enhanced is the default (the user's choice after the Checkpoint A
playtest, 2026-10-30, sharpening 40 % included); settings files without
the graphics fields, such as 1.0's, take it too.

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
  (its msaa_writeback set to Always while sharpening is on; its blit
  pipelines are made at startup for the sample counts the adapter supports
  for Rgba16Float — an unconditional 2× pipeline ended the client at
  startup on lavapipe, which offers 1, 4 and 8: menu flow select-back1,
  2026-10-30). A first
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

### Upscaler comparison (Checkpoint B, 2026-10-30)

The plan's step 7, a bounded experiment for choosing how HD textures are
generated. Nothing from it ships; its outputs (upscaled retail textures)
stay in private/runs/upscale.

Input: `GDL_TEXTURE_DUMP` runs of the Tower, G1, C2 and J5 (frozen_frames,
own HOME and saves) — 3,626 distinct textures, 8.4 M texels; most are
small (1,224 at 32×32, 756 at 16×16, 506 at 64×64, 11 at 256×256). The dump
now also writes DIR/index.tsv (texture name, TEXDEF label, size, record
flags) so samples can be chosen and padded by kind.

Methods (tools/python/upscale_compare.py; each 4×, every texture, written
as a texture pack):

| Method | Identity |
| --- | --- |
| Catmull-Rom | Pillow 12.1.1 BICUBIC (Keys cubic, a = −0.5), per channel in float |
| xBRZ | 1.9 (2026-01-24), GPL-3.0; xBRZ_1.9.zip SHA-256 b2dff73b…a0fc9; scale 4, default ScalerCfg, ARGB |
| Real-ESRGAN animevideov3 | Real-ESRGAN v0.3.0, BSD-3-Clause (LICENSE in the tagged source, 4fbaa947…fed1); realesr-animevideov3.pth b8a83768…0c96 |
| Real-ESRGAN general-x4v3, denoise 0.5 | inference_realesrgan.py's default: realesr-general-x4v3.pth (8dc7edb9…6292) and realesr-general-wdn-x4v3.pth (1641f8c4…6dca) mixed 0.5 / 0.5 (utils.py dni) |
| Real-ESRGAN general-x4v3, denoise 1 | realesr-general-x4v3.pth alone |

The Real-ESRGAN network (SRVGGNetCompact: 3×3 convolutions with PReLU, 16
or 32 body layers, PixelShuffle 4 plus the nearest-upsampled input) runs
in float32 numpy from the upstream weights (a restricted unpickler, no
torch). Against the upstream program realesrgan-ncnn-vulkan 20220424
(binary 22c2296a…cdbda9, fp16, animevideov3-x4) on five opaque C2
textures: mean difference 0.08–0.18 / 255, max 2 / 255, once our input is
padded the way ncnn pads its tiles (10 mirrored texels; with zero padding
the borders differed by up to 62).

Our handling, the same for every method: each axis is padded before and
cropped after scaling, wrapped where the record repeats and replicated
where it clamps (flags 0x4 / 0x8); fully transparent texels first take the
colour of the nearest kept texels (no dark fringes); opaque textures stay
opaque, cut-outs' alpha is thresholded at half (the material's alpha class
is unchanged), blended alpha is upscaled with the colour (Real-ESRGAN: as
grey through the network, upstream's default).

Results (RX 580, 1920×1080, Enhanced settings, frozen frames; local
viewer private/runs/upscale/index.html with every scene switchable between
methods at native size, sheets sheet-1..6.png of 48 samples by category):

| Method | CPU s (32 threads, this tool) | RX 580 s (upstream ncnn) | Pack MB (PNG) |
| --- | --- | --- | --- |
| Catmull-Rom | 4 | – | 162 |
| xBRZ | 48 | – | 116 |
| Real-ESRGAN animevideov3 | 182 | 13.9 measured | 238 |
| Real-ESRGAN general, denoise 0.5 | 360 | ≈ 26 ESTIMATE (34 / 18 layers) | 229 |
| Real-ESRGAN general, denoise 1 | 334 | ≈ 26 ESTIMATE | 192 |

The ncnn run (all 3,626 textures, PNG load and save included) peaked at
+140 MiB of video memory. In the game every 4× pack costs about the same:
+340–400 MiB peak video memory at the busiest of the four levels (whole-GPU
samples, so ±30 MiB) and 3–4 s longer level starts (window to first frame:
PNG decoding and generated mip chains; the nearest-neighbour control pack
+2.8 s), so the cost comes from the texel count, not the method.

What the frames show: Catmull-Rom adds no detail (barely different from no
pack under 16× anisotropic filtering); xBRZ keeps hard edges but
posterises the painted, photographic textures into flat blobs; animevideov3
is clean and painterly but smooths away grain (grass, dirt, bark);
general-x4v3 gives the most convincing detail on surfaces, characters and
items (stone cracks, wood grain, faces), denoise 1 a little smoother than
0.5. Failure modes: both Real-ESRGAN models turn the PS2 dither in C2's
sky (SANDBACK, opaque and repeating like any wall) into zebra stripes, and
amplify the faint streaks of soft glows (the Tower's CENTERSTONEGLOW*
transporter beams) into spikes; general more than animevideov3. Fonts and
HUD art upscale cleanly with every method. Found by bisecting hybrid packs
(private/runs/upscale/texbisect.py).

The user's choice (Checkpoint B, 2026-10-31): Real-ESRGAN general-x4v3 at
denoise 0.5 for every category — world surfaces, characters, enemies,
items, foliage and grates, HUD and fonts — except glows, fire and magic
effects and skies and backdrops, which keep their original textures. How
the generator tells those two categories apart from data is decided in
the plan's steps 8–10 (the labels and flags alone do not separate them,
see unknowns.md).

### Lighting (Phase 4, toward Checkpoint C, 2026-10-31)

The plan's steps 11–14. Each is a Graphics-page setting, part of the
Enhanced preset (on by default until the user's Checkpoint C playtest
decides) and off in the original presentation.

- Dynamic lights (Settings::more_lights, "Dynamic lights: Up to 32 /
  Original 8"). The game's point lights (ProcessEffects' effect lights,
  do_players' dark-level light; rendering.md "Point lights") reach every
  material through one shared buffer, now 32 slots (lib.rs
  MAX_POINT_LIGHTS; gs.wgsl reads it in place and stops at the first
  unused slot). The original presentation keeps the port's selection, the
  8 nearest the camera (the original gives each object the lights within
  its reach, pb_do_poslights; its slot count is unread). Up to 32: every
  light whose sphere meets the view frustum, nearest the camera first.
- Torch light (Settings::torch_lights): a faint flickering point light at
  every world fire. Fires come from data: gdl_tools world-psys lists every
  level's world psys nodes (61 levels; private/runs/torch/world-psys.json)
  with texture, blend and 300 simulated psys frames; the additive emitters
  with P_TORCH, P_TORCH_P, BLUEP_TORCH, POOLFIRE, POOLFIREC, BLUEPOOLFIRE
  or FBALLX2 are torches, braziers, fire pools and H4's fireballs (the
  others: SMOKE3, MISTPART*, GREENSMOKE3, EMBER_SPARK2, STEAM, SLIMEDROP,
  FIREFLY, WATERDROP, ZZZ, BLUEEMBER_SPARK2 — smoke, mist, embers, drops).
  The sprites themselves are white (vertex colour 127–190), so a light's
  colour is its texture's alpha-weighted mean, brightest channel 1.
  Emitters within 2.5 units are one fire (a torch's flame and sparks); the
  light sits one unit above their mean, as ProcessEffects places effect
  lights, reach 16, attenuation 0.3 (the VU1 formula's gain about 0.7 at
  3 units, 0.2 at 8; NATIVE values chosen by eye: the lightmaps already
  hold the fires' light), strength 80–100 % from two sines of simulation
  time with a phase from the position (no game random source; constant
  under GDL_FREEZE_AT). Torches come after the game's lights in the slots.
  The effect is a warm glow on floors, walls and characters near fires
  (frozen-p4-lights2 against frozen-p4-a: the Tower's brazier, H4's
  braziers).
- Character shadows (Settings::shadows, "Character shadows: Shadow maps /
  Original"). One shadow-casting Bevy DirectionalLight along the level's
  light (LEVL +0xf0, as present_scene_light: every level's record holds
  (−1, −6, 2), 70° above the horizon, except B6's 45°; gdl_tools
  levl-list prints it), only while a level plays; one cascade to 120
  units (NATIVE value; a second cascade cost about 0.5 ms of render-thread
  time a frame and looked the same, frozen-p4-opt1 against frozen-p4-all2).
  Casters: the opaque and cut-out meshes under
  a player's body, an enemy or item slot or a critter (G1: about 2,600 of
  5,000 meshes, mostly the barrels' parts); every other mesh is marked
  NotShadowCaster (only while shadows are on: the marker changes entity
  archetypes and with them the order of tied draws). GsMaterial now takes
  part in Bevy's shadow (and prepass) pipelines with gs_prepass.wgsl,
  which applies the GS alpha test; its pipeline specialisation leaves
  prepass pipelines alone except for culling (both faces). The shadow
  falls on unlit parts — the world, whose light is baked into vertex
  colours and lightmaps — as a shade to 55 % (NATIVE value chosen by eye;
  not on psys sprites, additive draws or the separate lightmap pass).
  Lit parts (characters, items) keep their VU1 lighting: receiving shadows
  there darkened the enemies under their own heads and arms in the steep
  light (frozen-p4-shadows2). The blob shadows (player SHADOWL1, critter
  SHADOW<n>L1) are hidden while shadows are on; the player's water splash
  ring, which takes the blob's place in water, stays. Frozen frames
  frozen-p4-shadows3 against frozen-p4-lights-ni (all five scenes, no
  motion blending: each pair of shots identical).
- Ambient occlusion (Settings::ambient_occlusion). Bevy's SSAO (GTAO
  with a visibility bitmask, effect radius fixed at 0.73 view units,
  quality High) from a depth and normal prepass of the 3D camera, of and
  on the light-baked world only: opaque parts whose lightmap is folded
  into the base pass (GsParams options.y; gs_prepass.wgsl discards the
  others in the prepass with normals — not in the shadow map's — and
  gs.wgsl multiplies only those draws' final colour by the occlusion).
  The prepass holds only opaque parts that write depth and culls as the
  main pass: a cut-out's alpha test there samples another level of detail
  than the main pass, and a texel it kept but the main pass rejected would
  hide what lies behind (gs_prepass.wgsl, GsMaterial::specialize). SSAO
  needs a single-sampled view, so with it the window cameras use no MSAA
  and the 3D view takes SMAA where MSAA was chosen
  (Settings::msaa_in_use / smaa_in_use).
  Objects lit by VU1 (players, enemies, items, exits, critters) neither
  cast nor take it since the Checkpoint C playtest (2026-10-31, the
  Tower's level circle): with them in the prepass, the occlusion left a
  dark halo on the floor behind a character, strongest on the far side,
  that followed him as a faint second outline (Bevy 0.18's bitmask GTAO
  computes a distance falloff but does not apply it, so a figure several
  units in front still occludes the floor behind it on screen); and the
  exits' swirl pools showed straight dark lines: the pool's ring and pit
  are an object, its opaque pit parts at slightly different heights under
  the translucent swirl — steps the original's flat shading never shows
  (frozen-dbg-circle-m4: the prepass depth in bands; frozen-dbg-circle-m5:
  the ring and pit VU1-lit, the floor lightmapped; the prepass and the
  main pass agree on every visible opaque surface, frozen-dbg-circle-m3).
  The shadow maps ground the objects instead. Frozen frames at the level
  circle (GDL_DEBUG_TELEPORT 45.07,−6.43,−104.70 and 30,−6.43,−104.70):
  the pool as without occlusion and no halo (frozen-fix2-circle,
  frozen-fix2-centre and their -dbg views, against frozen-dbg-circle-ao,
  frozen-dbg-centre-a). Left: the floor's T-junction cracks (single
  pixels along some polygon edges, where the background shows through in
  the original too) are faint dotted lines in the occlusion.
  Result: contact shading in the world's own creases (wall–floor
  junctions, steps, plinths, corners), objects untouched; frame means
  (red, whole frame) without → with it: Tower 28.4 → 27.5, G1 19.6 →
  19.0, C2 82.9 → 79.3, J5 33.0 → 32.1, H4 20.2 → 19.7 (frozen-c-shadows
  against frozen-c-ao2; the earlier occlusion of every opaque draw gave
  27.2, 18.9, 78.8, 32.1, 19.6, frozen-c-ao). The original presentation
  is unaffected (frozen-base-aofix2 against frozen-base-ref: the Tower,
  J5, C2 and H4 identical, G1 the known 3,491 pixels).
  DEBUG GDL_DEBUG_AO (opaque draws): 1 the occlusion term alone, 2 the
  prepass normal, 3 the occlusion with red where the draw lies in front
  of the prepass depth and blue behind it, 4 the prepass's and the draw's
  view distances in quarter-unit bands (red, green), 5 the lighting
  class (red unlit, green lightmapped, blue VU1-lit) × the occlusion.
- SMAA (Settings::smaa; "Anti-aliasing: Off / 2x / 4x MSAA / SMAA"): Bevy's
  SMAA on the 3D view. Polygon edges as smooth as 4× MSAA in G1; cut-out
  edges (the spearmen's tridents) slightly more stepped.
- Bloom (Settings::bloom): Bevy's bloom on the 3D view, additive,
  intensity 0.25, prefilter threshold 1.0 (softness 0.3), mip chain from
  256 (Bevy's default 512: the same look, fewer passes; NATIVE values by
  eye). The frame holds GS display values that only DisplayToLinear clamps,
  so stacked additive layers exceed 1 and only they bloom: the Tower's
  crystals and brazier gain halos; C2 and G1's means are unchanged
  (frozen-p4-bloom1). The plan's soft shoulder in DisplayToLinear is not
  added: below 1 it would change every bright surface.
- The original presentation against the 1.0 build (frozen frames, all
  enhancements off): Tower, J5 and C2 identical; G1 3,491 pixels (mean
  0.083 / 255) and H4 31,981 (0.417 / 255) differ once the SMAA and bloom
  plugins are in (frozen-base-p4d). Bisected: either plugin alone changes
  G1; both together with the material prepass change H4 too; neither
  plugin, or 40 extra startup entities, or three extra component types:
  identical; two empty systems added to the render sub-app (one in
  ExtractSchedule, one in Render's Prepare set), with neither plugin:
  exactly the same two differences (private/runs/frozen-bis-*). So the
  render schedule's composition alone moves them. G1 is the tie order of
  the spearmen's coplanar cut-out faces (the Checkpoint A spear edges):
  the scene dumps of both variants are identical (every drawn mesh,
  texture and material parameter). H4 is the brazier flames' particles:
  the dumps differ only in the psys sprite meshes, and the scene dump
  diagnostic itself flips H4 as well — the frozen-frame harness steps
  particles per update, but which emitters are stepped during a catch-up
  follows the frame in which the updates land, a real-time effect (a
  later run, frozen-base-aofix2, has H4 identical to 1.0 again with the
  same render schedule). Neither is a change in what the original presentation draws; the
  1.0-identity gate is sensitive to Bevy's render schedule. The user's
  decision (2026-10-31): keep Bevy's SMAA and bloom and change the gate
  (no hand-written passes). tools/python/baseline_gate.py captures the
  original presentation's frozen frames with scene dumps at the shots and
  judges each level against the 1.0 build: identical pixels; or "draw
  order" — pixels differ but the scene dump equals 1.0's (every drawn
  mesh, transform, texture and material parameter), G1's tie; or
  "particles" — only particle sprites differ, the level is captured again
  (up to twice) and must then be identical or draw order; anything else
  fails. Reference: frozen-base-ref's frames and frozen-dump-ref's dumps
  from the 1.0 binary (gdl_play-d100e5f; its H4 taken from a run whose
  frames equal frozen-base-ref: the 1.0 binary itself landed in the other
  particle state in one of two runs, 31,981 pixels, kept as
  H4-otherstate). Dumps compare after mapping the current format to 1.0's
  (the image fields added later — mip level count, sampler — dropped;
  psys sprites' light_dir.w −1 read as 0, the fix below). The gate on the
  current build (gdl_play-aofix3): the Tower, J5, C2, H4 identical, G1
  draw order (baseline-gate-gate-aofix3); its particle and failure
  verdicts checked on a synthetic current-format dump in the other H4
  state (particles) and with one altered material line (different).
- Fidelity fix found on the way: psys sprites (DrawPsysSub, drawn by
  pbBlit as GS sprites, not by the VU1 programs) took the point lights in
  the port — their meshes carry a +z normal, so a light in front doubled a
  flame's brightness. Their material now has light_dir.w −1 (no VU1
  lighting). The frozen original-presentation scenes have no point lights
  and stay identical to 1.0 (frozen-base-p4b).

Checks (RX 580, 2026-10-31; binary private/runs/gfx-ref/gdl_play-p4d,
SHA-256 in the perf reports). Frame times, mean / p99 ms, 1920×1080
fullscreen widescreen, uncapped, motion blending on, 900 updates
(perf_bench.py gfx-p4d-*; each row adds one effect to Checkpoint A):

| Settings | Tower | G1 walk | C2 | J5 | H4 |
| --- | --- | --- | --- | --- | --- |
| Checkpoint A | 10.51 / 15.35 | 8.93 / 12.51 | 10.08 / 14.93 | 8.60 / 14.16 | 8.23 / 12.49 |
| + dynamic lights, torch light | 10.46 / 15.31 | 8.90 / 12.45 | 9.97 / 13.89 | 8.56 / 14.17 | 8.49 / 12.70 |
| + character shadows | 10.96 / 16.01 | 9.62 / 13.41 | 10.96 / 18.14 | 9.02 / 15.11 | 8.82 / 13.34 |
| + ambient occlusion (SMAA) | 11.05 / 16.79 | 9.29 / 13.44 | 10.65 / 17.96 | 9.39 / 15.14 | 9.35 / 13.72 |
| + bloom | 10.65 / 15.81 | 9.14 / 13.25 | 10.33 / 15.33 | 9.11 / 14.91 | 8.98 / 13.55 |
| everything (the new Enhanced) | 11.70 / 17.73 | 10.05 / 14.24 | 11.85 / 20.13 | 10.00 / 15.98 | 9.88 / 14.39 |

The plan's target (p99 ≤ 16.7 ms with everything on) holds in G1, J5 and
H4, not in the Tower (2.8 % of frames over 16.7 ms) and C2 (8 %). The cost
is render-thread CPU, not the GPU: 1280×720 takes the same time
(gfx-p4x-a720 / all720). A Chrome trace of C2 (profile feature,
private/runs/prof-p4) puts it in Bevy's per-view work: shadow caster
visibility 0.64 ms a frame, encoding the shadow cascades 1.1 ms (two
cascades, since cut to one), the depth/normal prepass 0.6 ms, bloom 0.6
ms (mip chain since halved), the shadow and prepass queue and
specialisation systems about 0.7 ms; frames that carry a simulation update
pay most (C2: 12.3 → 16.0 ms mean). SSAO's quality level made no
difference. Run-to-run spread of p99 is large (the Tower's everything-on
p99 25.3 ms and 18.9 ms in two runs of the same build).

Simulation: 600 G1 updates with every enhancement on have the same
per-update hashes as Checkpoint A and the 1.0 build
(private/runs/gfx-simhash-p4d); the Windows build (x86_64-pc-windows-gnu,
pinned 1.93.1) under Wine on the RX 580 gives the same 600 hashes
(private/runs/gfx-wine-p4d). The 146 reference-trace replays
(tools/regress.sh r91) equal r90. Unit tests: settings (presets, MSAA/SMAA
under ambient occlusion), overlay (rows, the anti-aliasing cycle), torch
clustering and flicker range. GUI: title_start_check gfx-p4e 6/6 and,
with everything on through the attract sequence to the overlay,
gfx-p4e-enh-overlay 2/2 (the overlay shows "SMAA (for 4x MSAA: ambient
occlusion)"); the first run (gfx-p4d) caught a startup panic —
present_shadow_light took Res<Sim>, absent before the title's Start —
fixed by gdl_play-p4e, which differs from p4d only there. Menu flows
(software renderer, default = Enhanced settings) menu-flows-gfx-p4e:
23/23 pass.

### HD textures (plan steps 8–10, 2026-10-31)

The user's choice at Checkpoint B: Real-ESRGAN general-x4v3 at denoise
0.5 for every category except glows, fire and magic effects and skies and
backdrops. Generated on the player's machine from their own disc; nothing
retail-derived ships.

- Network (gdl_client::hd::esrgan): SRVGGNetCompact (34 3×3 convolutions
  with PReLU, PixelShuffle ×4 plus the nearest-upsampled input) as wgpu
  compute on a device of its own; weights from tools/python/hd_weights.py
  (the two upstream checkpoints mixed 0.5 / 0.5 in float32, embedded,
  4,853,208 bytes, SHA-256 e6626f1f…ef4b; THIRD_PARTY.md). Kernel: 8×8
  invocations of 2×2 pixels × 16 output channels, input chunks of 8
  channels with a halo in workgroup memory, each output group's weights a
  36,864-byte uniform block at a dynamic offset (scalar reads), one
  submission per layer of at most about 2 M padded texels (short: the
  device drives the desktop too; a first version with one submission per
  batch of 268 MB had the RX 580's context reset by the driver).
- Processing (hd::prep): upscale_compare.py's — PAD 12, wrapped or
  replicated per the record's clamp flags, transparent texels filled from
  kept neighbours, opaque / cut-out / blended alpha classes kept, numpy's
  float32 order and round-half-to-even. Check against the Checkpoint B
  numpy pack (gdl_play --hd-check): all 2,207 C2 textures, mean difference
  0.0000 / 255, largest 1 (the plan's limit: mean ≤ 1 / 255); 27 s of
  network time on the RX 580 (from 109 s for the first kernel).
- Census (hd::census, gdl_play --hd-census): every OBJECTS package of the
  catalog (643: levels, realm maps, monsters, every player class, colour
  and tier, items, weapons, the front end), each bitmap record decoded as
  materials_for (and, for the front-end and realm-map packages, hud_image)
  does and named by texture_pack's hash: 20,596 distinct textures, 100.9 M
  texels, the largest 256×256. Each texture's uses: world nodes (MB flags,
  lightmapped part, node radius), package tree nodes (MB flags, tree),
  particle systems (by texture label) and texture modifier frames (a frame
  takes its target record's uses: header-table, sequence and type-3 node
  modifiers).
- Which textures keep their originals (hd::classify; unit-tested):
  particle textures; additive draws; blended textures drawn without depth
  writes or camera-facing; nodes of effect trees (EffectInfo's 80 names,
  the trees InitEffects binds by name, trees with "FX" in the label — the
  bosses' attacks — and the players' SFX packages' powers and combos, not
  their thrown weapons or familiars); frames of those, and blended frames
  whose target no traced draw uses (fire, spit, steam and glow sets found
  by name elsewhere); skies: static, world-only, never lightmapped, not
  camera-facing, on a node of radius ≥ 60 (or ≥ 40 on at most two nodes).
  On this disc: 6,854 textures (13.3 M texels) keep their originals (89
  skies and backdrops — horizons, star fields, cloud and fog layers, C2's
  SANDBACK —, the rest effects); 13,742 are generated. Rules first tried and
  dropped: radius alone (siege posts, fences, webs), every animated blended
  texture (water, rapids, coins, keys), a texel-softness measure (SANDBACK
  11, ICEHORIZON3 32 against ground 24–46). Known misses, upscaled: some
  world fire sets drawn as ordinary blended meshes (houses' fireplaces), the
  realm map's glow overlays and other HUD glows (the user chose HUD
  upscaled); a few animated items (a key, wizard heads) stay original with
  the untraced frames. Labels were checked against the result, not used.
- Pack (hd::generate, gdl_play --hd-generate CONFIG DIR): the data
  directory's gdl-native/SLUS-20047/hd-textures/<generator>/ —
  realesr-general-x4v3-dn05-<model hash>-p<processing version> (DEBUG
  GDL_HD_DIR: another root) — one `<name>.png` per texture (texture_pack's
  format: 4×, alpha doubled), written atomically by worker threads while
  the GPU runs the next batch; existing files are skipped, so an
  interrupted run resumes; manifest.json (generator, model SHA-256,
  processing version, the kept textures with their reasons) marks the pack
  complete. The texture pack loader takes the user's own pack first, then
  the generated one.
- Setting and overlay: Settings::hd_textures (off by default; not part of
  the presets, Original turns it off). The Graphics page's "HD textures"
  row turns it on; gdl_play then runs the generator as a child process
  (hd_textures_job) and shows its progress on the row, stops it when the
  setting is turned off, and puts a complete pack in use: textures decoded
  from then on take it (the next level; the front end after a restart).
- Results (RX 580, this disc): the whole pack in 6 min 22 s (gdl_play
  --hd-generate, 13,742 textures; peak 1.2 GB of memory), 2.0 GB on disk,
  manifest complete. Its files match the Checkpoint B numpy pack where both
  have the texture (36 of a 300-texture trial: 7 identical, the rest within
  1 / 255). In the game (frozen-c-hd against frozen-c-all2, everything on):
  surfaces, characters, items and foliage gain the Checkpoint B detail; C2's
  SANDBACK sky and the Tower's crystals and glows unchanged. Cost
  (perf_bench, 1920×1080, everything on, without → with): video memory
  +167 to +282 MiB (whole-GPU samples beside the desktop's 2.5 GB), level
  start 0.66–0.78 s → 1.9–3.4 s (PNG decoding and mip chains), resident
  memory 0.52–0.63 → 0.86–1.13 GB, frame time within run-to-run spread
  (mean −0.8 to +0.4 ms). The plan's 2× fallback for video memory is not
  needed at these sizes. GUI checks on the real GPU: with the setting on
  and no pack, the game starts the generator and it runs to its end
  (frozen-hdjob, DEBUG GDL_HD_LIMIT 40); turning the setting off mid-run
  stops it, the files written stay (frozen-hdjob3: 101); a parent that
  ends closes the child's output and the child stops. The Windows build
  (x86_64-pc-windows-gnu) under Wine runs the network on Vulkan with the
  same result (50 C2 textures, mean 0.0000 / 255, max 1); Direct3D 12, which
  wgpu may choose on Windows, is not checked here. The original
  presentation passes baseline_gate.py (gate-hd3; H4 re-captured once).

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
frame 0.8 % of the pixels — edges — differ between 1× and 4×. Since
2026-10-31 the row also offers SMAA (`smaa`), and ambient occlusion
replaces a chosen MSAA by SMAA (Settings::msaa_in_use / smaa_in_use;
"Lighting").

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
11 MB) and a line in DIR/index.tsv (name, TEXDEF label, size, record
flags; research tooling, 2026-10-30); a pack's PACK/<name>.png (any size, 8-bit RGB/RGBA/grey) replaces
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
| Lighting (2026-10-31): original presentation | identical to 1.0 in the Tower, J5, C2; G1 (spear tie order) and H4 (flame particles) differ once Bevy's SMAA and bloom plugins are built in, reproduced by two empty render systems ("Lighting") | frozen-base-p4d, frozen-bis-* |
| Lighting: every effect on, 600 G1 updates | simulation hashes equal to Checkpoint A and 1.0; Windows build under Wine the same | gfx-simhash-p4d, gfx-wine-p4d |
| Original presentation gate (2026-10-31, user's decision): frames and scene dumps against 1.0 | the Tower, J5, C2, H4 identical; G1 draw order (dump equal) | tools/python/baseline_gate.py, baseline-gate-gate-aofix3 |
| HD textures (2026-10-31): generator against the Checkpoint B numpy pack | all 2,207 C2 textures mean 0.0000 / 255, max 1 | gdl_play --hd-check |
| HD textures: the whole pack on the RX 580 | 13,742 textures, 6 min 22 s, 2.0 GB; 6,854 kept original (effects, skies) | private/runs/hd/root, pack manifest |
| HD textures: in the game | +167–282 MiB video memory, level start +1.2–2.7 s, frame time unchanged | perf-bench-gfx-hd1-*, frozen-c-hd |
| Lighting: frame time, 1920×1080 | everything on: mean 9.9–11.9 ms, p99 14.2–20.1 ms (Tower, C2 over 16.7) | perf-bench-gfx-p4d-* |
| Lighting: reference-trace replays (146) | all equal to r90 | tools/regress.sh r91 |
| Lighting: GUI | title_start_check 6/6, with everything on to the overlay 2/2; menu flows 23/23 | title-start-gfx-p4e*, menu-flows-gfx-p4e |
| Lighting: ambient occlusion on the light-baked world only (playtest 2026-10-31) | the level circle's swirl pool as without occlusion, no halo around the player; original presentation unchanged; frame time within run-to-run spread of p4e (A/B pair: Tower 11.66 → 11.62, G1 10.04 → 9.91, H4 10.33 → 10.08 ms mean); title_start_check 6/6 and 2/2 to the overlay with no validation errors (final binary gdl_play-aofix3) | frozen-fix2-*, frozen-base-aofix2, perf-bench-gfx-ab1-*, title-start-gfx-aofix3* |

Not provided: a render scale (the 3D renders at the window's own
resolution — 1920×1080, 2560×1440 or 4K are native high resolutions),
gamepad button remapping.
