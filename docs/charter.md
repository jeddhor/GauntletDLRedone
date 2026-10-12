# Project charter

The controlling design is the supplied master plan. Reconstruct the user's PS2
Gauntlet: Dark Legacy in Rust, with Bevy desktop presentation on Linux x86-64 and
Windows x86-64. The final game must execute native gameplay and use native graphics;
PCSX2 and a BIOS are development references, never runtime dependencies.

Initially target only the exact SHA-256-qualified SLUS_200.47 disc in
profiles/ps2/slus-20047.json. The serial and SYSTEM.CNF VER do not prove a retail
revision number. Unknown inputs must be reported and rejected by release imports.

Original implementation code defaults to MIT. Consulted upstream code is evidence,
not permission to transplant code. Record all reuse and licenses in THIRD_PARTY.md.
Do not claim clean-room development. Avoid leaked SDKs and external leaked symbols;
prefer the symbols/debug metadata contained in the user's supplied executable.
Preserve retail data locally; distributable tools and source exclude all retail assets.

Acceptance follows the master plan's M00–M21 gates. Track these requirements:

| Requirement | Acceptance evidence |
| --- | --- |
| Supported import | Exact source profile; complete validated manifest; failure/cancellation recovery |
| Native runtime | Standalone Linux/Windows launch without emulator, BIOS or guest CPU interpreter |
| Campaign/progression | Fresh-profile full critical-path walkthrough plus reachable-content ledger |
| Original rules | Source-linked dossiers and measured reference/native state/event scenarios |
| Original visuals/audio/UI | Representative asset and event comparisons; complete dependency coverage |
| Local co-op | One to four slots; joins, transitions, continues, reconnects and ownership scenarios |
| Keyboard/controllers | Focus, remapping, multiple-device and reconnect checks |
| Durable saves | Atomic writes, backups, corruption/interruption and migration checks |
| Cross-platform delivery | Fresh import and release QA on named Linux and Windows machines |
| Optional enhancements | Baseline fidelity scenarios remain passing with each setting enabled |

The first playable target is M12: a small original-data encounter with authentic
movement, animation, collision, attack, enemy, pickup, transition and save/reload.
No generated replacement art or plausible invented mechanics can close those gates.
Initial exclusions match master plan §5.2. Work sequentially. Make reversible
implementation choices independently and ask only for missing evidence or user tests.

Evidence states: verified locally, verified upstream, inferred, unknown, contradicted.
Every behavior/format dossier records source identity, location, experiment, outcome,
confidence, unresolved conditions, implementation and regression coverage.
