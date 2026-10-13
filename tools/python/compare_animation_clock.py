#!/usr/bin/env python3
"""Compare finite clock updates at observed game-update boundaries.

Select same-clip/same-start pairs. Action transitions and caller restarts are
reported as exclusions, not counted as native clock acceptance.
"""
import argparse
import json
import pathlib
import struct
import subprocess

from compare_animation_keys import bounded_json


def clock_state(raw):
    if len(raw) != 64:
        raise ValueError('AnimInfo record size differs')
    u = lambda p: struct.unpack_from('<H', raw, p)[0]
    f = lambda p: struct.unpack_from('<f', raw, p)[0]
    return dict(sequence=u(14), previous_sequence=u(28), reset_byte=raw[18], option_bits=raw[19],
                enabled_word=u(30), loop_word=u(52), mode_bits=u(54), frame=f(24), blend_fraction=f(20),
                start_time=f(32), blend_end_time=f(36), period_scale=f(44), last_time=f(48))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, required=True)
    parser.add_argument('--metadata', type=pathlib.Path, required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    parser.add_argument('--starts', action='store_true', help='compare initialization using observed clip/start choices; action prediction excluded')
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private output directory')
    trace, metadata = bounded_json(args.trace), bounded_json(args.metadata)
    profile = bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile'] != profile['profile_id'] or len(trace['samples']) > 121:
        raise ValueError('unsupported source/count')
    sequences = metadata['animations']['trees'][0]['sequences']
    requests, targets, skipped = [], [], []
    for before, after in zip(trace['samples'], trace['samples'][1:]):
        if before['observation']['scalars']['AFrame'] == after['observation']['scalars']['AFrame']:
            skipped.append(dict(step=after['step'], reason='no observed game update'))
            continue
        a, b = (clock_state(bytes.fromhex(s['animation']['animinfo_raw_hex'])) for s in (before, after))
        changed = a['sequence'] != b['sequence'] or a['start_time'] != b['start_time']
        if changed != args.starts:
            skipped.append(dict(step=after['step'], reason='outside selected start/update comparison scope'))
            continue
        if any(s['animation']['source_sha256'] != metadata['source_sha256'] for s in (before, after)):
            raise ValueError('source identities differ')
        globals_ = after['animation']['clock_globals']
        now = struct.unpack('<f', bytes.fromhex(globals_['ATime']))[0]
        scale = struct.unpack('<f', bytes.fromhex(globals_['AnimTimeScale']))[0]
        if args.starts:
            if b['blend_fraction'] != 0.0 or b['frame'] != 0.0:
                skipped.append(dict(step=after['step'], reason='start/blend inputs not recovered'))
                continue
            raw=bytes.fromhex(after['animation']['animinfo_raw_hex'])
            playback_scale=struct.unpack_from('<f',raw,40)[0]
            seq=sequences[b['sequence']]
            factor=struct.unpack_from('<h',bytes(seq['raw']),34)[0]
            requests.append(dict(state=a,input=dict(sequence=b['sequence'],frames=seq['frames'],timing_factor=factor,
                repeat=seq['repeat'],start_frame=0,enabled_word=1,playback_scale=playback_scale,blend_duration=0,
                now=now,anim_time_scale=scale),evaluate=True,sampled=True))
        else:
            requests.append(dict(state=a, now=now, frames=sequences[a['sequence']]['frames'], anim_time_scale=scale))
        targets.append(dict(step=after['step'], state=b))
    args.out.mkdir(parents=True)
    request_path, native_path = args.out/'requests.json', args.out/'native.json'
    request_path.write_text(json.dumps(requests, indent=2, allow_nan=False)+'\n')
    command='anim-clock-start' if args.starts else 'anim-clock'
    subprocess.run(['target/release/gdl_tools', command, str(request_path), str(native_path)], check=True)
    native = bounded_json(native_path)
    results = []
    for out, target in zip(native['outcomes'], targets, strict=True):
        actual = out['state'] if args.starts else out['output']['state'] if out['output'] else None
        differences = {}
        if actual:
            for field, value in target['state'].items():
                equal = struct.pack('<f', actual[field]) == struct.pack('<f', value) if isinstance(value, float) else actual[field] == value
                if not equal:
                    differences[field] = dict(actual=actual[field], expected=value)
        else:
            differences['error'] = out['error']
        results.append(dict(step=target['step'], sequence=target['state']['sequence'], byte_equal=not differences,
                            differences=differences, return_code=None if args.starts else out['output']['return_code'] if out['output'] else None))
    report = dict(schema_version=1, source_profile=trace['source_profile'], fixture_sha256=trace['fixture_sha256'],
                  source_sha256=metadata['source_sha256'], scope='observed clip/start/blend choices supplied to InitAnim + CalcAnimInfo + tagged-node sample reset handshake; action choice rules excluded' if args.starts else __doc__.strip(), return_code_evidence='source-derived, not directly captured',
                  count=len(results), equal_count=sum(r['byte_equal'] for r in results), skipped=skipped,
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} clock states byte equal; {len(skipped)} pairs excluded")


if __name__ == '__main__':
    main()
