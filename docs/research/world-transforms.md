# World matrix consumer investigation

Source identity: profiles/ps2/slus-20047.json, original SLUS_200.47 ELF SHA-256
69056dc6969c3740a82422ecdf7f771b19b26e20aff208d620c6efec66a3d527.
Original functions: GetWorldMat EE0x1d06b8, MulMat4 EE0x1d55a0. Instruction
exports remain in private/reports/ghidra-animation-world/ and
ghidra-animation-compose/. No emulator code is part of the native runtime.

GetWorldMat copies the leaf's16 matrix words, adds an optional translation using
scalar EE arithmetic, then walks immediate-parent through outermost ancestor.
Parent pointer is at box+0x74; flags are at+0x60. Parent flag4 selects scalar
translation-only addition. Otherwise MulMat4 multiplies that parent by the
current result, clearing the returned translation-only indication. The leaf's
initial indication is its own flag4. Caching a parent-world product and multiplying
it once by the leaf would reassociate the operations and change rounding.

MulMat4 writes1 into both input+0x3c words before its VU loads. For each output
column, it performs multiply, accumulator add of the second product, accumulator
add of the third, then the final fourth product/add. Native world_transform
preserves that order and exposes parent W writes through mutable input boxes.
GetWorldMat does not read box+0x40 scale vectors. Renderer scale/flag consumers
must be recovered separately; these vectors must not silently be inherited here.

The configured PCSX2 reference uses FPU/VU0/VU1 roundmode3 and separate division
roundmode0. Pinned v2.6.3 commitbc8151d2a46d4aba039ea5580afbfc7bfcf6d730
VUops.cpp, microVU_Upper/Macro/Misc.inl were read from the user's existing local
tag. Upper's MADD/MADDA paths emit separate multiply and ordinary SSE addition;
Misc's packed addition does not apply the scalar EE alignment mask. An original
finite add_rz helper therefore preserves opposite-sign sticky contributions even
at exponent gaps too large for a naive float64 sum. Unsupported exceptional and
underflow domains fail; neither this subset nor PCSX2 certifies all PS2 hardware
numeric corner cases. Code was independently written; no GPL source transplanted.

Frozen Sorceress matrix parent chains cover38 boxes:35 source-bound skeletal
nodes plus actor box0x1993490 and ancestors0x1693ce0/0x16935e0. The last ancestor
has flag4. Depths range4–13. This is fixture-qualified heap identity, not a fixed
runtime address contract. The earlier byte-pattern search of computed frozen
world matrices only finds two identity-root matches at the actor box; these are
candidate/tautological matches, not accepted world-pose comparison evidence.
Artifacts: sor-frozen-world-experiment-v1/ (raw boxes, source/fixture/RAM hashes,
native outcomes, candidate search).

Direct stock debugger comparisons now provide stronger evidence:

- world-breakpoint-case-001/: natural CreateDynobjGrid call, two full VU matrix
  products followed by one translation-only ancestor; all64 output bytes agree.
  Its return word was not captured. Matrices are translations/identity rotations.
- world-breakpoint-case-003/: another natural call; all64 bytes and observed
  return word0 agree, with clocks unchanged during the function. Its matrices
  also have identity rotations.
- world-breakpoint-case-004-controlled-bone/: a0 deliberately substituted with
  live source-bound SOR node13 LEFTU#0 matrix0x1993b90. Eight parent matrices
  include nontrivial rotations and a final translation-only ancestor. The
  original function and native evaluator agree on all64 bytes and return word0;
  clocks remain unchanged. This is a controlled function-input probe, not a
  naturally occurring gameplay call, renderer or attachment-motion acceptance.

Case002 remains input-only/excluded: asynchronous breakpoint-table reordering
caused the attempted return stop to miss. Screenshots were renamed to identify
the actual entry/missed-return observations. Do not count that case as passing.
Stock Step Out also failed to advance; explicit entry and observed-RA return
breakpoints worked. Removing the entry breakpoint and verifying one enabled
return breakpoint avoids the asynchronous checkbox/reordering problem.

reference_world_matrix.py performs bounded read-only PINE captures around these
stock stops. Register/PC/return values must come from inspected screenshots;
PINE cannot read them. It validates the entire loaded original function against
ELF bytes, retains128-byte raw boxes/unknown fields, rejects parent cycles and
depths above128, records clocks and preserves first differing output words.
--controlled-input explicitly labels debugger argument substitutions. The helper
does not drive the debugger or modify registers/memory. The controlled experiment
used the stock GUI to edit only a0's low word, preserving original-argument
evidence in world-controlled-bone-probe-context-v1.json. No saves were written.

The sole diagnostic breakpoint was removed afterward. The unchanged copied
fixture was reloaded and its globals, full animation pool, nodes/local matrices
and action context passed the frozen reload barrier. Two subsequent vblank steps
were verified (poison-fields-after-world-probe-restore-v1.json). Reference remains
paused on Xvfb. Native inspectors remain closed.

Forty-five Rust tests, five Python tests, release tools build and strict
workspace/all-target Clippy pass. The native world helper remains an experiment;
renderer composition, actor transforms, scale inheritance, attachment binding,
mirror/reverse paths and broad numeric/pose coverage are still open. Next trace
the renderer's actual matrix/scale traversal and GetPlayerDMotion/collision
consumers rather than claiming M07 complete from these function samples.
