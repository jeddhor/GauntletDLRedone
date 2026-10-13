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
| Five remaining pixel formats | Records retained; numeric inventory known | Recover each upload/consumer and compare distinctive samples |
| Mipmap semantics | Raw counts/metadata retained | Decode sizes/layouts and compare native/reference LOD selections |
| Original geometry | All 1,729 G1 integer packets independently agree; native terrain viewer runs | Resolve 62 triangle-count discrepancies, original transform/winding and material consumers |
| Animation/rig | 35-node SOR tree, 137 clips, frame-zero reader and native rest-hierarchy inspector | Verify active clip/descriptor, original channel accumulation/interpolation and matrix assembly |
| Animation cache words | 35 live caches matched to SOR; +0xc-as-clip-ID assumption contradicted | Trace frame/data indexing consumers before using cache fields as comparison output |
| Collision and navigation | Local WorldCollide and player wall functions identified | Recover precise query inputs/outputs before native response implementation |
| Update/input cadence | Clock/player-control disassembly exported | Guest-update-boundary captures, vblank/tick deltas, input-edge experiments |
| RNG reproducibility | Main loop reseeds using CPU time in static path | Observe actual seed calls and consumers with frame/timing context |
| Damage/enemy rules | Local original-symbol functions exported | Controlled encounter scenarios; minimize first divergence |
| Reference stepping transport | Xvfb F10 verifies one vblank per step; paused PINE reads repeat | Tie captures to actual guest function/update edges; verify debugger sample |
| Reference scenario coverage | Full-heap reload guard; idle/movement/attack-state/one-key schedules repeat | Recover complete attack events and transition/game-save scenarios |
| Animation numeric policy | Native finite numeric/channel subset matches walking/attack keys and2,011 evaluated records | Local matrices now match2,170 observations; recover world/VU composition and animation clocks/blending/mirror/events; exceptional numeric domains and long interpolation gaps remain |
| Native saves/campaign/co-op/audio | Not implemented | Follow dependencies M10–M18; no release-scope claims yet |
| Windows runtime | Not built/tested | Platform builds and real Windows QA at M20 |

External data consultation cannot establish PS2 offsets or behavior without local
verification. Four unnamed archive entries remain after two new hash/name matches;
the original six are all retained by stable identity. Some contain development text
which is data only and must not be executed.
