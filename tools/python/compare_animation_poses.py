#!/usr/bin/env python3
"""Compare original local matrix/scale observations with the native sampler.

World composition, materials, clip clocks, blend and mirror paths are excluded.
Source-bound original node initial translations are checked, not adjusted.
"""
import argparse
import hashlib
import json
import pathlib
import struct
import subprocess

from compare_animation_keys import bounded_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, required=True)
    parser.add_argument('--animation-source', type=pathlib.Path, required=True)
    parser.add_argument('--rig-source', type=pathlib.Path, required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True, help='new private output directory')
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private directory')
    trace = bounded_json(args.trace)
    profile = bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile'] != profile['profile_id'] or len(trace['samples']) > 121:
        raise ValueError('unsupported trace source/count')
    source_hash = hashlib.file_digest(args.animation_source.open('rb'), 'sha256').hexdigest()
    requests, expected, skipped = [], [], []
    for sample in trace['samples']:
        a = sample['animation']
        if a['source_sha256'] != source_hash:
            raise ValueError('animation source identity differs')
        info = bytes.fromhex(a['animinfo_raw_hex'])
        if len(info) != 64 or len(a['poses']) > 16384:
            raise ValueError('unsupported observation shape')
        blend, = struct.unpack_from('<f', info, 20)
        if blend != 0 or info[19] & 1:
            skipped.append(dict(step=sample['step'], reason='blend/mirror path is unsupported'))
            continue
        clip, = struct.unpack_from('<H', info, 14)
        frame, = struct.unpack_from('<f', info, 24)
        for pose in a['poses']:
            raw = bytes.fromhex(pose['local_raw_hex'])
            if len(raw) != 80:
                raise ValueError('local pose byte count differs')
            requests.append(dict(tree=0, node=pose['node_index'], sequence=clip, frame=frame))
            expected.append(dict(step=sample['step'], node=pose['node_index'], expected_hex=raw[:76].hex()))
            if len(requests) > 4096:
                raise ValueError('native request policy exceeded')
    args.out.mkdir(parents=True)
    request_path = args.out/'requests.json'
    request_path.write_text(json.dumps(requests, indent=2)+'\n')
    native_path = args.out/'native.json'
    subprocess.run(['target/release/gdl_tools', 'anim-transforms', str(args.animation_source),
                    str(args.rig_source), str(request_path), str(native_path)], check=True)
    native = bounded_json(native_path)
    if native['source_sha256'] != source_hash:
        raise ValueError('native source identity differs')
    results = []
    for output, target in zip(native['outcomes'], expected, strict=True):
        s = output['sample']
        raw = struct.pack('<19I', *s['matrix_bits'], *s['scale_bits']) if not s['error'] else None
        results.append(dict(**target, request=output['request'], actual_hex=raw.hex() if raw else None,
                            byte_equal=raw.hex() == target['expected_hex'] if raw else False, error=s['error']))
    report = dict(schema_version=1, source_profile=trace['source_profile'], source_sha256=source_hash,
                  rig_sha256=native['rig_sha256'], fixture_sha256=trace['fixture_sha256'], scope=__doc__.strip(),
                  count=len(results), equal_count=sum(r['byte_equal'] for r in results), skipped=skipped,
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} local poses byte equal; {len(skipped)} skipped observations")


if __name__ == '__main__':
    main()
