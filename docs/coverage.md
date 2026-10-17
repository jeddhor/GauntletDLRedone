# Milestone acceptance ledger

This is a native reconstruction in progress. No native playable encounter,
campaign, save system, or multiplayer gameplay is currently implemented.
Content denominators remain provisional and do not indicate reachability.

| Milestone | Status | Evidence / remaining exit requirement |
| --- | --- | --- |
| M00 Charter | Complete | Charter, native-route decision, provenance/private-data policy and measurable requirements |
| M01 Disc/ELF | Complete | Exact profile; 56-file inventory; independent extent checks; repeated extraction manifests identical; section/symbol/executable inventory |
| M02 Tool/upstream audit | Complete | Pinned audit; R5900 import/analysis; stock PCSX2 scalar/FP/three-operand MULT/VU/LQ/SQ samples agree with Ghidra and original ELF words. Decompiled types remain qualified |
| M03 Reference baseline | Partial | Durable fixture, frozen/live/heap reload checks, stock debugger and Xvfb stepping; repeated idle/movement/attack-state/key-pickup schedules agree. Full attack-event, transition and game-save scenarios remain |
| M04 Archive | Extraction complete; gate partial | 2,915/2,915 entries validated and independently compared; all retained; runtime WorldDir binds first-level package to entries 2194–2198; individual lookup trace completion remains |
| M05 Texture/material proof | Partial | Complete observed metadata scan; six base color families; title/environment/character pixels rendered diagnostically. Formats 130/146, TEXA/GS materials, mipmaps and representative fidelity comparisons remain |
| M06 Geometry/first scene | Partial | All first-level integer packets independently agree; world nodes read; native terrain viewer runs; glTF imports 930 nodes in Blender. Header triangle discrepancies, original transforms/materials and renderer/character fidelity remain |
| M07 Rig/animation | Partial investigation | Bounded metadata and fresh frame-zero readers; loaded SOR_YEL binding; native rest-hierarchy inspection. 76,542 source keys decoded; walking/attack key caches and2,011 evaluated channel records match. All35 frozen local matrices,2,170 walking/attack local poses and1,715 walk-release/blend poses match. Native bounded player animation selection matches42 game updates with original requested actions supplied; world composition, mirror flags, action production and full events remain; ordinary Sorceress request timing/event word matches18 attack +24 walking updates; source-order world helper matches two natural translation calls and one controlled bone-chain call |
| M08 Level/collision | Partial | Native static WorldCollide/FloorCollide (grid walk, object and triangle tests, edge distances) reproduces 85/85 distinct live queries byte for byte and 462/462 static item placements across the level; all 1,360 runtime object positions reproduced. Dynamic objects, walls/slopes/water, TriLineCol and the PlayerCollide* stages remain. See research/collision.md |
| M09 Clock/input | Partial investigation | Repeatable vblank-stepped idle/movement fixture; one update per two vblanks observed locally. Native animation clock matches28 same-clip update states; metadata factor/period conversion recovered. Bounded logical motion intent/run-delay timer matches24 walking +18 attack updates, each repeated, with original control records supplied. Native pad polling, requested-action mapping, full update order, input edges, game-mode cadence and RNG consumer contract remain |
| M10 Movement/camera | Partial | Native request, facing, static wall/floor collision and position composition match 96/96 measured updates byte for byte (six directions, including floor pull-backs); request/facing 112/112. Pad production, closed-loop matrix/floor-state evolution, camera, camera limits, contacts with items/players/enemies and wall hits remain; no replay or render-FPS tests yet |
| M11 Combat/enemy/pickups | Partial | Item load path matches 384/519 live records (items.md). Native transcriptions without reference comparison yet: ProcessItems cases, pickups, enemy generation/AI (logics 0/2/4/5/6/7/14/17/18/23/29/30, generator placement 12–14, phasing), enemy/player damage, player missiles, melee and targeting, enemy missiles, DamageItem (generators, chests, barrels, rocks, walls, explosions), potion magic blast, experience/levels (attribute formula matches the save). Level 1 of every realm A–K runs a scripted 900-update sweep without unported paths. Critters, bosses, shields, turbo pending (research/gameplay.md) |
| M12 Playable slice | Not accepted | Interim playtest (research/playtest.md) plays G1–G4 of The Poison Fields with the native simulation: enemies, generators, items, missiles, melee, magic, exits, original audio. Labelled interim player driver/animation/camera/materials; no reference comparison of encounters; gates not met |
| M13 Progression/world logic | Partial | Exit flow (DoExit/do_exit) and destination wave, level start locator, WldBtm, per-level enemy loading, AddExp/ModifyExp ported (gameplay.md); saves, hub/tower, persistence and rules beyond levels G1–G4 pending |
| M14 Content expansion | Not implemented | Private archive-family catalog exists; reachable content/support matrix pending |
| M15 Bosses/endgame | Partial | Critter engine (research/critters.md): boss AI, attacks, damage, BOSSGEN, death, coin spew, good-wizard timing, auto exit; grab and fire cones (B6/C5/F2/K5 run headless); the lich (G5) and the chimera with its child heads (A5) complete headless; golems, gargoyles and generals spawn from statues with CritterGolemAI, collision and waypoints. Hub/endgame pending |
| M16 Local multiplayer | Not implemented | One-to-four player scenarios and ownership/camera rules pending |
| M17 Audio/UI completion | Partial | Native audio (AUDATPS2.ROM/VBK/ADS decoders, ~35 cue ports, ALSA mixer; research/audio.md); interim title/pause/HUD/banners; original menus, front end and attention-area music pending |
| M18 Saves/migrations | Not implemented | Native durable save/recovery contract pending |
| M19 Enhancements | Deferred | Fidelity prerequisites not accepted |
| M20 Platform/performance QA | Not reached | Linux asset-inspector smoke only; Windows and gameplay performance untested |
| M21 Release/importer | Not reached | No qualified gameplay package or fresh-install supported-revision importer |

The private catalog includes 61 dictionary-labelled WORLDS.PS2 records, 643
OBJECTS.PS2 records, 651 ANIM.PS2 records and other families. These are observed
archive entries, not a declaration of 61 campaign areas or complete asset support.

Checks actually performed: 57 Rust parser/numeric/channel/transform/clock/selection/motion-intent/libm/motion-request tests (synthetic), 112 owned-data request/facing comparisons, 15 owned-data fqdist comparisons, 85 owned-data WorldCollide comparisons, 96 owned-data displacement comparisons, five Python transport/configuration/schedule/display-identity tests,
all-entry independent archive comparison, complete native texture-metadata scan,
byte-for-byte comparison of all 15 native title PNG pixel buffers,
repeated disc/ELF manifests, release builds and strict Clippy checks, native
window/input/alpha-mode visual smoke, native static scene captures, all-packet
independent geometry comparison, reference startup and repeatable original scene.
No unavailable fixture is counted as passing gameplay fidelity.

Resumed 2026-10-14. Exact artifacts, passing check logs, environment state and
ordered next steps are in [state.md](state.md). M07 and M09 remain partial; M10
has a qualified request/facing component but no gate is closed.
