#!/usr/bin/env python3
"""Compare ordinary motion-code/timer outputs with source-bound control traces.

Original logical PlayerControl words/magnitudes and prior player state are
supplied. PCSX2 keyboard polling/latency, requested-action mapping, displacement,
collision, camera and forced-mode transitions are excluded.
"""
import argparse
import json
import pathlib
import struct
import subprocess
from compare_animation_keys import bounded_json


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace',type=pathlib.Path,required=True)
    parser.add_argument('--out',type=pathlib.Path,required=True)
    args=parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private case directory')
    trace=bounded_json(args.trace);profile=bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    if trace['source_profile']!=profile['profile_id'] or not 1<=len(trace['samples'])<=121:
        raise ValueError('trace source/count differs')
    requests=[];targets=[];skipped=[]
    for before,after in zip(trace['samples'],trace['samples'][1:]):
        if before['observation']['scalars']['AFrame']==after['observation']['scalars']['AFrame']:
            skipped.append(dict(step=after['step'],reason='no observed game update'));continue
        c=before['animation']['action_context']
        word=lambda name:struct.unpack('<I',bytes.fromhex(c[name]))[0]
        f=lambda name:struct.unpack('<f',bytes.fromhex(c[name]))[0]
        control=bytes.fromhex(after['observation']['PlayerControl']['raw_hex'])
        if len(control)!=240:
            raise ValueError('source control record count/stride differs')
        held,edges=struct.unpack_from('<II',control,4)
        magnitude=struct.unpack_from('<f',control,32)[0]
        secondary=struct.unpack_from('<f',control,40)[0]
        boss=struct.unpack('<i',bytes.fromhex(before['animation']['action_globals']['BossType']))[0]
        requests.append(dict(control_word_04=held,control_word_08=edges,
            magnitude_20=magnitude,secondary_magnitude_28=secondary,
            status_93c=word('status_93c_pair')&0xffff,
            state_934=struct.unpack_from('<h',bytes.fromhex(c['state_934_pair']))[0],
            flags_138=word('flags_138'),word_80c=word('word_80c'),
            current_action=before['observation']['players'][0]['fields']['current_action']['values'][0],
            boss_type=boss,walking_timer=f('walking_timer_874'),
            spf=after['observation']['scalars']['SPF']['float32']))
        c=after['animation']['action_context']
        targets.append(dict(step=after['step'],code=word('motion_code_1dc'),walking_timer=f('walking_timer_874')))
    args.out.mkdir(parents=True)
    request_path=args.out/'requests.json';native_path=args.out/'native.json'
    request_path.write_text(json.dumps(requests,indent=2,allow_nan=False)+'\n')
    subprocess.run(['target/release/gdl_tools','motion-code',str(request_path),str(native_path)],check=True)
    native=bounded_json(native_path);results=[]
    for outcome,target in zip(native['outcomes'],targets,strict=True):
        output=outcome['output'];differences={}
        if output:
            for field in ('code','walking_timer'):
                equal=struct.pack('<f',output[field])==struct.pack('<f',target[field]) if field=='walking_timer' else output[field]==target[field]
                if not equal:differences[field]=dict(actual=output[field],expected=target[field])
        else:differences['error']=outcome['error']
        results.append(dict(step=target['step'],byte_equal=not differences,differences=differences))
    report=dict(source_profile=trace['source_profile'],fixture_sha256=trace['fixture_sha256'],scope=__doc__.strip(),
                count=len(results),equal_count=sum(r['byte_equal'] for r in results),skipped=skipped,
                first_divergence=next((r for r in results if not r['byte_equal']),None),results=results)
    (args.out/'comparison.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')
    print(f"{report['equal_count']}/{len(results)} motion codes/timers byte equal")


if __name__=='__main__':main()
