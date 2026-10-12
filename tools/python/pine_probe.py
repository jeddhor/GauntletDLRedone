#!/usr/bin/env python3
"""Bounded read-only PCSX2 PINE diagnostics; not a deterministic stepping bridge.

PINE has no input-schedule/guest-update-boundary operation in the inspected
reference source. Running-state reads are explicitly non-atomic observations.
Never include this tool in the native gameplay runtime.
"""
import argparse
import hashlib
import json
import math
import os
import pathlib
import socket
import struct


class Pine:
    def __init__(self, connection):
        self.connection = connection

    def receive(self, length):
        result = bytearray()
        while len(result) < length:
            data = self.connection.recv(length - len(result))
            if not data:
                raise ValueError('PINE disconnected during reply')
            result.extend(data)
        return bytes(result)

    def request(self, payload):
        if not 1 <= len(payload) <= 4096:
            raise ValueError('PINE request policy limit')
        self.connection.sendall(struct.pack('<I', len(payload) + 4) + payload)
        length = struct.unpack('<I', self.receive(4))[0]
        if not 5 <= length <= 65536:
            raise ValueError('invalid/over-limit PINE reply size')
        response = self.receive(length - 4)
        if response[0] != 0:
            raise ValueError('PINE command failed')
        return response[1:]

    def text(self, opcode):
        response = self.request(bytes([opcode]))
        if len(response) < 5:
            raise ValueError('truncated PINE string reply')
        length = struct.unpack_from('<I', response)[0]
        if length != len(response) - 4 or response[-1] != 0:
            raise ValueError('invalid PINE string length/terminator')
        return response[4:-1].decode('utf-8')

    def status(self):
        response = self.request(b'\x0f')
        if len(response) != 4:
            raise ValueError('invalid PINE status reply')
        return struct.unpack('<I', response)[0]

    def read_ee32(self, addresses):
        if len(addresses) > 512 or any(a < 0 or a + 4 > 32 * 1024 * 1024 or a % 4 for a in addresses):
            raise ValueError('unaligned/out-of-EE-RAM read or batch limit exceeded')
        response = self.request(b''.join(b'\x02' + struct.pack('<I', a) for a in addresses))
        if len(response) != len(addresses) * 4:
            raise ValueError('PINE memory reply length differs')
        return list(struct.unpack('<' + 'I' * len(addresses), response))


def finite_float32(value):
    value = struct.unpack('<f', struct.pack('<I', value))[0]
    return value if math.isfinite(value) else None


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--socket', type=pathlib.Path,
                   default=pathlib.Path(os.environ.get('XDG_RUNTIME_DIR', '/tmp')) / 'pcsx2.sock.28012')
    p.add_argument('--out', type=pathlib.Path, required=True)
    p.add_argument('symbols', nargs='*', default=['AFrame', 'Time', 'SPF', 'vb_elapsed', 'ticks_elapsed'])
    args = p.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('reference evidence must stay in private/')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    elfpath = pathlib.Path('private/disc/SLUS_200.47')
    elf = elfpath.read_bytes()
    if hashlib.sha256(elf).hexdigest() != profile['executable']['sha256']:
        raise ValueError('local ELF identity differs from profile')
    metadata = json.loads(pathlib.Path('private/reports/elf-metadata.json').read_text())
    symbols = {}
    for s in metadata['symbols']:
        symbols.setdefault(s['name'], []).append(s)
    addresses = []
    for name in args.symbols:
        candidates = [s for s in symbols.get(name, []) if s['size'] >= 4 and s['type'] == 1]
        if len(candidates) != 1:
            raise ValueError(f'unknown/ambiguous/non-scalar object symbol: {name}')
        addresses.append(candidates[0]['guest_address'])
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(args.socket))
        pine = Pine(connection)
        context = dict(emulator_version=pine.text(8), title=pine.text(11), serial=pine.text(12),
                       executable_crc=pine.text(13), status_before=pine.status())
        if context['serial'] != 'SLUS-20047':
            raise ValueError('connected reference serial differs')
        # Compare bounded immutable function-entry probes before accepting data.
        # Full image identity comes from the recorded launch, not just these probes.
        probes = []
        for name in ['main', 'ClockOncePerFrame', 'hash_me', 'GetPlayerDMotion']:
            candidates = [s for s in symbols[name] if s['type'] == 2]
            if len(candidates) != 1:
                raise ValueError(f'ambiguous code probe: {name}')
            address = candidates[0]['guest_address']
            segment = next(s for s in profile['load_segments'] if
                           s['virtual_address'] <= address and address + 16 <= s['virtual_address'] + s['file_size'])
            file_offset = segment['file_offset'] + address - segment['virtual_address']
            expected = elf[file_offset:file_offset + 16]
            observed = struct.pack('<4I', *pine.read_ee32([address + i * 4 for i in range(4)]))
            if observed != expected:
                raise ValueError(f'loaded code probe differs: {name}')
            probes.append(dict(symbol=name, ee_address=address, file_offset=file_offset, bytes_hex=observed.hex()))
        values = pine.read_ee32(addresses)
        context['status_after'] = pine.status()
    report = dict(schema_version=1, source_profile=profile['profile_id'], disc_sha256=profile['image']['sha256'],
                  address_space='EE physical RAM', context=context, code_probes=probes,
                  coherence='paused diagnostic' if context['status_before'] == context['status_after'] == 1
                  else 'live non-atomic diagnostic; not a guest-update-aligned reference scenario',
                  samples=[dict(symbol=n, ee_address=a, raw_u32=v, raw_bytes_hex=struct.pack('<I', v).hex(),
                                float32_interpretation=finite_float32(v))
                           for n, a, v in zip(args.symbols, addresses, values, strict=True)])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open('x') as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write('\n')
    print(report['coherence'])


if __name__ == '__main__':
    main()
