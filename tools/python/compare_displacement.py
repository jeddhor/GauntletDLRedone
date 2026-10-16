#!/usr/bin/env python3
"""Compare the native request + static collision composition with traces.

Native chain per update: motion-code → motion request → PlayerCollideWalls →
PlayerCollideFloor → floor-height adjustment → slope rescale → position add.
Inputs are the record before the update (matrix rows +0x20..+0x48, collision
offset +0x81c and flags +0x84 for UpdateObjColOffset, position +0x50, +0x828/+0x82c, floor state +0x88c/+0x89c/+0x8ac/+0x100/+0x54/+0x8a0).
Compared byte for byte with the record after the update: position +0x50,
floor height +0x88c and wall object +0x8a0. Items, players, enemies, camera
limits, moving floors and transporters are not native and would show as
divergences here.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json
from compare_motion_request import build

WORLDS = pathlib.Path('private/wad/002194-7dd261b6.dat')
OBJECTS_EE = 0x11e4cb8


def object_index(address):
    if address == 0:
        return None
    if (address - OBJECTS_EE) % 60 or not 0 <= (address - OBJECTS_EE) // 60 < 4096:
        raise ValueError('object pointer outside the fixture object table')
    return (address - OBJECTS_EE) // 60


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, action='append', required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    args.out.mkdir(parents=True)
    results = []
    for n, path in enumerate(args.trace):
        trace = bounded_json(path)
        codes, steps, targets, _ = build(trace)
        pairs = [(b, a) for b, a in zip(trace['samples'], trace['samples'][1:])
                 if b['observation']['scalars']['AFrame'] != a['observation']['scalars']['AFrame']]
        code_path, code_native = args.out/f'{n}-codes.json', args.out/f'{n}-codes-native.json'
        code_path.write_text(json.dumps(codes, indent=2, allow_nan=False)+'\n')
        subprocess.run(['target/release/gdl_tools', 'motion-code', str(code_path), str(code_native)], check=True)
        requests = []
        for step, outcome, (before, _) in zip(steps, bounded_json(code_native)['outcomes'], pairs, strict=True):
            old = bytes.fromhex(before['movement']['player_raw_hex'])
            f = lambda o: struct.unpack_from('<f', old, o)[0]
            u = lambda o: struct.unpack_from('<I', old, o)[0]
            step['request']['base_code'] = outcome['output']['base_code']
            requests.append(dict(request=step['request'],
                                 matrix=[list(struct.unpack_from('<3f', old, 0x20+16*r)) for r in range(3)],
                                 collision_offset_81c=list(struct.unpack_from('<3f', old, 0x81c)), offset_flags_84=u(0x84),
                                 position=list(struct.unpack_from('<3f', old, 0x50)), radius_828=f(0x828),
                                 height_82c=f(0x82c), floor_y_88c=f(0x88c), floor_object_89c=object_index(u(0x89c)),
                                 flags_8ac=u(0x8ac), word_100=struct.unpack_from('<i', old, 0x100)[0], y_54=f(0x54),
                                 wall_object_8a0=object_index(u(0x8a0))))
        step_path, native_path = args.out/f'{n}-steps.json', args.out/f'{n}-native.json'
        step_path.write_text(json.dumps(requests, indent=2, allow_nan=False)+'\n')
        subprocess.run(['target/release/gdl_tools', 'player-displacement', str(WORLDS), str(step_path), str(native_path)],
                       check=True)
        for outcome, target, (_, after) in zip(bounded_json(native_path)['outcomes'], targets, pairs, strict=True):
            new = bytes.fromhex(after['movement']['player_raw_hex'])
            diff = {}
            o = outcome['output']
            if o is None:
                diff['error'] = outcome['error']
            else:
                got = dict(position=struct.pack('<3I', *o['position_bits']).hex(),
                           floor_y=struct.pack('<I', o['floor_y_bits']).hex(),
                           wall_object=o['wall_object'])
                want = dict(position=new[0x50:0x5c].hex(), floor_y=new[0x88c:0x890].hex(),
                            wall_object=object_index(struct.unpack_from('<I', new, 0x8a0)[0]))
                diff = {k: dict(native=got[k], observed=want[k]) for k in got if got[k] != want[k]}
            results.append(dict(trace=path.name, step=target['step'], byte_equal=not diff, differences=diff,
                                wall_result=o and o['wall_result'], floor_result=o and o['floor_result']))
    report = dict(scope=__doc__.strip(), traces=[str(p) for p in args.trace], count=len(results),
                  equal_count=sum(r['byte_equal'] for r in results),
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} updates: position, floor height and wall object byte equal")


if __name__ == '__main__':
    main()
