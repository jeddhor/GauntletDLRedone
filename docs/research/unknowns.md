# Unknowns and next discriminating experiments

Evidence is qualified by profiles/ps2/slus-20047.json. No unknown is silently
replaced by a plausible gameplay rule or generated asset.

| Question | Current evidence | Next experiment |
| --- | --- | --- |
| Retail revision label | Exact disc/ELF/archive hashes and serial established | Optional trusted revision catalog; exact-hash profile is already usable |
| Full game types | Symbols recovered; initial STABS output contains library/ELF structures | Inspect .mdebug records/analyzer options; recover game layouts from instruction consumers if absent |
| Runtime code overlays | DVP payloads and IRX modules inventoried | Trace runtime file/module requests and address reuse |
| First slice resource IDs | User chose Yellow Sorceress / The Poison Fields | Bind paused reference globals/lookup traces to archive IDs |
| Material/color pipeline | Title RGB/orientation plausible; raw alpha preserved | Compare upload flags, blend equations, known transparent/background pixels |
| Five remaining pixel formats | Records retained; numeric inventory known | Recover each upload/consumer and compare distinctive samples |
| Mipmap semantics | Raw counts/metadata retained | Decode sizes/layouts and compare native/reference LOD selections |
| Original geometry | Object headers/format leads available | Bounded mesh/VIF investigation for slice static segment |
| Animation/rig | Function names and upstream schema leads | One local rig/idle/attack sampler experiment |
| Collision and navigation | Local WorldCollide and player wall functions identified | Recover precise query inputs/outputs before native response implementation |
| Update/input cadence | Clock/player-control disassembly exported | Guest-update-boundary captures, vblank/tick deltas, input-edge experiments |
| RNG reproducibility | Main loop reseeds using CPU time in static path | Observe actual seed calls and consumers with frame/timing context |
| Damage/enemy rules | Local original-symbol functions exported | Controlled encounter scenarios; minimize first divergence |
| Reference stepping transport | Stock PINE has inspection/state commands, no input/update stepping | Validate GUI debugger/input recording; instrument only after measured blocker |
| Reference scenario reproduction | Startup and user-selected playable area captured | Save/reload and repeat idle/movement/attack/pickup/transition scenarios |
| Native saves/campaign/co-op/audio | Not implemented | Follow dependencies M10–M18; no release-scope claims yet |
| Windows runtime | Not built/tested | Platform builds and real Windows QA at M20 |

External data consultation cannot establish PS2 offsets or behavior without local
verification. Four unnamed archive entries remain after two new hash/name matches;
the original six are all retained by stable identity. Some contain development text
which is data only and must not be executed.
