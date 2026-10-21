# Unknowns and next discriminating experiments

Evidence is qualified by profiles/ps2/slus-20047.json. No unknown is silently
replaced by a plausible gameplay rule or generated asset.

| Question | Current evidence | Next experiment |
| --- | --- | --- |
| Retail revision label | Exact disc/ELF/archive hashes and serial established | Optional trusted revision catalog; exact-hash profile is already usable |
| Full game types | Symbols recovered; initial STABS output contains library/ELF structures | Inspect .mdebug records/analyzer options; recover game layouts from instruction consumers if absent |
| Runtime code overlays | DVP payloads and IRX modules inventoried | Trace runtime file/module requests and address reuse |
| First slice lookup trace | Frozen/live WorldDir=levels/levelG1 binds entries 2194–2198 | Confirm each original loader request and external dependencies |
| Material/color pipeline | Title RGB/orientation plausible; raw alpha preserved | Compare upload flags, blend equations, known transparent/background pixels |
| Remaining alpha-only pixel formats | Six base color families and format 146 (lightmap indices, research/rendering.md) decoded; format 130 unresolved | Recover upload/consumer semantics and compare distinctive samples |
| Lightmap CLUT source | Observed in the G1 dump (white, alpha 8·i); generating code not found; assumed for all packages (HYPOTHESIS) | Find the CLUT upload in pbLoadLightMaps/pbInitTexture or a second level's dump |
| setPrimColor scale / VU1 lighting | Colour scales (MBLightmapMod, +0xa8/+0xb8) and the VU1 microcode use are not recovered; vertex colours applied as observed | Export the VU1 microcode or compare GS vertex colours with DoLighting active |
| Mipmap semantics | Raw counts/metadata retained | Decode sizes/layouts and compare native/reference LOD selections |
| Original geometry | All 1,729 G1 integer packets independently agree; native terrain viewer runs | Resolve 62 triangle-count discrepancies, original transform/winding and material consumers |
| Animation/rig | 35-node SOR tree, 137 clips, frame-zero reader and native rest-hierarchy inspector | Local channels/matrices/blending and bounded player animation choice now compare exactly; world helper compares two natural translations and one controlled bone chain; recover render scales, attachments and full events |
| Animation cache words | 35 live caches matched to SOR; +0xc-as-clip-ID assumption contradicted | Trace frame/data indexing consumers before using cache fields as comparison output |
| Collision and navigation | WorldCollide/wall/item/player functions identified; PlayerCollidePlayers returns a player slot, not a floor result. Request +0x860 equals final displacement in 110/112 measured updates; debugger stops show both exceptions are PlayerCollideFloor return-2 pullbacks to FloorCollide's contact on floor object "G1#109" | Recover FloorCollide 0x11ec58 and its world-collision records; bind runtime floor objects to parsed world nodes by name. fqdist's VU0 microprogram is now native (15/15) |
| Update/input cadence | Fixture updates every two vblanks; bounded clock and logical code/run-delay timer match42 updates; native request/facing match112 updates over eight control angles, with original controls supplied | Recover PlayerControls pad→control production, input edges, requested-action mapping, full update order and other mode cadence |
| Movement trigonometry | Original fdlibm-structured atan2f/atanf/sinf/cosf implemented natively from instructions; exercised through112 request/facing matches | Large-argument reduction 0x1eb4a0, exceptional/subnormal domains, direct function probes if a later divergence implicates libm |
| RNG reproducibility | Main loop reseeds using CPU time in static path | Observe actual seed calls and consumers with frame/timing context |
| Damage/enemy rules | Transcribed natively (gameplay.md); headless synthetic scenarios run G1–G4 without unsupported paths | Controlled reference encounter captures (enemy spawn, hit, kill, generator damage) and first-divergence comparison |
| Player action production | Ordinary throw subset native; magic/turbo/melee requests come from animation events not yet produced natively | Recover PlayerMotion's pad → requested action and DoPlayerAction event production for magic and melee |
| Derived combat stats | Resolved: check_player_atts' Get<Att> ranges reproduce +0x118..+0x12c of the captured record (profile-check); PlayerProcessPowerups re-derives and clamps them every update | — |
| Enemy per-frame objects | Resolved: OANIM entries (tree header +8 + node +0x34, 0x28 each: first object, count, start) drive the frame object; reverse-order sequences not modelled in the client | — |
| Reference stepping transport | Xvfb F10 verifies one vblank per step; paused PINE reads repeat | Tie captures to actual guest function/update edges; verify debugger sample |
| Reference scenario coverage | Full-heap reload guard; idle/movement/attack-state/one-key schedules repeat | Recover complete attack events and transition/game-save scenarios |
| Animation numeric policy | Native finite numeric/channel subset matches walking/attack keys and2,011 evaluated records | Local matrices now match2,170 observations; bounded animation selection matches42 updates; world helper has three qualified calls; recover render scale/attachments, mirror and remaining events; exceptional numeric domains and long interpolation gaps remain |
| Native saves | Implemented (research/saves.md): envelopes, backups, recovery, raw-save migration; synthetic tests and the PELE record check | Power-loss interruption on real storage not testable here; multi-player ownership (co-op players save nothing) |
| Original memory-card saves | Reader is structural only; the user's cards are unformatted and the private copy is empty | Decode a real BASLUS-20047save file when one exists (a copied card) and compare with the savestate region |
| Level-start runtime fields | profile::level_start ports load_player_geo/load_player stores; +0x800/+0x804 (100.0 in the capture), +0x7b8..+0x7d0, +0x974..+0x980 and reset_player_powerups are not reproduced | Trace the writers of +0x800 and +0x974 and port reset_player_powerups |
| Secret characters and Sumner | Classes 8–15 and Sumner (player_sumner_desc) refused at load | Port the unlock rules, per-colour clip packages and the Sumner branches |
| Front-end presentation | Menus are native text with the original structure/labels over the original title picture; text ROMs decoded (hints, scroll messages); FNT fonts, the SELECT scene, help_displayed rules and ScrollText's runtime order unverified/not ported | Decode the fonts; port the select scene and NewHelp's repeat rules; confirm ScrollText order in a capture |
| Shop presentation, inventory, hints, settings | Shop purchases ported with a text list (the 3D item display, shop music, final-stats screen and the shop on the way back to the Tower are not); inventory, hints and controls/screen/compass options not ported | Port draw_shop_screen/shop_show_*, the inventory screen, hint_menu and the options |
| Death and continue | kill_player, PlayersSetWaveDied (WorldNumLevels = LEVL count, HYPOTHESIS), drop_keys, PlayerRestoreState in the Tower ported; the dying action 0x7e and its counter are replaced by a 2 s wait; the continue/join screen not ported | Port the dying action and the continue screen |
| PlayerProcessPowerups presentation | Familiar (phoenix), growth scale (PlayerProcessScale), see-through, skin effects and attached models not ported | Port PlayerProcessFamiliar and PlayerProcessScale (simulation-relevant) |
| Missile textures across packages | Thrown trees whose textures live in another package draw magenta (Sorceress WND_THROW1) | Resolve textures through a global MBOX texture namespace |
| Windows runtime | Not built/tested | Platform builds and real Windows QA at M20 |
| Loaded MBOX object tables | SetSafeRock/ItemFindMBObjectL1 search every loaded model table; native uses the labels of all runner packages (HYPOTHESIS) | Trace the level's package load list (LoadLevel) and model_count |
| SetPlayerVars side writes | Called every frame from game_main (and ProcessItems); StopTime, EnemyShrink, ActivePlayerMask, +0x93e and SetNumPlayers ported; NumVirtualPlayers' 0x20f954 override and NumLivePlayers not captured; ProcessItems' 0x220170 → 0x220160 store not ported | Capture 0x20f954 |
| Good-wizard speech timing | Phases wait 6 s each (CaptionText/AudioStillPlaying not simulated) | Measure the sequence in a reference boss kill |
| CritterDamagedPlayer anger value | f12 after damage_player is undefined; effect damage used | Read the value in a reference capture |
| Rotated node composition | Rotators compose rotated nodes as rows × parent rows (unverified) | Transcribe GetWorldMat and compare a rotator level |
| Gameplay camera | StdCam module ported (research/camera.md): 250/250 traced updates and pb matrices bit-exact in G1, including transmitter switches. Stage-start zoom (StdCamStartUpdate) and its stack-record contents (HYPOTHESIS: zero) are not trace-checked; fixture-started runs settle with StdCamInit(1) (HYPOTHESIS); trigger/boss cameras (old Cam array, do_camera) not ported; source of the 64-row viewport inset and 384 window height not identified | Trace a level start (camera from the start transmitter through the zoom); port do_camera/StandardCamera for trigger and boss cameras; find the window setup writer |
| Interim throw hold | G1 teleport test: player stayed in action 0x61 for ~900 updates with one missile static (interim player) | Reproduce with the reference player controller |
| Co-op join and camera | Joining players copy player 1's record; camera = average of living players | Port the join flow (LoadPlyrData, can_join) and the multi-player camera |
| Enemy tree alpha at spawn | enemy_alpha starts at 255 (HYPOTHESIS); presentation only | Read MBTreeGetAlpha for a fresh tree in a capture |

