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
EE addresses. Synthetic transport tests pass. It has not yet been validated
against an enabled PINE instance. Live reads are explicitly non-atomic; even
paused reads are diagnostics until tied to a documented guest boundary.

Stock PINE does not provide the proposed input schedule or guest-update stepping
contract. A GUI debugger/input-recording workflow may suffice initially. A source
patch is justified only if that remains a measured capture blocker.
