# M09 investigation: loop/input/timing, not yet an accepted simulation

Exact ELF identity is in profiles/ps2/slus-20047.json. Original-symbol exports
and instruction listings are in private/reports/ghidra[-clock]/.

Observed static paths:

- main at EE:0x001001e0 calls the clock, LoadVU1GameLogic, PlayerControls,
  options, audio, game_main, camera and frame-end paths in that order, subject
  to branches. More detailed game_main entity order needs recovery.
- main calls srand(pbGetCPUTime() >> 3) within its loop. CPU-time-derived
  reseeding is a critical reference/replay concern; a fixed initial seed alone
  cannot be assumed to reproduce it.
- ClockOncePerFrame at EE:0x00102e68 reads vb_counter deltas, CPU ticks and
  pause/menu/debug controls. Its ordinary nonzero bounded-delta path computes
  SPF from vb_elapsed / 60.0 and FPS from 1/SPF. Special paths use zero elapsed
  or an explicit two-vblank / 1/30 fallback. It increments AFrame and updates
  Time/ITime conditionally. Actual guest-update/vblank cadence must be measured.
- LoadVU1GameLogic at EE:0x00100de0 is exactly jr ra + nop in this build.
  Its name alone would have misleadingly suggested mandatory VU gameplay logic.
  This says nothing about the semantics of other vector programs.
- GetPlayerDMotion at EE:0x001448e0 and PlayerControls at EE:0x00105718 have
  source names but provisional layouts/parameter types. Do not implement a
  player state by blindly accepting decompiler casts and offsets.

PCSX2 logs report NTSC fields and varying interlaced CRTC configuration. These
are presentation/reference observations, not proof of a fixed native simulation rate.

The inspected PCSX2 source includes PINE read-memory/status/context and state
operations. tools/python/pine_probe.py implements a bounded read-only subset,
validates four loaded function-entry probes against the exact ELF and tags
EE addresses. Synthetic transport tests and the enabled live connection pass.
Live reads are explicitly non-atomic; even
paused reads are diagnostics until tied to a documented guest boundary.

Stock PINE does not provide the proposed input schedule or guest-update stepping
contract. A GUI debugger/input-recording workflow may suffice initially. A source
patch is justified only if that remains a measured capture blocker.

The bounded Xvfb keyboard/PINE workflow now verifies one vblank per F10 step.
In the preserved Poison Fields fixture, AFrame advances every second vblank,
SPF remains 1/30, and two idle plus two straight-movement runs are identical
at every sampled clock/control/player field. See reference-baseline.md. This
establishes a short repeatable fixture, not a universal fixed 30 Hz contract.


## Ordinary logical motion intent and run-delay timer — paused checkpoint

This bounded native subset is implemented in gdl_core/player_input.rs and exposed
through gdl_tools motion-code. It is logical action intent, not displacement.
Original GetPlayerDMotion 0x1448e0 classifies the accepted control word/magnitude
branches; PlayerMotion 0x1480a8 applies the run-delay timer at instructions
0x14a000–0x14a054 and stores the resulting code at player+0x1dc (0x14a09c).
BossType is an ELF-symbol/source-bound signed word at 0x25d2c4, observed -1.

Accepted inputs: primary held word at PlayerControl+4, edge word+8, magnitude+0x20,
secondary magnitude+0x28 zero; player status halfword+0x93c bits0x40/0x10/0x80
clear, signed state halfword+0x934 zero, flags+0x138 bit0x400 clear, word+0x80c
zero, bounded current action 0/17–20/91–92/95–98, BossType negative, finite
nonnegative walking timer+0x874, positive normal SPF. Primary special bits
0x10000/0x8000/0x100/0x20000/0x4000/0x08000000 and edge bits0x800/0x1000/0x2000
are rejected. Magnitude is zero or normal in0..1. Unsupported branches are
reported as errors rather than approximated. These bounds intentionally leave
forced modes, angle-dependent actions, secondary input and boss states open.

Held bit0x200 returns base code15, preceding bit0x400 code16. Otherwise magnitude
above0.75 yields13, positive magnitude8, zero1. For base13 and prior timer below
source literal0x3dcccccd, negative BossType selects code8. Base8/13 increments
timer with source-qualified guarded RZ addition of SPF; other codes reset it.
This output is the stored motion code before later PlayerMotion overrides/action
mapping. It does not recover every other GetPlayerDMotion/PlayerMotion write.

reference_animation.py now captures walking timer, stored code, signed-state
word pair and BossType and includes them in the frozen reload barrier. The
comparator uses original logical control records from the observed completed
update, prior player context, and observed SPF. Non-updating vblanks are excluded.
It predicts code/timer only; it does not validate keyboard polling, latency or
input-edge production. Raw trace/source/fixture identities and first divergence
are retained alongside bounded requests/native outcomes.

Complete walking raw traces poison-fields-walk-motion-code-b/c are identical at
every sampled field, as are attack-motion-code-a/b. Walking-a predates the state
934 capture and is incomplete evidence. Native sor-walk-motion-code-v1/v2 each
match24/24 updates; sor-attack-motion-code-v1/v2 each match18/18, including timer
float bytes. Walking timers at vblanks2/4/6/8 are0.033333335/0.06666667/
0.10000000149/0.133333325; code8 occupies the first three updates and13 begins at8.
The threshold observes the prior timer, so rounding/order matters. Attack code15
resets the timer to0. Synthetic coverage checks the magnitude cutoff, exact timer
threshold, primary-button precedence and rejected inputs; it does not substitute
for reference coverage of code16 or unsupported paths.

Next producer/displacement evidence is in ghidra-player-motion-events/PlayerMotion
and ghidra-movement-support/. Source PlayerCollidePlayers 0x146030 returns another
player slot or -1, filtering other player+0xfc activity1/4 and status+0x93c bit0x20.
It is not a floor query. Fixture activity words are[1,0,0,0]; a runtime capture
binding for activity remains pending. PlayerMotion reuses s4 with another meaning
from0x1495fc, so its later value cannot be interpreted as that earlier slot.
Movement calls libm sinf0x1e7350/cosf0x1e7160, using range reduction0x1eae30 and
kernels0x1ebdf0/0x1eb348. These are distinct from animation ffSin/ffCos; native
movement must not silently reuse animation trigonometry.

M09 remains partial: native pad polling/mapping/edges, requested-action producer,
actual displacement/collision/camera, complete update order and RNG remain open.
Work resumed 2026-10-14. The displacement request and facing turn that consume
this code are now recovered for the ordinary path; see movement.md. Current
check results and logs are in ../state.md.
