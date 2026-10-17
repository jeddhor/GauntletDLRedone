#!/usr/bin/env python3
"""Compare native item records (AddItemInstList..PlaceItem) with the fixture's
live Items array.

The fixture is mid-play, so fields the game changes after load can differ for
items that were touched; mismatches are reported per (type, offset) for review,
never silently masked. Presentation bytes (+0x70..+0xc7 render/anim nodes) and
+0xe6 (dyngrid link) are not compared, nor +0x4c (CopyMat3 leaves NewItem's
uninitialised stack word there), the presentation pointers +0xf8 of types 2
and 13, or type-8 +0xda (drawn from rand, whose captured state is unknown).
Pointers are converted to the native slot/object index + 1. Session globals are read from the
fixture; RandomItemNum at load is derived as final - 0x1b7 * random calls.
"""
import argparse
import collections
import json
import pathlib
import struct
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reference_snapshot import frozen_memory  # noqa: E402
from run_play import resolve, ELF  # noqa: E402

FIXTURE = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
ITEMS, NUM_ITEMS, DEFS, OBJECTS = 0x23c724, 0x23c728, 0x21f3c0, 0x21f35c
RANGES = [(0x04, 0x4c), (0x50, 0x70), (0x74, 0x78), (0xc8, 0xe6), (0xe8, 0x110)]
SKIP = {2: {0xf8, 0xfa}, 13: {0xf8, 0xfa}, 8: {0xda}}


def run(worlds, wdata, session, out):
    out.unlink(missing_ok=True)
    subprocess.run(['target/release/gdl_tools', 'item-records', str(worlds), str(wdata), '0', str(ELF),
                    str(session), str(out)], check=True)
    return json.loads(out.read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=pathlib.Path, help='new comparison report (private/reports/...)')
    args = parser.parse_args()
    if args.report.exists():
        raise SystemExit(f'{args.report} exists')
    m = frozen_memory(FIXTURE)
    u = lambda a: struct.unpack_from('<I', m, a)[0]
    i = lambda a: struct.unpack_from('<i', m, a)[0]
    worlds, wdata = resolve(['LEVELS/LEVELG1/WORLDS.PS2', 'WDATA/TOWN.WAD'])
    final_random = u(0x23c76c)
    session = dict(cur_world=i(0x23e180), mode_950=i(0x20f950), boss_type=i(0x25d2c4), num_virtual_players=i(0x20f990),
                   game_mode=u(0x20f978), global_958=i(0x20f958), global_970=i(0x20f970), random_item_num=final_random,
                   golem_statue_anim=int(u(0x233598 + 116) != 0), death_statue_anim=int(u(0x233598 + 120) != 0),
                   critter_3_loaded=0, critter_7_loaded=0, rng_seed=0,
                   difficulty=i(0x21edcc))
    scratch = args.report.with_suffix('.native.json')
    session_path = args.report.with_suffix('.session.json')
    session_path.write_text(json.dumps(session, indent=2) + '\n')
    native = run(worlds, wdata, session_path, scratch)
    if native['random_calls']:
        session['random_item_num'] = (final_random - 0x1b7 * native['random_calls']) & 0xffffffff
        session_path.write_text(json.dumps(session, indent=2) + '\n')
        native = run(worlds, wdata, session_path, scratch)
    items, count, defs, objs = u(ITEMS), u(NUM_ITEMS), u(DEFS), u(OBJECTS)
    records = [bytes.fromhex(r) for r in native['records']]
    mismatch = collections.Counter()
    examples = {}
    exact = 0
    def_mismatch = []
    for k in range(min(count, len(records))):
        ram = bytearray(m[items + 0x110 * k: items + 0x110 * (k + 1)])
        nat = records[k]
        ptr = struct.unpack_from('<I', ram, 0)[0]
        ram_def = (ptr - defs) // 0x50 if ptr else None
        nat_def = struct.unpack_from('<I', nat, 0)[0]
        kind = i(ptr) if ptr else None
        def index(o, base, size):
            v = struct.unpack_from('<I', ram, o)[0]
            struct.pack_into('<I', ram, o, 0 if v == 0 else (v - base) // size + 1)
        if kind in (5, 12):
            index(0xf0, objs, 60)
        if kind in (5, 11):
            index(0xf8, items, 0x110)
        if ram_def != nat_def:
            def_mismatch.append(k)
        bad = False
        for lo, hi in RANGES:
            for o in range(lo, hi, 2):
                if o in SKIP.get(kind, ()):
                    continue
                if ram[o:o + 2] != nat[o:o + 2]:
                    bad = True
                    key = (kind, o)
                    mismatch[key] += 1
                    examples.setdefault(key, (k, ram[o & ~3:(o & ~3) + 4].hex(), nat[o & ~3:(o & ~3) + 4].hex()))
        if kind == 8 and (ram[0x97] & 4) != (nat[0x97] & 4):
            bad = True
            mismatch[(8, 0x97)] += 1
        exact += not bad
    wobj_ram = []
    for k in range(u(0x23cb48)):
        o = (u(0x23c8f0 + 4 * k) - objs) // 60
        f = lambda base: struct.unpack_from('<I', m, base + 4 * k)[0]
        wobj_ram.append(dict(object=o, inity=f(0x2c1b48), y=f(0x2c18f0), starty=f(0x2c1da0), endy=f(0x2c1ff8),
                             snd=f(0x2c2250)))
    wobj_diff = [dict(index=k, ram=r, native=n) for k, (r, n) in
                 enumerate(zip(wobj_ram, native['wobjs'])) if r != n]
    object_diff = []
    for k, words in enumerate(native['objects']):
        base = objs + 60 * k
        ram = [u(base + 0x10), struct.unpack_from('<H', m, base + 0x14)[0], m[base + 0x16], m[base + 0x17]]
        if ram != words:
            object_diff.append(dict(object=k, ram=[hex(x) for x in ram], native=[hex(x) for x in words]))
    report = dict(fixture=str(FIXTURE), session=session, native_report=str(scratch), ram_items=count,
                  native_items=len(records), errors=[(k, o['error']) for k, o in enumerate(native['outcome']) if 'error' in o],
                  definition_mismatch=def_mismatch, exact_items=exact,
                  mismatch=[dict(kind=k, offset=hex(o), count=c, example=examples.get((k, o))) for (k, o), c in
                            sorted(mismatch.items(), key=lambda x: (str(x[0][0]), x[0][1]))],
                  wobjs=dict(ram=len(wobj_ram), native=len(native['wobjs']), differing=wobj_diff),
                  objects_differing=object_diff[:40], objects_differing_count=len(object_diff),
                  world_has_gates=dict(ram=i(0x23e1a0), native=native['world_has_gates']),
                  max_level_coins=native['max_level_coins'], random_calls=native['random_calls'])
    with args.report.open('x') as file:
        json.dump(report, file, indent=1)
        file.write('\n')
    print(json.dumps({k: report[k] for k in ('ram_items', 'native_items', 'exact_items', 'world_has_gates')}))
    print('errors', len(report['errors']), 'def mismatches', len(def_mismatch), 'mismatch keys', len(report['mismatch']),
          'wobj diffs', len(wobj_diff), 'object diffs', len(object_diff))


if __name__ == '__main__':
    main()
