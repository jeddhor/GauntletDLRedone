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
geometry and M09 clock/input experiments. M00/M01 complete; M02 direct debugger
instruction cross-check remains. M04 extraction is complete, with first-package
binding; individual runtime lookups remain to trace. Native animation, gameplay,
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
  read. Three base pixel formats implemented; five remain open.
- All 1,729 Poison Fields integer packets independently agree, 944 unique vertex
  counts match, source radii fit, 1,360 world nodes / 3,461 collisions retained.
  62 declared triangle counts differ; do not force a match.
- Native gdl_scene renders 1,706 placed parts with original base textures.
  Diagnostic opaque/unlit/double-sided materials remain explicit. Static alpha
  edges, lightmaps, scene transforms and VU/GS assembly remain unaccepted.
- Frozen and paused state observations match after reload. Xvfb F10 advances
  exactly one vblank. In this fixture AFrame advances every second vblank with
  SPF=1/30. Two idle, Up, diagonal Up+Right and mapped Cross(K) schedules reproduce
  every sampled byte. Cross schedules do not yet establish actual attack events.
- Eleven Rust parser tests and three Python transport/configuration tests pass.
  Release scene build and strict workspace/all-target Clippy pass. Native
  overview/close-view captures remain under private/reference/captures/.

Animation exports are now in private/reports/ghidra-animation. Player/clock
exports remain in ghidra-player/ghidra-clock. Inspect original instructions before
accepting decompiler layouts. PlayerAttacking reads a signed state at player+0x8c8;
this field is now bound and two 36-vblank Cross traces agree. Enum meanings and
event timing still need validation. The loop reseeds RNG from
guest CPU time; short schedules agree, but RNG consumers/replay scope remain open.

Next useful actions:

1. Complete the direct PCSX2 debugger instruction sample comparison in Xvfb.
2. Extend source-linked attack/animation probes and repeat event schedules;
   establish pickup/transition/save scenarios without inventing rules.
3. Resolve geometry transforms/counts/materials; export static glTF with source
   provenance and unknown sidecars for independent inspection.
4. Recover the selected character package, rig and actual idle/attack clips from
   the original-function exports. Continue bounded consumer experiments.

reference_trace.py releases held keys on failure. Run_viewer.py also requires the
virtual session. README and baseline/geometry dossiers give commands and evidence
limits. Keep this checkpoint current as formats/configurations change. Do not
claim a complete port or native playable slice yet.
