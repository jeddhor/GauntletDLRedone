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
100,000 nodes/100,000 sequences/1,000,000 sequence-info records. It does not
claim to validate compressed frame payloads or implement an animation sampler.

Actual SOR data: 35 nodes, 137 named clips and 4,795 skeletal sequence-info
records. READY is 30 frames at rate 30; WALK1 is 12 at 30; WALK2 is 11 at 30.
Other clips have rates 10,12,15,24,45,60. Frame count zero exists, including INIT,
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
Compressed deltas, interpolation and matrix assembly remain unimplemented.

A frozen runtime scan links 35 allocated animation-cache records to the SOR
node sequence-table pointers. AnimDataList is an ELF NOTYPE/zero-size symbol;
the actual four-byte pointer width and 0xa0 stride come from GetAnimData loads,
not symbol size. The fixture allocates 1,050 records of a 3,072-record capacity.
An attempted comparison incorrectly treated cache word +0xc as a clip ID and
failed. The original and corrected reports are preserved separately. Its
consumer uses cached frame/data indexing; active clip and output-transform
binding remain unresolved. These failures are not accepted native pose tests.

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
