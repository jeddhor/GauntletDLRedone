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
"Inv". Tables are read from the ELF. Not ported: the exit swirl (mode 2).

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
package last to the HUD's texture search (SelectLoadStart). Verified
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
Control Style (do_controlsmenu), Screen Adjust (pbScreenSetAdjust),
vibration output.

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
name. The SELECT 3D scene behind the columns is not drawn. GUI check: A1
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
MPEG2 via init_movie/do_movie. Not ported yet.
