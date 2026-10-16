#!/usr/bin/env python3
"""Compare native PlayerMotion request/facing outputs with raw movement traces.

Inputs come from the captured slot-0 record before the update, the control
record and SPF observed at the completed update, and the camera block before
it. Native motion-code supplies GetPlayerDMotion's base code. Compared outputs:
request +0x860..+0x868 and facing +0x86c, byte for byte. A separate diagnostic
checks whether position + request reproduces the observed position; collision,
pushes and camera limits are not implemented, so it is not a native result.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json


def build(trace):
    """Per-update native inputs and observed outputs from a raw movement trace."""
    codes, steps, targets, skipped = [], [], [], []
    for before, after in zip(trace['samples'], trace['samples'][1:]):
        if before['observation']['scalars']['AFrame'] == after['observation']['scalars']['AFrame']:
            skipped.append(dict(step=after['step'], reason='no observed game update'))
            continue
        old = bytes.fromhex(before['movement']['player_raw_hex'])
        new = bytes.fromhex(after['movement']['player_raw_hex'])
        control = bytes.fromhex(after['movement']['control_raw_hex'])
        camera = bytes.fromhex(before['movement']['camera_raw_hex'])
        if len(old) != 0x2b00 or len(new) != 0x2b00 or len(control) != 240 or len(camera) != 0xa0:
            raise ValueError('raw movement record sizes differ from layout')
        f = lambda raw, o: struct.unpack_from('<f', raw, o)[0]
        u = lambda raw, o: struct.unpack_from('<I', raw, o)[0]
        g = lambda name, kind='<I', sample=before: struct.unpack(kind, bytes.fromhex(sample['movement']['globals'][name]))[0]
        spf = g('SPF', '<f', after)
        held, edges = struct.unpack_from('<II', control, 4)
        codes.append(dict(control_word_04=held, control_word_08=edges, magnitude_20=f(control, 0x20),
                          secondary_magnitude_28=f(control, 0x28), status_93c=struct.unpack_from('<H', old, 0x93c)[0],
                          state_934=struct.unpack_from('<h', old, 0x934)[0], flags_138=u(old, 0x138),
                          word_80c=u(old, 0x80c), current_action=u(old, 0x1e0), boss_type=g('BossType', '<i'),
                          walking_timer=f(old, 0x874), spf=spf))
        steps.append(dict(request=dict(
            camera_90=f(camera, 0x90), camera_98=f(camera, 0x98), control_angle_1c=f(control, 0x1c),
            control_magnitude_20=f(control, 0x20), control_secondary_28=f(control, 0x28), base_code=None,
            current_action_1e0=u(old, 0x1e0), status_93c=struct.unpack_from('<H', old, 0x93c)[0],
            facing_86c=f(old, 0x86c), speed_124=f(old, 0x124), scale_964=f(old, 0x964), scale_96c=f(old, 0x96c),
            velocity_830=f(old, 0x830), velocity_838=f(old, 0x838), knockback_848=list(struct.unpack_from('<3f', old, 0x848)),
            flags_8b0=u(old, 0x8b0), attack_state_8c8=struct.unpack_from('<i', old, 0x8c8)[0], position_y_54=f(old, 0x54),
            matrix_44=f(old, 0x44), quest_item_state=g('QuestItemState'), boss_type=g('BossType', '<i'), spf=spf,
            world_bottom=g('WldBtm', '<f')), forward_40=f(old, 0x40), forward_48=f(old, 0x48), turn_968=f(old, 0x968),
                          position_50=list(struct.unpack_from('<3f', old, 0x50))))
        targets.append(dict(step=after['step'], request=new[0x860:0x86c].hex(), facing=new[0x86c:0x870].hex(),
                            position_before=old[0x50:0x5c], position_after=new[0x50:0x5c]))
    return codes, steps, targets, skipped


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', type=pathlib.Path, required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    trace = bounded_json(args.trace)
    profile = bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile'] != profile['profile_id'] or not 1 <= len(trace['samples']) <= 121:
        raise ValueError('trace source/count differs')
    codes, steps, targets, skipped = build(trace)
    args.out.mkdir(parents=True)
    code_path, code_native = args.out/'code-requests.json', args.out/'code-native.json'
    code_path.write_text(json.dumps(codes, indent=2, allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools', 'motion-code', str(code_path), str(code_native)], check=True)
    for step, outcome in zip(steps, bounded_json(code_native)['outcomes'], strict=True):
        if outcome['output'] is None:
            raise ValueError(f"motion-code subset rejected an update input: {outcome['error']}")
        step['request']['base_code'] = outcome['output']['base_code']
    request_path, native_path = args.out/'requests.json', args.out/'native.json'
    request_path.write_text(json.dumps(steps, indent=2, allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools', 'motion-request', str(request_path), str(native_path)], check=True)
    pack = lambda values: struct.pack('<%df' % len(values), *values).hex()
    results = []
    for outcome, target in zip(bounded_json(native_path)['outcomes'], targets, strict=True):
        differences, diagnostic = {}, None
        output = outcome['output']
        if output:
            request = output['request_output']['request']
            if pack(request) != target['request']:
                differences['request'] = dict(actual=pack(request), expected=target['request'])
            if pack([output['facing']]) != target['facing']:
                differences['facing'] = dict(actual=pack([output['facing']]), expected=target['facing'])
            # Diagnostic only: EE 0x14c14c adds the final displacement. Equality
            # shows the request survived collision/push stages unchanged here.
            uncollided = pack(output['position_if_request_final'])
            diagnostic = dict(request_reproduces_position=uncollided == target['position_after'].hex(),
                              native_position_if_request_final=uncollided,
                              observed_position=target['position_after'].hex())
        else:
            differences['error'] = outcome['error']
        results.append(dict(step=target['step'], byte_equal=not differences, differences=differences,
                            position_diagnostic=diagnostic))
    report = dict(source_profile=trace['source_profile'], fixture_sha256=trace['fixture_sha256'], trace=str(args.trace),
                  scope=__doc__.strip(), count=len(results), equal_count=sum(r['byte_equal'] for r in results),
                  skipped=skipped, first_divergence=next((r for r in results if not r['byte_equal']), None),
                  results=results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    report['request_reproduces_position_count'] = sum(bool(r['position_diagnostic'] and r['position_diagnostic']['request_reproduces_position']) for r in results)
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} request/facing outputs byte equal; "
          f"request reproduces position in {report['request_reproduces_position_count']}/{len(results)} (diagnostic)")


if __name__ == '__main__':
    main()
