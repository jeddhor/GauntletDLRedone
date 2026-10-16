# Static world collision (M08)

Source identity: profiles/ps2/slus-20047.json. Instruction exports:
private/reports/ghidra-world-collide/, ghidra-trilist/, ghidra-trilinecol/,
ghidra-collide-helpers/, ghidra-linedist/, ghidra-linedist2/, ghidra-floor-collide/.
Native code: gdl_core/collision.rs; data: gdl_formats/world.rs; CLI
`gdl_tools collision-query`; comparator tools/python/compare_collision.py.

## Data: loaded unchanged from WORLDS.PS2

WorldInfo (0x21f358) points into the WORLDS file loaded in place (fixture base
0x11e4c40). Triangle records, grid rows, grid cells and cell contents are
byte-identical to archive entry 2194. Only 60-byte object records change at load:
runtime flag bits outside every collision test, pointers (+0x18 parent, +0x28
matrix) and positions. Static objects store the top-down parent-chain sum formed
with scalar EE adds; all 1,319 match the fixture bit for bit. The 41 objects with
flag 0x1000000 (all of them dynamic-grid objects) keep their local file position.
Together, **1,360/1,360 runtime positions are reproduced exactly**. The load-time
inverse cell size equals 1/3 rounded to nearest (0x3eaaaaab).

- Triangle (40 bytes): i16 y-range ×64 at +0/+2; +4 normal-xz scale (+inf on
  the 950 flat triangles, never read there); normal +8; origin +0x14; local-plane
  vertices (u1,v1,u2,v2) ×1/64 as i16 at +0x20..+0x26, vertex zero at the origin.
- Grid: cell size +60, dims +64/+68, rows (+32: u16 min_x, u16 max_x, u32 cell
  offset), cells (+20: count<<22 | byte offset), contents (+28: i16 object, i16
  count, count×i16 triangle offsets from the object's first triangle +0x38).
  Levels G1: 70×172 cells of 3.0, 6,835 cells, 9,401 entries, 22,186 references.

## Algorithm (reviewed instruction by instruction)

WorldCollide normalises the segment in place (NormalVector → fqdist VU0 program),
clips the swept x/z extents (radius-expanded) against the world minimum only,
steps cells with NextGrid, and calls WorldObjCollide per entry (zero-count entries
re-read the same entry, preserved). A per-query CheckNum byte stamps triangles.
WorldObjCollide rejects by projected distance against (radius + object radius),
then CTriListCollide filters by normal-y range and the y×64 range and calls
BTriLineCol. That transforms the segment into the triangle's frame
(BodyVectorNorm), resolves plane crossing or same-side containment, otherwise
measures edges with LineLineDist3D2D → LineLineDist/PointLineDist/
PointLineDist2D/Det3Vectors, rejecting beyond r². Hits are ranked by d×10000
(CheckFlags 0x10, d≠0) or the squared distance from the segment start, ×0.95
when |dir·normal| < 0.25; ties keep the first. Water objects (0x200) go to
water_col. FloorCollide is a vertical segment through this path.

Numerics: EE scalar add/sub guarded-RZ, mul RZ, DIV.S nearest, SQRT.S nearest
(forced by pinned PCSX2 recSQRT_S); VU0 macro ops (vsub/vadd/vmul/vopmula/
vopmsub/vdiv/vsqrt) run under the EE MXCSR, toward zero, with OPMSUB as a
separate multiply then subtract. fqdist is VU0 microcode (see movement.md).

## Comparison

Movement captures now retain WorldCollide's globals at every vblank (profile
movement_layout.collision_blocks, also in the reload barrier). Each paused vblank
holds one complete call: ColFirstP/ColLastP, CheckFlags, normal bounds and results.
The comparator accepts a radius only if native ColMinY/ColMaxY match exactly
(r = 1.5 throughout), uses object mask 0x23c and tries exclusion masks 1 then 0.

**85/85 distinct observations byte-equal (198 samples, six directions)** in
direction, length, y bounds, col_obj, col_tri_idx, col_dist, col_pos and water.
Report: private/reports/collision-comparison-v2/. Coverage is limited: one flat
floor object ("G1#109", object 1296), triangles 2749/2759/2773/2774/2775, 80
plane crossings and 5 edge-distance hits, CheckFlags 0x17 and 7. Exclusion mask 1
always matched first; mask 0 was not discriminated. Caller identity per sample
is not established.

## Not yet native

WorldDynCollide (41 dynamic objects in a runtime-built grid; none within 30 units
of the fixture start), matrix/offset objects, TriLineCol (CheckFlags 0x20), wall
queries, slopes, water hits, PlayerCollideFloor/Walls/Items/Players, SlideAlongWall,
PlayerNewFloor and CameraLimitPlayerDpos. The CheckNum wrap after 255 queries
is reproduced per native world, but its original value depends on every caller.
