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

## Item placement as a whole-level collision check

PlaceItem 0x153068 drops each item with FloorPos 0x11ed38 (FloorCollide radius
1.0, +4/−10, CheckFlags 0x17, exclusion 0) and adds 0.1 with an EE add. The
fixture's live Items array (519 records of 0x110 bytes; item i = WORLDS item
instance i) retains those heights. `gdl_tools place-items` reproduces **462/462
static placements bit for bit** across the whole level (34 items skipped by the
definition's +0xa bit 0). The other 23 items ride dynamic objects (ELEV1/2/3/5/
9/13, PLAT1, the wooden arm, identified by position); their RAM heights follow
those objects' current motion, which needs WorldDynCollide and world animation.
Report: private/reports/place-items-g1-v1.json.

## Dynamic objects and run-time words (2026-10-22)

WorldCollide runs the static grid pass first and WorldDynCollide 0x11fbe8 after
it; both reach WorldObjCollide 0x11da08, whose compares are strict (c.lt.s at
0x11dd84 water, 0x11ddc0 solid), so on an equal metric the static object keeps
the hit. Native dyn_collide tests every dynamic object instead of the dyngrid
cells (WorldObjCollide's segment-sphere rejection applies as in the original);
the Tower's static grid lists none of its dynamic objects (checked for 2902,
2903). The exclusion test ORs the parent's +0x35, as the original does.

The original reads the object words (+0x10 flags, +0x35 exclusion, the node y)
live. Native keeps copies in the collision world: sync_collision after
DoWorldAnimation, and sync_collision_object after each later writer
(ProcessItemWobjs, the rotator, the Tower trigger chain), so queries later in
the same update see the new words (Tower trace, gameplay.md). WorldObjGetAllFlags
and PlayerCollideFloor/Walls' flag tests read those run-time words.

WorldCollide's tail writes the output struct only when the caller passes one and
something was hit (+0x44 object, +0x40 metric, +0x30 position, rows from
CreateMat3Norm of the triangle normal — turned by the object's world matrix
through WorldVector 0x1d5948 for matrix objects; 0x1000 objects with +0x10 bit 0
keep the file normal). PlayerCollideFloor's two re-queries pass player_floor_col
(0x2341a0); its forward probe passes none.

WorldDynCollide passes each dynobj entry to WorldObjCollide as its matrix
argument (a3 at 0x11fd70), and CreateDynobjGrid 0x11f5f8 stored the object's
GetWorldMat there. So 0x1000000 objects are tested at the pose of the last grid
build, which follows SetupDynGrid in world_update (0x163508), in GauntWorldInit
after AddItemInstList (0x1633a0) and in init_next_level (0x126a6c); 0x1000 roots
use their live node (+0x30). WorldCollide's tail then turns the hit into the
output struct with a fresh GetWorldMat. Native caches the matrices in
create_dynobj_grid (called from setup_dyn_grid and at load). The dynobj list
(InitDynobjList 0x11f3f8) is the first grid cell word's entries — cells[0],
count << 22 | contents offset, plain i16 object indices; in G1, L1, A1 and K4
it is exactly native's flag-selected set (0x1000 or 0x1000000 with triangles),
in the same order, and no grid row uses cell 0.

object_matrix is GetWorldMat 0x1d06b8: the node's local times each object
ancestor's local (while the object has 0x1000000) with MulMat4, then the world
root nodes — an identity under MulMat4 (+0x60 0x1000) and a translation-only
identity (+0x60 4). Read from the G1 and Tower savestates (0x1693be0 under
0x16935e0; 0x1dee0a0 under 0x1dedaa0; every object node hangs from the first
through its parents). The earlier port multiplied by the parent's composed
world with scalar sums, which reassociates the products and kept −0.0 where the
root's MulMat4 gives +0.0.

## Not yet native

The dyngrid cells themselves (NextDynGrid order: a dynamic object reached
through two cells is tested once per query via +0x42, so only the candidate
order could differ). The CheckNum wrap after 255 queries is reproduced
per native world, but its original value depends on every caller.
