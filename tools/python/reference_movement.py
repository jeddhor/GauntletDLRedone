#!/usr/bin/env python3
"""Fixture-qualified PlayerMotion displacement observations, never a runtime dependency.

Retains the entire slot-0 player record, all four PlayerControl records, the
MboxWinPtr camera block and source-consumed globals as raw bytes. Field meanings
are limited to the instruction consumers recorded in movement_layout.evidence.
"""
import struct

from reference_snapshot import RAM_SIZE


def movement_probe(memory, profile):
    layout = profile['movement_layout']
    symbols = profile['symbols']
    player = symbols['players']['guest_address']
    record = layout['player_record_capture_bytes']
    if record != profile['player_layout']['stride_bytes']:
        raise ValueError('movement capture must retain one complete player record')
    control = symbols['PlayerControl']
    camera, = struct.unpack_from('<I', memory, symbols[layout['camera_pointer_symbol']]['guest_address'])
    camera_bytes = layout['camera_capture_bytes']
    if not camera or camera % 16 or camera+camera_bytes > RAM_SIZE:
        raise ValueError('invalid frozen camera pointer')
    globals_ = {name: symbols[name]['guest_address'] for name in layout['global_symbols']}

    def chunks(read, address, size):
        return b''.join(read(address+o, min(2048, size-o)) for o in range(0, size, 2048))

    def capture(read):
        pointer, = struct.unpack('<I', read(symbols[layout['camera_pointer_symbol']]['guest_address'], 4))
        if pointer != camera:
            raise ValueError('camera pointer binding changed')
        return dict(player_ee_address=player, player_raw_hex=chunks(read, player, record).hex(),
                    control_ee_address=control['guest_address'],
                    control_raw_hex=read(control['guest_address'], control['size']).hex(),
                    camera_ee_address=camera, camera_raw_hex=read(camera, camera_bytes).hex(),
                    globals={name: read(address, 4).hex() for name, address in globals_.items()},
                    scope='raw slot-0 player record, controls, camera block and PlayerMotion globals')

    # The reload barrier reads at most 2048 bytes per request.
    regions = [(player+o, memory[player+o:player+min(record, o+2048)]) for o in range(0, record, 2048)]
    regions += [(control['guest_address'], memory[control['guest_address']:control['guest_address']+control['size']]),
               (camera, memory[camera:camera+camera_bytes])]
    regions.extend((address, memory[address:address+4]) for address in globals_.values())
    return capture, regions
