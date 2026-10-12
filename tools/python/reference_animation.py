#!/usr/bin/env python3
"""Fixture-qualified animation cache observations, never a runtime dependency.

Derive heap locations from the frozen source-bound tree and original cache tags.
The player AnimInfo candidate is observational until its actor caller is bound.
"""
import hashlib
import json
import pathlib
import struct

from reference_snapshot import RAM_SIZE


def cache_probe(memory, profile):
    metadata = json.loads(pathlib.Path('private/reports/sor-animation-metadata-v1.json').read_text())
    data = pathlib.Path('private/wad/000034-9a06cdbb.dat').read_bytes()
    if hashlib.sha256(data).hexdigest() != metadata['source_sha256']:
        raise ValueError('animation source/metadata identities differ')
    tree = metadata['animations']['trees'][0]
    anim_address = profile['symbols']['players']['guest_address'] + 0x94
    sequence_pointer, header_pointer = struct.unpack_from('<II', memory, anim_address)
    sequence_file = tree['file_offset'] + struct.unpack_from('<I', bytes(tree['raw_header']))[0]
    header_file = tree['file_offset'] + struct.unpack_from('<I', bytes(tree['raw_header']), 4)[0]
    loaded_base = sequence_pointer - sequence_file
    if loaded_base < 0 or header_pointer != loaded_base + header_file or loaded_base + len(data) > RAM_SIZE:
        raise ValueError('frozen animation pointer pair differs from source layout')
    # Clip record zero contains no relocation fields in its first 44 bytes.
    if memory[sequence_pointer:sequence_pointer+44] != data[sequence_file:sequence_file+44]:
        raise ValueError('loaded animation clip record differs')
    array = profile['reload_barrier_arrays'][0]
    pointer, = struct.unpack_from('<I', memory, array['pointer_guest_address'])
    count, = struct.unpack_from('<I', memory, profile['symbols'][array['count_symbol']]['guest_address'])
    stride = array['stride_bytes']
    if stride != 160 or count > array['max_capacity'] or pointer+count*stride > RAM_SIZE:
        raise ValueError('unsupported animation cache array')
    records = []
    for node in tree['nodes']:
        if node['node_type'] != 1:
            continue
        tag = header_pointer + node['sequence_info_offset']
        matches = [pointer+i*stride for i in range(count) if struct.unpack_from('<I', memory, pointer+i*stride)[0] == tag]
        if len(matches) != 1:
            raise ValueError('frozen animation cache tag is missing or ambiguous')
        records.append(dict(node_index=node['index'], node_label=node['label'], ee_address=matches[0], tag=tag))

    def capture(read):
        info = read(anim_address, 64)
        if struct.unpack_from('<II', info) != (sequence_pointer, header_pointer):
            raise ValueError('player animation pointer binding changed')
        result = []
        for record in records:
            raw = read(record['ee_address'], stride)
            if struct.unpack_from('<I', raw)[0] != record['tag']:
                raise ValueError('animation cache binding changed')
            result.append(dict(**record, raw_hex=raw.hex()))
        return dict(animinfo_ee_address=anim_address, animinfo_raw_hex=info.hex(), caches=result,
                    source_sha256=metadata['source_sha256'],
                    scope='fixture-bound SOR cache and candidate player AnimInfo; output pose/matrices not identified')
    return capture
