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
| Remaining alpha-only pixel formats | Six base color families decoded; formats 130/146 remain unresolved | Recover upload/consumer semantics and compare distinctive samples |
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
| Derived combat stats | PlayerUpdateAtts attribute words match the save; +0x128/+0x12c derivation (LoadPlyrData) unknown | Export LoadPlyrData and compare across a captured level-up |
| Enemy per-frame objects | OANIM node type 2; client matches object stems to sequences by frame count | Decode the OANIM channel data at the tree's blocks |
| Reference stepping transport | Xvfb F10 verifies one vblank per step; paused PINE reads repeat | Tie captures to actual guest function/update edges; verify debugger sample |
| Reference scenario coverage | Full-heap reload guard; idle/movement/attack-state/one-key schedules repeat | Recover complete attack events and transition/game-save scenarios |
| Animation numeric policy | Native finite numeric/channel subset matches walking/attack keys and2,011 evaluated records | Local matrices now match2,170 observations; bounded animation selection matches42 updates; world helper has three qualified calls; recover render scale/attachments, mirror and remaining events; exceptional numeric domains and long interpolation gaps remain |
| Native saves/campaign/co-op/audio | Not implemented | Follow dependencies M10–M18; no release-scope claims yet |
| Windows runtime | Not built/tested | Platform builds and real Windows QA at M20 |
| Loaded MBOX object tables | SetSafeRock/ItemFindMBObjectL1 search every loaded model table; native uses the labels of all runner packages (HYPOTHESIS) | Trace the level's package load list (LoadLevel) and model_count |
| SetPlayerVars side writes | ActivePlayerMask ported; 0x220158, 0x22015c/0x2201a4 scale and player +0x93e zeroing not ported; ProcessItems' 0x220170 → 0x220160 store not ported | Identify consumers before porting |
| Good-wizard speech timing | Phases wait 6 s each (CaptionText/AudioStillPlaying not simulated) | Measure the sequence in a reference boss kill |
| CritterDamagedPlayer anger value | f12 after damage_player is undefined; effect damage used | Read the value in a reference capture |
| Rotated node composition | Rotators compose rotated nodes as rows × parent rows (unverified) | Transcribe GetWorldMat and compare a rotator level |
| Camera follow | Simulation camera = player 0 rest pose after each update (HYPOTHESIS) | Port do_camera and compare camera matrices in a capture |
| Interim throw hold | G1 teleport test: player stayed in action 0x61 for ~900 updates with one missile static (interim player) | Reproduce with the reference player controller |
| Enemy tree alpha at spawn | enemy_alpha starts at 255 (HYPOTHESIS); presentation only | Read MBTreeGetAlpha for a fresh tree in a capture |

External data consultation cannot establish PS2 offsets or behavior without local
verification. Four unnamed archive entries remain after two new hash/name matches;
the original six are all retained by stable identity. Some contain development text
which is data only and must not be executed.

Resumed 2026-10-14 by a new agent session. The current resume queue and log paths
are in [state.md](../state.md); movement evidence is in [movement.md](movement.md).
