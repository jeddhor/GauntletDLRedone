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
  EnablePlayerControls first (disable flag 0, ClearAllPlayerControls(4));
  each loop pass runs PlayerControls; after 15 vblanks a page ends on the
  triangle edge (PlayerControl +8 bit 0x8000000, entry 0xc) of an active
  player (+0xfc) in the mask, or at once when no masked player is active;
  ClearAllPlayerControls(2) after each page (unless AuxLevels & 8); the end
  clears every player's +0x93e and ClearAllPlayerControls(4). Cross does not
  close a scroll. Native: Game::scroll_box_open/pressed/page_end/close,
  stepped by gdl_play's step_message_box with the real pads.
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
the records (menu::page_record) and keeps its own input logic. The
ICON_ARROW selector (start_optmenu_nostack: MBNewNode parent, POWERUPS
tree via AtreeMatch(powerup_anim), scale +0x3c 0.05; show_optmenu: screen
point (item x — centred: −width/2 − x — + icon x +0x40 −16, the selected
entry's drawn y +0x14 + FontHeight/2), MBScreenToWorld at optmenu_icon_z
1.1 (window 640×448 over the 512×384 screen, pb +0x90 × viewport scale),
CopyMat3 of the camera inverse, PitchMat3(π for odd selections + π·t /
OPTMENU_ICON_TIME 15 while +0xd8 glides from +0xd4 to the new entry),
AnimateATree) is drawn by gdl_play with an offscreen camera over the whole
frame and composited above the menu blits. HYPOTHESES: the front-end
window values (only in-game ones are captured; placement round-trips
either way) and AnimateATree's rate (sequence 0 at its timing factor). audio_options' sliders follow
position_audioslider (empty_bar 264×32, pink_bar 264·v/255, marker_left
−52, marker_right +264−24, the knob at fill − 0x14; unselected alpha
100/150/160).

### Leaving a menu (end_optmenu, finish_optmenu, the fire scroll)

Read from the instructions (2026-10-23; ghidra-optmenu, ghidra-scroll1,
ghidra-menuaudio1). start_optmenu(menu, player) with a menu pushes:
end_optmenu(1, 0) on the current top (its background blit +0xdc removed;
with flag 0x20 it keeps drawing through finish_optmenu, +0x70 counting
vblanks to OPTMENU_FINISH_FRAMES 30, its text fading; else removed).
start_optmenu(0) pops: end_optmenu(−1, 0). DoOptions' −1/−2 results pop
and play AudioMenuExit; quit_game / quit_level end every level with
end_optmenu(−1, −1); end_all_optmenus (Manage Character, Shop, Inventory's
end) uses end_optmenu(−1, 1): removal without fade or burn. A pop to a
menu without a background (+0x44 0) or with none below, and every
end_optmenu(−1, −1), start the fire scroll over the closed menu's
background: StartFireScroll(z 0 → 1.0, +0x44, −1, +0x48..+0x54) — the
derived rectangle start_optmenu_nostack wrote back — and MBBlitOrder of
the +0xdc blit behind its z mask. fscroll_descs 0x220448 holds four
identical entries (greenCircTrans / greenCircTransM), so the type cycle
(last_fscroll_type) changes nothing. ServeFireScroll runs at the top of
DoOptions every frame (21 frames of two vblanks, ClearAllPlayerControls(2)
each); DoOptions returns nonzero while the last menu finishes
(optmenu_active 1 → finish_optmenu) and then while the fire is served, so
main pauses the game until both are done. Native: gdl_play's MenuEnding
(end_menu / serve_menu_ending; the sim's pause gate includes it; GUI flows
menu-burn and pause-burn).

Sounds (do_optmenu 0x10c4f8 and DoOptions): up/down in a menu with
entries AudioCursorV; accept AudioCursorSelect when the menu has flag
0x400 (title_options, tower/game menus and options, hint_menu, the
submenus and choice menus) — the title's Options plays it explicitly —
and AudioBuzzer on a disabled entry; triangle AudioMenuExit only with flag
0x800 (sel_def_menu); a DoOptions pop AudioMenuExit (not after the audio
menu, which plays AudioPlayerStart); a stored choice AudioPlayerStart;
Control Style left/right AudioCursorH, its accept AudioPlayerStart;
Screen Adjust's accept AudioPlayerStart. Native: menu::Cue, played by
play_menu_cues.

Choice menus (DoOptions 0xa..0x11) store and mark the accepted entry and
stay open until Back. Control Style ignores triangle (−1 → 0); Select
stores the style and pops. The audio menu's entry actions (0x17..0x19) do
nothing on accept. OptionsStart 0x10d3e0 (START in play): tower_menu or
game_menu — game_menu's Quit Level (0x26) disabled (+0x20 −1) when
CurWorld is 0xc — then AudioStopAll (queue, tracks, every SFX voice).
game_menu's Quit Level opens quitlevel_question ("Abort Level?", No −1 /
Yes 0x26), like the Tower's Quit Game (quitgame_question). While a menu is
up DoOptions calls LowerBGMusic(0.1, −1) (the music eases to 0; the audio
menu LowerBGMusic(0, 1)), and game_main keeps running BGMusicUpdate in
mode 0x4010 while paused (ScrollMessageBox runs it in its loop).

