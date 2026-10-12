# Yellow Sorceress / The Poison Fields reference fixture

The user selected Yellow Sorceress in The Poison Fields and saved the paused
scene. The profile label is PELE. The source-qualified state is preserved in
private/reference/captures/poison-fields-original.p2s; SHA-256:
cc9b835f2d061bfc8a7950fb75b7a621d4e7217738a3aebcfb58609171a27399.
The ZIP passes its CRC checks; EE RAM is 32 MiB. Four immutable code-entry probes
match the exact local ELF before guest fields are accepted.

Source-linked instructions establish four player records at EE 0x221550, stride
0x2b00 bytes. GetPlayerPos at 0x1317d8 reads XYZ at offsets 0x50/54/58;
GetPlayerColPos at 0x131808 reads XYZ at 0x70/74/78. PlayersRestoreHealth
at 0x130a60 writes a float at offset 0x1a10. The public revision binding records
these locations and consumers. Unused records still contain nonzero health;
player activity flags and class IDs are not yet accepted.

Fixture observation: position (24.056019, 0, 5.14752), collision position
(24.056019, 2.5000184, 5.14752), health 483.02002, AFrame 2123,
Time 71.2990265, SPF 1/30, vb_elapsed 2. WorldDir is levels/levelG1,
CurLevel is 0 and the last filename is STREAMS\\TOWN1.ADS. These are saved
observations, not inferred spawn, health-drain, indexing or rounding rules.

PINE is enabled globally and verified against serial SLUS-20047 / CRC 15030637.
The saved scene survived reload unchanged at the sampled fields. A later desktop
diagnostic differed by 54 updates while player fields remained unchanged; those
reports are retained, and timing experiments always reload the frozen fixture.

At the user's request, all automated GUI testing now uses an authenticated Xvfb
display. Launch helpers require the recorded virtual session and cannot silently
fall back to the desktop. The user allows desktop presentation for necessary
manual tests. Xvfb does not support the physical Vulkan presentation queue;
Mesa lavapipe/llvmpipe is selected for presentation. PCSX2's software GS renderer
and the exact BIOS/disc/state remain the reference. Backend configuration changes
are recorded; Xvfb images are not silently equated with previous hardware output.

F10 is mapped to FrameAdvance; private slot 1 contains a verified copy of the
fixture. The trace tool loads only that slot, verifies all initial sampled bytes,
holds specified keyboard controls, and verifies each F10 step advances exactly
one vblank before reading paused EE state. It releases controls even on failure.
These are emulator vblank pauses, not guaranteed guest function boundaries.

Actual experiments: two 12-vblank idle runs and two 24-vblank Up schedules are
byte-identical across every sampled field, including control and clock words.
In this fixture, AFrame advances every second vblank and SPF remains 1/30.
Up moves slot 0 to (22.455997, 0, 2.3762474); health remains 483.02002.
This supports a reproducible short movement experiment. It does not establish
the update cadence in every game mode, camera mapping, collision or complete
simulation state. Diagonal and Cross schedules are being recorded separately;
attack events require additional source-derived fields. Two later 36-vblank
Cross traces also agree after adding the signed word read by PlayerAttacking
at offset 0x8c8. This is a mode-dependent state predicate, not a recovered
animation enum or proof of a projectile/damage event.

Tools: reference_snapshot.py (frozen or paused live fields), reference_trace.py
(bounded Xvfb schedule), virtual_display.py (isolated GUI environment). All raw
reports, state copies, control bytes and captures remain private. No native
runtime component depends on these tools, PCSX2 or BIOS.
