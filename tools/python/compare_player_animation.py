#!/usr/bin/env python3
"""Compare bounded native player animation selection at observed game updates.

The post-update requested action is supplied as the original producer's input.
Native code predicts the selected action/clip/mode/blend and thirteen clock
fields. This does not validate pad/action production, events or update ordering.
"""
import argparse
import hashlib
import json
import pathlib
import struct
import subprocess

from compare_animation_keys import bounded_json
from compare_animation_clock import clock_state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, required=True)
    parser.add_argument('--animation-source', type=pathlib.Path, required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    parser.add_argument('--events', action='store_true', help='compare ordinary Sorceress event-word/start-time subset; projectile creation and hits excluded')
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private output directory')
    trace = bounded_json(args.trace)
    profile = bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile'] != profile['profile_id'] or not 1 <= len(trace['samples']) <= 121:
        raise ValueError('unsupported source/count')
    if args.animation_source.stat().st_size > 64*1024*1024:
        raise ValueError('animation source exceeds policy')
    source_hash = hashlib.sha256(args.animation_source.read_bytes()).hexdigest()
    requests, targets, skipped = [], [], []
    for before, after in zip(trace['samples'], trace['samples'][1:]):
        if before['observation']['scalars']['AFrame'] == after['observation']['scalars']['AFrame']:
            skipped.append(dict(step=after['step'], reason='no observed game update'))
            continue
        a, b = (clock_state(bytes.fromhex(s['animation']['animinfo_raw_hex'])) for s in (before, after))
        if any(s['animation']['source_sha256'] != source_hash for s in (before, after)):
            raise ValueError('source identities differ')
        old, new = (s['observation']['players'][0]['fields'] for s in (before, after))
        raw_map = bytes.fromhex(before['animation']['action_map_raw_hex'])
        if len(raw_map) != 1200 or raw_map.hex() != after['animation']['action_map_raw_hex']:
            raise ValueError('action map shape/identity changed')
        action_map = [struct.unpack_from('<i', raw_map, i*8)[0] for i in range(150)]
        raw_context = before['animation']['action_context']
        word = lambda name: struct.unpack('<I', bytes.fromhex(raw_context[name]))[0]
        counters = struct.unpack('<hh', bytes.fromhex(raw_context['idle_counter_pair']))
        context = dict(current_action=old['current_action']['values'][0],
                       requested_action=new['requested_action']['values'][0], frame=a['frame'],
                       flags_130=word('flags_130'), flags_134=word('flags_134'),
                       flags_138=word('flags_138'), flags_8ac=word('flags_8ac'),
                       status_93c=word('status_93c_pair') & 0xffff,
                       idle_counter_1d8=counters[0], idle_counter_1da=counters[1],
                       word_80c=word('word_80c'))
        globals_ = after['animation']['clock_globals']
        scalar = lambda name: struct.unpack('<f', bytes.fromhex(globals_[name]))[0]
        requests.append(dict(state=a, context=context, action_map=action_map,
                             now=scalar('ATime'), anim_time_scale=scalar('AnimTimeScale'), sampled=True))
        targets.append(dict(step=after['step'], state=b,
                            current_action=new['current_action']['values'][0],
                            attack_type=new['attack_state_word']['values'][0]))
        if args.events:
            requests[-1]['events'] = dict(event_word=word('event_word_8d8'),
                throw_start_time=struct.unpack('<f',bytes.fromhex(raw_context['time_8d4']))[0],
                now=after['observation']['scalars']['Time']['float32'],
                class_word=word('class_word'), word_720=word('word_720'))
            c = after['animation']['action_context']
            targets[-1]['events'] = dict(event_word=struct.unpack('<I',bytes.fromhex(c['event_word_8d8']))[0],
                throw_start_time=struct.unpack('<f',bytes.fromhex(c['time_8d4']))[0])
    args.out.mkdir(parents=True)
    request_path, native_path = args.out/'requests.json', args.out/'native.json'
    request_path.write_text(json.dumps(requests, indent=2, allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools', 'player-anim-select', str(args.animation_source),
                    str(request_path), str(native_path)], check=True)
    native = bounded_json(native_path)
    if native['source_sha256'] != source_hash:
        raise ValueError('native source hash differs')
    results = []
    for outcome, target in zip(native['outcomes'], targets, strict=True):
        output = outcome['output']
        differences = {}
        if output:
            actual = output['animation']['state']
            for field, value in target['state'].items():
                equal = struct.pack('<f', actual[field]) == struct.pack('<f', value) if isinstance(value, float) else actual[field] == value
                if not equal:
                    differences[field] = dict(actual=actual[field], expected=value)
            for field in ('current_action', 'attack_type'):
                if output[field] != target[field]:
                    differences[field] = dict(actual=output[field], expected=target[field])
            if args.events:
                for field,value in target['events'].items():
                    actual = output['events'][field]
                    equal = struct.pack('<f', actual) == struct.pack('<f', value) if isinstance(value,float) else actual == value
                    if not equal:
                        differences['events.'+field] = dict(actual=actual,expected=value)
        else:
            differences['error'] = outcome['error']
        results.append(dict(step=target['step'], byte_equal=not differences,
                            differences=differences,
                            missile_request=output['events']['missile_request'] if output and output['events'] else None,
                            return_flags=output['animation']['return_flags'] if output else None))
    report = dict(schema_version=1, source_profile=trace['source_profile'],
                  fixture_sha256=trace['fixture_sha256'], source_sha256=source_hash,
                  scope=__doc__.strip(), return_flags_evidence='source-derived, not directly captured',
                  event_scope='event word and throw-start time compared; missile requests source-derived; projectile creation, positions, successful launches and hits excluded' if args.events else None,
                  count=len(results), equal_count=sum(r['byte_equal'] for r in results), skipped=skipped,
                  first_divergence=next((r for r in results if not r['byte_equal']), None), results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} player animation states byte equal; {len(skipped)} pairs excluded")


if __name__ == '__main__':
    main()
