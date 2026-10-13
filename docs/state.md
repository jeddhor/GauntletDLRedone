# Paused development checkpoint — 2026-10-13

Work is paused at the user's request. This checkpoint finishes the current bounded
motion-intent experiment and records the resume point. Do not start new project
work until the user asks to resume. No question, approval or manual test is pending.

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
M10–M18 gameplay/campaign/co-op/audio/UI/save gates remain unimplemented. M19 is
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

Latest comparison artifacts: `private/reports/sor-walk-motion-code-v1/` and `-v2/`
(24/24 each), `sor-attack-motion-code-v1/` and `-v2/` (18/18 each).
Complete walking raw traces are `poison-fields-walk-motion-code-b.json` and `-c.json`;
attack raw traces are `poison-fields-attack-motion-code-a.json` and `-b.json`.
Walking `-a.json` lacks the later state-934 context and is incomplete evidence.
Corrected header binding event reports: `sor-attack-events-v3/` (18/18) and
`sor-walk-release-events-v2/` (24/24). No divergence in these qualified subsets;
the next unknown is native requested-action/displacement production.

The sequence words are now distinct: `root_transfer_flags` at +0x26 and
`frame_order_flags` at +0x2a. Both are zero in all 137 SOR clips, so earlier SOR
observations could not detect the old binding error. Synthetic distinct-word
coverage checks the corrected binding. Root transfer is not exercised by SOR.

## Verification at this checkpoint

Pinned Rust 1.93.1 / Bevy 0.18.1; checks are offline, locked, release builds.
All commands completed successfully, checked separately:

- Rust workspace tests: 46 passing (27 core + 19 formats); other targets/doc tests
  have zero tests. Log: `private/reports/native-motion-code-tests-v3.log`.
- Strict workspace/all-targets Clippy: passing; log
  `private/reports/native-motion-code-clippy-v3.log`.
- Final whole-workspace release build: passing; log
  `private/reports/paused-checkpoint-build.log`.
- Python unittest discovery: 5 passing; log
  `private/reports/paused-checkpoint-python-tests.log`.

Earlier combined-shell output masked a stale test fixture that still used the
renamed `Sequence.flags` field. That fixture is fixed and checks rerun separately.
The earlier 45-test/pass claim for `native-world-tests-v2.log` and
`native-world-clippy-v2.log` was incorrect; those failed historical logs are
retained, as are `native-motion-code-tests-v1.log` and `-clippy-v1.log`.
They are not passing evidence. Current logs above supersede those claims.

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
software GS, PINE default slot 28011. Session identities are recorded in
`private/reports/reference.pid` and `private/reference/xvfb-session.json`.
Last verified virtual display: :191, server PID 2638848, authority
`/tmp/xvfb-run.V25bts/Xauthority`; holder PID 2638858. The exited wrapper left
server/holder reparented. The guard now checks actual Xvfb executable/display/auth
arguments and PID start ticks; it still refuses desktop fallback. Inspect fresh
identities before reuse rather than assuming these PIDs persist.

Last observed reference state is paused after the repeated 36-vblank K attack
schedule, with keys released. The diagnostic world probe's a0 substitution was
cleared by reloading the unchanged copied fixture; the full frozen globals/heap/
pose/action-context barrier and two verified vblanks passed afterward
(`poison-fields-after-world-probe-restore-v1.json`). No guest edit remains.
Stock debugger remains open with all diagnostic breakpoints removed
(`debugger-world-probe-cleanup-v1.png`). Native inspectors are closed. Final build
and checks have finished; no further experiment is running from this checkpoint.
Reference/Xvfb may remain idle; original/copy saves are preserved for reuse.

## Exact next steps when the user resumes

1. Read this file, coverage, clock, animation, world-transforms and unknowns.
   Inspect surviving Xvfb/PCSX2 identities; reload the preserved copied fixture
   using the full frozen barrier before new bounded schedules. Never overwrite
   originals. No user manual test is presently required.
2. Recover PlayerMotion's requested-action producer and actual displacement.
   Source exports: `private/reports/ghidra-player-motion-events/001480a8-PlayerMotion.txt`,
   `ghidra/001448e0-GetPlayerDMotion.txt`, and `ghidra-movement-support/`.
   GetPlayerDMotion is logical intent. Keep motion codes, action IDs and clip IDs
   distinct. PlayerCollidePlayers at 0x146030 returns another player slot or -1;
   it is not a floor query. Its filters read other player +0xfc activity 1/4 and
   +0x93c bit 0x20. The fixture's activity words are [1,0,0,0], but a runtime
   activity-field capture binding is still pending. PlayerMotion reuses s4 with
   another meaning from 0x1495fc; do not carry the earlier slot meaning forward.
   Movement's libm sinf 0x1e7350 / cosf 0x1e7160 differ from animation ffSin/ffCos;
   recover their range reduction/kernels and relevant statistics/scale inputs.
3. Qualify original floor/wall/item/player query contracts with bounded input/output
   fixtures, raw flags and original instructions. Implement accepted headless
   queries for M08 before claiming collision-aware native movement.
4. Qualify renderer matrix/scale traversal, actor ancestors, attachments,
   reverse/mirror and remaining event consumers for M07. Change viewer world-pose
   integration only after evidence; its cached host matrix products/decomposition
   and scale handling currently remain diagnostic.
5. Close M05 original material/GS/TEXA/alpha formats 130/146 and hand-glow dependency;
   resolve M06's 62 triangle discrepancies and transform/material fidelity.
6. Complete M09 input edges/update order/RNG; build M10 native movement/camera/replay,
   M11 real missiles/enemy/damage/pickups and M12 original-data encounter with exit
   and native durable progress. No collision or gameplay substitute closes a gate.
7. Continue M13–M21 campaign/progression/content/bosses, co-op, audio/UI, saves,
   enhancements only after fidelity, Windows/Linux QA and fresh-install release.

Source checkpoints before this pause: `e8896f3` selection, `3517cd9` ordinary throw
requests, `ad798e1` world helper/header separation. The following checkpoint commit
contains the motion-intent/timer helper, stale fixture correction and these pause
notes. Find it with `bash tools/git-local.sh log -1 --oneline`.
