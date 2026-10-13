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
| M06 Geometry/first scene | Partial | All first-level integer packets independently agree; world nodes read; native terrain viewer runs; glTF imports 930 nodes in Blender. Header triangle discrepancies, original transforms/materials and static character remain |
| M07 Rig/animation | Partial investigation | Bounded metadata and fresh frame-zero readers; loaded SOR_YEL binding; native rest-hierarchy inspection. 76,542 source keys decoded; walking/attack key caches and2,011 evaluated channel records match. All35 frozen local matrices,2,170 walking/attack local poses and1,715 walk-release/blend poses match. Native bounded player animation selection matches42 game updates with original requested actions supplied; world composition, mirror flags, action production and full events remain; ordinary Sorceress request timing/event word matches18 attack +24 walking updates; source-order world helper matches two natural translation calls and one controlled bone-chain call |
| M08 Level/collision | Investigation leads | World/collision schemas and original-symbol functions; native verified queries pending |
| M09 Clock/input | Partial investigation | Repeatable vblank-stepped idle/movement fixture; one update per two vblanks observed locally. Native animation clock matches28 same-clip update states; metadata factor/period conversion recovered. Full update order, input edges, game-mode cadence and RNG consumer contract remain |
| M10 Movement/camera | Not implemented | Depends on accepted animation, collision and clock/input |
| M11 Combat/enemy/pickups | Not implemented | Original attack/damage functions identified; reference scenarios and implementation pending |
| M12 Playable slice | Not implemented | Proposed target: Yellow Sorceress / The Poison Fields, original-data encounter and transition/save |
| M13 Progression/world logic | Not implemented | Original rules and persistence evidence pending |
| M14 Content expansion | Not implemented | Private archive-family catalog exists; reachable content/support matrix pending |
| M15 Bosses/endgame | Not implemented | Phase logic, gates and critical-path coverage pending |
| M16 Local multiplayer | Not implemented | One-to-four player scenarios and ownership/camera rules pending |
| M17 Audio/UI completion | Not implemented | Source resources preserved; audiovisual/event mapping pending |
| M18 Saves/migrations | Not implemented | Native durable save/recovery contract pending |
| M19 Enhancements | Deferred | Fidelity prerequisites not accepted |
| M20 Platform/performance QA | Not reached | Linux texture-viewer smoke only; Windows and gameplay performance untested |
| M21 Release/importer | Not reached | No qualified gameplay package or fresh-install supported-revision importer |

The private catalog includes 61 dictionary-labelled WORLDS.PS2 records, 643
OBJECTS.PS2 records, 651 ANIM.PS2 records and other families. These are observed
archive entries, not a declaration of 61 campaign areas or complete asset support.

Checks actually performed: forty-five Rust parser/numeric/channel/transform/clock/selection tests, five Python transport/configuration/schedule/display-identity tests,
all-entry independent archive comparison, complete native texture-metadata scan,
byte-for-byte comparison of all 15 native title PNG pixel buffers,
repeated disc/ELF manifests, release builds and strict Clippy checks, native
window/input/alpha-mode visual smoke, native static scene captures, all-packet
independent geometry comparison, reference startup and repeatable original scene.
No unavailable fixture is counted as passing gameplay fidelity.