External data consultation cannot establish PS2 offsets or behavior without local
verification. Four unnamed archive entries remain after two new hash/name matches;
the original six are all retained by stable identity. Some contain development text
which is data only and must not be executed.

Resumed 2026-10-14 by a new agent session. The current resume queue and log paths
are in [state.md](../state.md); movement evidence is in [movement.md](movement.md).
| Physical pad → joyGetStatus entry | Entries 0–3 are the d-pad (left/right/up/down, observed level bits); remap shows attack = entry 0xd, magic 0xa, turbo 0xc, potion 0xb (by GetPlayerDMotion use) | Press each PS2 button in the reference and read plr_levels 0x2bacc0 |
| Player node under PlayerRootNode | 150 traced updates consistent with GetWorldMat = local matrix; −0.0 → +0.0 | Trace a moving floor (node parent ≠ root) and a grabbed player |
| Milestones table | update_player_milestone reads Milestones (0x70 each) / NumMilestones; kind-5 locators are the likely source | Export the Milestones builder and compare with a capture; port get_player_pos respawn |
| Panel inputs | button_x_tex/button_tri_tex (ResetModels), NumLivePlayers, menu_on/optmenu/full-screen inventory flags from the front end, MBOX texture search order across packages | Export ResetModels; front-end port |
| MBTreeSetAlpha draw use | Node +0x53 = ~alpha and flag 0x200; drawn as material alpha × (255 − alpha)/255 (HYPOTHESIS) | Trace the +0x53 byte through the draw path or compare a fading exit/crystal in a capture |
| Sumner's ghost placement | WIZARD tree at the nearest waypoint's matrix; not seen in the native Sumner camera view | Capture a shard/rune speech in the reference |
| Tree texmod frame | DoTexModNode's frame argument taken as (int)(frame + 0.5) (HYPOTHESIS) | Read AnimateNode's frame argument |
| setPrimColor +0x48 ambient term | Lightmap/no-shade add and per-tree ambient add (0..999) feed +0x48; drawn as colour × (1 + add) / lit ambient + add/255 (HYPOTHESIS) | Read the VU1 microcode's use of +0x48 or capture a flash (lightning in A5/F2, an enemy death) |
| Skin chrome (alt tex −3) | setChrome's axes ported; the VU1 UV formula (n·u + azimuth/2π, n·v + elevation/π) is a HYPOTHESIS | Read the VU1 chrome path or capture a gold/silver shield |
| Keep-A pass UVs (alt tex −4) | The alternate texture's second pass uses the base UVs (HYPOTHESIS) | Read the VU1 microcode's keep-A path or capture an enemy death dissolve |
| Player shadow splash | UpdatePlayerShadow's −2 TextureSplash drawn natively; never observed (no water entry in checks) | Walk into water in a level with flag-0x200 collision (A4, C4, I4) |
