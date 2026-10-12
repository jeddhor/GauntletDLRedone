# Current development checkpoint

Objective: complete the supplied PS2-to-native Rust/Bevy master plan milestone by
milestone. User has authorized continued development and emulator reference use.
No permission to acquire other commercial images or leaked SDK/symbol artifacts.
Single-agent sequential work; PCSX2 source checkout remains unchanged.

The session reserves root .git read-only. Source history uses the ignored
private/source-control.git metadata with tools/git-local.sh; no retail data is staged.

Source profile: profiles/ps2/slus-20047.json. Original ISO and source BIOS hashes
remain unchanged. The native workspace is at the project root. Private extraction,
reports, emulator configuration, Ghidra project/module and captures are ignored.

Active dependencies: finish M02 direct debugger decoding cross-check and M03
repeatable reference state; finish M04 first-level resource mapping. M05 metadata
and title RGB decode have a tested native viewer, with fidelity gates still open.
M09 clock/RNG/input static investigation provides concrete experiments, not a
native gameplay implementation. Full milestone statuses are in docs/coverage.md.

User-selected scenario: Yellow Sorceress, The Poison Fields. Paused reference
screen has profile label PELE, level 1, 483 health. Do not substitute a guessed
fresh state or claim exact attack/movement results from this screenshot.

Pending user setup: enable PINE in the running reference with slot 28012 and
save a reference state through the menu. Automated F1 attempts did not produce
a file, while screenshots worked. Keep the paused scenario intact. Window IDs
change when display/settings dialogs change; query current IDs instead of relying
on old IDs. Avoid input to unrelated desktop windows.

Established findings: 56 verified disc files; 8,836 ELF symbols / 3,584 named
function entries; 2,915 archive entries all independently agreeing; 651 validated
texture tables / 33,227 records; native Bevy 0.18.1 viewer runs. Initial Ghidra
STABS export recovered library/ELF structures, not player/entity types. Main loop
reseeds RNG from CPU time. Clock consumes vblank deltas. LoadVU1GameLogic is a
two-instruction no-op in this build. Five texture formats and material semantics
remain unsupported. Two previously unnamed entries have additional matching
Jester package name hypotheses; the original name report remains unchanged.

No first-divergence gameplay report exists yet: no accepted native simulation
to compare. Private source-linked disassembly/decompiler exports are under
private/reports/ghidra and ghidra-clock. Do not translate casts blindly.

Next useful actions:

1. Confirm enabled reference PINE and saved state; run the bounded memory probe
   against the expected serial and code-entry probes, preserving raw word values.
2. Extend bounded probes to WorldDir/CurLevel and selected player records with
   source-derived widths. Bind The Poison Fields to exact archive identities.
3. Verify guest debugger disassembly for representative R5900 instructions.
4. Capture/repeat idle, straight/diagonal movement and attack with documented
   stepping boundaries; recover authoritative state layout and clock deltas.
5. Recover slice mesh, animation and collision families in dependency order;
   keep hypotheses outside accepted native gameplay until comparisons pass.

Useful commands are in README.md. Tests currently pass; the most recent complete
Clippy run for the native client used release profile and denied warnings.
Python transport tests are synthetic; PINE live behavior still awaits validation.
