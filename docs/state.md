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
