#!/usr/bin/env python3
"""Closed-loop check of the interim PlayerSim against movement traces.

Starting from the exported start state, the native simulation receives only
each update's original control level/edge words and evolves its own state.
Position +0x50 and facing +0x86c after every update are compared byte for byte.
Errors accumulate here, so this also measures the interim matrix rebuild.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json

START = pathlib.Path('private/reference/start-states/poison-fields-sor-v2.json')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, action='append', required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    args.out.mkdir(parents=True)
    summary = []
    for n, path in enumerate(args.trace):
        samples = bounded_json(path)['samples']
        pairs = [(b, a) for b, a in zip(samples, samples[1:])
                 if b['observation']['scalars']['AFrame'] != a['observation']['scalars']['AFrame']]
        inputs = [list(struct.unpack_from('<II', bytes.fromhex(a['movement']['control_raw_hex']), 4)) for _, a in pairs]
        ip, op = args.out/f'{n}-inputs.json', args.out/f'{n}-native.json'
        ip.write_text(json.dumps(inputs)+'\n')
        subprocess.run(['target/release/gdl_tools', 'player-sim', 'private/wad/002194-7dd261b6.dat',
                        'private/disc/SLUS_200.47', str(START), str(ip), str(op)], check=True)
        results = []
        for (_, a), u in zip(pairs, bounded_json(op)['updates']):
            new = bytes.fromhex(a['movement']['player_raw_hex'])
            if u.get('error'):
                results.append(dict(step=a['step'], error=u['error']))
                break
            results.append(dict(step=a['step'],
                                position_equal=struct.pack('<3I', *u['position_bits']) == new[0x50:0x5c],
                                facing_equal=struct.pack('<I', u['facing_bits']) == new[0x86c:0x870]))
        summary.append(dict(trace=path.name, updates=len(pairs),
                            position_equal=sum(r.get('position_equal', False) for r in results),
                            facing_equal=sum(r.get('facing_equal', False) for r in results), results=results))
        print(f"{path.name}: positions {summary[-1]['position_equal']}/{len(pairs)}, facing {summary[-1]['facing_equal']}/{len(pairs)}")
    (args.out/'comparison.json').write_text(json.dumps(dict(scope=__doc__.strip(), traces=summary), indent=2)+'\n')


if __name__ == '__main__':
    main()
