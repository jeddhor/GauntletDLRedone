# Work plan — 2026-10-18 (user: keep going until every milestone is actually finished)

User feedback on the 2026-10-18 build: renders but is not playable — the
Tower's crystals and Sumner were missing, lighting is wrong. The interim
build is not an end point. Order of work, each verified against the
original (PCSX2 reference on Xvfb :191, private captures):

1. Tower new-game flow: Tower item load (done: crystals, triggers), world
   animations (done), TowerInit/new player, crystal hide/fade-in,
   WelcomeMessage, crystal delivery (UnlockLevel), NeedCrystals at closed
   doors, Sumner (GWIZ tree, SumnerAnimate, speeches, captions, ghost),
   scroll items reading SCROLLS<level>.
2. Camera — DONE 2026-10-19 (research/camera.md): the gameplay camera is
   the StdCam module (not StandardCamera; new_camera_switch is 1), ported
   and bit-exact over 250 traced updates incl. transmitter switches; stage
   start sequence, boss camera (11 arenas run; not trace-checked), trigger
   cameras with Tower links (bit-exact vs capture), shakes, player-dpos
   frustum limit (wired with step 3). Open: do_camera/StandardCamera (old
   Cam array, used by front-end modes), stage-start and boss traces.
3. Player (IN PROGRESS; research/player.md): DONE 2026-10-19 — record-based
   controls (game/pad.rs), DoPlayerAction, GetPlayerDMotion, PlayerMotion and
   its contact helpers, do_players, the node round trip; 150/150 traced
   updates equal (replay-player-g1-v4). Remaining: switch gdl_play and the
   sweeps from the interim PlayerDriver/PlayerSim to do_players (E), the
   Unsupported paths listed in player.md (PlyrSfxDoDamage class effects,
   team moves, legend items, thunder hammer/breath/familiar, respawn,
   milestones, join/continue), physical pad mapping measurement, and traces
   with melee/magic/powerups/knockback/transporters/moving floors (F).
