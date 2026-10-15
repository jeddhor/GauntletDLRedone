#!/usr/bin/env python3
"""Read-only capture of a guest stack frame and player record at a debugger stop.

PINE cannot read registers or the PC. The stop address and register values
(sp and any others) must come from an inspected stock-debugger screenshot and
are recorded as supplied observations. The covering original function is
compared with the ELF before any bytes are accepted. No guest writes occur.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import struct

from pine_probe import Pine, configured_socket


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--function', required=True, help='original symbol covering the stop, e.g. PlayerMotion')
    parser.add_argument('--function-address', type=lambda v: int(v, 0), required=True)
    parser.add_argument('--function-bytes', type=lambda v: int(v, 0), required=True)
    parser.add_argument('--stop', type=lambda v: int(v, 0), required=True, help='PC observed in the debugger')
    parser.add_argument('--register', action='append', default=[], help='NAME=0xVALUE observed in the debugger')
    parser.add_argument('--frame-bytes', type=lambda v: int(v, 0), required=True)
    parser.add_argument('--extra', action='append', default=[],
                        help='NAME=0xADDRESS:0xSIZE additional bounded read (ELF symbol or observed pointer)')
    parser.add_argument('--screenshot', type=pathlib.Path, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('choose a new private output')
    if not args.screenshot.resolve().is_relative_to(pathlib.Path('private').resolve()) or not args.screenshot.is_file():
        raise ValueError('the inspected debugger screenshot must be retained privately')
    registers = {}
    for item in args.register:
        name, value = item.split('=', 1)
        registers[name] = int(value, 0)
    sp = registers.get('sp')
    if sp is None or sp % 16 or not 0 < sp < 32*1024*1024 or not 0 < args.frame_bytes <= 4096 or args.frame_bytes % 4:
        raise ValueError('sp observation and bounded frame size required')
    if not args.function_address <= args.stop < args.function_address+args.function_bytes or args.function_bytes > 32768:
        raise ValueError('stop is outside the bounded function')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    source = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    elf = pathlib.Path('private/disc/SLUS_200.47').read_bytes()
    if hashlib.sha256(elf).hexdigest() != source['executable']['sha256']:
        raise ValueError('local ELF identity differs')
    segment = next(s for s in source['load_segments'] if s['virtual_address'] <= args.function_address and
                   args.function_address+args.function_bytes <= s['virtual_address']+s['file_size'])
    offset = args.function_address-segment['virtual_address']+segment['file_offset']
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047':
            raise ValueError('unexpected reference title')

        def read(address, size):
            return b''.join(struct.pack('<' + 'I'*(min(2048, size-o)//4),
                                        *pine.read_ee32(list(range(address+o, address+o+min(2048, size-o), 4))))
                            for o in range(0, size, 2048))
        if read(args.function_address, args.function_bytes) != elf[offset:offset+args.function_bytes]:
            raise ValueError('loaded function differs from ELF')
        player = profile['symbols']['players']['guest_address']
        stride = profile['player_layout']['stride_bytes']
        frame = read(sp, args.frame_bytes)
        record = read(player, stride)
        globals_ = {name: read(profile['symbols'][name]['guest_address'], 4).hex()
                    for name in ('AFrame', 'vb_counter', 'SPF')}
        extras = {}
        for item in args.extra:
            name, span = item.split('=', 1)
            address, size = (int(v, 0) for v in span.split(':', 1))
            if address % 4 or size % 4 or not 0 < size <= 4096 or not 0 < address < 32*1024*1024-size:
                raise ValueError(f'invalid bounded extra read {item}')
            extras[name] = dict(ee_address=address, raw_hex=read(address, size).hex())
    report = dict(schema_version=1, source_profile=source['profile_id'], label=args.label,
                  function=args.function, function_ee_address=args.function_address, stop_ee_address=args.stop,
                  supplied_debugger_registers={k: f'0x{v:08x}' for k, v in registers.items()},
                  screenshot=str(args.screenshot), screenshot_sha256=hashlib.sha256(args.screenshot.read_bytes()).hexdigest(),
                  frame_ee_address=sp, frame_raw_hex=frame.hex(), player_ee_address=player, player_raw_hex=record.hex(),
                  globals=globals_, extras=extras, scope='read-only debugger-stop frame; register values are screenshot observations')
    with args.out.open('x') as file:
        json.dump(report, file, indent=2)
        file.write('\n')
    print(f'captured {len(frame)} frame bytes at 0x{sp:08x}')


if __name__ == '__main__':
    main()
