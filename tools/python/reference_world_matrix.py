#!/usr/bin/env python3
"""Read original GetWorldMat inputs/outputs at stock debugger breakpoints.

Register/PC values must be supplied from inspected debugger screenshots. PINE
does not expose them. This script never drives the debugger or writes guest data.
Before: stop at GetWorldMat entry. After: remove entry, stop at observed RA.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import struct
import subprocess

from pine_probe import Pine, configured_socket
from compare_animation_keys import bounded_json


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase',choices=['before','after'])
    parser.add_argument('--matrix',type=lambda v:int(v,0))
    parser.add_argument('--output-pointer',type=lambda v:int(v,0))
    parser.add_argument('--offset-pointer',type=lambda v:int(v,0),default=0)
    parser.add_argument('--return-pc',type=lambda v:int(v,0),required=True)
    parser.add_argument('--return-word',type=lambda v:int(v,0))
    parser.add_argument('--controlled-input',action='store_true',help='label arguments deliberately substituted through the debugger; not a natural gameplay call')
    parser.add_argument('--screenshot',type=pathlib.Path,required=True)
    parser.add_argument('--out',type=pathlib.Path,required=True)
    args=parser.parse_args()
    if args.return_pc%4 or not 0<args.return_pc<32*1024*1024:
        raise ValueError('invalid observed return PC')
    private=pathlib.Path('private').resolve()
    if not args.out.resolve().is_relative_to(private) or not args.screenshot.resolve().is_relative_to(private):
        raise ValueError('reference evidence must stay private')
    if args.screenshot.stat().st_size>16*1024*1024:
        raise ValueError('screenshot exceeds policy')
    source=bounded_json(pathlib.Path('profiles/ps2/slus-20047.json'))
    profile=bounded_json(pathlib.Path('profiles/ps2/slus-20047-reference.json'))
    metadata=bounded_json(pathlib.Path('private/reports/elf-metadata.json'))
    function=next(s for s in metadata['symbols'] if s['name']=='GetWorldMat' and s['type']==2)
    if not 1<=function['size']<=2048:
        raise ValueError('source function size exceeds policy')
    elf_path=pathlib.Path('private/disc/SLUS_200.47')
    if elf_path.stat().st_size!=source['executable']['size']:
        raise ValueError('ELF source size differs')
    elf=elf_path.read_bytes()
    if hashlib.sha256(elf).hexdigest()!=source['executable']['sha256']:
        raise ValueError('ELF source identity differs')
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3);connection.connect(str(configured_socket()));pine=Pine(connection)
        if pine.status()!=1 or pine.text(12)!='SLUS-20047':
            raise ValueError('expected supported paused reference')
        def read(address,size):
            if address%4 or size%4 or not 0<size<=2048 or address<0 or address+size>32*1024*1024:
                raise ValueError('invalid bounded EE read')
            return struct.pack('<'+'I'*(size//4),*pine.read_ee32(list(range(address,address+size,4))))
        addr,size=function['guest_address'],function['size']
        segment=next(s for s in source['load_segments'] if s['virtual_address']<=addr and addr+size<=s['virtual_address']+s['file_size'])
        file_offset=addr-segment['virtual_address']+segment['file_offset']
        if read(addr,size)!=elf[file_offset:file_offset+size]:
            raise ValueError('original matrix function differs in EE RAM')
        clock={name:read(profile['symbols'][name]['guest_address'],4).hex() for name in ('ATime','AFrame','Time')}
        evidence=dict(source_profile=source['profile_id'],executable_sha256=source['executable']['sha256'],
                      screenshot=str(args.screenshot),screenshot_sha256=hashlib.sha256(args.screenshot.read_bytes()).hexdigest(),clock=clock)
        if args.phase=='before':
            if args.out.exists() or args.matrix is None or args.output_pointer is None:
                raise ValueError('before needs pointers and a new private case directory')
            raw_records=[]
            layout=profile['animation_layout']
            def box(address):
                if not address or address%16:
                    raise ValueError('invalid aligned matrix box')
                raw=read(address,layout['matrix_box_capture_bytes'])
                raw_records.append(dict(ee_address=address,raw_hex=raw.hex()))
                return dict(matrix=list(struct.unpack_from('<16f',raw)),flags=struct.unpack_from('<I',raw,layout['matrix_box_flags_offset'])[0]),struct.unpack_from('<I',raw,layout['matrix_box_parent_offset'])[0]
            leaf,parent=box(args.matrix);parents=[];seen={args.matrix}
            while parent:
                if parent in seen or len(parents)>=128:
                    raise ValueError('matrix parent chain exceeds policy or cycles')
                seen.add(parent);value,parent=box(parent);parents.append(value)
            offset=list(struct.unpack('<3f',read(args.offset_pointer,12))) if args.offset_pointer else None
            evidence.update(pc_from_debugger=addr,registers_from_debugger=dict(a0=args.matrix,a1=args.output_pointer,a2=args.offset_pointer,ra=args.return_pc),
                            probe_kind='controlled debugger argument substitution' if args.controlled_input else 'natural original call',
                            matrix_boxes=raw_records,output_before_hex=read(args.output_pointer,64).hex())
            request=dict(leaf=leaf,parents=parents,offset=offset)
            if pine.status()!=1:
                raise ValueError('reference resumed during capture')
            args.out.mkdir(parents=True)
            (args.out/'before.json').write_text(json.dumps(evidence,indent=2,allow_nan=False)+'\n')
            (args.out/'requests.json').write_text(json.dumps([request],indent=2,allow_nan=False)+'\n')
            print(f'Captured leaf and{len(parents)} parent matrices')
        else:
            if (args.out/'after.json').exists() or args.return_word is None or not 0<=args.return_word<=0xffffffff:
                raise ValueError('after needs observed return word and unused output paths')
            before=bounded_json(args.out/'before.json')
            if before['source_profile']!=source['profile_id'] or before['executable_sha256']!=source['executable']['sha256']:
                raise ValueError('case identity differs')
            if before['registers_from_debugger']['ra']!=args.return_pc:
                raise ValueError('return PC differs from observed entry RA')
            raw=read(before['registers_from_debugger']['a1'],64)
            evidence.update(return_pc_from_debugger=args.return_pc,return_word_from_debugger=args.return_word,output_raw_hex=raw.hex(),
                            parents_after=[dict(ee_address=r['ee_address'],raw_hex=read(r['ee_address'],128).hex()) for r in before['matrix_boxes'][1:]])
            if pine.status()!=1:
                raise ValueError('reference resumed during capture')
            (args.out/'after.json').open('x').write(json.dumps(evidence,indent=2,allow_nan=False)+'\n')
            subprocess.run(['target/release/gdl_tools','world-matrix',str(args.out/'requests.json'),str(args.out/'native.json')],check=True)
            native=bounded_json(args.out/'native.json')['outcomes'][0]
            computed=struct.pack('<16f',*native['output']['matrix']) if native['output'] else None
            result=dict(source_profile=source['profile_id'],executable_sha256=source['executable']['sha256'],
                        probe_kind=before['probe_kind'],
                        clock_unchanged=before['clock']==clock,byte_equal=computed==raw,
                        return_equal=int(native['output']['translation_only'])==args.return_word if native['output'] else False,
                        error=native['error'],differences=[dict(index=i,expected=raw[i*4:i*4+4].hex(),actual=computed[i*4:i*4+4].hex()) for i in range(16) if computed and raw[i*4:i*4+4]!=computed[i*4:i*4+4]],
                        scope=__doc__.strip())
            (args.out/'comparison.json').open('x').write(json.dumps(result,indent=2,allow_nan=False)+'\n')
            print(f"World matrix byte equal:{result['byte_equal']}; return equal:{result['return_equal']}; clock unchanged:{result['clock_unchanged']}")


if __name__=='__main__':
    main()
