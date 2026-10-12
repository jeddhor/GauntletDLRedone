#!/usr/bin/env python3
"""Inspect a frozen retail EE state or paused PINE, with source identity probes.

Snapshots contain private retail bytes. This is research tooling, never a native
runtime dependency. A paused observation is not a guest-update boundary.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import struct
import zipfile

from pine_probe import Pine, configured_socket, finite_float32

RAM_SIZE = 32 * 1024 * 1024


def fixture_regions(memory, profile):
    """Heap barriers: paused PINE status alone does not mean state loading ended."""
    result = []
    for array in profile.get('reload_barrier_arrays', []):
        pointer_address = array['pointer_guest_address']
        pointer, = struct.unpack_from('<I', memory, pointer_address)
        count_address = profile['symbols'][array['count_symbol']]['guest_address']
        capacity_address = profile['symbols'][array['capacity_symbol']]['guest_address']
        count, = struct.unpack_from('<I', memory, count_address)
        capacity, = struct.unpack_from('<I', memory, capacity_address)
        size = count * array['stride_bytes']
        if count > capacity or capacity > array['max_capacity'] or pointer % 4 or pointer + size > RAM_SIZE:
            raise ValueError('invalid fixture heap array')
        for address, n in [(pointer_address, 4), (count_address, 4), (capacity_address, 4), (pointer, size)]:
            for offset in range(0, n, 2048):
                length = min(2048, n-offset)
                result.append((address+offset, memory[address+offset:address+offset+length]))
    return result


def describe(read, profile):
    symbols = profile['symbols']
    result = {'scalars': {}, 'strings': {}, 'players': []}
    for name in ('AFrame', 'Time', 'SPF', 'vb_counter', 'vb_elapsed', 'ticks_elapsed', 'game_mode', 'CurLevel'):
        address = symbols[name]['guest_address']
        raw = read(address, 4)
        word, = struct.unpack('<I', raw)
        result['scalars'][name] = dict(ee_address=address, raw_hex=raw.hex(), raw_u32=word,
                                       float32=finite_float32(word))
    for name in ('WorldDir', 'wad_last_filename'):
        symbol = symbols[name]
        raw = read(symbol['guest_address'], symbol['size'])
        result['strings'][name] = dict(ee_address=symbol['guest_address'], raw_hex=raw.hex(),
                                       value=raw.split(b'\0', 1)[0].decode('ascii', errors='backslashreplace'))
    control = symbols['PlayerControl']
    result['PlayerControl'] = dict(ee_address=control['guest_address'],
                                   raw_hex=read(control['guest_address'], control['size']).hex())
    layout = profile['player_layout']
    base = symbols[layout['base_symbol']]['guest_address']
    for slot in range(layout['count']):
        player = {'slot': slot, 'activity': 'unknown', 'fields': {}}
        for name, field in layout['fields'].items():
            address = base + slot * layout['stride_bytes'] + field['offset_bytes']
            count = 3 if field['type'] == 'float32x3' else 1
            raw = read(address, count * 4)
            words = struct.unpack('<' + 'I' * count, raw)
            values = list(struct.unpack('<' + 'i' * count, raw)) if field['type'] == 'int32' else [finite_float32(w) for w in words]
            player['fields'][name] = dict(ee_address=address, raw_hex=raw.hex(), values=values)
        result['players'].append(player)
    return result


def frozen_memory(path):
    if path.stat().st_size > 128 * 1024 * 1024:
        raise ValueError('state exceeds compressed input policy')
    with zipfile.ZipFile(path) as state:
        matches = [i for i in state.infolist() if i.filename == 'eeMemory.bin']
        if len(matches) != 1 or matches[0].file_size != RAM_SIZE:
            raise ValueError('state does not contain exactly one 32 MiB EE RAM image')
        return state.read(matches[0])  # CRC validated by zipfile; no path extraction


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=pathlib.Path)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('reference output must remain private')
    source = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    elf = pathlib.Path('private/disc/SLUS_200.47').read_bytes()
    if hashlib.sha256(elf).hexdigest() != source['executable']['sha256']:
        raise ValueError('ELF differs from supported source profile')
    connection = None
    try:
        if args.state:
            memory = frozen_memory(args.state)
            def read(address, size):
                if address < 0 or address + size > RAM_SIZE:
                    raise ValueError('out-of-RAM snapshot read')
                return memory[address:address + size]
            context = dict(state_sha256=hashlib.sha256(args.state.read_bytes()).hexdigest(),
                           ee_ram_sha256=hashlib.sha256(memory).hexdigest(), mode='frozen EE state')
        else:
            connection = socket.socket(socket.AF_UNIX)
            connection.settimeout(3)
            connection.connect(str(configured_socket()))
            pine = Pine(connection)
            context = dict(serial=pine.text(12), crc=pine.text(13), status_before=pine.status(),
                           mode='paused PINE diagnostic; not guest-update aligned')
            if context['serial'] != 'SLUS-20047' or context['status_before'] != 1:
                raise ValueError('expected supported serial and paused reference')
            def read(address, size):
                if address % 4 or size % 4 or size > 2048:
                    raise ValueError('unaligned or over-limit snapshot read')
                return struct.pack('<' + 'I' * (size // 4),
                                   *pine.read_ee32(list(range(address, address + size, 4))))
        probes = []
        for name in ('main', 'ClockOncePerFrame', 'GetPlayerDMotion', 'GetWadInfo'):
            address = profile['symbols'][name]['guest_address']
            segment = next(s for s in source['load_segments'] if
                           s['virtual_address'] <= address and address + 16 <= s['virtual_address'] + s['file_size'])
            offset = address - segment['virtual_address'] + segment['file_offset']
            raw = read(address, 16)
            if raw != elf[offset:offset + 16]:
                raise ValueError(f'EE image code probe differs: {name}')
            probes.append(dict(symbol=name, ee_address=address, raw_hex=raw.hex()))
        report = dict(schema_version=1, source_profile=source['profile_id'], context=context,
                      code_probes=probes, observation=describe(read, profile))
        if not args.state:
            context['status_after'] = pine.status()
            if context['status_after'] != 1:
                raise ValueError('reference resumed during observation; snapshot rejected')
        args.out.parent.mkdir(parents=True, exist_ok=True)
        with args.out.open('x') as output:
            json.dump(report, output, indent=2, allow_nan=False)
            output.write('\n')
        print(context['mode'])
    finally:
        if connection:
            connection.close()


if __name__ == '__main__':
    main()
