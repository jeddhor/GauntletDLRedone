# PlayerMotion displacement request, facing turn and original libm

Source identity: profiles/ps2/slus-20047.json; ELF SHA-256
69056dc6969c3740a82422ecdf7f771b19b26e20aff208d620c6efec66a3d527. Instruction
exports: private/reports/ghidra-player-motion-events/ (PlayerMotion 0x1480a8),
ghidra-libm-trig/, ghidra-libm-atan/, ghidra-libm-atanf/. Label-only helpers
(`gcc2_compiled.`) are exported by exact entry address (`@001eae30` etc.);
no names are guessed for them. Native code: gdl_core/libm.rs and
player_motion.rs, exposed by `gdl_tools motion-request`.

## Static evidence

PlayerMotion's pre-collision request (EE 0x148178..0x148880), reviewed
instruction by instruction against the decompiler sketch:

1. Camera yaw = atan2f(MboxWinPtr+0x90, +0x98) + 0x40490fdb, then
   GetPlayerDMotion(player, yaw) returns the base motion code.
2. For ordinary actions, angle = PlayerControl+0x1c and scale = control
   magnitude +0x20 × player+0x96c. With positive scale, direction f21 = f23 =
   wrap(angle + yaw), wrap using 0x40490fdb / 0xc0490fdb / 0x40c90fdb.
   Zero scale keeps facing +0x86c.
3. If +0x8c8 < 11 (or flags+0x8b0 bit 0x8000), knockback +0x848..+0x850 × SPF
   and velocity +0x830/+0x838 × SPF form the base; otherwise the base is zero.
4. Limit = +0x124 × SPF × 1.5 (SPF × 40 for flags 0x18160). y = base y
   clamped to [0, limit]. x/z = +0x96c-scaled magnitude × +0x124 × SPF × +0x964
   × sinf/cosf(direction) + base, each clamped to ±limit.
5. The request is stored to player +0x860/+0x864/+0x868 before
   PlayerCollideItems, PlayerCollidePlayers, PlayerCollideWalls/Floor,
   CameraLimitPlayerDpos and further item/player/enemy stages modify the
   stack displacement.
6. EE 0x14c134 copies position to +0x854..+0x85c and EE 0x14c14c adds the
   final displacement to position +0x50..+0x58 with scalar adds.
7. EE 0x14c288..0x14c340 turns facing +0x86c: current = atan2f(matrix +0x40,
   +0x48); rate = SPF × 0x417b53d1 × +0x968; difference = wrap(f23 − current);
   facing = current ± rate when outside ±rate, otherwise f23. Codes 0x22/0x23
   store f23 directly; a collision-response branch at 0x14c1bc is separate.

f23 is redefined on named special paths (status+0x93c bit 0x8 at 0x14a794,
0x149b58, 0x14b49c). The native subset rejects bit 0x8. The other two paths'
conditions are not yet modelled; agreement below is measured on the ordinary
path only. sp+0x144, which selects the collision-response facing branch, is
written only around action 0x89/0x8f collision outcomes in the decompiler
view (inferred); those actions are rejected.

The globals MboxWinPtr 0x2a1550, QuestItemState 0x23e210, BossActive 0x25d190,
WldBtm 0x23e21c, SPF 0x20f908 and BossType 0x25d2c4 match both instruction
operands and ELF symbols (size 4). PlayerControl is 240 bytes, four 0x3c records.

## Original libm

The executable's atan2f (wrapper 0x1e7b50, core 0x1e9db0), atanf 0x1e6eb8,
sinf 0x1e7350, cosf 0x1e7160, reduction 0x1eae30 and kernels 0x1ebdf0 (sin)
and 0x1eb348 (cos) have the structure and constants of fdlibm/newlib float
routines. They use only add/sub/mul/div/neg/cvt FPU instructions, with no
madd/msub, so they compose the already-qualified scalar subset: guarded
toward-zero add/subtract, toward-zero multiply and nearest division. Tables
atanhi/atanlo/aT (0x2b8c98/0x2b8ca8/0x2b8cb8) and npio2_hw (0x2b94c8) are read
from the ELF; their bytes equal the frozen RAM. `_LIB_VERSION` is 1 in the
fixture, so atan2f returns the core result for finite non-(0,0) arguments.

Native libm rejects non-finite inputs, atan2f(0,0), subnormal intermediates
and reduction above 0x43490f80 (the unimplemented 0x1eb4a0 large-argument
path). Its synthetic tests check branch constants and closeness to host math
only. Fidelity evidence is the owned-data comparison below. These routines are
distinct from animation ffSin/ffCos.

## Reference experiment

