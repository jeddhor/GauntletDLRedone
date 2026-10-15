# Development checkpoint — 2026-10-15

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
  Formats 130/146 and original GS/TEXA/material-alpha binding remain open. GS dump
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

Pinned Rust 1.93.1 / Bevy 0.18.1; checks are offline, locked, release builds.
Each command was run separately on 2026-10-15 and exited 0:

- Rust workspace tests: 51 passing (32 core + 19 formats); other targets/doc
  tests have zero tests. Log: `private/reports/native-motion-request-tests-v2.log`.
- Strict workspace/all-targets Clippy: `native-motion-request-clippy-v2.log`.
  (v1 failed on one indexed loop, fixed; v1 logs are retained, not passing evidence.)
- Whole-workspace release build: `native-motion-request-build-v1.log`.
- Python unittest discovery: 5 passing; `native-motion-request-python-tests-v1.log`.
- On resume (2026-10-14), the prior 46 Rust / 5 Python results were rerun and
  confirmed before changes: `resume-2026-10-14-tests.log`.

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

Tooling changes: `tools/ghidra/ExportResearch.java` accepts `@ADDRESS` entries for
label-only helpers; `reference_trace.py --movement`; new `reference_movement.py`
and `compare_motion_request.py`; profile `movement_layout` and four ELF-verified
symbols (MboxWinPtr, QuestItemState, BossActive, WldBtm).

## Exact next steps

1. Read this file, coverage, movement, clock, animation, world-transforms and
   unknowns. Inspect surviving Xvfb/PCSX2 identities; reload the preserved copied
   fixture with the full barrier before new schedules. Never overwrite originals.
2. Explain the two preserved collision divergences (Left vblank 18, Up+Right
   vblank 4). Wall object +0x8a0 is zero in both. Candidates in source order after
   the request: PlayerCollideItems 0x146348, PlayerCollidePlayers 0x146030 (returns
   a slot or -1; filters +0xfc activity 1/4 and +0x93c bit 0x20), PlayerCollideFloor
   0x145218, CameraLimitPlayerDpos 0x1a2428, item re-check/ModifyPlayerDpos 0x147b68,
   PlayerCollideEnemies. Use bounded debugger stops or raw captures of the stack
   displacement to locate the adjusting stage; do not approximate it.
3. Qualify those query contracts and implement accepted headless queries (M08)
   before composing request → final displacement → position natively.
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
