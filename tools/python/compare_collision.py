#!/usr/bin/env python3
"""Compare native static WorldCollide with the last query captured each vblank.

ColFirstP/ColLastP, CheckFlags and ColMinNormY/ColMaxNormY come from the capture.
The radius is accepted only when the native ColMinY/ColMaxY reproduce the
captured values exactly. The object mask is FloorCollide's 0x23c; exclusion
masks 1 and 0 are tried and recorded, since the caller is not identified.
Compared byte for byte: ColDirVec, ColVecLen, ColMinY/MaxY, col_obj (object
index → loaded record address), col_tri_idx, col_dist, col_pos and water_col.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json

WORLDS = pathlib.Path('private/wad/002194-7dd261b6.dat')
OBJECTS_EE = 0x11e4cb8  # WorldInfo object table in the preserved fixture
RADII = [1.5]


def fields(sample):
    b = {k: bytes.fromhex(v) for k, v in sample['movement']['collision_blocks'].items()}
    q, c = b['floor_wall_cam_water_col_and_query'], b['col_result_and_extents']
    at = lambda a, n=4: q[a-0x21f510:a-0x21f510+n]
    return dict(first=at(0x21f650, 12), last=at(0x21f660, 12), dir=at(0x21f670, 12), len=at(0x21f680),
                min_y=at(0x21f684), max_y=at(0x21f688), norm=at(0x21f68c, 8), tri_idx=at(0x21f694),
                flags=struct.unpack('<I', at(0x21f69c))[0], col_obj=c[0:4], col_dist=c[4:8], col_pos=c[0x10:0x1c],
                water_obj=at(0x21f644), water_dist=at(0x21f640), water_pos=at(0x21f630, 12))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, action='append', required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    observations = {}
    for path in args.trace:
        for sample in bounded_json(path)['samples']:
            f = fields(sample)
            key = tuple(f[k].hex() if isinstance(f[k], bytes) else f[k] for k in sorted(f))
            observations.setdefault(key, (f, []))[1].append(f"{path.name}:{sample['step']}")
    cases = list(observations.values())
    requests = []
    for f, _ in cases:
        for radius in RADII:
            for exclusion in (1, 0):
                requests.append(dict(radius=radius, first=list(struct.unpack('<3f', f['first'])),
                                     last=list(struct.unpack('<3f', f['last'])), check_flags=f['flags'],
                                     min_normal_y=struct.unpack('<f', f['norm'][:4])[0],
                                     max_normal_y=struct.unpack('<f', f['norm'][4:])[0],
                                     object_mask=0x23c, exclusion_mask=exclusion))
    args.out.mkdir(parents=True)
    request_path, native_path = args.out/'requests.json', args.out/'native.json'
    request_path.write_text(json.dumps(requests, indent=2, allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools', 'collision-query', str(WORLDS), str(native_path), str(request_path)],
                   check=True)
    outcomes = bounded_json(native_path)['outcomes']
    pack = lambda bits: struct.pack('<%dI' % len(bits), *bits)
    results = []
    per = len(RADII)*2
    for i, (f, where) in enumerate(cases):
        tried = []
        for o in outcomes[i*per:(i+1)*per]:
            if o['error']:
                tried.append(dict(request=o['request'], error=o['error']))
                continue
            diff = {}
            expect = dict(dir=f['dir'], len=f['len'], min_y=f['min_y'], max_y=f['max_y'])
            got = dict(dir=pack(o['direction_bits']), len=pack([o['length_bits']]),
                       min_y=pack([o['min_y_bits']]), max_y=pack([o['max_y_bits']]))
            s = o['solid']
            got.update(col_obj=struct.pack('<I', OBJECTS_EE+60*s['object']) if s else b'\0'*4,
                       tri_idx=struct.pack('<I', s['triangle']) if s else None,
                       col_dist=pack([s['distance_bits']]) if s else None, col_pos=pack(s['position_bits']) if s else None)
            expect.update(col_obj=f['col_obj'], tri_idx=f['tri_idx'] if s else None,
                          col_dist=f['col_dist'] if s else None, col_pos=f['col_pos'] if s else None)
            wtr = o['water']
            got.update(water_obj=struct.pack('<I', OBJECTS_EE+60*wtr['object']) if wtr else b'\0'*4)
            expect.update(water_obj=f['water_obj'])
            for k in expect:
                if expect[k] != got[k]:
                    diff[k] = dict(native=got[k].hex() if got[k] else None, observed=expect[k].hex() if expect[k] else None)
            tried.append(dict(request=o['request'], byte_equal=not diff, differences=diff))
        match = next((t for t in tried if t.get('byte_equal')), None)
        results.append(dict(samples=where, byte_equal=match is not None,
                            exclusion_mask=match['request']['exclusion_mask'] if match else None, tried=tried))
    report = dict(scope=__doc__.strip(), traces=[str(p) for p in args.trace], count=len(results),
                  equal_count=sum(r['byte_equal'] for r in results),
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} distinct WorldCollide observations byte equal "
          f"({sum(len(r['samples']) for r in results)} samples)")


if __name__ == '__main__':
    main()
