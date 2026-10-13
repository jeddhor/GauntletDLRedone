# Sorceress tree and animation metadata

Exact source identities remain in the revision profile and private archive
manifest. Independent dictionary labels locate PLAYERS/SOR/ANIM/ANIM.PS2 at
entry 34 and the YEL00 ANIM/OBJECTS/TEXDEF/TEXTURES at entries 90/89/88/87.
These are PS2 data from the supplied disc, not substituted character assets.

Frozen runtime binding: SOR_YEL is visible inside slot 0 at EE 0x221be8 and
in the loaded tree blob at 0x9bb300. Comparing all 2,332 YEL00 ANIM bytes with
that blob finds only 39 differing bytes, consisting of pointer relocation and
loaded flags/metadata. The differences are retained in
private/reports/sor-yel00-runtime-binding.json. This is observed heap placement
for this fixture, not a permanent heap address or a general character-ID rule.

The native metadata reader accepts disk versions 0/8, explicit tree-relative
and animation-header-relative offsets, tree/node/clip records and skeletal
sequence-info records. It retains raw metadata and rejects truncated ranges,
invalid parents/cycles, non-finite positions, pointer overflow, unsupported
versions and excessive counts. Input limit 64 MiB; 4,096 trees; aggregate
100,000 nodes/100,000 sequences/1,000,000 sequence-info records. Channel payload validation and arithmetic now live in separate bounded modules,
with evaluated channel acceptance limited to the measured scenarios below.

Actual SOR data: 35 nodes, 137 named clips and 4,795 skeletal sequence-info
records. READY is30 frames with timing factor30; WALK1 is12 with factor30; WALK2 is11 with factor30.
Other clips have timing factors10,12,15,24,45,60. Frame count zero exists, including INIT,
and cannot be rejected as an arbitrary malformed clip. YEL00 has the same
35-node hierarchy and geometry references but no local clips; animation data
are a separate package. Nodes carry parent indices, initial translations,
typed consumers, flags and sequence-table offsets. No enum/event meanings are
invented from clip labels alone.

Original-symbol instruction exports in ghidra-animation[-sampler] provide
consumer evidence. SetupAnimHeader at 0x1106f8 relocates five pointers relative
to the passed animation header. GetAnimAngXYZVal at 0x10e9c8 has explicit
initial-only, compressed/uncompressed and cached-frame paths; compressed
channels add indexed table deltas. It skips frame-bitmap storage using a
32-bit-aligned calculation. Do not accept upstream comments' bitmap sizes or
interpolation assumptions without checking these instructions and live poses.
InitAnim computes rate-dependent scale and transition clocks; decompiler casts
do not establish native integer/float types. TypeFlags ordering, angular wrap,
key selection, interpolation and root motion remain under investigation.

The fresh frame-zero reader now follows the inspected consumer: TypeFlags at
EE 0x21edd8 are 1,2,4,16,32,64,256,512,1024; absent angles/position default to
zero and absent scales to one. Initial-only records have no bitmap prefix;
other records skip ceil(frame_count/32) 32-bit words before initial channel
floats. All 4,795 SOR frame-zero records read within bounds. Synthetic coverage
includes the 33-frame alignment boundary, truncation and non-finite data.
Compressed deltas and fresh channel interpolation are now implemented and
qualified below. Local matrix assembly is now qualified in the later section;
world composition remains open.

A frozen runtime scan links 35 allocated animation-cache records to the SOR
node sequence-table pointers. AnimDataList is an ELF NOTYPE/zero-size symbol;
the actual four-byte pointer width and 0xa0 stride come from GetAnimData loads,
not symbol size. The fixture allocates 1,050 records of a 3,072-record capacity.
An attempted comparison incorrectly treated cache word +0xc as a clip ID and
failed. The original and corrected reports are preserved separately. Its
consumer uses cached frame/data indexing; active clip interpretation was subsequently verified across the measured
READY/walking/attack cache scenarios. Output matrix binding remains unresolved. These failures are not accepted native pose tests.

Further evidence locates a candidate AnimInfo inside slot 0 at +0x94: its
sequence/header pointers match the loaded SOR tree, count=137, index=0 (READY),
frame count=30 and current frame=11. An initial one-update probe read the heap
before state loading completed; the newer full-pool reload barrier explicitly
asserts every measured cache equals the frozen state before stepping.