Audio menu (case 8, do_audiomenu 0x10cc00): Music Volume (0x17), Sfx
Volume (0x18), Mono (0x19, value "Stereo"; +0x1c = stereo + 1 marks Mono or
Stereo, +0x20 = stereo moves the highlight to the value). do_optmenu
returns 3/4 for a left/right edge and 5/6 while held, so a slider moves by
vb_elapsed_menu per frame (0..255). Music: AudioSetVolMusic +
AudioTuneUpdateVol(0x80). SFX: AudioSetVolSfx on a change; at an end, with
sfx_sound_count ≥ 0x10, AudioVolSfx(1) (AudioSFX2(0x14, 0x7f, 0x7f, 1));
resting on Sfx Volume with the count ≥ 0x3d, AudioVolSfx(0): snd.286
0x23d438 [idx.285] (the two ELF words, alternating). Mono's edge:
AudioCursorH, +0x20 ^= 1 (GamePrefs word 2), AudioSetStereo. Native:
Menu::hold each frame with the held d-pad; GamePrefs are stored when the
menu ends.

## Screen saver (ScreenSaver 0x12acf8)

Read from the instructions (2026-10-23; ghidra-saver1..3). main calls
ScreenSaver before DoOptions every frame. Outside game_mode & 0x8000
(attract, title), 0x400e (movies) and 0x4015, once the world is loaded,
saver_timer counts vb_elapsed_ctl and resets whenever a player's
PlayerControl +4 level word differs from saver_levels; at 36000 vblanks
(ten minutes; AuxLevels 0x100000000 forces it) ScreenSaverStart runs:
AudioStopAll, AudioStreamStop, optmenu_active saved and set to 100, every
node except types 7/9 hidden (MBTreeSetFlags 2), saverroot, and the four
weapons. ScreenSaver then loops on its own — ClockOncePerFrame,
ScreenSaverUpdate (DoTexMods for powerup_anim, ScreenSaverUpdateWeap ×4),
PlayerControls, MBEndFrame — until a level word changes, so the game is
halted; blits (the HUD panels) keep drawing. ScreenSaverEnd removes the
weapons, shows every node, ClearAllPlayerControls(−2) (ClearControls 0x104088
first: plr_repedges/edges/levels, right_jstick, old_plr_levels), restores
optmenu_active, and in mode 0x400b AudioSelect(1), in 0x4012 ShopMusicStart.

Weapons (ssave_weap 0x2c12c0, 0x90 bytes; POWERUPS trees ssave_desc
SAVER_AXE / SAVER_HAM / SAVER_SWD / SAVER_WND): InitWeap = CreateWeap,
ResetWeap, delay k × 30 + RandInt(15) + 1. ResetWeap: position
ssave_initpos (±20 in x or y, z 1), timer 0, delay ssave_delay 3 × (Random(1)
+ 0.5) × 60 + 1, end ssave_endtime 10 × (Random(1) + 0.5) × 60, node removed.
StartWeap (once the timer reaches the delay): node (re)created, position,
normalised ssave_initdir, pitch 0, bounce 0, +0x34/+0x38 = ssave_speed /
ssave_dpitch × (Random(0.2) + 0.9) (unused by UpdateWeap), delay 0, shown.
UpdateWeap: position += ssave_speed 10 × vb/30 × direction; pitch −=
ssave_dpitch 2π × vb/30 (wrapped to (−π, π]); node rotation
CreateDirMatrix(direction) · PitchMat3 · the window matrix (MboxWinPtr
+0x70), position = MulVec4Mat4(position, that matrix): positions are in
the window camera's space. Clip code: 5 beyond ssave_max_z 40, 6 nearer
than ssave_min_z 5, else MBWorldSphereClip(−1, world position) (6 near, 5
far, 2 right, 1 left, 3 top, 4 bottom). Bounce state +0x24: 0 waits until
the weapon is inside (code 0 → 1); armed (≥ 1), a nonzero code turns it —
1/2: x = ±(Random(0.5) + 0.5), y += ssave_rand 0.1 × (Random(2) − 1); 3/4
the same with y and x; 6: z = Random(0.5) + 0.5; 5: z = −(Random(0.5) +
0.5) and past its end time ResetWeap — then NormalVector and −10; a
negative state becomes vb the next frame. AnimateATree each update.
Native: game::saver (the game RNG; presentation arithmetic, not compared),
gdl_play's simulate (the halt) and present_saver (the scene camera shows
only SAVER_LAYER; HYPOTHESIS: the trees play sequence 0 at their timing,
the window matrix is the camera basis and eye). NATIVE: the pause menu's
text is not hidden while the saver runs over a paused game (the original
keeps only its persistent blits). GUI flow saver (GDL_DEBUG_SAVER
shortens the count; menu-flows-m17-v7).

## Title screen

init_titlescreen 0x1841a0: TITLE00–03 blits at screenblitxy (0,0),
(256,0), (0,256), (256,256) (256×256 over 256×128: the 512×384 screen),
titleblit_z −1000 (MBBlitSetZ stores negative z as raw GS units 0x1000000 +
z: far behind); titleglow = GLOWCROP_00 (TITLE OBJECTS name table; the
GLOWCROP_ header record precedes the frames) at (0xc0, 0) 0x80 × 0x80,
frame (titleglow_frame >> 2) % 10, fading in over 60 vblanks, shown while no
menu or the title menu is up. do_titlescreen draws DrawGlowText(1.0,
"Press Start") at y 0x140 until the title menu opens. The attract-mode
timeout and the flybys are ported (Movies section).

## Sumner's hints

