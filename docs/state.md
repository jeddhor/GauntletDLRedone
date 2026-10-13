# Current development checkpoint

Objective: complete the supplied PS2-to-native Rust/Bevy master plan milestone
by milestone. User authorizes continued implementation and reference use.
Single-agent sequential work. No external commercial images/leaked symbols.
PCSX2 source is read-only research; native code remains separate MIT code.

Source profile: profiles/ps2/slus-20047.json. Input identities remain unchanged.
Root .git is session-reserved; history uses private/source-control.git through
tools/git-local.sh. Never stage retail data, private/, user .vscode/ or the user's
ghidra-emotionengine-reloaded checkout.

Current milestone work: M03 repeatable baseline, M05 material proof, M06 static
geometry, M07 metadata and M09 clock/input experiments. M00–M02 complete; stock debugger scalar,
FP, three-operand MULT, VU and 128-bit load/store samples agree. M04 extraction is complete, with first-package
binding; individual runtime lookups remain to trace. Native gameplay animation clocks/transitions, gameplay,
collision response, saves and campaign remain unimplemented. See docs/coverage.md.

User-selected fixture: Yellow Sorceress, The Poison Fields, profile PELE. Verified
state private/reference/captures/poison-fields-original.p2s has SHA-256
cc9b835f2d061bfc8a7950fb75b7a621d4e7217738a3aebcfb58609171a27399.
It is copied to private emulator slot 1. Never overwrite the preserved original.
WorldDir=levels/levelG1 binds entries 2194–2198. Four player records, position,
collision position and health have instruction-derived offsets in the revision
reference profile. Activity flags and class-ID interpretation remain open.

The user requested Xvfb to avoid competing for desktop control. All automated
GUI work now uses virtual_display.py, held by xvfb-run in the recorded private
session. Helpers require it and refuse desktop fallback. The user allows desktop
presentation for necessary manual tests. Do not send desktop keyboard/mouse input.
Xvfb selects Mesa software presentation while PCSX2 keeps software GS; document
backend changes. PINE works, default slot 28011. Reference operations require
host namespace/socket access. Current PID/display/authority are in ignored
private/reports/reference.pid and private/reference/xvfb-session.json. Query window
IDs within that virtual session. Processes can outlive tool sessions; inspect first.

Verified this checkpoint:

- 56 disc files, 8,836 symbols / 3,584 named original functions.
- 2,915 archive outcomes independently agree; 651 texture tables / 33,227 records
  read. Six base color families implemented; two alpha-only formats remain open.
  CT16 colors use GS channel shifts; its alpha selector is explicitly diagnostic,
  not an accepted TEXA/material-alpha conversion.
- All 1,729 Poison Fields integer packets independently agree, 944 unique vertex
  counts match, source radii fit, 1,360 world nodes / 3,461 collisions retained.
  62 declared triangle counts differ; do not force a match.
- Native gdl_scene renders 1,706 placed parts with original base textures.
  Diagnostic opaque/unlit/double-sided materials remain explicit. Static alpha
  edges, lightmaps, scene transforms and VU/GS assembly remain unaccepted.
  Static glTF exports 930 placements and imports into Blender's empty scene.
- Frozen and paused state observations match after reload. Xvfb F10 advances
  exactly one vblank. In this fixture AFrame advances every second vblank with
  SPF=1/30. Two idle, Up, diagonal Up+Right and mapped Cross(K) schedules reproduce
  every sampled byte. Cross schedules do not yet establish actual attack events.
- Twenty-five Rust parser/numeric/channel/transform tests and four Python transport/configuration/schedule tests pass.
  Release scene build and strict workspace/all-target Clippy pass. Native
  overview/close-view captures remain under private/reference/captures/.
- Animation metadata: SOR has 35 nodes, 137 clips and 4,795 skeletal info records.
  Clip rates vary (10,12,15,24,30,45,60). YEL00 source entry 90 is bound to the
  frozen loaded SOR_YEL blob at 0x9bb300, with 39 relocated/flag bytes differing.
  The metadata reader does not validate frame sampling or emit animation events.
  Fresh frame-zero reader follows original 32-bit bitmap alignment and reads
  4,795 records. Native rest-hierarchy inspector shows 19 placed character parts.
  A runtime-cache comparison incorrectly treated +0xc as a clip ID; original
  and corrected reports are preserved. Resolve active clip/descriptor before
  accepting any cache comparison; do not adjust channels to hide this mismatch.
  Candidate slot0 AnimInfo at +0x94 identifies READY/frame11. Guarded toward-zero
  addition experiments reproduce 20 isolated deltas and 28 READY lower-key caches
  byte for byte. Ordinary IEEE and simple RZ variants remain as failed evidence.
  Native key accumulation and fresh channel sampling now match116 READY key
  observations,1,542 walking keys,2,356 attack keys and2,011 evaluated channel
  records byte for byte. Both new schedules reproduce all observations. All137
  clips decode76,542 keys. Local matrices/scales also match all35 frozen nodes and2,170 walking/attack
  records; main SOR initial translations are source-bound, unlike30 YEL00 values.
  World/VU composition, clocks/blending/mirror/events remain open.
  New gdl_core has no Bevy/emulator dependency. See animation.md for scope.
- Stock GS dump framing now reads 1,806 events and records raw TEXA/ALPHA/TEST
  writes (private/reports/poison-fields-gs-registers-v1.json). Material binding
  remains open. Xvfb captures require exposing the window; an occluded reference
  capture was black, while the exposed original game rendered correctly.
- One original key pickup reproduces: Right40/Down26 vblank schedule; keys at
  player+0x1a14 change0→1 at step50. Source GrabPowerup's key branch is exported.
  The segmented input tool is bounded to120 vblanks. Reload now compares the
  full animation pool to frozen RAM, because paused status can occur during
  asynchronous memory restoration. The key schedule is unchanged with this guard.

Animation exports are now in private/reports/ghidra-animation. Player/clock
exports remain in ghidra-player/ghidra-clock. Inspect original instructions before
accepting decompiler layouts. PlayerAttacking reads a signed state at player+0x8c8;
this field is now bound and two 36-vblank Cross traces agree. Enum meanings and
event timing still need validation. The loop reseeds RNG from
guest CPU time; short schedules agree, but RNG consumers/replay scope remain open.

Next useful actions:

0. Native --animate inspector now builds and renders coherent READY/WALK1/IDLE1
   poses in Xvfb; frame stepping, clip selection and preview toggles work. Attack frame2 also renders. Zero-frame INIT
   and preview-toggle checks pass. Add source metadata/bone/attachment inspection, then continue
   original world/VU composition and gameplay clock/transition recovery.

1. Extend source-linked attack/animation probes and repeat event schedules;
   establish pickup/transition/save scenarios without inventing rules.
2. Resolve geometry transforms/counts/materials using source consumers and the
   independently inspectable glTF export.
3. Recover the selected character package, rig and actual idle/attack clips from
   the original-function exports. Continue bounded consumer experiments.

reference_trace.py releases held keys on failure. Run_viewer.py also requires the
virtual session. README and baseline/geometry dossiers give commands and evidence
limits. Keep this checkpoint current as formats/configurations change. Do not
claim a complete port or native playable slice yet.
