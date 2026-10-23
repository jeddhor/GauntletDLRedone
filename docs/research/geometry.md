# Poison Fields static geometry investigation

Source profile: ps2-slus-20047-fb47d40667df. The frozen WorldDir value
`levels/levelG1` binds this user-selected area to archive entries 2194–2198.
Full source identities are retained in private/reports/poison-fields-package.json.
The OBJECTS payload hash begins 84fb6f543d92; WORLDS begins 6a605f48225d.

The bounded native readers cover the observed OBJECTS 0xf00b000d and WORLDS
0xf00bab02 versions. Unknown header/record bytes and full model containers are
retained. Inputs are limited to 64 MiB, table counts to 50,000, total parts to
100,000, and retained packet bytes to 128 MiB. Metadata overlap, partial packet
overlap, invalid graph links/cycles, non-finite coordinates and inconsistent
attribute counts fail explicitly. Float-position VIF packets remain unsupported.

Observed geometry consists of a first inline subobject descriptor followed by
additional descriptors. The descriptor qword count equals the container's
stored count plus one. Each container has an eight-byte prefix followed by
four-byte-aligned stream records. Locally observed position storage is signed
8/16-bit XYZ with an extra trailing position; normals and colors are packed
16-bit records; UVs use unsigned 8/16-bit values or paired diffuse/lightmap UVs.
Position and diffuse UV scaling of 1/128 and lightmap UV scaling of 1/32768
agree with separately pinned decoding. The normal high bit suppresses the
associated strip triangle; direction and parity determine candidate winding.

Actual-disc checks performed:

- All 944 object records, 151 bitmap records, 944 model definitions and 1,729
  subobject packets parse. Every packet decodes through the integer path.
- All decoded vertex positions fit the source object bounding radii within
  0.015 units; all 944 deduplicated position counts match declared counts.
- Emitted topology totals 8,237 triangles. 62 object records differ from the
  declared triangle counts; the difference is explained below (2026-10-23).
  Counts are not forced to fit.
- All 1,729 packet outputs agree with the unchanged pinned gdl_tools decoder
  on positions, topology, UVs, lightmap UVs, colors and normals within 3e-7.
  This is an independent implementation check with shared format knowledge,
  not validation against the original VU/GS consumer.
- The world reader retains 1,360 nodes and 3,461 collision records. Translation
  sums along the candidate next/child graph cover all nodes without cycles or
  multiple incoming links. 930 nodes match model definitions, yielding 1,706
  placed mesh parts. Unmatched nodes are preserved and reported; they include
  groups, particle records and other unresolved semantics.
- Native Bevy scene captures show the original level layout and base textures
  in overview and near the saved player's position. The initial missing
  tonemapping feature produced a magenta fallback; selecting Tonemapping::None
  corrected this native presentation configuration error.

Native viewer choices are explicitly diagnostic: original XYZ is retained,
materials are unlit and double sided, base textures use nearest repeat sampling,
source vertex colors multiply the texture, alpha is treated as opaque, and
lightmaps are not applied. Fifteen bitmap records are undecoded/external; these
are reported and would use magenta if referenced as base material. These choices
cannot satisfy M05 material fidelity or M06 winding/transform acceptance.
Static plants therefore have opaque rectangular edges. No player, enemies,
animation, particle behavior or gameplay collision is implemented by this viewer.

Implementation: crates/gdl_formats/src/objects.rs, world.rs; gdl_scene binary.
Research harness: tools/python/verify_geometry.py. Reports and captures stay
under ignored private/. Pinned gdl_tools format/primitive knowledge was consulted;
its source is not transplanted into the Rust runtime (see THIRD_PARTY.md).

The report exporter also emits private/exports/poison-fields-gltf-v1/scene.gltf,
scene.bin, original base PNGs and provenance.json. It retains source record IDs,
raw descriptors, candidate translations and lightmap UV attributes. Diagnostic
material choices remain explicit. Blender 5.0.1 imported all 930 placed mesh nodes
into an initially empty scene without error; 87 used images were imported from
the 134 included source PNGs. Unreferenced assets and unresolved nodes remain
listed rather than invented. This validates an independently inspectable export,
not original animation, collisions or material equivalence.

Next discriminating checks: original world-position consumers, VIF/VU primitive
assembly, triangle-count meaning, GS material/lightmap state, and one character
rig. Do not repair unexplained transforms by changing coordinates until the
runtime source or a measured original pose establishes the intended contract.

## The declared triangle count (2026-10-23)

`gdl_tools objects-triangles OBJECTS.PS2 NEW_REPORT.json` compares each
object's header count (+36) with the decoded strips. The header counts
distinct faces: the emitted triangles (after the normal-bit suppression)
minus faces that repeat an earlier face of the same part with the same
winding (strip joins redraw them); a face repeated with the reverse winding
(two-sided geometry) is counted. G1: 944/944 objects. Every dictionary-named
OBJECTS.PS2 (643 packages, 106,310 objects, no decode errors;
private/reports/objects-triangles-all-v1.json): 106,243 fit exactly; 88,012
have no repeats at all. The 67 left (ITEMS/levelC 32, WEAPONS 25,
MONSTERS/SKY 6, LICH, DJINN, TRE and LEVELS/levelB3 one each) count some of
their same-winding repeats; the count is authoring metadata no runtime path
draws from (the VU draws the strips), so the residue is recorded, not
modelled. The native meshes draw every emitted triangle, as the GS does.