HintMenu opens hint_menu ("How Can I Help You?"); DoOptions case 0x16 picks
the next hint of the category (next_general/boss/legend/rune_hint) and opens
hint_general_menu (flags 0x71: fade, PARCH, "Player n", Back) titled "A
Hint for You" (general; the other categories use a HINTS_E.ROM line as the
title); case 0x17 draws the hint with DrawScrollListText(hints, x −256,
font 6) at DrawStringScale = the menu scale (0.667) in the menu's off
colour. NATIVE: the client pages through every hint of a category and
shows the hint_menu label as the title of the non-general categories;
next_*_hint's choice is ported (see Sumner's hint pad below). GDL_DEBUG_HINTS opens
the menu for presentation checks.

## Inventory (draw_fullscreen_inventory)

draw_inventory_panel 0x129668 per column (x 0x80·p): Panel_Pieces
(s1_plyr/s2_plyr + player number, S1/S2_border), I_Pieces (window_empty,
fangs/feather/claw, eight crystals; +0x10 text scale, +0x14/+0x18 text
offset), Quest_Pieces (legend bits class +0xcf4; "_empty" when not held),
Shard_Pieces (class +0xcf0 | player +0x1a24), all at z 3; print_n_of_m
(IFONT_FONT 6) for garg items (class +0xd04/+0xd06/+0xd08 of 12/20/28) and
crystals 1–8 (MAX_CRYSTALS); animate_panel_piece mode 0 swirls them in over
120 units (SWIRL_RATE 1 per vblank, swirl_mult 60, cos/sin_table 60
steps, size ×(2 − f²), alpha); "Continue" glow text with the X button and
"Inv". Tables are read from the ELF. Leaving (2026-10-22):
end_fullscreen_inventory 0x129f50 / end_inventory_panel 0x129d78 set
panel_fx 2 with timer 0 and draw until draw_inventory_panel reports done
(timer > 14; the timer advances by SWIRL_RATE (1) × vb_elapsed_ctl after
each draw): animate_panel_piece mode 2 moves each piece out from (0x40,
0xb4) by (base − that) × 10f, scales it by 1 + 5f and fades it (f = timer /
15), the counts fade by f × 255, no "Continue". The Tower menu's inventory
(InventoryClose) and do_shop's (case 10 → end_inventory_panel → case 11
until done → remove_inventory_panel) both play it; the shop's panel also
opened at twice SWIRL_RATE before (fixed). GUI: x11grab frames of the
Tower inventory closing.

## Shop (shop_setup / write_shop_menu)

Per column (shop_lx 0/128/256/384): S1/S2_PLYRn (z 64000), S1/S2_BORDER
(z 63900, corner highlight), SHOP_TOP_<pcol> (z 63800); shop_misctab
SHOP_SCROLL_1/2 (z 64000 − 20) for the shopping player. SHOP.WAD ITEM
records (0x50: +0 icon, +0x20 description, +0x40 scale, +0x48 price):
calc_shop_ypos stacks them (icon 0x18, description lines at scale × 0.5
+ 0x10) relative to the selection; write_shop_menu fades items within
shop_dim_dy 64 of shop_topy 72 / shop_boty 224, prints prices at shop_rx −
0x40 (font 6 × 0.5) and descriptions centred on shop_cx, the selection
with DrawGlowText, MORE_UP/MORE_DOWN when cut off. NATIVE: items jump to
their places (no scroll speed), no skip list / highlight timers, the
stats/level panels (shop_show_*) are not drawn.

## Returning to the Tower: the shop (game mode 0x4012)