reference_trace.py --movement (reference_movement.py) retains, at every
verified vblank, the complete 0x2b00-byte slot-0 player record, all four
control records, the 0xa0-byte camera block and the source globals as raw
bytes. The reload barrier now also requires all of those bytes to equal the
frozen fixture before stepping. Schedules hold a direction for 24 vblanks and
release for 8: Up (twice), Down, Left, Right, Up+Right and Down+Left. Raw traces:
private/reports/poison-fields-move-*-a.json and -up-b.json; schedules
schedule-move-*-v1.json. Up a/b are identical in every raw captured byte.

Observations: updates occur every second vblank; the request is written each
update and is retained one update after release, then becomes zero. Facing
turns 0.5236 per update and settles on the direction. Down begins with a turn
difference near π and turns positive; the wrap decision depends on exact float
rounding.

## Native comparison

compare_motion_request.py derives every input from the raw records, chains
native motion-code to obtain GetPlayerDMotion's base code, and compares request
+0x860..+0x868 and facing +0x86c byte for byte. Reports:
private/reports/sor-motion-request-*-v2/ (v1 is an identical earlier run before
a Clippy-only refactor).

- 112/112 updates match request and facing across seven traces, covering
  eight control angles through native atan2f/sinf/cosf and the near-π wrap.
- Diagnostic only: native position + request reproduces the observed position
  in 110/112 updates. Left vblank 18 and Up+Right vblank 4 differ; wall-object
  +0x8a0 is zero in both, so a later item/player/enemy/camera stage adjusted the
  displacement. These are the first collision divergences for M08 and are not
  approximated.

## Locating the two adjusted updates (stock debugger stops)

Method: reload the copied fixture through the full barrier, hold the schedule's
keys for the preceding vblanks (`reference_trace.py --leave-held`; the prefix is
compared with the original trace), then use stock-debugger execute breakpoints
before PlayerCollideWalls (0x148c48), before PlayerCollideFloor (0x148cd8) and
after it (0x148ce0). `reference_frame_capture.py` reads PlayerMotion's live frame
(sp from the inspected screenshot), the player record and named globals by PINE,
after checking the loaded PlayerMotion bytes against the ELF. Registers come
only from retained screenshots. Artifacts: private/reports/motion-stage-probe-
upright-v1/ and motion-stage-probe-left-v1/.

| Update | dpos before walls / before floor | Walls (s4) | Floor (v0) | dpos after floor |
| --- | --- | --- | --- | --- |
| Up+Right vblank 4 | request, unchanged | 0 | 2 | (−0.240409, 0, −0.064413) |
| Left vblank 18 | request, unchanged | 0 | 2 | (0.214021, 0, −0.129353) |

PlayerCollidePlayers returned −1 and PlayerCollideItems returned 0 in both. The
floor object +0x89c is 0x11f7c78; its loaded record begins with the name
"G1#109" (next record "G1FLOOR_LINE#1"), so it can be bound to parsed world
nodes by name. `player_floor_col` +0x30 holds FloorCollide's contact point; for
Up+Right it equals the final position bit for bit, and for Left it equals it
within a few z ulps after the scalar position add. Both updates take
PlayerCollideFloor's "pull target back to the floor contact, re-query, return 2"
path (decompiler view at the dpos subtraction before LAB_00145844; inferred from
the observed values and return code, not yet single-stepped). Up+Right was
repeated: the after-floor frame and player record are identical in two runs.

So these divergences are FloorCollide world-geometry outcomes, not items, players,
walls or camera limits. Native reproduction needs FloorCollide 0x11ec58 and its
collision data. PlayerCollideFloor and the PlayerMotion slope rescale (0x148e30)
also call fqdist 0x1d6b90, which runs **VU0 microprogram `vcallms 0x44`** on
(x, z) staged at 0x2a1fc0. That microcode must be recovered before fqdist can be
reproduced natively; it is a gameplay-relevant vector-unit dependency.

Procedure hazard found: key releases sent while the debugger window had focus
did not reach PCSX2's input, leaving Up/Right held. The resulting
`poison-fields-probe-left-pre18-a.json` diverged at step 2 (control angle 3π/4)
and is retained as invalid input evidence. Releases must target the render window.
After the probes, all breakpoints were disabled, input cleared and the fixture
reloaded through the barrier (`poison-fields-after-motion-stage-probes-v1.json`).

## Remaining gaps

Not native yet: keyboard/pad → PlayerControl production, FloorCollide and the
other collision/push stages, fqdist's VU0 microprogram, camera limit and camera
update, requested-action mapping, knockback, special actions/codes, the
facing-matrix rebuild (CreateYPRMatrix) and movement for other characters or
areas. The request helper is an M09/M10 component, not native movement.