Animation accumulation also exposes a numerical contract. Plain IEEE float32
and simple toward-zero addition did not reproduce all cached values. The exact
PCSX2 v2.6.3 FPU addition path masks the smaller operand's low significand bits
according to exponent distance, retaining one alignment guard bit, before the
configured single-precision addition. A separately written guarded/toward-zero
experiment matches all 20 isolated one-delta updates and all 28 tested READY
lower-key caches byte for byte. Inputs and first failed variants are preserved
in private/reports/sor-*guard-experiment.json and cache reports. This validates
a finite addition subset for this reference; it is not a complete hardware-FPU,
interpolation, matrix or gameplay acceptance claim. The native finite numeric/key decoder now reproduces the measured cases below;
exceptional numeric domains remain explicitly unsupported.

The rig inspector joins model labels using the source tree prefix and node
label, truncated to the 16-byte model-definition field, then sums initial
translations along the parent graph. This is a labelled rest-hierarchy
experiment. It has no channel evaluator, event timeline, or accepted character
pose and cannot satisfy M07. It does not create replacement geometry.

Character texture export decodes 17 of 18 records. The YEL_HANDGLOW record
points beyond this package's TEXTURES payload and requires an external
dependency; it is preserved and reported. Twelve character records use CT16
palettes (format 48). Their five-bit RGB expansion follows the inspected GS
shift convention, and their high bit is shown as a binary TEXA selector.
Actual TEXA-dependent alpha, blend state and material fidelity remain open.

Next: recover and compare individual original channel values at named frame
boundaries, then matrix assembly and distinct idle/walk/attack poses. Preserve
the first mismatch instead of adjusting curves until the silhouette looks right.

## Native key and channel evaluation

`gdl_formats::animation_keys` exposes bounded borrowed payloads and retains bitmap,
initial-value and row offsets/bytes. `gdl_core` is independent of Bevy and implements
original channel accumulation plus fresh frame sampling. Absolute float rows and
indexed delta rows remain distinct. Missing axes retain original defaults. All
137 SOR clips decode, producing 76,542 encoded keys without reported errors.
This is actual disc parser coverage, not acceptance of every clip's gameplay role.

The finite arithmetic subset uses one alignment guard bit and toward-zero add,
subtract and multiply. Non-finite, subnormal and overflow domains fail explicitly;
no complete EE FPU or real-hardware multiplier certification is claimed. The
reference's 255 reciprocal spacing entries equal nearest-rounded float32 1/n.
Sampling currently rejects interpolation gaps of 256 or more, pending validation
of the original division branch. Original instruction constants are retained as
float bits: snap distance 0x3e000000, angle interpolation cutoff ±0x3fc90fdb,
wrap thresholds ±0x40490fdb and wrap amount 0x40c90fdb. Frame zero and constant
tracks use their original direct paths. Clip clocks, transition blending, mirror
flags, root motion, matrices and events are not included in this sampler.

Actual reference/native checks:

- READY lower/upper keys before and after one update: 116/116 byte equal.
- 24-vblank Up schedule: READY → WALK1; 1,542/1,542 key caches byte equal.
- 36-vblank Cross(K) schedule: READY → THROW1S → THROW1 → THROW1R, repeating;
  2,356/2,356 key caches byte equal.
- DoAnimation writes evaluated angles/position/scale to cache +0x30/+0x60/+0x90
  when blend fraction is zero. Comparing those fields gives 2,011/2,011 channel
  records byte equal, including fractional interpolation weights between sparse keys. Initial-only records
  are excluded because their cache vectors may be stale.
- Independent a/b repetitions of both schedules match every captured observation.

Reports remain private: sor-ready-native-cache-comparison-v1.json,
sor-walk-native-cache-comparison-v2.json, sor-attack-native-cache-comparison-v1.json,
sor-evaluated-channel-comparison-v1.json and sor-native-key-clips-v1/. Public
comparison tools contain no retail vectors. Twenty-two Rust tests cover numeric
rounding/domain rejection, sparse bitmap word boundaries, payload bounds, exact
keys, snap/cutoff/wrap and earlier samples preceding invalid later data.

Next bind original node matrix outputs through AnimateNode/DoAnimation and recover
CreateRYPMatrix/CreatePYRMatrix, then compare local and world transforms before
accepting a native animated pose.

## Local matrices and source rig positions

The original DoPlayerAction instructions at0x177c60/0x177c78 construct
player+0x90/player+0x94; its call at0x178c24 passes the instance to DoAnimateTree.
That function supplies instance+4 as AnimInfo. The former candidate is now bound
to the original actor caller. AnimateNode follows child/sibling pointers and
passes node+0x1c cache, node+0 matrix, matrix+0x40 scale and node+0x10 initial
position to DoAnimation. The complete35-node runtime graph matches main SOR
sequence tags and parent indices. Public reference-layout fields now record
these instruction-derived offsets; fixture addresses remain dynamically derived.

