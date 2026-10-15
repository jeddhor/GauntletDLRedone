#!/usr/bin/env python3
"""Compare native fqdist with the VU0 staging/result blocks in movement traces.

At each paused vblank, 0x2a1fc0 holds the most recent call's operands and
0x2a1fa0 its full vf16 result. Native output must match the x lane (returned
distance) and y lane (z squared) byte for byte; z/w must remain the operand's
untouched lanes. Distinct pairs are compared once; caller identity is unknown.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, action='append', required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    pairs = {}
    for path in args.trace:
        trace = bounded_json(path)
        for sample in trace['samples']:
            blocks = sample['movement']['vu0_blocks']
            key = (blocks['fqdist_operands'], blocks['fqdist_result'])
            pairs.setdefault(key, []).append(f"{path.name}:{sample['step']}")
    pairs = sorted(pairs.items())
    requests = [list(struct.unpack_from('<2f', bytes.fromhex(i))) for (i, _), _ in pairs]
    args.out.mkdir(parents=True)
    request_path, native_path = args.out/'requests.json', args.out/'native.json'
    request_path.write_text(json.dumps(requests, indent=2, allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools', 'vu0-fqdist', str(request_path), str(native_path)], check=True)
    results = []
    for ((operands, result), where), outcome in zip(pairs, bounded_json(native_path)['outcomes'], strict=True):
        observed, staged = bytes.fromhex(result), bytes.fromhex(operands)
        if outcome['error']:
            native = None
        else:
            native = struct.pack('<2f', outcome['x_lane'], outcome['y_lane']) + staged[8:16]
        results.append(dict(operands=operands, observed=result, native=native.hex() if native else None,
                            byte_equal=native == observed, error=outcome['error'], samples=where))
    report = dict(scope=__doc__.strip(), traces=[str(p) for p in args.trace], count=len(results),
                  equal_count=sum(r['byte_equal'] for r in results),
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} distinct fqdist results byte equal "
          f"({sum(len(r['samples']) for r in results)} samples)")


if __name__ == '__main__':
    main()
