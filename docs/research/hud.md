# Player panels (HUD), MB blits and fonts

Instruction-derived from the SLUS-20047 ELF (Ghidra exports
private/reports/ghidra-hud1..4) and checked against the Tower GS dump
(snaps/…20261020081101.gs.zst, sprite listing via a bounded research script;
private/reference/captures/render/tower-newgame-v1-240.png). Port:
`gdl_core::game::hud` (data) and gdl_play's `present_hud` (sprites).

## Blit layer

- MBNewBlit 0x1c2b00 appends a 0x38-byte blit to the default node: word 0
  flags (1 hidden, 0x10 per-corner colours, 0x40 absolute GS units,
  0x400/0x800 width/height taken from the texture, 0x4000 corner highlight,
  0x1000000 no size doubling), +4 texture handle, +8/+0xa position, +0xc z
  × 32, +0x10/+0x12 size, +0x14..+0x1a crop (texels × 16), +0x1c colour.
  MBNewTempBlit uses a 32-entry list reset by MBResetBlits each frame.
- Screen space is 512×384: MBBlitSetPos/SetSize multiply by pbg +0x40 (the
  dump: x × 1.25, y × 448/384 into the 640×448 frame).
- Size −1 = the texture's MBRomTexPtr +0x16/+0x18 (OBJECTS bitmap record
  width/height), doubled for record flag bit 0. MBBlitSetTex resizes
  texture-sized blits and rescales the crop (fractions are kept).
  MBBlitSetTex(b, −1, n) = the current handle + n (the next OBJECTS record:
  turbo gleam frames, font pages).
- MBBlitSetColor: (rgb >> 1 & 0x7f7f7f) + 0x010101 (0xRRGGBB → GS 0x80 =
  1.0); MBBlitSetAlpha(a): alpha 0x80 − a/2 (0 opaque, 0xff → 1).
- DrawBlit: one SPRITE, RGBAQ from the colour word, z = base − 32·z, ZTST
  GEQUAL (later equal-z draws win). Dump state: TFX MODULATE, ALPHA 0x44,
  TEST alpha GREATER 1, TEX1 0x60 (bilinear). Order: node blits, temp
  blits, then MBRenderText (pbRenderNode).

## Fonts

- FontInit loads 13 fonts in font_desc order (font8x8, 8Hifonts, bars,
  arrows, score, scoratt, font32, initials, scoratt8, namefont, kanji10a,
  kanji10b, kanji20a) from "fonts/%s.fnt" with font_space; MBNewFont
  divides glyph ordinals into texture pages of 0x80 (0xd2 for 10/11, 100
  for 12).