A counterexample to the rest inspector's assumption is preserved:30 YEL00 initial
translations differ slightly from runtime values, whereas all35 main SOR initial
translations match byte for byte. No cached output values were edited to conceal
that mismatch. Native sampled poses therefore use main SOR translations and
YEL00 model names/geometry; the older translation-only inspector remains labelled.

CreateRYPMatrix0x1d6530 and CreatePYRMatrix0x1d63f8 use ffcos/ffsin, not platform
trigonometry. ffsin0x1d6d18 reduces quadrants and evaluates SIN_POLY0x1d6cb8.
The independently expressed native polynomial preserves each original scalar
operation and constant bit pattern. Its pi/2 overshoot (about3.7e-6) is retained,
including compounded basis error; normalization would change reference bytes.
Input reduction is bounded to16 turns and exceptional domains remain rejected.
The PYR flag0x8000 is source-supported; actual Sorceress comparisons below use
the RYP path. Synthetic quarter-turn tests cover the differing axis orders.

All35 local matrix/scale pairs match the frozen READY fixture byte for byte.
Across source-bound, stepped captures,875 walking and1,295 attack pose records
match (2,170/2,170). These include constant tracks, every node, local translation,
polynomial rotations and separate scale vectors. Reproduction commands are
compare_animation_poses.py with original entry34 as both animation and initial
translation source. Outputs: sor-walk-pose-comparison-v2/ and
sor-attack-pose-comparison-v2/. The old YEL00 assumption report is preserved in
sor-node-initial-source-comparison-v1.json.

GetWorldMat uses a separate VU multiplication path and parent flags. World
composition, scale inheritance, renderer traversal and full pose presentation
are not accepted by the local-matrix comparison. A native animation inspector
now uses these verified local outputs with explicitly unaccepted host matrix
composition and diagnostic original base materials. Its preview loop is a viewer
clock, not recovered gameplay update order or transition behavior.

Native inspector QA: release build and strict Clippy pass. Xvfb READY frame11 and
stepped frame12 render coherent original geometry; PageUp selects IDLE1; explicit
WALK1/frame6 renders with the source rig; Space toggles the labelled preview loop.
The camera now leaves room for diagnostic text, and paused poses are not recomputed
every render frame. Captures remain in private/reference/captures/. Twenty-five
Rust tests and four Python tests pass. No complete M07 gate is claimed: attachment
visualization, world/VU composition, gameplay timing and events remain unresolved.

Evidence precision: all captured AnimInfo frame indices in these walking/attack
traces are integers. Sparse-key interpolation weights can be fractional; this is
not a fractional-frame reference-input test. The snap-distance boundary currently
has synthetic coverage. Source metadata rates and frame counts also do not alone
explain action timing: the observed THROW1S/THROW1/THROW1R schedule progresses
differently from a simple nominal-rate viewer loop. Recover DoPlayerAction's
rate/transition inputs before accepting animation duration or event timing.

Final UI checks: THROW1 source clip107 has three frames; out-of-range frame3 is
rejected and measured frame2 renders. INIT is clip5 with zero frames and remains
at frame0 when scrubbed forward. Walking preview starts and pauses with Space.
Final release build and workspace/all-target strict Clippy pass after these fixes.
Automated GUI interaction was confined to the authenticated virtual display;
completed native inspectors were closed. Reference PCSX2 remains paused/running.

## Clock consumer correction in progress

The field at sequence+0x22 was previously named frame_rate, based on external
metadata convention. InitAnim at0x110394–0x1103b4 establishes a different
contract: positive values multiply1/30 and player playback scale; nonpositive
values fall back to playback scale. The result at AnimInfo+0x2c multiplies
AnimTimeScale to produce a frame period. CalcAnimInfo takes its reciprocal.
Native metadata now calls the raw signed halfword timing_factor and permits
nonpositive fallback values. Old private reports and captures are retained with
their original labels as earlier interpretations.

