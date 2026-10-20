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

## Native presentation

A HUD camera with a fixed 512×384 orthographic projection on its own render
layer draws the frame's blit list as pooled quads (blit.wgsl: MODULATE,
alpha test, standard blending, bilinear, linear clamp), sorted far to near
by z. Textures are decoded lazily per OBJECTS record (format 146 with the
white alpha CLUT). Samples are clamped half a texel inside the crop: at the
original 640×448 frame edge taps never reach the neighbouring glyph cell.
MBOX search order (STATIC, the character package, the level's packages) is
a presentation choice; the panel labels are unique across them. Verified:
the Tower panel against tower-newgame-v1-240.png (layout, tints, fonts).

## Open

button_x_tex/button_tri_tex (ResetModels), NumLivePlayers, del_player_blits,
low_res flags, the shadow/0x1000 text flags, menu_on/optmenu state from the
front end, ability blits, SandglassBlit/ThermoBlit (boss and timers), the
inventory/map/shop panels and the scroll parchment are not ported.