- FNT: +4 height, glyph entries {code, width, x, y} from +0xc to a code-0
  entry (gdl_formats::fnt; all 16 FONTS/*.FNT parse to their terminator).
- Glyph crops are fractions of the TEXDEF definition size; advance =
  (int)(width × space scale); a missing glyph for ' ' advances font_space.
  DrawText x < 0 centres the string on −x; y > 0x17f drops the message.
- The dump binds the format-146 glyph textures (8Hifonts, INITIALS) as
  PSMT4 with CBP 8968 — the white CLUT with alpha 8·i that lightmaps use;
  SCORE digits are PSMT8. INITIALS cells have index-1 guide columns; at
  alpha 8 they pass AREF 1 and are drawn faintly (as the native port does).

## Panels

- reset_players_sub 0x12e6e0 creates per box (box_lx = 0/0x80/0x100/0x180):
  bkg_blit 0..5 (top bar y 0x130, S4 and frame y 0x140, an unused 0x94 bar,
  coin, heart), 12 rune stones SM_RUNE_<col>_0n, 8 key icons SM_KEY_<col>,
  4 ability blits, seven turbo blits from tb_info (name, x, y, z
  63995..63999), level_blit BTMBK_LEVL at y −0x143 (off screen), RUNE13 and
  the quest blit. Panel z 64000, text z 63990 (MBSetFontZ).
- setup_player_display 0x134d90: S3 / S4 (tinted player_rgb, inactive
  player_inactive_rgb) / S4_FRAME; display modes 1/2/5/6 swap in
  BK_RUNE_STONE_02 and S4_<CLASS> (player package), show runes by +0x1a26.
  Verified against the dump: tints (61,61,1) and (16,16,53), positions.
- WritePlayerInfo 0x1368c0 runs once per frame per player (+0x93e bit 0,
  cleared by SetPlayerVars, which game_main calls every frame):
  write_health_and_items (LV, name in INITIALS ×0.667, gold/health in SCORE
  right-aligned at box_lx + 0x3c / + 0x74, key/potion icons and counts,
  quest/rune13), show_crystals (the last collected crystal/garg item/coin,
  name cut to 14 bytes: "SM_CRYSTAL_ORA"), ShowRuneStones, the It marker.
- draw_power_meter 0x135390: +0x804 follows +0x800 (up vb_elapsed, down
  2·vb_elapsed); power_bar_state (<40 % yellow, <99 % red over a yellow bar,
  full: glow loop); +0x2adc/+0x2ae0 animate the gleam (TRBO_GLEEM1+n) and
  glow alpha. The dump's (86,1,1)/(128,128,1) bar = state 2 at 34 %.
- TriggerCamUpdate's letterbox temp blits (z 0.5) cover the panels.

## Scroll message box

- ScrollMessageBox 0x128158 (modal; paused = 1, its own frame loop): per
  page (a SCROLL_E line; −1 = every line) the Scroll_A blit is sized to
  max(text width + 0x60, done width + 0x20) (≤ 0x200) × (text height +
  0x60) at (0x100 − w/2, 0xa0 − h/2); DrawScrollText draws the lines from
  y 0xc0 − h/2 (font from the entry's FONT name in font_desc — font32 —,
  entry scale and STRS +0x10 space scale, colour 0x160c03, line step
  FontHeight + 4, centred on 256, font z 0.25); BUTTON_TRI (ResetModels'
  button_tri_tex) at (0xdf, y + 8) and DrawGlowText(0.5, "Press     when
  done.") in font32 (glow pass: FONT32_GLOW, colour 0x8200ea, flag 0x4000
  grows glyphs ~2 units, alpha pulse hilite_fade 40 / hold 5; then white).
  Input counts after 15 vblanks; a press advances the page.
- After the last page the fire scroll (StartFireScroll/ServeFireScroll):
  21 frames (one per two vblanks) of greenCircTrans (+1 + n, colour, z
  0.125) with z-only blits (flag 0x10000: FBMSK all, TEST ATST 4 = EQUAL,
  AREF 0) of greenCircTransM (+1 + n, z 0.1875) and Scroll_A (z 0.0625):
  the burnt hole (mask alpha 0) hides the scroll (z 1.0) and the text (z
  0.25), and the fire is clipped to the parchment's silhouette. OBJECTS'
  bitmap name table (header +96/+80; "GREENCIRCTRANS" is a header record
  before the frames) is the MBOX_FindTexture name space. The native blit
  material reproduces the coincident z masks as screen-space alpha masks.
- The welcome box precedes CrystalCamActivate, so no letterbox is drawn
  under it.

## Help messages (NewHelp / DoHelp)

- helptab 0x24bf08: 0x100 entries of 0x1c bytes (+4 pause kind: −1 → help
  0x3c, 1 → 0x1e vblanks of do_players pause; +8 priority; +0xc display
  rule: 1 any player saw it, 2 this player, 3 all players; +0x10
  ENGLISH.ROM entry, +0x14 line or −1 for every line; +0x18 audio).
  help_displayed bytes live at player +0x1910 + id (|= 0x11 when shown).
- NewHelp 0x18e710: refused during trigger cameras, for a lower or equal
  priority than the shown one, and (ids < 0x1d, 0x2c/0x2d/0x37/0x50) while
  help_interval runs (announcer_interval_tab 0, 120, 240, 420, 600).
  Placement: the caller's world point through MBWorldToScreen, 0x3e above
  (most callers pass the player's +0x60 attention point or an item's +0x50),
  else (0x100, 0xc0) or over the player's box; clamped to the screen. Box:
  border_names[player] (SCROLL_A) sized text width + 0x40 × text height (dy
  8) + 0x10, MBBlitSetAlpha 0x40; text colour text_colors[player] (none:
  0x160c03). Lifetime lines × 60 + 30 vblanks.
- DoHelp 0x18ed58 at the start of do_players' player section counts the
  interval, the pause and the lifetime and draws write_text 0x18e120 (the
  level-up, player-name and multi-line variants included).
- Verified natively (GDL_DEBUG_HELP=11 in G1): "COLLECT GOLD TO BUY
  POWERUPS" on the translucent parchment above the player.

## Pickup notices (add_got_it / do_got_it)

add_got_it 0x12b060 fills one of 0x18 slots for kinds 1–10, 0xd, 0xf,
0x10. do_got_it 0x12b228 (after WritePlayerInfo in do_players) creates an S3
bar (z 63980) and a picture (z 63979, 0x80 wide): gold COINHUD (Tower) /
JUNK (< 11) / GOLD, keys KEY / KEY_RING (> 1), food MEAT (≥ 100) / FRUIT /
BADFRUIT (< 0) / BADMEAT (< −99), MAGIC, SPECIALS (5–9), RUNESTONE,
LEGEND, CRYSTAL, GOLDNICON; it slides up from y 0x180 by vb_elapsed to
0x130 (picture 0x10 lower), stays 90 vblanks and slides down past 399. An
unknown kind returns from the whole function (kept). Checked natively with
GDL_DEBUG_GOTIT=1:50 (the "Treasure" panel over player 1's box).

## Option menus (show_optmenu)

Menu records (0xe8 bytes; title_menu 0x21ce30, title_options, tower_menu,
game_menu, tower_options, game_options, audio_options, quitgame_question,
quitlevel_question, hint_menu, hint_general_menu, …): +0 optmenu_active,
+4 title scale, +8 title, +0xc/+0x10 item x/y (−1 centred), +0x14/+0x18
measured size, +0x1c entries (0x24 each: label, action, gap, value string,
colour, +0x20 < 0 disabled), +0x20 flags (button row 1/2/4/8, 0x10
"Player n", 0x20 fade over OPTMENU_FINISH_FRAMES 30, 0x40 FONT32_PARCH
glyphs, 0x80 FONT32GAR0–5 intro burn, 0x200 entry colours), +0x24 button
row y, +0x28/+0x2c/+0x30 off/on/highlight colours, +0x34 scale, +0x38 icon
tree (ICON_ARROW), +0x44 background (SCROLL_A) and rectangle (margins 64),
+0x58 burn (LOGO_BURN1–5, frame (frames >> 3) % 5, z 1.0). Items use
OPTMENU_FONT 6 (font32); the selection (optmenu_choice_twice) draws a
FONT32_GLOW layer (flag 0x4000, highlight colour, alpha pulse over
OPTMENU_FADE 40 / hold 5) then the "on" colour in plain font32;
unselected PARCH items are white. Background z OPTMENU_SCROLLZ − 1
(63899). gdl_core::game::optmenu draws them; the client maps its pages to
the records (menu::page_record) and keeps its own input logic. Not drawn:
the ICON_ARROW selector (an animated powerup tree placed with
MBScreenToWorld at depth 1.1). audio_options' sliders follow
position_audioslider (empty_bar 264×32, pink_bar 264·v/255, marker_left
−52, marker_right +264−24, the knob at fill − 0x14; unselected alpha
100/150/160); Mono/Stereo is drawn but not selectable natively.

## Title screen

init_titlescreen 0x1841a0: TITLE00–03 blits at screenblitxy (0,0),
(256,0), (0,256), (256,256) (256×256 over 256×128: the 512×384 screen),
titleblit_z −1000 (MBBlitSetZ stores negative z as raw GS units 0x1000000 +
z: far behind); titleglow = GLOWCROP_00 (TITLE OBJECTS name table; the
GLOWCROP_ header record precedes the frames) at (0xc0, 0) 0x80 × 0x80,
frame (titleglow_frame >> 2) % 10, fading in over 60 vblanks, shown while no
menu or the title menu is up. do_titlescreen draws DrawGlowText(1.0,
"Press Start") at y 0x140 until the title menu opens. Native: the
attract-mode timeout and the flyby are not ported.

## Sumner's hints

HintMenu opens hint_menu ("How Can I Help You?"); DoOptions case 0x16 picks
the next hint of the category (next_general/boss/legend/rune_hint) and opens
hint_general_menu (flags 0x71: fade, PARCH, "Player n", Back) titled "A
Hint for You" (general; the other categories use a HINTS_E.ROM line as the
title); case 0x17 draws the hint with DrawScrollListText(hints, x −256,
font 6) at DrawStringScale = the menu scale (0.667) in the menu's off
colour. NATIVE: the client pages through every hint of a category and
shows the hint_menu label as the title of the non-general categories;
next_*_hint's progress-based choice is not ported. GDL_DEBUG_HINTS opens
the menu for presentation checks.

## Player select

init_player_select 0x1851b0 creates 11 sel_bg blits per column (sel_xpos
0/128/256/384, sel_bg_pos {x, y, z}): S1_plyrN/S2_plyrN (z 9), S12_WEAP
(z 8, height 0x140, set_selected_blit), S12_<CLASS>_<COL> (z 7,
update_class_spec), selscrn_questmark, <CLASS>_NAME (8, 272) and the
S1/S2 borders (z 3). setup_sel_menu copies sel_def_menu (scale 0.667,
title-menu colours) with entries New/Load (sel_new_entries),
Save/Change/Load/Quit/Done, Yes/No, or the file list (case 8: x + 8, y
0x46, scale 0.6). do_sel_menu draws the hint rows (L/R "Change"; name entry
"Enter Your Name" with U/D Change, L/R Edit, X Accept, △ Cancel).
gdl_core::game::select draws a column for the client's Select / Class /
Colour / Name pages. NATIVE (labelled): the memory-card states and texts,
the four-player join/ready flow, serve_blits' effects, the class stats
icons and the Tower scene behind the columns (the title is shown instead);
the save list shows each save's name.

## Native presentation

A HUD camera with a fixed 512×384 orthographic projection on its own render
layer draws the frame's blit list as pooled quads (blit.wgsl: MODULATE,
alpha test, standard blending on display values, bilinear, linear clamp),
sorted far to near by z. The front end has its own STATIC-backed Hud. Textures are decoded lazily per OBJECTS record (format 146 with the
white alpha CLUT). Samples are clamped half a texel inside the crop: at the
original 640×448 frame edge taps never reach the neighbouring glyph cell.
MBOX search order (STATIC, the character package, the level's packages) is
a presentation choice; the panel labels are unique across them. Verified:
the Tower panel against tower-newgame-v1-240.png (layout, tints, fonts).

## Open

button_x_tex/button_tri_tex (ResetModels), NumLivePlayers, del_player_blits,
low_res flags, the shadow/0x1000 text flags, menu_on/optmenu state from the
front end, ability blits, SandglassBlit/ThermoBlit (boss and timers), the
inventory/map/shop panels and DrawBlit's corner_hilite colour sweep are not
ported. Help sites whose original point is a local vector (ids 0x1b, 0x14,
0xe) use the player's attention point or none; AudioHelp/AudioExp cues are
emitted as events but not voiced; low-res/Japanese help variants are not
modelled. The scroll box's timing uses wall-clock vblanks (59.94 Hz).
