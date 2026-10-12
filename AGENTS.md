# Native PS2 reconstruction

Follow Gauntlet_Dark_Legacy_PS2_Rust_Bevy_Master_Plan.md. Read docs/charter.md,
docs/state.md, the active revision profile and relevant research before changing code.

1. Work sequentially; parallel agents require explicit user authorization.
2. Never invent addresses, layouts, original constants, filenames, or test results.
3. Label hypotheses and placeholders; they cannot satisfy fidelity gates.
4. Preserve the ISO and saves; keep assets, BIOS, binaries and captures in ignored private/.
5. Retain indexed source identities, hashes and unknown bytes in every conversion.
6. Verify the revision and guest address space before interpreting code or probing memory.
7. Use bounded experiments and parsers. Review instructions before translating pseudocode.
8. Keep simulation independent of Bevy, render FPS and presentation state.
9. Pin tools; explain updates and rerun affected comparisons.
10. Distinguish synthetic tests, actual disc checks, unavailable tests and upstream claims.
11. Update research, unknowns and coverage with implementation changes.
12. Preserve minimized reproducers and the first divergent reference observation.
13. Stop speculative implementation when evidence affects the contract; continue independent work.
14. Do not silently change edition, architecture, license, or release scope.
15. Report concrete artifacts and actual verification at each milestone.
16. Follow pcsx2/AGENTS.md for changes inside that separate reference checkout.
