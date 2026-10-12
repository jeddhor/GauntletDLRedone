# Milestone acceptance ledger

This is a native reconstruction in progress. No native playable encounter,
campaign, save system, or multiplayer gameplay is currently implemented.
Content denominators remain provisional and do not indicate reachability.

| Milestone | Status | Evidence / remaining exit requirement |
| --- | --- | --- |
| M00 Charter | Complete | Charter, native-route decision, provenance/private-data policy and measurable requirements |
| M01 Disc/ELF | Complete | Exact profile; 56-file inventory; independent extent checks; repeated extraction manifests identical; section/symbol/executable inventory |
| M02 Tool/upstream audit | Partial | Pinned audit; R5900 import/analysis tested; direct debugger sample cross-check remains |
| M03 Reference baseline | In progress | ISO boots; title/menu captures; user selected Yellow Sorceress / The Poison Fields; paused scene captured. Durable state and repeatable scenario suite pending |
| M04 Archive | Extraction complete; gate partial | 2,915/2,915 entries validated and independently compared; all retained; first-level runtime lookup mapping pending |
| M05 Texture/material proof | Partial | 651/651 observed metadata tables, 33,227 records; 15 title base PNGs; native Bevy viewer tested. Five formats, material/alpha/mipmap fidelity and representative comparisons pending |
| M06 Geometry/first scene | Not implemented | Recover original vertex/VIF/primitive/transform semantics |
| M07 Rig/animation | Not implemented | Recover original skeleton, channels, sampling and events |
| M08 Level/collision | Investigation leads | World/collision schemas and original-symbol functions; native verified queries pending |
| M09 Clock/input | Investigation | Static loop/clock/control exports; measured update/input contract pending |
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

Checks actually performed: six Rust parser tests, two Python transport tests,
all-entry independent archive comparison, complete native texture-metadata scan,
byte-for-byte comparison of all 15 native title PNG pixel buffers,
repeated disc/ELF manifests, release builds and strict Clippy checks, native
window/input/alpha-mode visual smoke, reference startup and user-selected scene.
No unavailable fixture is counted as passing gameplay fidelity.
