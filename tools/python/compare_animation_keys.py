#!/usr/bin/env python3
"""Compare native exact-key reports with private, paused animation observations.

Initial-only records have stale cache vectors and are explicitly excluded. This
does not validate evaluated poses, clip transitions, matrices or animation events.
"""
import argparse
import json
import pathlib
import struct


def bounded_json(path):
    with path.open('rb') as file:
        raw = file.read(64*1024*1024+1)
    if len(raw) > 64*1024*1024:
        raise ValueError('comparison input exceeds policy')
    return json.loads(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, required=True)
    parser.add_argument('--key-dir', type=pathlib.Path, required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('choose a new private comparison output')
    trace = bounded_json(args.trace)
    profile = bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile'] != profile['profile_id'] or len(trace['samples']) > 121:
        raise ValueError('trace source/count is unsupported')
    clips, results, skipped = {}, [], []
    for sample in trace['samples']:
        observation = sample['animation']
        info = bytes.fromhex(observation['animinfo_raw_hex'])
        if len(info) != 64 or len(observation['caches']) > 16384:
            raise ValueError('animation observation shape differs')
        clip, = struct.unpack_from('<H', info, 14)
        frame, = struct.unpack_from('<f', info, 24)
        if clip not in clips:
            native = bounded_json(args.key_dir / f'{clip:03}.json')
            if native['source_sha256'] != observation['source_sha256'] or native['sequence_index'] != clip:
                raise ValueError('native key source/sequence differs')
            clips[clip] = {t['node_index']: t for t in native['outcomes']}
        for cache in observation['caches']:
            track = clips[clip][cache['node_index']]
            if track['error']:
                raise ValueError('native track decoder returned an error')
            if track['initial_only']:
                skipped.append(dict(step=sample['step'], node=cache['node_index'], reason='initial-only cache vectors may be stale'))
                continue
            raw = bytes.fromhex(cache['raw_hex'])
            if len(raw) != 160:
                raise ValueError('cache record stride differs')
            low, high = struct.unpack_from('<hh', raw, 8)
            for name, key, offsets in [('lower', low, [16, 64, 112]), ('upper', high, [32, 80, 128])]:
                if key < 0:
                    skipped.append(dict(step=sample['step'], node=cache['node_index'], reason=f'{name} key is uninitialized'))
                    continue
                decoded = next((k for k in track['keys'] if k['frame'] == key), None)
                expected = b''.join(raw[p:p+12] for p in offsets)
                actual = struct.pack('<9I', *decoded['bits']) if decoded else None
                results.append(dict(step=sample['step'], clip=clip, frame=frame, node=cache['node_index'],
                                    key=key, cache=name, byte_equal=expected == actual,
                                    expected_hex=expected.hex(), actual_hex=actual.hex() if actual else None))
                if len(results)+len(skipped) > 100_000:
                    raise ValueError('comparison record policy exceeded')
    report = dict(schema_version=1, source_profile=trace['source_profile'], fixture_sha256=trace['fixture_sha256'],
                  scope=__doc__.strip(), results=results, skipped=skipped,
                  comparison_count=len(results), equal_count=sum(r['byte_equal'] for r in results),
                  first_divergence=next((r for r in results if not r['byte_equal']), None))
    with args.out.open('x') as file:
        json.dump(report, file, indent=2, allow_nan=False)
        file.write('\n')
    print(f"{report['equal_count']}/{len(results)} exact-key caches byte equal")


if __name__ == '__main__':
    main()