In this fixture AnimTimeScale at0x2bc3a0 is0x3d088889 (1/30). Its ELF symbol is
NOTYPE/size0; original LWC1 consumers establish float32 width, not symbol size.
ATime is a four-byte ELF object at0x20f914. READY/WALK1 factors30 produce period
1/30; THROW1R factor15 produces period1/60. The captured actor advances THROW1R
by two frames per game update, consistent with this conversion. THROW1S can be
interrupted before its declared seven frames finish, so action transitions must
still be traced. Nominal viewer timing is being corrected; no gameplay duration
or transition acceptance is inferred from this formula alone.

The headless frame_period helper preserves source multiply order and rejects
unverified zero/exceptional playback domains. Preview timing now uses its
explicit fixture clock-scale input instead of treating the metadata halfword
as FPS. Clock probes now include raw ATime/AnimTimeScale and include them in
the reload barrier. Finish builds/comparisons and implement bounded CalcAnimInfo
state evaluation next.

Clock state follow-up: finite CalcAnimInfo is implemented in gdl_core separately
from rendering and action transitions. It preserves13 fields, the reset/mode
words, enabled/loop state, period scaling, source snap rules, blend time fraction,
sequence-change rebase and original0/1/15 return codes. Same-clip/same-start-time
pairs at changed AFrame match12/12 attack and16/16 walking states byte for byte.
A/b walking traces reproduce all observations. Caller restarts/action changes
are listed as exclusions and do not count as clock parity. Return codes were not
directly captured; loop/freeze/zero-frame/blend branches have source and synthetic
evidence, not full reference acceptance. Reports: sor-attack-clock-comparison-v1/
and sor-walk-clock-comparison-v1[-v2]/. Thirty-three Rust tests and strict Clippy pass.

Next implement and qualify InitAnim plus AnimateTree caller selection; recover
player action transition/event inputs before asserting clip duration.

Valid-sequence InitAnim initialization is now implemented separately. Supplied
observed clip IDs, source header counts/factors, zero start/blend and player scale
produce matching13-field states after CalcAnimInfo and the source DoAnimation
reset handshake in8 start/restart cases (6 attack,2 walking). This validates
initialization with known inputs, not the player's action-selection rules.
Reports: sor-attack-clock-start-comparison-v2/ and
sor-walk-clock-start-comparison-v2/. The invalid-sequence diagnostic/fallback
branch is deliberately not accepted by the bounded helper. A walk-release
schedule is the next actual reference test for nonzero transition blending.

The initial walk-release schedule (Up24, then no keys12) did not yet reach an
idle blend. The sampled control/movement remains Up throughstep26 and clears
by28; position then holds while RUN1 continues to frame5 bystep36. Preserve this
input-pipeline observation instead of assuming keyboard release and game input
change occur on the same boundary. A longer idle segment is now being measured.

## Observed transition blending

Up24/idle24 reaches READY atvblank42, following RUN1's final frame. At that
point frame0/previous clip10/blend fraction0.5 are observed. Source DoPlayerAction
sets its only nonzero transition-duration literal0x3d8888b5 at0x1787f4. Supplying
that literal to native InitAnim/CalcAnimInfo plus the tagged-node sampling reset
handshake reproduces all13 clock fields. The complete49-sample capture reproduces
a/b, including input-clear and idle-return boundaries.

Native transition interpolation follows InterpPYR/InterpXYZ rather than the
sparse-key cutoff path. Differences wrap at ±pi; final angles are not normalized
by InterpPYR. Equal channels copy old bits; mirroring applies afterward to angle
Y/Z, position X and scale X. Mirroring has source/synthetic evidence, but remains
excluded from actual-pose acceptance until separately captured. Zero blend uses
the direct new-channel path. DoAnimation bypasses blending for no-axis records.

At a nonzero blend, DoAnimation retains cache+0x30/+0x60/+0x90 as the prior
unblended channels. With those observed old channels and the recorded fraction
supplied, the native pipeline matches every local matrix/scale pair:35/35 at the
transition and1,715/1,715 over each full walk-release trace. These inputs qualify
the interpolation algorithm, not player action choice. Public pose comparison
now handles observed blends and explicitly excludes unmeasured mirror paths.
Reports: sor-blend-comparison-v1.json, sor-blend-clock-comparison-v1.json,
sor-walk-release-poses-v1/ and sor-walk-release-poses-v2/.

AnimFixPos's inspected root-transfer path rotates the child's translation into
the parent's frame, transfers X/Z, clears child X/Z and selected cache positions,
and resets the sampling marker. All137 SOR header flag words at sequence+0x26
are zero, so this path is not exercised by the selected source clips. General
root-motion, moving-object and other-character cases remain open. Thirty-four
Rust tests and strict Clippy pass after transition support.