game_main sends every exit to TowerWave (0xd00) and the all-dead end of a
level (players in state 0/0xb) through init_shop(0); do_shop's result 2
leads to init_player_select(2) (NATIVE: straight into the Tower) and the
Tower's init_next_level. init_shop skips the shop for skip_wave > 1 (not
0xc) or without a player in state 1/5, otherwise ResetModels,
setup_player_display and shop_setup (S1/S2_PLYRn, borders, SHOP_TOP_<col>
from SELECT). Per player (+0x980): 0 init_piles — gold (+0x1a20 − class
backup +0x30), kills (+0xb2c/+0xb3c − backup) and experience (+0x1a1c −
backup) scaled against LEVL +0xe0/+0xe4/+0xe8 + 1 into 0x40..0xd0 and
ordered; 1..3 show_piles (SHP_GOLD/BONES/EXP growing 1.5 × vb per frame,
"Gold/Kills/Exp.: n" at pile_ypos, Continue on X); 4 shop_show_lv (only
after a level-up: Level n, rank, the four attributes and max health at the
old level with lines highlighted every 0x3c frames from 0x5a; levels
25/50 GameText 0xb8/0xb9); 5 init_show_gold; 6/7 the shop (show_gold
shrinks the gold pile, do_shopping: up/down over affordable unskipped
items with wrap at the afford count, X buys, back (turbo) returns to EXIT);
8 the stats after shopping (0x1e); 9..11 the inventory panel; 0x14 the
final stats after Battle 3 (LastWorldPlayed 8 / LastLevelPlayed 3); 100
done (player state 5). Input: PlayerControl +8 edges (X 0x2000000 = attack
entry 0xd, back 0x8000000, up 0x30, down 0xc0). gdl_play draws the shop
over the realm's scene (the panels cover the screen) and adds the SELECT
package last to the HUD's texture search (SelectLoadStart). game_main runs
SetPlayerVars (which clears the panels' +0x93e written bits) in every mode
before its dispatch, and do_players after an unfinished do_shop /
do_mapscreen and before do_player_select (unless paused); gdl_play's shop,
map, select and movie branches follow it — the shop's player panels had
lost their level, name, gold and health (written once, then skipped),
against the reference's tally (private/reports/ref-g1-exit-stand-v1.png:
warped into G1's exit portal, 600 vblanks; the counting row highlighted). Verified
natively (Shift+F9 debug exit from G1): piles, Level 10 panel, item scroll,
inventory, Tower start at the realm's door. NATIVE/HYPOTHESES: the list
does not scroll, the inventory panel closes as the reverse of its opening,
the final stats' "%d Days" argument (missing in the decompile) is the
whole days. The Tower menu's Shop runs init_shop(1): TowerSetStartPos
(ClosestStartPos 0x150958: the nearest start locator 0..13 of an open
realm to the players' average position), PlayersRestoreHealth, the shop
from step 6 (no piles), stats, then init_next_level into the Tower at
that start. DoOptions opens a menu only from PlayerControl's Start edge
(so not while controls are disabled), after WelcomeFlag < 0 and outside
the stage start; gdl_play's pause key follows the same gates.

## Option submenus and their effects

DoOptions 0x10d4e0 cases 0xa..0x11: the choice menus mark the current
value (entry +0x1c = 1 → " ~", the font's check mark, after the label)
and store the accepted entry: compass_options (Hide 0x20 / Show 0x1f) →
GamePrefs +0x18; gameoption_options → difficulty_options (+0x14, the
diff_mul_tab index ResolveWorldData uses) and multiplayer_options (+0x1c,
pvp_mode); controls_options → control_rumble_options (PlayerControl
+0x30), control_autoaim_options (+0x34) and control_autoattack_options
(+0x38; On = entry 0 → 1), saved with the character. gdl_play keeps
GamePrefs in options.json and applies difficulty/multiplayer at the next
level start. UpdateCompass: POWERUPS' COMPASS object (StartCompass under
the world root, world-aligned) at MBWindowTo3D(CompassZ 10, CompassPos
(64, 128)), scale 1.5, alpha 128, hidden in menus or with the option off
(HYPOTHESIS: the window point over the 640 × 384 viewport). Not ported:
Control Style (do_controlsmenu), Screen Adjust (pbScreenSetAdjust).

Vibration (2026-10-22, gdl_core pad.rs): damage_player's do_vibe 0x1069a8
(strength 3 / 0x1e frames for flags 0x10040, 2 / 0x14 for 0x120, 1 / 0xf
for 0x90, else 0 / 10) with the vibration option (PlayerControl +0x30, the
control_rumble_options value) on: joyBeginShake(vibe_inten 0x211468
[strength − 2 + option, ≤ 4] = 0.2…1.0) on joy_remap's port, vibes[p] =
(1, frames). PlayerControls' serve_vibes 0x106a90 (controls enabled, after
ReadControls) counts frames down and calls vibe_off → joyEndShake below 0;
vibrators_off 0x106b10 on ResetModels and TriggerCameraActivate.
joyBeginShake sets only the large motor (actuator byte 1 = intensity ×
255). Host: the requests drive the port's gamepad's strong motor
(GamepadRumbleRequest); a new game stops every motor. main runs
PlayerControls before game_main, so a vibe started before the host's
update_controls is first served in the next update. Actual disc headless:
game-run-rumble-G1-v1.json (begin/end 11 updates apart for strength 0).

## Boss health meters

CritterNewInst calls CritterAddHealthMeter for the root and each child:
a type with +0x5c bit 4 gets HealthMeterStart(health, type, +0xf8 pieces,
+0xfa width, +0xfc/+0xfe insets, +0x5c bit 8) (at most 3 meters; the index
at critter +0x484): "<TYPE>_METER_BG<n>" (with bit 8; advancing
HealthMeterX by the width) and "_FG<n>" blits at (x + 256·(n − 1), 0),
alpha 0x70. ProcessCritter's CritterUpdateHealthMeter moves the shown value
towards the health by 3 × vb_elapsed and sizes/crops the foreground
(value/max × (256 − insets); two-piece meters fill the first then the
second); the backgrounds are tinted 0x8080ff while the boss's +0xb4c
quest-item time runs. Verified natively in G5 (the lich's bar). Not drawn:
the in-world meter tree (+0x5c bit 0x800).

## Rune thermometer and wave timer

init_thermometer 0x1244b8 (init_next_level) shows THERMBASE at (0x188,
−1) and THERMCOL (z 63912/63911) in a level with a rune item when a
present player holds shard GetWorldOrder(5) = bit 9 (world 8: the class
record +0xcf8 bit 0x1000) and no present player holds the rune;
modify_thermometer 0x124760 every frame: f = clamp((|RunePos − Cam[0]
attention| − 8) × therm_const, 0, 1) with therm_const = 1 / (|world
extent| × 0.7), f = 1 − (1 − f)², column height round((1 − f) × 73) +
0x1b at y 0x66 − that, crop v0 (101 − h)/128; AudioRuneGetCloser below
0.25 and AudioRuneIsNear below 0.025, once each. Verified natively in G1
(GDL_DEBUG_ANY_LEVEL=1 GDL_DEBUG_OLD_SHARDS=3fe). init_wave_timer /
do_wave_timer need LEVL flag 4, which no shipped level sets.

## The realm map between levels (game mode 0x400f)

game_main sends a normal exit to init_mapscreen(0x78, skip) (skip_wave in
0xd..0xffff skips the map); do_mapscreen then init_gamemovie (movies are
not ported) and init_next_level. ResolveWorldData sets LEVL +0x68 =
world_data +0x30 + LEVL +0x5c × 0x48 (WDATA MAPS chunk: glow x, y and up
to eight dash x, y; negative x ends). With a record: LoadModel
maps/level<code>, map_<code>_00..03 at screenblitxy (0,0)/(256,0)/(0,256)/
(256,256) z 64100, ldmap_<code>_0n above (z 63950, alpha 0xff), dash
_<code>_n appearing every 0x1e vblanks (AudioMapDot), then
map_<code>glow pulsing (mapglow_fade 60, hold 10) for mapglow_tottime 210,
after which the glow fades out and the ldmap layer in at 4 per vblank over
map2_tottime 180; "Loading..." (glow, 1.0 at 0x154, 0x140) and 60
vblanks later the level starts. Without a record: TRANSITION_SCREEN
(0, 0, 0x200, 0x140) for 0x78 vblanks. Verified natively (F9 from G1): the
Forsaken Province map, path to Town, glow and the Town picture.

## Sumner's speeches and captions

SumnerLevelSpeechStart queues a speech for players with a new level tier
(0.5 s, DisablePlayerControls, CaptionTextReset). SumnerDoSpeech 0x1750a0:
Sumner's camera (SumnerCamActivate: table 0 for level speeches, 1 shard, 2
rune; index = the nearest waypoint's +0x72) and the ghost at that waypoint;
after 120 vblanks each player's caption — GameText 0x18 ("%s %s is now\na
level %d %s!") with the colour (0x16), class (0x17), level and rank (0x15
or the class list) — through CaptionTextSub 0x165290 at y 0x138, scale
0.667: characters revealed at 1.75 frames each ('.'/',' +2, tab 5, '\r'
30 + page), the partial line re-centred on x 256, white over a shadow.
GameText's FONT '8Hi_fonts5' matches no font_desc name, so StringInitSub
leaves font 0 (font8x8). The text (font z 0.25) draws over the letterbox
(z 0.5), which is now a z-sorted blit pair (AAAWHITE, colour 0).
A player's speech ends once the caption is revealed, 0x168 frames passed
and AudioSoundPlaying(AudioExp's line) is clear (audio_queue track timing,
2026-10-21). During it
StartLevelUpFX (LevelUpEffects 0x2407e8: headers 0x39 + colour) after 0xf0
frames and StartGemFX (gem_color.39 0x23f588 = 6, 4, 2, 5) after 0x10e,
both parented to the player. GDL_DEBUG_EXP=<exp>:<update> sets player 1's
experience for checks.

Shards and runes (TowerCheckMessages 0x175e88 with param 1 from
EnterTower): per present player the live bits (+0x1a24 shards, +0x1a26
runes) are ORed into the class record (+0xcf0/+0xcf2) and its backup
(+0x1d7c/+0x1d7e); the lowest shard bit 1..8 not yet in the backup starts
SumnerShardSpeechStart(n) (3 s, speech index n), the lowest new rune bit
0..12 SumnerRuneSpeechStart(n) (index 100 + n); without a new rune, the
last played level being Battle 2 (LastWorldPlayed 8, LastLevelPlayed 2,
set by LoadGauntWorldStart for every non-Tower level; title: 0xd/0) without
rune 13 gives 0xd (Rune13No), and twelve runes plus a new shard 9 gives 0xe
(SumnerSpeechEnd at once). Captions are CaptionText 0x165568 over
ScrollText[0] (SCROLL_E): NewShards line n, MoreShards, AllShards,
NewRunes line 0, Rune13No, All12RunesNo, All12RunesYes, Rune13Yes; −1
lines page with caption_delay 0x3c once revealed (scale = the entry's
× 0.667, font = the entry's). Speech sounds: AudioShardSpeech (shard_sidx,
0xe00ac, 0x3000b) and AudioRuneSpeech (0xe00a4/a8/a5/a6/a7), queued.
After the caption the speech time runs out and SumnerSpeechEnd 0x174cc8:
ghost removed, TriggerCameraEnd, controls back, then by index — SHARD<n>
custom effect (InitCustomEffect over weapon/powerup/item packages; flag
0x80000 keeps it after its end) at L1WINDOWFRAME, WindowCamActivate(0),
L1XPLIGHTRAY01 hidden, all eight shards → the Temple exit (FindExit
0x500) transparent and event 10, else event 0xe; RUNE<n+1> at L1RUNEPLACE
with RuneCamActivate(0); the Hell exit 0x600 transparent; event 0x14/0x15
(nine shards and twelve runes), 0x18 (twelve runes) or 0x16; index 0x70
(rune 13) RUNE13 at L1RUNE13, rune camera 1, Battle 3 exit 0x803, event
0x1e/0x20. sumner_event_flag steps (each waits for the trigger camera's
delay and hold below 5, or plays AudioTowerFX when the hold drops below
0xfa/0xdc): 10→0xb (window camera 1, the exit and the light ray fade in
over 0xb4 vblanks: flag 0x74 counts down by 100 → 0x10 → AllShards);
0xe→0xf (MoreShards); 0x15 (rune camera 0, Hell exit fade 0x7e → 0x1a →
All12RunesYes); 0x18→0x19 (All12RunesNo); 0x1e→0x1f (rune camera 2,
ActivateSpecialTrigger(0xff), Battle 3 exit fade 0x88 → 0x24 → Rune13Yes);
0x16/0x20 → 0x17/0x21 → 0. Follow-up speeches last 2 s. EnterTower shows
every delivered shard/rune effect at its last frame (start ATime −
frames/30). Verified natively (GDL_DEBUG_SHARDS=2, GDL_DEBUG_RUNES=1,
GDL_DEBUG_OLD_SHARDS=1fe GDL_DEBUG_OLD_RUNES=fff): NewShards/MoreShards
captions with paging, the window and rune cameras (rune camera 5.67 s =
40 + 300 vblanks), the twelve runes in the rune circle and the light ray.
Not verified against the reference: the ghost (WIZARD tree, presented at
the waypoint; not seen in the Sumner camera's view) and the window
camera's SHARD effect.

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

## Control Style (DoOptions 0xd/0xe, action 0x21)

controls_options entry 0 starts controlstyle_options (0x21e408) with
control_style = PlayerControl +0x2c and start_controlsmenu (three
opt_ctlblit blits: CONTROLER_1 (96, 104), CONTROLER_2 (352, 104),
CONTROLER_3 (128, 232), texture size). Each frame the entry label is
opt_controls_desc[style] (Default, Arcade, One Handed, Robotron) and
do_controlsmenu draws the sixteen lines of GetStringListMsg(3, style)
(ENGLISH.ROM list CONTROLS_DESC → CONTROLS1–4, font 2, scale 0.4) in
OPTCTL_FONT −1 (the entry's) at OPTCTL_SCALE 1.0 with stringtext_dy
OPTCTL_DY 2, colour optmenu_rgb_ctls (black), at ctl_label_pos (alignment
0 left / 1 centre / 2 right of 256 + x; y centred). Left/Right step the
style mod 4; Select (0x21) or Back stores it in +0x2c and
end_controlsmenu. Native (gdl_core::game::optmenu::ControlsMenu,
menu::Page::ControlStyle) and seen in gdl_play; the remap itself is
update_controls' +0x2c set (game/pad.rs).

## Screen Adjust (DoOptions 0x12, options entry "Screen")

DoOptions case 0x16 loads screen_dx/dy from GamePrefs words 3/4
(0x21edc4/0x21edc8) and starts screen_options (no entries; flags 0x17b:
Back, Select→Accept, Center). do_screenmenu draws NewTempBlitZ(1.0)
AAAWHITE corner marks (16×2 and 2×16 at the four corners of the 512×384
screen), a centre cross ((0xf0, 0xbf) 32×2, (0xff, 0xb0) 2×32) and the
d-pad icons BUTTON_U/L/R/D around it. Left/Right: dx ∓/± 4 clamped to
±0x80; Up/Down: dy ∓/± 1 clamped to ±0x28; Center (2): 0, 0; each change
calls pbScreenSetAdjust(dx, dy) (pbFrameMode adds them to the GS DISPLAY
DX/DY fields); Accept stores them in GamePrefs, Back restores the saved
values. Native: menu::Page::Screen; gdl_play shifts the final
DisplayToLinear pass by dx/4 of 640 columns and 2·dy of 448 rows
(HYPOTHESIS: MAGH 3 VCK units and frame-mode raster lines) and applies
the saved values at boot; keyboard C is Center (NATIVE).

## Audio cues (2026-10-21)

The Audio* functions that were emitted as named placeholders (silent in
gdl_play) are transcribed in gdl_core::game::sound_cues (addresses and
tables in its module doc): cursor/buzzer/click, Tower FX, tally loops,
rune pickup (with LowerBGMusic) and rune counts, the stage-entry lines,
map dots, Death's hurt and loop, heartbeat, wait-to-exit, praise, It swap,
halo/horn/mask/gauntlets/thunder hammer/breath, turbo defence and turbo,
footsteps by surface (water 4, shield 3, floor flags), weapon ricochets
(once a second), level-up and help lines (AudioExp by character × 9,
AudioHelp/AudioHasItem), legend-item sounds by world (jump table 0x2a90c0:
snd1–4.517–520), transport / world explosion / world hit by world, the good
wizard's lines (gwiz_sidx), the thermometer's rune hints. Background
music (stage areas and parts, map and shop music, boss attention) and the
kind-0xd world sounds are described in audio.md.

## Player-select screen (2026-10-21)

game/pselect.rs ports init_player_select 0x1851b0 / do_player_select
0x188228 with setup_sel_menu, do_sel_menu, do_optmenu 0x10c4f8, the column
blits (setup_tex, init_blit_fx, serve_blits, update_class_spec,
update_class_attr), the name entry (InitGetName / get_initials / DoGetName:
'0'..'9', '@' = end, 'A'..'Z', '_'; an empty name takes default_names),
set_hidden_player 0x12f500 (the 27 Hidden codes and Cheats), LimitSeltype
(Sumner 0x10 with +0x9a8 bit 8), useable_class (classes 8.. need +0x9a8
bit class − 8; locked ones show the "shadw" art and the pulsing question
mark), player_selected / change_player / init_player_change and the
memory-card states. Each column runs its own state at +0x2ad4 (New/Load;
Save/Change/Load/Quit/Done; quit question; name; class/colour; card;
format/create; no data; file list; loading; writing). Tables from the ELF:
sel_def_menu, sel_new/active/yesno_entries, sel_xpos, sel_bg_pos, the
sel_msg_* and sel_attr_* words, hs_name_column/cx, flash_pal,
saveFileSize 0x47400. Entry points: after the shop
(init_player_select(2): finished players at menu 1), the Tower menu's
Manage Character (DoOptions 0xd → mode 1), and game_main in the Tower when
an unassigned controller presses Start (check_active_players → mode 1).
Done (psel_done > 8 with everyone chosen and no blit effect running) →
init_next_level (the Tower). NATIVE: the host store is one formatted PS2
card in slot 1 whose eight files are store slots 0..7; its Gauntlet
directory exists once a file is saved or after "Create Save File"; free
space shows 8 MB less its files; keyboard letters type into player 1's
name. The picture behind the columns (castle, moon, fog) is the column
art itself — setup_tex's S1_plyr<n>/S2_plyr<n> halves from the "select"
package (SelectLoadStart → MBOX_BGLoadModelStart("select")); nothing in
init/do_player_select draws a tree. Actual check (2026-10-22):
reference-select-screen-v1.png (Tower → Manage Character) against the
native select screen — the same castle art in the inactive columns. The
fixed camera of modes 0x8009/0x400b/0x4012 (main's MBCameraUpdate) only
moves the 3D view hidden under these full-screen blits. GUI check: A1
exit → tally/shop → player 1 at Save…Done → Done → the Tower; in the Tower
player 2 pressed Start (U), chose New, the default name JUSTIN and the
Jester: the Tower reloaded with both players, each with its own model and
panel.

The title's Start (do_titlescreen → init_player_select(0)) builds the
Tower behind the select screen with player 1's port active; that build's
EnterTower does not count (TowerCounter restored: the original enters the
Tower in init_next_level after the select). Loading a file returns to menu
1 with Done chosen (+0x2ac4); Done enters the Tower. GUI check: title →
Start → Load → SLOT 1 → TEST → Sorceress → Done → the Tower and Sumner's
welcome. DEBUG: GDL_DEBUG_NATIVE_SELECT keeps the native save list (and
the PS2-capture import).

## Movies (2026-10-22)

init_gamemovie 0x1657d0(param) sets game mode 0x400e, BGMusicStop and
AudioStopAll; unless NoMovies (0), AuxLevels 0x10 or a skip_wave it opens
"/MPEG/victory.pss" (param 0x2c, movietype 2), "/MPEG/garm.pss" (0x2b,
movietype 2) or the level's own LEVL +0x34 name (movietype 1). Callers: the
map screen's end (LoadWorldData of the destination, then init_gamemovie —
the realm movies movieA1.. and the legend-item movies iceaxe, javelin,
bellows, scimitar, soulsave, fscroll, lamp, lantern, goodbook play before
their level) and game_main's BossType 0x2c/0x2b level end.
do_gamemovie 0x165908: an accept edge (active_player_edge 0x2000000, any
player not in state 0) kills a movietype-1 movie; when it ends, movietype 1
→ init_next_level(0), 2 → init_shop. LEVL +0x34 names per realm (actual
disc, gdl_tools levl-list): A1 movieA1, A2 iceaxe, A3 movieA3; B1 movieB1,
B4 movieB4, B5 javelin; C1 movieC1, C3 bellows, C4 movieC4; D1 movieD1, D3
movieD3, D4 scimitar; E1 soulsave; G1 movieG1, G3 fscroll, G4 movieG4; H1
movieH1; I1 movieI1, I3 movieI3, I4 lamp; J1 movieJ1, J2 lantern, J3
movieJ3; K2 movieK1, K3 movieK3, K4 goodbook. init_stats_display (0x4016,
"FINAL STATS") has no caller or reference in this build.

The PSS format and the decoder choice are in docs/decisions/0003-movies.md
(gdl_tools pss-inspect: all 36 files demux; private/reports/pss-inspect-
MPEG-v1.json, -MPEG2-v1.json). Native: gdl_core::game::movie, the client's
present_movie (full-screen HUD-layer quad, soundtrack on the music
channel). The realm movies are rendered with the map and empty player
panels in their first frames. HYPOTHESIS: the 480-line movie is shown over
the 448-line frame's full height (InitMovie sets a 640×480 interlaced
display; the native window keeps the game's frame).

Attract sequence (init_attract_mode 0x182708, attract_tab 0x249568):
Screen2D 0, movie table 0 (midway), movie table 1 (opening), movie table 2
(title2), the title screen, Screen2D 1, a flyby, movie table 3 (war_dwa /
val_kni / wiz_sor / jes_arc by attract_movie_idx), Screen2D 2, a flyby,
credits, movie table 4 (fail, only after a failed game), repeat; movies from
MPEG2 via init_movie/do_movie. init_attract_mode skips the credits unless
(attract_count & 0xf) == 0xf (or both right shoulder buttons — JoyLevels
0x400000 R1 and 0x800000 R2 — force 0x8000; checked in the reference
2026-10-23: R1+R2 held through the title's timeout start the credits, L1+R1
do not), the
table-0 screen once did_mc_check is set, the fail movie without a failed
game and the title after the first cycle; attract_movie_idx advances each
cycle (the class movies war_dwa, val_kni, wiz_sor, jes_arc in turn). Start
(new_start / attract_advance) ends any entry for the title screen
(ExitAttract → init_titlescreen); the title's timeout is
init_titlescreen_timer's 0x708 vblanks, counted while no menu is up, after
which init_attract_mode(-1) resumes the sequence. main calls
init_attract_mode(-1) at boot.

Ported (2026-10-22; gdl_core::game::attract, the client's run_attract):
the sequence, Screen2D table 0 (SONY "sony_screen00..03" at screenblitxy
during the card check: one state per 30 Hz frame over ports 0–1 × slots
0–3; the native card is port 0 slot 0, formatted with room, so the check
takes 18 frames and shows no message), Screen2D tables 1/2 ("group1"
Group_shot_midway00..03 for screen2dtimeleft 60 vblanks, with
DrawGlowTextMLines(1.0, −0x1b0, −0x140, "Press\nStart"): two DrawGlowText
lines FontHeight(glow_font) apart, centred on y 0x140 and x 0x1b0 — the
reference's lower-right Press/Start; NATIVE: the next flyby level loads
when the flyby starts, not behind the group shot), the MPEG2
movies, the credits (init_credits: CRED_ALL00..03, FontInitSpecial
("credits", 8) = font 0xd, AudioStreamStart("castle2b", 1); scroll_credits:
credit_text / credit_text2 / credit_text3 from y 0x17f − scroll, 21/15/15
pixels apart, the later lists at scale 0.833, MBFontMsgSetAlpha fades =
alpha 0x80 − value/2 over 16 lines at each edge, the scroll + 1 + finished
sections per frame; done when nothing is drawn or timer1 > 0x1517).
Synthetic tests: the boot cycle order, the forced credits, the 18-frame
card check. GUI: SONY screen, MIDWAY/OPENING/TITLE2, title; credits via
GDL_DEBUG_CREDITS. Not ported: the card-check messages for a missing,
unformatted or full card, the "Version" text.

Flybys (2026-10-22; gdl_core::game::flyby, camera.md "Old camera"): for a
flyby entry init_attract_mode calls next_attract_wave (attract_wave −1 at
boot → DAT_0020f96c = 0; then NextAttractWave 0x1623e0: from the world of
attract_wave, the next worlddatas entry with world data, its levels from the
+0x28 cursor, the first with LEVL +4 bit 2). This disc's attract levels in
that order: A2 B5 C4 D1 F1 G1 H2 I4 J2 K1 L1 (Temple and the Secret world
have none). init_flyby → new_attract_mode(0x8008, attract_wave) builds the
level with no player (controls_remove_active_player; the panels are not set
up) and InitCamera(0) starts the old camera at StartAttTransmitter.
game_main's 0x8008 case runs world_update, do_items, do_enemies,
enemy_update (DoTexMods only) and do_flyby: timer1 += vb_elapsed unless
JoyLevels & 0x300000 is held; exit_attract once flyby_seq_done > 0x1e (the
transmitter path ran out) or timer1 > 0xe10, then init_attract_mode(−1);
ProcessEffects; DrawGlowText(1.0, "Press Start") at y 0x140. set_logo(1)'s
MBOX_NewBlit("LOGO_B", 0x170, −0x130) shows nothing in the reference.
Actual reference (2026-10-22, turbo, private/reports/
reference-attract-flyby-v1-*.png, reference-attract-cycle-v1-sheet.png):
the flybys show C4, D1, F1, G1 in turn (the order above, continuing from
earlier attract runs), live enemies, "Press Start" centred at the bottom,
no panels, the 3D view over the whole frame. Native: actual-disc headless
flybys of all eleven attract levels end by the path's end
(game-run-flyby-<level>-v1.json, 316–981 updates); GUI: C4 and A2 flybys
with Press Start, Start → the title, the end → the next attract entry.
DEBUG: GDL_DEBUG_FLYBY=<level> starts with that level's flyby. Not compared
with a reference camera trace.

## Game over (2026-10-22)

init_over 0x127e70 (game_main when active_players is 0, or quit_game after
OptionsDone): game mode 0x4014, BGMusicStop, AudioStopAll, AudioEndGame
(empty), timer1 = 0xf0, timer2 = 0, every player inactivated,
active_players 0, TriggerCameraEnd. In 0x4014 game_main runs
world_update, do_items, do_enemies, do_players and do_over 0x127ee8:
AudioGameOver 0x15e228 (AudioFX 0x1005a, 0xe0, 0x14) as timer2 passes 60,
DrawStringText(centred, y 0x78, font 6, white, entry 0xa9 "GAME OVER") at
DrawStringScale 2.0 with the message text cut to (timer2 − 60) >> 3
letters, and once timer1 runs out EndTower, fail_movie_show = 1 and
init_attract_mode(0x8002) — the FAIL movie, then the attract sequence
(the title only through Start). Ported (gdl_core::game::movie::GameOver,
the client's sim.over); the Tower's join check does not run in 0x4014.
GUI: Quit Game → GAME OVER over the Tower → FAIL.

## Sumner's hint pad (2026-10-22)

HintMenu 0x10e800 (SumnerHintsActivate after 2 s at the pad): hints_inited
→ next_general_hint(0) once; next_boss/legend/rune_hint(0) on every
opening. DoOptions case 0x16 (hint_menu entries 0x27..0x2a):
- 0x27 next_general_hint(1): from index 3 down while no player has all
  gargoyle items of index − 1; the next line of GENERAL_HINTS[index]
  (ScrollTextListNum), then the lower entries; title "A Hint for You".
- 0x28 next_boss_hint(1) / 0x29 next_legend_hint(1): WorldOrder indices
  1..10 (1..9) — open worlds whose shard (PlayerHasShard(-1, the world
  number), as the original passes it) / legend item nobody holds, a second
  pass any open world; the lines 0..num of BOSS_HINTS / LEGEND_HINTS[index]
  where num = 1 + (any player's class word +0xd00 / +0xcfc has the world's
  bit) + (+0xd02 / +0xcfe: 2); titles BOSSHINTDESCS / LEGENDHINTDESCS line
  index.
- 0x2a next_rune_hint(1): rune_idx_table 0x21ec20 order, unheld runes
  first; RUNE_HINTS[rune − 1] lines by +0xcf8 / +0xcfa; title
  RUNEHINTDESCS line rune − 1.
DrawScrollListText(file 1 = HINTS_E.ROM, list 0 boss, 1 rune, 2 legend,
3 general) at the entry's scale (0.8 for the boss/legend/rune lists).
Ported (gdl_core::game::hints; the client's ShowHint). NATIVE: the lines
are one centred block (the original steps stringtext_y + 16 from y 0x70).
GUI: the four categories at a new game in the Tower (The Lich / Book of
Protection / The First Runestone).
