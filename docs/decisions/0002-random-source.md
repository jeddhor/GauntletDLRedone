# 0002: Random source — match the algorithm, not captured values

The executable draws gameplay randomness from newlib `rand` (EE 0x1fb780:
state × 0x41c64e6d + 0x3039, result & 0x7fffffff) through `pbRand` 0x1aedc8 and
`RandInt` 0x1d6b50 (`rand() % n`, unsigned). `srand` is reseeded from CPU time
every main loop, so captured random outcomes depend on wall-clock timing that a
native port cannot reproduce.

Native code (`gdl_core::rng::Rand`) implements the same generator and the same
call sites. Its seed is supplied explicitly per session/step, which keeps runs
deterministic and replayable. Comparisons with captures therefore exclude
fields drawn from `rand` and say so; they can never satisfy a value-fidelity
gate. `RandItemIdx` (RandomItemNum = vb_counter at load, +0x1b7 per call) is a
separate deterministic counter and is compared exactly.

Reversible: if a later capture records per-frame seeds, comparisons can feed
them in without changing the generator.
