#!/usr/bin/env python3
"""Export the fixture's live Items array in the native record convention
(research only; for gdl_tools game-run items_seed=).

Conversions follow compare_items.py: +0x00 becomes the definition index
(Defs, 0x50 bytes each), kinds 5/12 +0xf0 the WORLDS object index + 1
(60 bytes each), kinds 5/11 +0xf8 the item slot + 1; presentation words
(+0x70..+0xc7 render/anim nodes, +0xe6 dyngrid link, types 2/13 +0xf8) are
zeroed. Also exported: the item world objects' runtime words (+0x10 flags,
+0x14, +0x16, +0x17) for every object.
"""
import argparse
import json
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reference_snapshot import frozen_memory  # noqa: E402

FIXTURE = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
ITEMS, NUM_ITEMS, DEFS, OBJECTS, NUM_OBJECTS_HINT = 0x23c724, 0x23c728, 0x21f3c0, 0x21f35c, 4096


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('out', type=pathlib.Path, help='new private JSON')
    ap.add_argument('--fixture', type=pathlib.Path, default=FIXTURE)
    ap.add_argument('--objects', type=int, required=True, help='WORLDS node count of the level')
    a = ap.parse_args()
    if a.out.exists() or not a.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise SystemExit('choose a new private output')
    if not 0 < a.objects <= NUM_OBJECTS_HINT:
        raise SystemExit('object count out of range')
    m = frozen_memory(a.fixture)
    u = lambda x: struct.unpack_from('<I', m, x)[0]
    items, count, defs, objs = u(ITEMS), u(NUM_ITEMS), u(DEFS), u(OBJECTS)
    if not (0x100000 <= items < 0x2000000 and 0 < count <= 4096):
        raise SystemExit('unexpected Items binding')
    records = []
    for k in range(count):
        r = bytearray(m[items + 0x110 * k: items + 0x110 * (k + 1)])
        ptr = struct.unpack_from('<I', r, 0)[0]
        kind = struct.unpack_from('<i', m, ptr)[0] if ptr else None
        struct.pack_into('<I', r, 0, (ptr - defs) // 0x50 if ptr else 0)

        def index(o, base, size):
            v = struct.unpack_from('<I', r, o)[0]
            struct.pack_into('<I', r, o, 0 if v == 0 else (v - base) // size + 1)
        if kind in (5, 12):
            index(0xf0, objs, 60)
        if kind in (5, 11):
            index(0xf8, items, 0x110)
        if kind in (2, 13):
            struct.pack_into('<I', r, 0xf8, 0)
        r[0x70:0xc8] = bytes(0x58)
        r[0xe6:0xe8] = bytes(2)
        records.append(dict(slot=k, active=ptr != 0, record=bytes(r).hex()))
    objects = []
    for k in range(a.objects):
        base = objs + 60 * k
        objects.append([u(base + 0x10), struct.unpack_from('<H', m, base + 0x14)[0], m[base + 0x16], m[base + 0x17]])
    with a.out.open('x') as f:
        json.dump(dict(fixture=str(a.fixture), items=count, records=records, objects=objects), f)
        f.write('\n')
    print(f'{count} item records, {len(objects)} objects')


if __name__ == '__main__':
    main()