4. Rendering (IN PROGRESS; research/rendering.md): DONE 2026-10-19/20 —
   GS material state (MODULATE, lightmaps with the observed CLUT, alpha
   test/blend, additive, depth flags, clamps, bilinear), projection from pb
   +0x90, world node MB flags and CameraFace, world/tree particle systems
   (gdl_core::psys), world/package texmods, game-driven node state
   (EnterTower's light ray, fade objects, distance alpha), package tree node
   flags, VU-fitted lighting of colourless parts, held weapon and shadow,
   effect header/familiar trees posed per frame; tools: reference_camera.py,
   GDL_DEBUG_CAMERA, render_pixel_check.py, reference_screenshot.py --slot.
   2026-10-21: OANIM and texmod (alt texture, fade) tree nodes in effect
   and item trees, MBTreeSetAlpha fades of items and world nodes;
   DoLighting/SetAmbientSpecial state, A5/F2 lightning, end-of-stage
   intensity; skins (death dissolves, hit flashes, exit glow, chrome).
   Later 2026-10-21: effect point lights, hit effects (AddEnemyHitCol),
   boss meters, rune thermometer, compass.
   Fog: MBFogInit is empty in this build (nothing to draw).
   Critter shadows (CritterAddShadow). Alternate-texture modes per
   pbObjTexSub: −4 skins as a keep-A overlay pass (was a replacement), −2
   replacement (player shadow splash).
   Chrome (−3) environment mapping from setChrome (VU1 UV use HYPOTHESIS).
   Tree-node texture scrolls (CalcTexScroll / MBTreeSetUVScaleAdd).
   Step 4 rendering items from the plan are ported; open hypotheses are
   listed in research/unknowns.md.
   2026-10-22 (user: "lighting effects are all wrong"): ProcessItemWobjs
   fade objects were inverted (the Tower's lit podium and its green beams
   showed at a new game's start; fixed from the instructions); the GS
   frame's PSMCT16S + DTHE output in DisplayToLinear; VU1 microcode read
   (tools/python/vu_disasm.py): point-light formula, the 128/255 colour
   clamps; DoLighting moved before game_main with LEVL flag 8 (S5) and the
   do_players player light. Tower vs reference at the same camera:
   private/reports/render-tower-compare-v2.txt.
   2026-10-22: enemy replay against the reference (gameplay.md "Enemy
   replay"): reference_enemies.py probe, game-run enemy_replay with the
   rand seed solved per update; 179/180 updates equal for all 25 G1
   enemies; fixed DoEnemyItemCollide's delay distance. Two 270-update
   combat traces (now with the whole Items array, delta-coded): enemies
   250/270 and 247/270, players 268/270 and 266/270; the rest is do_items
   spawns, ProcessEffects hits/kill credit and ±0.0 matrix signs. Fixed:
   unsigned auto-aim test, show_crystals timer rounding, state 6/8
   offsets, the clock loop word, InitAnim's enemy +0x94 frame count.
   ProcessItems replay (--items-replay): 248/270 updates equal apart from
   four world-animation-driven records; item trees now searched as
   AtreeMatchAnyHeader does (realm package, POWERUPS, level package), so
   keys, gems, food, treasure and scrolls animate; the client's interim
   "YOU DIED" banner removed (the original's all-dead end and GAME OVER
   are ported). Items ride moving floors (PlaceItem parent). Enemy node
   round trip (GetObjWorldMat/UpdateObjWorldMat), spawn AnimInfo/matrix and
   check_enemy_pos probe fixes: enemies 270/270 (combat) and 267/270
   (fight) with ProcessItems in the replay. Whole-update replay with the
   effects array (gameplay.md): enemies and effects 270/270 on both traces;
   fixed effect spin/CreateDirMatrix, missile streak words, SetSkinFX
   integer words, generator hit/death effects, load-time world_update.
   Varied trace 175/175 everywhere after porting ResolvePlyrData (class
   specials had no effects or sounds), VU0 MulVecMat3/4 in the class SFX
   code, the opened-chest enemy loop, and NewHelp in headless runs.
   Tower new-game trace (reference_trace.py --slot 2, trace-tower-v1):
   enemies, player 0, effects and world animation 170/170 after fixing
   ProcessItemWobjs' fading-object "moving" words, the collision copies of
   the object words (now synced by each writer), PlayerCollideFloor's
   re-queries writing player_floor_col, run-time flags in the floor/wall
   tests and matrix-object floor normals; the same fixes bring G1 fight
   player 0 to 270/270. Items since: whole-update replays match every item
   word (fight 270/270, varied 175/175; the Tower only an audio handle) after
   porting the item reset-byte handshake, load-time ProcessItemWobjs,
   CreateDynobjGrid's cached poses, GetWorldMat/UnparentMatrix as node
   chains, sound items' world-animation nodes and SetItemGeo's clock start;
   world_update's order and paused gate (gameplay.md, items.md).
   Other realms (reference_warp.py captures, slots 3–7; gameplay.md):
   A1 270/270 and B1 250/250 in every part; K2, the ship, 210/210 for
   enemies, player, effects, world animation and its General critter
   (items 206: audio driver handles). Critters are now probed, seeded and
   compared (critters.md "Reference comparison"): they ride moving floors
   through their +0x70 node, turn their heads (NodeLookAtPos), and the
   replay maps their node pointers by CritterGetNode's table. B1 fixes:
   EnemyCollideFloor's second-query object, SfxSkipItem's indestructible
   tail, PlayerMotion's carried PlayerGetTarget target, the state-8 action
   guard and +0x3a4; the replay restores item side state per attempt.
   G1 from its new-game capture 250/250 (the replay now tries neighbouring
   rand call counts); B6's dragon boss 250/250 in every part (slot 8 via
   do_exit's wave skip; newcam_mode, Cam[0]'s up vector and the effect
   loop word fixed); the frame replay's camera stage checks the boss camera
   (B6 250/250, CameraLimitPlayerDpos's boss branch ported) and the StdCam
   stage-start zoom (G1 250/250); G5's lich 250/250 once critter SFX follow
   their node. C5, E2, F2, H4, J5 and K5 250/250 in every part from boss
   activation (D5 and I5 250/250 but the boss stays inactive; I5 items
   249/250, an audio handle): PlayerCollideItems' aliased to/out, the rock
   UpdateObjWorldMat lag, DoRockFly seeding, and parented effect nodes'
   local yaw and UnparentMatrix write-back fixed (gameplay.md "The other
   boss levels"). A5's chimera with its three heads, D5 and I5 through
   activation: 250/250 in every part (object nodes compose through every
   ancestor; NewWorldObject's 0x100 flag rule; head reset byte and
   look-at). A1's gargoyle statue (warped capture w1l1-statue-v1) 250/250 in
   every part: statue trees through SetItemGeo, spawned critters' AnimInfo
   and GMETER, do_ai's real jump table (0x11 → move_logic23, 0x1a →
   move_logic16), init_enemy's AnimateATree, the generator stage change's
   DeleteItem, critter files in EnemyType slot order, the replay's frame
   matrices. A1's golem statue 240/240; B6's dragon killed (warped health)
   through the coin spew 250/250 (InitEffects' per-level headers,
   CritterDoTexmodNode's skin, world_update's boss part in the replay);
   PlayerGetTarget's gate residue modelled. The good wizard and the auto
   exit after B6's death compared in three chained traces, every part equal
   until game_main's level end (BossDeath's BOSSKEY effect; the replay now
   runs main's audio queue and do_auto_exit). Next: the game_main level end
   and tally against a reference, general/Death statues.
5. Front end and HUD (IN PROGRESS; research/hud.md): DONE 2026-10-20/21 —
   FNT fonts, MB blit/font layer, original player panels (portrait, LV,
   name, gold, health, keys/potions, turbo meter, runes, crystal display,
   letterbox over the panels) driven by WritePlayerInfo from do_players;
   game_main's per-frame SetPlayerVars; ScrollMessageBox (parchment,
   font32 text, glow prompt, fire-scroll burn); NewHelp/DoHelp help boxes
   (with the help pause and displayed flags); pickup notices (do_got_it);
   option menus drawn from the ELF records (title, options, Tower/realm
   pause, quit questions, hint menu + Sumner's hint pages, audio sliders);
   title screen (TITLE blits, GLOWCROP, Press Start); player-select columns
   (art, names, menus, hints) and Manage Character; fullscreen inventory;
   shop; display-space blending for every draw (Hdr frame +
   DisplayToLinear); OANIM enemy frames; item/missile trees animated.
   2026-10-21: Sumner's shard/rune speeches, captions (CaptionText over
   SCROLL_E), SumnerSpeechEnd effects and cameras, the sumner_event_flag
   machine, EnterTower's delivered shard/rune display, the ghost (WIZARD).
   2026-10-21: the return to the Tower through the shop (init_shop /
   do_shop: piles, level panel, shopping, inventory, final stats) and the
   realm map between levels (init_mapscreen / do_mapscreen).
   2026-10-21: the player-select screen (pselect.rs: every column state,
   name entry, Hidden codes, card states on the native store), its entry
   points (after the shop, Manage Character, joining in the Tower) and
   per-player characters in the client. The title's Start opens the select
   screen over the Tower (init_player_select(0); the native save list stays
   behind GDL_DEBUG_NATIVE_SELECT for the capture import). Remaining: the
   SELECT scene behind the columns, credits, attract mode, movies. 2026-10-21: Control Style, Screen Adjust. 2026-10-21: the ICON_ARROW selector.
6. Audio completeness; secret characters; Sumner as a character;
   multiplayer join/camera rules; remaining unported paths from sweeps.
   2026-10-21: the Audio* cue placeholders, background music (areas,
   parts, map/shop music, boss attention) and kind-0xd world sounds
   (docs/research/audio.md). Remaining audio: SPU curves (golem names and
   the demon fireball were ported later). 2026-10-21: the EE voice queue and track timing in the
   simulation (level/shop/map exits and the good wizard wait on it), the
   good wizard after a boss (goodwiz.rs: model, captions, speech, exit),
   PlayerGiveShard, StartBlockFX/StartGenFX, boss meters hidden at
   BossDeath, the boss-arena camera eye fix. game_main's level end and
   next_world (exitflow.rs): realm exits → tally/shop → the Tower; Tower
   portals → the map → the level (docs/research/player.md).
   2026-10-21: multi-player placement — load_player_geo → get_player_pos /
   try_location (stage-start square, new_offset respawn probes beside
   another player, blocker damage), get_screen_pos on Cam[0] with the
   InitCamera screen limits, SetPlayerStartPos's StartTransmitter by start
   id (research/player.md, camera.md). Headless: game-run players=N, fall=F.
   2026-10-22: PlayerStartMissile's ballista/boss elec/acid/0x400 paths,
   InitPlayerMissiles' WeapThrowFx (weapon-powerup throw effects riding the
   missile), the held-weapon WEAP_HOLD effect, texmod targets named for
   particle textures with frames from other packages, signed psys record
   offsets (gameplay.md, rendering.md). gdl_tools anim-trees inspector.
   2026-10-22: in-game movies (init_gamemovie/do_gamemovie; PSS video via
   the system ffmpeg, audio decoded natively — decisions/0003-movies.md):
   the level movies after the map screen, victory/garm before the shop.
   Fixed: the map screen's end opened the select screen (shared signal).
   2026-10-22: the attract sequence at boot and after the title's timeout
   (SONY screen + card check, MIDWAY/OPENING/TITLE2 and the class movies,
   group shots, credits; flybys passed over — hud.md). Movie frames now
   rebind their material each frame (in-game movies showed a frozen frame).
   2026-10-22: game over (init_over/do_over: GAME OVER over the running
   level, then the FAIL movie and the attract sequence), also for Quit Game.
   2026-10-22: Sumner's hint pad picks hints by progress (next_*_hint).
   2026-10-22: every enemy move logic the disc selects is ported (15, 19,
   24, 27, 28, 31 added, plus set_suicide_bomber_index, which do_enemies
   called and the port had skipped — suicide-bomber flight never fired);
   golem audio (SetEnemyAudio 0x1d, AudioEnemyWalk/GolemBorn/GolemSwing).
   Logics 8/9/11/12/13/20/21/22 are selected by no disc data
   (enemy-logic-scan-v1.json). game-run spawn= DEBUG option.
   2026-10-22: the attract flybys (flyby.rs: InitCamera's flyby branch and
   the old Cam[0] transmitter path, do_flyby, NextAttractWave; the window
   without the 64-row inset) — reference cycle captured; headless flybys of
   all eleven attract levels; GUI checked. The select screen's "scene" is
   its column art (already drawn; hud.md). The inventory exit swirl
   (end_fullscreen_inventory, animate_panel_piece mode 2) for the Tower menu
   and the shop. Vibration (do_vibe/serve_vibes/vibrators_off → gamepad
   rumble). Team moves (teammove.rs: the class switch, PlayerSetParent /
   PlayerUnsetParent / PlayerFacePlayer under the DUMMY node; R2 = keyboard
   X / B). The plan's listed items are done; next: reference comparisons
   (multi-player, flyby camera) and the remaining Unsupported paths in
   player.md.
   2026-10-22: GUI walkthrough on the private Xvfb (xdotool on the client's
   own window): title → select (New, name, class) → the Tower's welcome →
   crystal camera → play, matching the reference's new-game frame. Found and
   fixed: the welcome scroll could not be closed with the in-game triangle
   (the box used host menu keys); ScrollMessageBox's input is now the
   original's (PlayerControls, the masked players' triangle edge, the
   control clears); the physical buttons were measured in the reference
   (player.md "Pad buttons") — gamepad/keyboard triangle and square were
   swapped and the shoulder buttons wrong; Enable/DisablePlayerControls
   ported in full (lockout 4); the pause menu keeps PlayerControls running.
   Then the M12 walkthrough (research/playtest.md): start, play, finish,
   save, restart and recover, and a death, all in the GUI through
   tools/python/client_driver.py; fixed the shop/map/select panels
   (SetPlayerVars in every mode) and the all-dead loop (a skipped shop goes
   to init_next_level).
   Scene comparisons (2026-10-22): every captured level (A1, B1, G1, K2,
   the eleven boss arenas, A1's statues) at the reference's camera
   (reference_screenshot.py --stage, reference_camera.py, GDL_DEBUG_CAMERA;
   private/reports/scene-<tag>-ref.png / -native.png). Found and fixed in
   gdl_play: multiplayer-only items drawn in single player (+0xe1), statues,
   rocks (SAFEROCK<stage>), switches (kind 5), GRU-style generators
   (ItemFindMBObjectL1's +L1), Skorne's OANIM lower body, E2's falling
   floor slabs (level-table models). Remaining differences seen are timing
   (animation phase, enemies' positions).
   Checks after the session's changes: r39 equal to r35 (r35 itself equal
   to r34 apart from the added good-wizard traces), sweep sw60 51/51 and
   identical to sw59, client smoke test (Tower new game, B6, E2, K5, I5,
   A1: no panic, no simulation error, no unported path). Boss deaths re-run
   for all eleven bosses (critters.md); fixed ExtractPYR's zero divisor and
   the client's level end after the auto exit.
   2026-10-22/23 (user: continue until M00–M18 are complete and tested):
   G1's exit replayed 186/186 in every part (gameplay.md "The G1 exit":
   item links, do_exit's sink and spin, the item loop word). Saves against
   the reference: a scratch card formatted and written by the game
   (launch_reference.py --scratch-card1, reference_driver.py), its layout
   decoded (saves.md), the native load equal to the original's record after
   two separate loads, PS2 card import (import_card / card-import /
   run_play --import-card) and the imported character loaded in gdl_play.
   Found and fixed: atan2f(0, 0) (the wrapper's DOMAIN path, +0.0) — any
   connected gamepad stopped the select screen; the client's card view was
   one slot off (save0000..; now save0001..0008 as the original, with a
   save0000 migration); automated client runs ignore physical controllers
   (GDL_NO_GAMEPADS). M03 closed with its scenario package
   (reference-baseline.md). Checks: r42 equal to r41 (= r35 + G1exit),
   Rust and Python suites, clippy.
   2026-10-23: M04–M11 and M13 closed with evidence (coverage.md). Enemy
   packages per level by GetEnemyDir (aux / numbered packages; E1's GENFX
   divergence), ItemGenDesc generator trees, a wider replay seed search
   (F1); D1/E1/F1/H1/I1/J1 new-game traces in the regression (r44/r45;
   first divergences recorded for H1, I1, J1). Content coverage matrix v2
   (61/61 levels sweep without unported paths, 23 replayed). Traversal bot
   (gdl_tools bot.rs; playtest.md): batch b4 ends 14 of 61 levels (the
   treasure rooms, B2, B5, and B6/D5/G5's bosses with the legend item as a
   labelled precondition); co-op matrix c1 (S1, B6 with 1–4 players). It
   found CritterDamage's breakable collision nodes, now ported. M18:
   recovery notice in the client and GUI recovery scenarios (corrupt,
   interrupt). M17: av-catalog (every cue/stream/world sound/movie resolves)
   and five GUI menu flow tests (menu_flows.py, GDL_UI_TRACE). M12
   acceptance report (research/m12-acceptance.md).
   2026-10-23 (later): Sumner's record/save/Tower branches (saves.md);
   back-face culling of object parts as VU1 does (gs_cull.py dump check,
   rendering.md); additive draws quantised per GS write; the shop's
   corner sweep and list scroll; the familiar, the attached powerup trees
   (on AnimateATree clocks), the player's translucency, the attached
   hand/head objects, the It sign, the pojo hiding the model, the level-99
   head (player.md); the halo-on-Death effect and DoEnemyPickupItems
   (items.md; the G1 fixture's 76 → 416 / 88 → 239 matched) in gdl_core;
   chrome UVs from setTexShift. Unknowns resolved by reading code:
   MBTreeSetAlpha, setPrimColor, AnimateNode's texmod frame, WorldNumLevels,
   MBNodeInsert/MBNodeInit, the MBOX search rule, save +0xbec. Evidence:
   level-smoke-v2 (61/61), exit-check-v1 (all 41 portal levels end), I5's
   yeti defeated from the reference's warp point, the bot ends D3, G1 and
   G3, the Tower is deterministic across render rates. Replays r60–r64
   unchanged.
   2026-10-23 (evening): D3, I2 and B2 reached in the reference by
   do_exit's wave skip (WARPED captures w4l3-start-v1, w4l3-leaf-v1,
   w9l2-start-v1, w2l2-start-v1) and replayed update by update (every part
   equal; D3's camera 119/120 at the stage-start zoom step, unknowns).
   Item kinds 10:0x31 (falling leaves, from contact to deletion), 12:0,
   12:2 and −1:6 (B2's random item) now compared (items.md). Found and
   fixed: the Transmitters table is static across levels, so a stale
   Cur3DTransmitter past the new level's count read the Tower's entries in
   the original and panicked natively (camera.md; Tower → B1/B2 and every
   level with fewer than 58 transmitters); unparented item translations
   pass through a translation-only root (−0.0 → +0.0, I2 item 257);
   compare_items' RandomItemNum derivation now subtracts ProcessItems'
   per-update +1. The replay regression is in the repo (tools/regress.sh);
   r65–r67 equal r64 on every earlier replay, with D3/I2/B2 added.
   2026-10-23 (night): the level-start sweep — all 35 levels without a
   reference replay warped, traced and replayed (gameplay.md "Level-start
   sweep"; regression r70 runs 82 replays). Fixed from it: NumEnemies
   (CAMS +0x34), WAR generator trees from the enemy package, the item node
   w column; documented: D2's general target words (stack), sound item
   +0xfe, J4's replay seeding, K4's 1-ulp locals, the zoom step in 17
   levels. compare_items.py --after-updates compares items after native
   updates. D1's ledge checked in the reference (bot graph, not game). The
   fire breath compared with the reference. Bot: drop edges, generators,
   idle reset (b26 ends B1 and A4); game_run --no-enemies (DEBUG) for
   progression-only traversal.
   2026-10-24: the dyngrid is native (collision.md "Dyngrid": cells of
   dyngrid_width 10, links, ObjCheckNum stamps, NextDynGrid's integer
   bounds with the end x as its end z) and CheckFlags 0x20 queries stop at
   their first solid hit — B5's combat kill (a lava ball the original's
   walk never reaches) is gone; two players at B6's boss equal the
   reference on all 250 updates (critter target-slot stack residue masked
   in all four slots); regression r72 (118 replays: 115 unchanged, 2
   improved, the new two-player one equal). traversal-v6: 36 of 61 levels
   ended by the bot. Game is Clone (state search tooling).
   2026-10-24 (later): InvertMat4's z sum order ((p2 + p0) + p1) fixes the
   K4 and I1 floor-parented items; enemies ride 0x1000 floors
   (do_enemy_collide's MBNodeSetParent; traces carry enemy_node_parents);
   sound items hold −tid for the update their loop is requested;
   PlayerStartMissile's MulVecMat3 is VU0. Regressions r73–r75 (118
   replays): 17 improved, none worse — K4 start/combat, J4 combat, B5
   combat, H1, I1 and J1 (v2 trace with RandomItemNum) now equal in every
   part, the +0xfe word gone from ten replays. Collision objects, triangles,
   grid and item definitions are Arc-shared (clone cost); explore.rs (DEBUG
   state search, random or graph-guided d-pad macros) is in game-run
   (explore=, nav=1), not yet finishing levels.
   2026-10-24 (co-op): the Tower and G1 with two and four players replay
   equal (regression r76: tower2p/4p need load_players 0, as the
   one-player Tower); a team move traced in the reference (WARPED
   w7l0-2p-team-v1, L2) replays equal except the carried partner's
   rotation (3 ulps): PlayerMotion's carried branch re-reads the world
   (GetObjWorldMat), the 0x58 start runs StartComboFX, the frame replay
   builds the player rig and seeds team links. explore.rs gains bot
   macros (nav=2: bot.rs on a shared graph, random first hint) and
   analog pad scripts (padfile=).
   2026-10-24 (morning): chained levels enter through init_next_level's
   record steps (game-run next_level=1; campaign_chain.py) instead of the
   previous level's exit state. tools/python/gs_replay.py replays a GS
   dump's draws in software: the Tower crystals' brightness difference is
   phase spread (reference frames 446..525, native 397..461), and the
   pale-green hit flash in native frames is the original's (reference GS
   dump of B4 at the flash: 183/220/183 against 179/219/179). Scene
   comparisons now cover all 61 levels (38 added, presentation.md).
   Item culling took the StdCam record's eye under the boss and trigger
   cameras; it now takes the MB window's eye (MBWorldSphereVisible), which
   lets I5's raft pad be touched — the bot reaches the yeti's arena on foot
   (items.md "Item culling under the boss and trigger cameras"). Traversal
   tooling: explorer frontier reports, level maps (game-run navdump=,
   level_map.py), a no-assist search (no_assist=1), opened gates counted as
   progress (they stay active records with +0xdc set), and the bot rides
   floors moving in any direction (I5's and K4's rafts).
   2026-10-24 (later morning): I5 ends from its own start (raft pad no
   longer culled); bit-level DAZ/round-toward-zero make the simulation
   1.4× faster (identical replays: r80 = r79 = r78); reference audio
   captures (PCSX2 Video Capture) compared with native: G1's music matches
   the native ADS decode, the music/effects balance agrees within ~1 dB,
   the whole reference output sits ~11 dB lower (open); the bot attacks a
   blocking breakable item (three tries per item); a level counts as won
   only with the player through it (state 5 or 4, not a death); the
   traversal tools seek rune stones and legend items. Critical path
   (bosses.md): every realm in WorldOrder, each boss with its legend item,
   all thirteen runes before H4. Campaign chain t6 (campaign_chain.py:
   52 levels, bot first, search fallback, --require; DEBUG health assist)
   running.
   2026-10-24 (midday): the 11 dB audio level difference is accounted for
   (audio.md "Reference output capture"): the reference's saved states
   (SPU2.bin voice records) show DCS's ADSR words leave every voice in
   sustain at half level — native had no envelope; gdl_core::dcs::Envelope
   now applies it to every voice and the music — and PCSX2 2.6.3 scales
   its final mix by 0x4fff / 0x8000 (an emulator stage, removed upstream
   after 2.6.3, not modelled); −0.5 dB remains (interpolation, codec).
   Campaign chain t7 (bot binary f16) cleared L1 and is in G1's search
   fallback (the bot took the rune but missed the exit in 60,000 updates).
   2026-10-24 (afternoon): traversal work (playtest.md "Rules added
   2026-10-24"): untouchable triggers skipped, trigger revisits, generator
   damage progress, riding bound, graph headroom and player-like strict
   test, near-goal reopening, gates approached head-on, subtype-31 switches
   shot (WeaponCheckItemCollision), the key cap, ranged fire and food
   without the assist, retries with start delays in the chain. Chain t11
   (from t10's G3 end state; t8 G1, t10 G2/G3) has ended G4 G5 B1 B2 B3 B4;
   D2 and I1 now end by the bot (b36). Not yet ended by any batch: C1 C3 C4
   D3 D4 E1 F1 H1–H3 I2–I4 J1–J4 J6 K1.
   2026-10-24 (evening): K2's campaign block was a native bug —
   GauntWorldInit's legend-item quest setup is unsigned (0x162f08 sltiu),
   native compared signed, so a carried legend item locked the player's
   attacks and potions in a level without a boss (fixed; chain t16 ended K2
   K3 K4). Every boss's death compared with the reference (bosses.md "Boss
   deaths against the reference"; WARPED boss health, regress r81 adds the
   replays with no change to the 123 others): nine equal in every part, D5
   and A5 with one residual each; fixed AddExp's level-up effect and
   ProcessSpewItems' UpdateObjWorldMat. The endgame completions equal
   through game_main's transitions: E2 to its level end, F2 to the garm
   movie, H4 (Garm) to the victory movie. GUI scenario ending-h4
   (scenarios-m15-v1, PASS): Garm's death (DEBUG boss_kill) → good wizard →
   victory movie → shop → the Tower, H4's completion byte kept through a
   save and reload. Legitimate play (no DEBUG assist): Tower shop purchases
   with the character's gold between chained levels (--shop-buy), loot
   (gold, food, potions, chests with spare keys), turbo, fighting during
   trigger waits; chain lg1 ends L1 and G1 with no assist (G2 in progress).
   Bot: exit portals avoided for other goals, triggers bound to a
   treasure's support first, optional pickups skipped after three stuck
   events. b37 (assisted, 200,000 updates): C1 C4 I2 J4 now end; still not
   ended in some batch: C3 (legend item) D3 D4 E1 F1 H1–H3 I3 I4 J1–J3 J6
   K1. K1's route traced from navdumps (the exit region is reached from the
   ledge region above (−66, −32, −78), which bridge 1093 — trigger 48 —
   joins to the western low area; hints under test).
   2026-10-24 (late evening): D5's legend-item phase compared with the
   reference (bosses.md; WARPED w4l5-legend-v1): the drider's node scale
   0.8 now scales CritterDoDamageFX/CritterDoSfx offsets natively; the
   frame replay seeds QuestWeaponSfx, QuestItemState from the sample
   before the update and restores QuestItemTimer per retry (regression
   r85: D5legend critters 294/300, B6legend players 290/290, the other
   144 unchanged). Bot (playtest.md "Rules added 2026-10-24 (late)"):
   island limit (G1 back to ending with its rune), breakable-wall height
   window, per-visit break tries, post-trigger unblock, Death avoided or
   dispelled with a potion, hazards soft. I3 ended for the first time
   (b48). Ended alive with their rune and legend item in some run: 53 of
   61 (H2 in chain t21, I3 the newest); C3 ended without its bellows; not
   yet D3 D4 H3 I4 J3 J6 K1. Later: the bot's drops limited to what
   PlayerCollideFloor allows (3 units, 4 on one object; the graph had 12)
   — D4 ends with its sword (b53), 54 of 61; G1, K2, D2, B5 end faster.
   Disk: the data volume filled (208 of 220 GB; chain t27 crashed reading
   a truncated report). Freed: target-dev2 (cargo clean, 2.4 GB), the
   losing tries of chained bot runs (1.46 GB; summary
   private/reports/campaign-bot-tries-summary-v1.json) and the scratch
   bot batches' losing runs (summary bot-batches-summary-v1.json). Chain
   t29 runs D4 onward from D2's end state (D3 skipped while it does not
   end); keep an eye on free space (bot reports are 25–40 MB each).
   Chain t26 (f98, from H2's end): H3 ends with its rune at 143,648 and
   H4 (Garm) after it; t29: D4, D5, C1 end in sequence. 55 of 61; not yet
   D3 I4 J3 J6 K1.
   2026-10-24 (night): **M12 accepted by the user** (lighting, crystals,
   Sumner resolved). Reported with it: a green potion not picked up (the
   nine-potion limit, most likely) and a frame-rate drop during the
   Sorceress's square attack — fixed in the client (point lights in one
   shared buffer, materials mutated only on change; 262 → 180 ms frames on
   the software renderer; m12-acceptance.md, rendering.md).
   2026-10-25: chains on f102 (labelled DEBUG: C3 and K1 skipped while
   they do not end; boss legend items held, --legend-bosses): t32 ends
   K5 A1 A6 A2 (legend item) A3 (rune) A4 A5 D1 D2 (rune) in sequence;
   t31 ends C4 C5 I1 and stops at I2 (rune not taken; I2 ended
   standalone in b37 — route hints for I2 J1 J2 J4 added); t33 runs I2
   onward. Bot regression b57 (f102: line test for goal nodes) — the
   eight ended levels checked all end. Legitimate chain lg2 died in G2 (all six tries, then the
   search); lg3 runs with the Death rule.
   2026-10-25 (later): standalone searches s1 for D3 I4 J3 J6 K1 did not
   end any; lg7 (no assist, larger search) died in G2 again (progress 22);
   t33 stopped at I2 — its hints need lift 47 between the walkway and the
   ground, which the graph lost with the 4-unit drop limit (playtest.md
   "2026-10-25 runs"). Bot: ride edges for vertically moving objects
   (playtest.md "Rules added 2026-10-25"): J6 ends for the first time (b62)
   and C3 with its bellows (b64b); I4 with its lamp by striking trigger 534
   through a wall (b68), J3 by striking trigger 502 (b70) and D3 by
   running at the bridge pieces' steps (b73) and K1 with hand-made hints
   (rune barriers switched off) — 61 of 61 levels traversed with their
   rune and legend items. Open reliability: C1 and J6 fail on c52/c54
   (ended on c38), J3 ends in 1 of 13 tries. 2026-10-26: one carried
   assisted campaign from a new game to the ending (tfull1 + tskip1:
   every non-secret level but J3 in order, runes and legend items taken,
   Garm's level ends); legitimate (no-assist) play still dies in G2. The chained campaign passes I2 (i2r4), I3 (t34), I4 I5 J1
   J2 (t35); t36 runs J3 onward. Legitimate play: the game's
   Easy difficulty (Game Options) roughly doubles the no-assist G2
   survival; every try still dies (lg13); chain lg14 (Easy, search) runs.
   2026-10-26 (night): J3 in the chain (tfj3h: 5 of 6 tries) and the
   assisted tail from it (tfull2 J6…H2, tf2h3 H3, tfull3 H4: Garm killed,
   auto exit at 6,848). Legitimate play reached A4 (lg21, a4t) but the
   chains never checked the Tower's realm gates: WorldOpen needs crystal
   sets (B 100 … J 250; a play of every level holds just enough for I and
   J), and lg21 entered B1 with 60 of 100 — only L1–G4 are a legitimate
   campaign. A5 (chimera, 0.2 damage with its heads up) killed every
   no-assist try without D4's scimitar; legend items serve other realms'
   bosses (bosses.md, corrected: bosses die without them natively). Bot
   seek_crystals, campaign_chain --gates with crystal replays; chain lg24
   (from lg21's G4 end, bosses after their item's realm) runs. Later the
   same night: game-run end states never ran game_main's level end (no
   completion recorded by any chain); c76 runs it, the probe also tests the
   Tower portal (K's records are K2 K3 K4 K1 K5); clean campaign lg27
   from a new game runs.
   2026-10-27: the clean campaign (no assist, Easy, gates, crystals,
   completions) has ended L1 G1–G5 B1–B6 A1–A6 K1–K5 D1–D4 C1 (lg27, lg28,
   lg30, lg31). C2 had killed every try; c90 (weak foes shot, not kited)
   gets through it and F1 (both had died every no-assist try), but C2's
   wins took the secret exit to S1, whose treasure room returns to C2. c91
   keeps chain bots off secret exits and does not count a world-12 end;
   c92 puts C2's stairs (1.25 risers, step objects) on the bot's graph, so
   the regular exit is a goal. H3 ends without the assist on c85 (2 of 3).
   c94 blocks failed climbs per object pair; c95 keeps c92's rule off
   drops (C2's west pit). Then lg34 (C2 by its regular exit, C3, C4, D5),
   lg35 (crystal replays with --replay-crystal-stuck for the I and J gates;
   I1–I5, C5, J1–J4), lg36 (J5's wraith fought from 26 units, c100; E1 E2
   F1 F2 H1 H2) and lg37 (H3, H4: Garm killed, the victory movie): **the
   legitimate campaign reaches the ending** without the assist, and its
   completion state survives a client save and reload (completion-lg37h4b
   PASS). M15 complete; the chain's Tower between levels is not simulated
   (labelled). Then, with the owner's approval, the project was
   relicensed GPL-3.0-or-later (decisions/0004-license.md) and the SPU2's
   Gaussian resampling taken from PCSX2 v2.6.3 into gdl_core::spu; the
   mixer uses it and G1's capture now matches the native render's
   spectrum to 14 kHz (audio.md "SPU resampling"). M17 complete.
   2026-10-27/28 (user: after a playtest on the RX 580 as a Green Wizard —
   a pink triangle at the feet, a garbled name-entry spinner, stutter —
   finish M19 and M20): the DUMMY node's object no longer draws and the
   select screen recomputes its panels (both fixed, commit 635002c); the
   stutter was frame hitches (materials re-prepared, textures re-uploaded)
   and 30 Hz motion — fixed, then the scene blended at the display rate,
   bindless materials, critters posed once per update (performance.md).
   M19 (enhancements.md): the frame keeps its shape in any window, Hor+
   widescreen, the F10 settings overlay (F11, F3), key bindings, MSAA as a
   setting (Bevy's default 4× had always been on; the baseline is now
   without), texture packs, reduced flashes; simulation hashes unchanged by
   any of them, 146 replays equal to r85 (r86). M20 (performance.md): the
   frozen-capture clocks and a HashMap texture search made captures depend
   on frame timing (fixed); Windows build with cpal audio, run under Wine
   with Linux-identical simulation hashes; platform checks (virtual
   gamepad hotplug, focus loss, resize, fullscreen, full disk) and
   30-minute four-player sessions (soak_test.py); sound banks are decoded
   ahead and music parts off the frame (a 26 ms and a 188 ms stall); GUI
   menu flows 16/16 (menu-flows-m20-final3). Residual (performance.md): a
   rare long present interval (2 in a 30-minute four-player session) and
   8–13 ms simulation updates every few minutes; ten TEST characters that
   test runs had left in the player's save directory were moved to
   private/runs/test-saves-moved-from-user-dir. Not tested: a Windows
   installation, WASAPI output, a physical controller unplugged.
   2026-10-28 (user playtest of that build: Start on the title went to the
   Tower with GAME OVER): the per-frame game-over check ran before the
   select screen's first update joined player 1; it now skips the select
   screen, as the original's init_over callers do (hud.md, playtest.md).
   title_start_check.py on the RX 580: 3/8 before, 8/8 after; menu flows
   16/16 (menu-flows-m20-selectfix1).
   2026-10-28 (user: Screen "never pops up", no graphics options, no
   exit on the title screens): in-game Screen Adjust and Control Style
   resumed the game under them (Menu::is_paused; fixed); the options lists
   end with a NATIVE "Display" entry opening the settings overlay, which
   controllers now drive; Back on Press Start quits, a controller's Start
   opens the title menu there (enhancements.md, playtest.md).
   2026-10-28 (user: widescreen sides not cleared under 4:3 screens; a
   potion in G1 that could not be picked up): full-frame 2D screens now
   keep the bordered frame; chests get their NULL1 content node (SetItem's
   tail 0x15296c, missed before) so OpenChest's content is taken through
   the open chest as in the reference (trace-G1-chest31-v3; native replay
   180/180 player updates, items but the content's y; regression r87 =
   r86 for all 146 replays; scenarios chest-g1 and death-g1 pass).
   2026-10-28 (user: a blinking cursor over the game-over movie; the map
   before G1 lacks "You are now entering THE POISON FIELDS!"): no original
   menu is drawn during an attract entry (the title menu's arrow showed
   through the FAIL movie); the map takes the destination's AUDS lines and
   CurWorld (AudioEnterNextStage, AudioMapDot) with the stage +0x14 name
   line ResolveWorldData fills; the AUDS record is LEVL +0x5a's (twelve
   levels had another level's music and bank). Reference ref-stage-k2-v1
   and ref-stage-g1-v1 equal the port (audio.md "Stage records", hud.md,
   playtest.md).
   2026-10-28 (user: MSAA washes out the Display overlay; pad Start does
   nothing on Press Start): the overlay camera clears and is composited
   premultiplied instead of loading the multisampled texture Bevy 0.18
   never writes back (enhancements.md); Start not reproduced with a
   virtual copy of the player's controller on the RX 580 under the
   player's settings (playtest.md, title_start_check --attract --xbox).
   2026-10-28 (M21, the owner's installer specification): Qt 6 installer,
   gdl_import, packages (decisions/0005-installer.md, research/release.md);
   Linux complete and checked (batch and GUI installs into fresh homes, the
   installed game runs; start-state template built in, hash-equal); the
   Windows importer and game under Wine; the Windows installer waits for
   the owner's approval to fetch Qt for Windows. Test installs filled the
   data drive once (427 MB free); they are now made in /tmp and removed.
   2026-10-28 (owner: a Windows machine with Qt over SSH; bundle Qt in both
   packages): the Windows installer is built there (Qt 6.11.1 MinGW,
   windeployqt); the Linux package carries Ubuntu's Qt and its less common
   dependencies (DT_RPATH, qt.conf); LIBRARY_LICENSES.txt and the verified
   Qt sources ship with the packages (toolchain.md). Windows 11: installs
   from the ISO and from the ISO mounted as a drive, shortcuts, the wizard
   starts on the bundled Qt; the importer's staging swap failed there (the
   manifest file still open) and is fixed. Linux checks with the system's
   Qt masked pass (research/release.md). The Windows programs and installer
   are now byte-reproducible. Open: the game on a Windows desktop, the
   glibc 2.43 floor of the Linux build.
   2026-10-29 (user: the widescreen sides still show the level on the
   screen before the realm map): the level end's TRANSITION_SCREEN
   (world_update's end-of-stage fade, while play is still widened; also
   before the tally) had the level beside it; the sides now fade to black
   with it at its alpha (enhancements.md). RX 580 at 1600×1000: the Tower's
   G portal and G1's exit go dark beside it and stay bordered into the map
   and the tally; per-update hashes equal the previous build's.
   2026-10-29 (user: show Xbox buttons for an Xbox controller): the
   original's prompts are separate textures (BUTTON_X/O/SQ/TRI, d-pad
   arrows), never text, so gdl_client::glyphs makes Xbox variants of the
   four face buttons from them (the disc kept, A/B/X/Y drawn in the
   symbols' stroke weight) and present_hud picks them by the
   button_prompts setting (Automatic: the last controller used; Sony pads
   and no controller keep the original). RX 580: the scroll, the Options
   row and the select screen show the letters with the virtual Xbox Series
   pad; the original without one; per-update hashes unchanged
   (enhancements.md "Button prompts").
   2026-10-29 (user: older Linux distributions supported, GCC source
   bundled, label "complete port"; the user tests Windows): the Linux
   package is built in an Ubuntu 20.04 container with Qt 6.10.2 built from
   the shipped source (tools/linux/; GLIBC_2.30 / GLIBCXX_3.4.28 at most);
   on Ubuntu 20.04 (Mesa 21.2.6) it installs, verifies, shows the wizard
   and plays with hashes equal to this host's (old-distro-focal4). An
   update over the previous release keeps saves and settings
   (update-v3). No GPU: a plain message (and a dialog on Linux); no
   FFmpeg: movies skipped. The Windows package ships GCC 13.1.0's source
   with the MinGW-Builds patches; its programs no longer embed this
   machine's folders. release_audit.py: no game data, saves or private
   paths in the release (research/release.md). The window title is
   "Gauntlet: Dark Legacy" (was "… - INTERIM native build"; the test tools
   find the client by that exact title); title_start_check m21-release1:
   6/6 on the RX 580. Final acceptance report: research/m21-acceptance.md.
   2026-10-29 (user: installing works on Linux and Windows from the ISO
   and from the disc, "everything seems to work"; put the repository on
   GitHub as jeddhor/GauntletDLRedone, private, and the packages as release
   1.0.0): the version is 1.0.0 (fbbdb54); the packages were rebuilt and
   checked again (research/m21-acceptance.md "Release identities (1.0.0)":
   only version strings and the programs' code layout differ from 0.1.0;
   600 G1 updates equal across 1.0.0 Linux, 1.0.0 under Wine and 0.1.0;
   Ubuntu 20.04, updates over 0.1.0 on Linux and Windows 11, title → select
   6/6). The history was audited before the push (research/release.md
   "Release 1.0.0 and the GitHub repository"); the repository's remote is
   origin (git@github.com:jeddhor/GauntletDLRedone.git), the release is the
   tag v1.0.0 with both packages, the source archive and library-source
   attached.

---

# Development checkpoint — 2026-10-18

User request (2026-10-17): continue through the end of M17 and complete M18
so progress can be saved; a "complete" build to test. Latest source commit:
`bash tools/git-local.sh log -1 --oneline`.

## Current position (2026-10-18)

- Launch: `python3 tools/python/run_play.py` (desktop) or `--virtual`. The
  build starts at "Press Start" → Start → SELECT A CHARACTER (saved
  characters, new character of the eight base classes × four colours, or
  "Import PELE" from the savestate capture) → the Tower. Saves:
  `$XDG_DATA_HOME/gdl-native/SLUS-20047/saves` (`--save-dir` overrides).
  Saving: Tower → Esc → Manage Character → Save Character.
- M18 (research/saves.md): game::profile ports the character record and
  save-region functions (checked: private/reports/profile-check-pele-v1.txt);
  gdl_core::save stores versioned envelopes with SHA-256, `.bak`, recovery
  and raw-save migration; gdl_formats::memcard reads cards read-only (the
  user's cards are unformatted; no card save exists to decode).
- M17 (interim presentation): original menu structure and labels as text
  over the original title picture (TITLE00–03), Settings → Audio volumes
  (options.json), Tower Shop (do_shopping) and Inventory (native summary),
  Quit Level / Quit Game; text ROMs decoded (gdl_formats::textrom): in-game
  NewHelp hints via helptab, scroll-item message boxes (SCROLL_E.ROM), the
  hint menu (HINTS_E.ROM lists).
- Simulation additions: PlayerProcessPowerups (powerup effects now apply),
  kill_player / PlayersSetWaveDied / drop_keys and the Tower restore,
  per-player class data from PDAT, per-class sim values from the record.
- Verification: 92 Rust tests (synthetic: profile round trip, save
  recovery/migration, memory card, menu flow, shop table, PDAT); Clippy
  clean; 51-level sweep sw10 identical to sw8; actual-data profile check of
  the PELE record; Xvfb smoke tests of import, save (version 2 + .bak), new
  Red Warrior, realm entry, Quit Level, death → Tower, Shop purchase,
  Inventory, title picture, a chest hint. No new PCSX2 comparisons.
- Not done (ledgers): original SELECT/TITLE scenes, fonts and text ROM
  tables, HUD, credits, hints, options other than audio, shop presentation,
  secret characters, Sumner, co-op character select, dying animation,
  cross-package missile textures (Sorceress WND_THROW1 draws magenta).

---

# Development checkpoint — 2026-10-17

Continuing the user's request to work through M11–M17 until a playable build
exists; the user will test at the end of M17. Latest source commit: find with
`bash tools/git-local.sh log -1 --oneline`.

## Current position (2026-10-17)

- Critter engine (research/critters.md): all eleven bosses run headless, the
  chimera with child heads (A5), fire cones (B6/C5/F2/K5), statue critters
  (golems, gargoyles, generals) with CritterGolemAI, non-boss collision and
  lookout waypoints (the native G1 lookout table equals the fixture's).
- Level data: ResolveWorldData defaults/difficulty are applied (the fixture's
  live G1 LEVL record matches; G1 item comparison 393/519 exact, was 384);
  the Forest realm now has generators and boss health.
- Simulation camera follows player 0 each update (HYPOTHESIS; do_camera not
  ported), so placed enemies and statues spawn along the player's route.
- Level sweep: every one of the 51 levels (realms A–K) runs a 1400-update
  scripted interim player (tools/python/game_run.py) without unsupported paths.
- gdl_play smoke on Xvfb :191 (realm G): title, start, walking, enemies attack.
  Golem/gargoyle/general models load from their MONSTERS/<desc>/<dir>
  packages (run_play `lib`).
- Verification: 50 core + 25 formats + 2 client Rust tests (synthetic),
  Clippy clean, release build. Fixture memory checks: LEVL record, lookouts,
  item records. No new PCSX2 encounter comparisons.

- Campaign: `run_play.py --campaign` starts in the Tower hub (exits gated by
  progress) and loads realm packages per level. Local co-op (interim): up to
  four players; gamepads through Bevy's bevy_gilrs feature (added 2026-10-17:
  bevy_gilrs 0.18.1, gilrs 0.11.2, gilrs-core 0.6.8 and their small deps,
  pinned in Cargo.lock; links the system libudev).

Known interim limits: interim player controller (melee holds still, hit
reactions interrupt throws), WeapThrowFx placeholder, wave exits/secret realm
(SaveStage/LoadStage) co-op join/camera rules (M16) interim, original menus (M17) not
started, saves (M18).

---

# Development checkpoint — 2026-10-16

Continuing the user's request (2026-10-15/16) to work through M11–M17 until a
playable build exists; the user will test at the end. Latest source commit:
find with `bash tools/git-local.sh log -1 --oneline`.

## Current position (2026-10-16)

- Native gameplay simulation (research/gameplay.md): items, pickups, enemies
  and generators, enemy AI subset, damage, player missiles, potion magic,
  experience/levels, exits and level progression G1→G4.
- Interim playtest (research/playtest.md): `python3 tools/python/run_play.py`.
  Start state v6 (`export_game_state.py`) plus the v2 player sim start.
- Headless runner: `python3 tools/python/game_run.py REPORT --level N --script ...`.
- Verification at this checkpoint: 43 core + 21 formats Rust tests (synthetic),
  strict Clippy clean, release build; headless G1–G4 runs without unsupported
  paths; Xvfb smoke of G1–G4. No new reference (PCSX2) comparisons were made for
  the gameplay transcriptions — they are not fidelity evidence yet.
- The 2026-10-15 Xvfb/PCSX2 session no longer existed on 2026-10-16; a new
  authenticated Xvfb (:191) was started via xvfb-run + virtual_display.py.
  PCSX2 was not restarted. Originals and copied slot 1 untouched.

Next: critters (GENERAL/GOLEM/GAR/DEATH), melee and enemy missiles, audio and UI
(M17), boss level G5 (M15), co-op (M16), saves (M18); reference encounter
captures to qualify the transcriptions.

---

# Previous checkpoint — 2026-10-15

Work resumed on 2026-10-14 in a new agent session after the 2026-10-13 pause.
This checkpoint records the PlayerMotion request/facing milestone and the resume
point. No question, approval or manual user test is pending.

The target remains the supplied PS2-to-native Rust/Bevy master plan, M00–M21,
with standalone Linux and Windows gameplay. There is no native playable encounter
or complete port. Existing Bevy applications are original-asset inspectors;
headless core helpers implement only the qualified subsets below.

## Exact position in the plan

M00–M02 are complete. M03 is a partial repeatable reference baseline. M04 archive
extraction is complete, but individual runtime resource lookup proof remains open.
M05 materials and M06 geometry remain partial. M07 is the active rig/animation
milestone; verified local poses, clocks, selection and a bounded world helper do
not close its renderer/scale/attachment/event gates. M08 has source investigation
leads and no accepted native collision queries. M09 has verified bounded clock
and logical motion-intent helpers, without a complete simulation/input contract.
M10 has a qualified pre-collision request/facing component and original libm
trigonometry, not native movement. M11–M18 gates remain unimplemented. M19 is
deferred; M20 platform QA and M21 release qualification have not been reached.
See [the acceptance ledger](coverage.md) for every milestone and remaining gate.

## Source identities and preserved reference

Profile: `profiles/ps2/slus-20047.json`, ID `ps2-slus-20047-fb47d40667df`.
Runtime offsets/guards: `profiles/ps2/slus-20047-reference.json`.

- ISO: 620,447,744 bytes; SHA-256
  `fb47d40667dfd5b51ffdf2cf7030a3737510b0c4f29dada2a953cb34c65545f2`.
- SLUS_200.47: SHA-256
  `69056dc6969c3740a82422ecdf7f771b19b26e20aff208d620c6efec66a3d527`.
- WAD.BIN: SHA-256
  `e047a4e060c2372407bd6e00c380602dcddeadb0f31637ba88630a8c029da946`.
- User fixture: Yellow Sorceress, The Poison Fields, profile PELE;
  `WorldDir=levels/levelG1`, archive entries 2194–2198.
- Preserved original: `private/reference/captures/poison-fields-original.p2s`;
  SHA-256 `cc9b835f2d061bfc8a7950fb75b7a621d4e7217738a3aebcfb58609171a27399`.
  Copied slot 1: `private/reference/config/PCSX2/sstates/SLUS-20047 (15030637).01.p2s`.
  Frozen RAM: `private/reference/captures/poison-fields-snapshot/eeMemory.bin`.
  Never overwrite the original or copied slot 1.

The source, BIOS and saves remain unchanged. All retail assets/captures/reports
stay in ignored `private/`. Root `.git` is session-reserved; source history is in
`private/source-control.git`, accessed using `bash tools/git-local.sh ...`.
User directories `.vscode/` and `ghidra-emotionengine-reloaded/` are untracked and
must remain untouched. The user's `pcsx2/` checkout is a separate research source.

## Implemented and measured

- Disc/ELF: 56 files, 8,836 symbols and 3,584 named original functions inventoried.
  All 2,915 archive outcomes independently agree; unknown bytes/identities retained.
- Assets: 651 texture tables / 33,227 records read; six base color families decoded.
  Format 146 = lightmap indices (research/rendering.md); 130 and TEXA
  variation beyond the G1 frame remain open. GS dump
  reader retains 1,806 events. CT16 alpha selection is still diagnostic.
- G1 scene: 1,729 integer packets independently agree, 944 vertex counts match;
  1,360 world nodes / 3,461 collisions retained. Native diagnostic viewer places
  1,706 parts; glTF imports 930 placements into Blender. Sixty-two declared
  triangle-count discrepancies, original transforms/materials and collision
  semantics remain unresolved.
- SOR animation: 35 nodes, 137 clips, 4,795 skeletal records; all 76,542 keys
  decode. Measured key caches: 116 READY, 1,542 walking and 2,356 attack observations;
  2,011 evaluated channel records agree byte for byte. Main SOR initial positions
  bind to all 35 live nodes; 30 YEL00 initial positions differ and are not substituted.
- Local transforms: all 35 frozen nodes and 2,170 walking/attack local pose records
  match. Each repeated 49-sample walk-release trace has 1,715 matching local poses,
  including nonzero READY blending. Mirror/reverse paths remain unaccepted.
- Native clock: InitAnim, CalcAnimInfo and bounded AnimateTree selection retain
  original source order and numeric policy. Timing factor scales period, not FPS.
  All 13 clock fields agree in 28 same-clip updates, eight supplied initialization
  cases, and measured nonzero blending.
- Bounded native player animation selection agrees in 24 walking-release and
  18 attack updates, each repeated. Original requested actions are supplied;
  native pad-to-requested-action production is not implemented. Action IDs and
  clip IDs have distinct source-bound mappings.
- Ordinary class-6/no-weapon throw event helper agrees on event word/start timestamp
  in 18 attack and 24 walking updates, repeated. Starts occur at vblanks 2/24;
  missile request markers at 14/36. This predicts requests, not actual projectile
  creation, hit, damage, direction/origin or sound. Measured charge here is zero.
- Source-order GetWorldMat/MulMat4 helper matches 64 output bytes in two natural
  translation calls, one with observed return 0. A deliberately substituted live
  bone argument, with eight nontrivial ancestors, matches 64 bytes and return 0.
  This controlled probe is not natural renderer/attachment evidence. Case 002
  missed its return stop and is excluded. Render scale inheritance is not recovered.
- Latest bounded logical motion-intent helper matches stored code and run-delay
  timer in 24 walking updates twice and 18 attack updates twice. It consumes
  original logical control records and prior player context, not keyboard input.
  Run intent 13 is downgraded to 8 while prior timer is below literal 0x3dcccccd;
  first full run code occurs at vblank 8. Attack code 15 resets the timer. Actual
  displacement, requested-action mapping, camera and collision remain unimplemented.

- PlayerMotion's ordinary pre-collision displacement request (+0x860..+0x868)
  and facing turn (+0x86c) are native (gdl_core/player_motion.rs), using native
  reconstructions of the executable's fdlibm-structured atan2f/atanf/sinf/cosf
  (gdl_core/libm.rs) over the qualified scalar FPU subset. They match 112/112
  updates byte for byte across Up (twice, raw-identical), Down, Left, Right,
  Up+Right and Down+Left, including a turn decided by rounding at the π wrap.
  Diagnostic: position + request reproduces 110/112 positions; Left vblank 18
  and Up+Right vblank 4 were adjusted by a later non-wall stage (first collision
  divergences). Reports: `private/reports/sor-motion-request-*-v2/`; raw traces
  `poison-fields-move-*.json`. Dossier: research/movement.md.
- Debugger stops place both position adjustments in PlayerCollideFloor's return-2
  pullback to FloorCollide's contact (`motion-stage-probe-*-v1/`).
- Native static world collision (gdl_core/collision.rs, research/collision.md):
  WORLDS triangles/grid load unchanged; all 1,360 runtime object positions are
  reproduced bit for bit; 85/85 distinct live WorldCollide queries match in every
  captured result field (`collision-comparison-v3/`), on one flat floor object.
- Native player displacement with static collision matches 96/96 updates
  (`displacement-comparison-v4/`); native InitJoyAng reproduces 98/98 control
  table words from the ELF; the closed-loop PlayerSim reproduces 96/96 positions
  and 92/96 facings (`closed-loop-comparison-v2/`).
- INTERIM PLAYTEST: `python3 tools/python/run_play.py` (research/playtest.md);
  labelled interim pieces include the start state, action pairs, matrix rebuild,
  animation clock, rest-state camera and alpha cutout. Not the M12 slice.
- fqdist's VU0 microprogram (pc 68..75, decoded from the fixture's VU0 micro
  memory) is native in gdl_core/vu0.rs with toward-zero VU mul/add/sqrt; 15/15
  distinct live operand/result pairs match in all lanes (`fqdist-comparison-v3/`).

Motion-code comparison artifacts: `private/reports/sor-walk-motion-code-v1/` and `-v2/`
(24/24 each), `sor-attack-motion-code-v1/` and `-v2/` (18/18 each).
Complete walking raw traces are `poison-fields-walk-motion-code-b.json` and `-c.json`;
attack raw traces are `poison-fields-attack-motion-code-a.json` and `-b.json`.
Walking `-a.json` lacks the later state-934 context and is incomplete evidence.
Corrected header binding event reports: `sor-attack-events-v3/` (18/18) and
`sor-walk-release-events-v2/` (24/24). No divergence in these qualified subsets.
The open producers are requested-action mapping and the collision stages after the request.

The sequence words are now distinct: `root_transfer_flags` at +0x26 and
`frame_order_flags` at +0x2a. Both are zero in all 137 SOR clips, so earlier SOR
observations could not detect the old binding error. Synthetic distinct-word
coverage checks the corrected binding. Root transfer is not exercised by SOR.

## Verification at this checkpoint

Pinned Rust 1.93.1 / Bevy 0.18.1; offline, locked, release. Run separately on
2026-10-15 after the collision work; each exited 0:

- Rust workspace tests: 57 passing (38 core + 19 formats);
  `private/reports/native-collision-tests-v1.log`.
- Strict Clippy: `native-collision-clippy-v3.log` (v1/v2 failed on negated
  float-comparison lints, fixed; retained, not passing evidence).
- Release build: `native-collision-build-v1.log`; Python: 5 passing,
  `native-collision-python-tests-v1.log`.
- With the final binary: WorldCollide 85/85 (`collision-comparison-v3/`), fqdist
  15/15 (`fqdist-comparison-v4/`); motion request/facing 112/112 last rerun in
  `sor-motion-request-*-v3/` (that code path is unchanged since).

The libm/motion-request Rust tests are synthetic. Fidelity evidence is the
owned-data comparisons above; none of them substitutes for collision or gameplay.

Historical corrections remain: earlier combined-shell output masked a stale
fixture; failed `native-world-*-v2.log` and `native-motion-code-*-v1.log` are
retained and are not passing evidence.

To use the installed pinned toolchain without rustup home writes:

```sh
export PATH=$HOME/.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin:$PATH
cargo test --offline --release --locked --workspace
cargo clippy --offline --release --locked --workspace --all-targets -- -D warnings
cargo build --offline --release --locked --workspace
python3 -m unittest discover -s tools/python -p 'test_*.py'
```

## Environment left for resumption

All automated GUI work used authenticated Xvfb, never the user's Wayland desktop.
PCSX2 is stock `/usr/games/pcsx2-qt`, 2.6.3+dfsg-1build1; private configuration,
software GS, PINE default slot 28011. Session identities: `private/reports/reference.pid`
and `private/reference/xvfb-session.json`. On 2026-10-15 the reference (PID 2643374)
was still running on Xvfb :191 (server PID 2638848). An unrelated Xvfb using
`-displayfd`, started 2026-10-14 11:09, is not part of this project and was not
touched. Inspect identities before reuse rather than assuming these PIDs persist.

Each movement trace reloaded the unchanged copied slot-1 fixture through the full
barrier, which now also includes the entire slot-0 player record, controls, camera
block and movement globals. Traces only press keys and read memory; no guest
writes or saves. Last observed reference state: paused after the Down+Left
schedule with keys released. Originals and copied slot 1 are unchanged.

Debugger probes (2026-10-15) used three execute breakpoints, now **disabled** but
still listed in the stock debugger. Afterwards input was cleared via the render
window and the fixture reloaded through the barrier; the reference is paused
2 idle vblanks after reload. Key releases must target the render window.

Tooling changes: `reference_frame_capture.py` (read-only debugger-stop frames),
`reference_trace.py --leave-held`; `tools/ghidra/ExportResearch.java` accepts `@ADDRESS` entries for
label-only helpers; `reference_trace.py --movement`; new `reference_movement.py`
and `compare_motion_request.py`; profile `movement_layout` and four ELF-verified
symbols (MboxWinPtr, QuestItemState, BossActive, WldBtm).

## Exact next steps

1. Read this file, coverage, movement, clock, animation, world-transforms and
   unknowns. Inspect surviving Xvfb/PCSX2 identities; reload the preserved copied
   fixture with the full barrier before new schedules. Never overwrite originals.
2. Done 2026-10-15: both collision divergences are PlayerCollideFloor return-2
   pullbacks to FloorCollide's contact point (floor object "G1#109"); see
   research/movement.md. Next recover FloorCollide 0x11ec58 (and WorldCollide
   0x11e160 if called), the world-collision record layout it reads, and the binding
   from runtime floor objects to parsed WORLDS.PS2 nodes by name. FloorCollide is
   a vertical radius sweep through WorldCollide → NextGrid/WorldObjCollide/
   CTriListCollide/WorldDynCollide (exports in private/reports/ghidra-world-collide/).
   fqdist's VU0 microprogram is native and matches 15/15 live results. Validate
   collision with bounded debugger stops like motion-stage-probe-*.
3. Static floor queries are native (85/85). Next: PlayerCollideFloor 0x145218
   (incl. SlideAlongWall), PlayerCollideWalls 0x144f28, PlayerNewFloor 0x145988,
   CameraLimitPlayerDpos 0x1a2428, then compose request → final displacement →
   position and compare all 112 updates (including the two pullbacks).
   The user asked (2026-10-15) to keep going until a playable build exists; an
   interim, clearly labelled walk-around playtest build follows native movement
   with floor/wall collision, then the M12 slice.
4. Recover PlayerControls 0x105718 pad → control record production (angles,
   magnitude, held/edge words), then the facing-matrix rebuild (CreateYPRMatrix)
   and camera update, toward M10 replay.
5. M07 renderer matrix/scale traversal, attachments, mirror/reverse and event
   consumers; M05/M06 material/GS/TEXA and triangle discrepancies.
6. M09 input edges/update order/RNG; M11 missiles/enemy/damage/pickups; M12 slice.
7. M13–M21 only after their dependencies, as in the master plan.

Source checkpoints: `ad798e1` world helper, `a0cb2fb` motion intent and pause.
The following commit contains the libm/request/facing work and this checkpoint;
find it with `bash tools/git-local.sh log -1 --oneline`.
