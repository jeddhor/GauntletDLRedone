#!/usr/bin/env python3
"""Fixture-qualified animation cache observations, never a runtime dependency.

Derive heap locations from the frozen source-bound tree and original cache tags.
DoPlayerAction and DoAnimateTree bind the player AnimInfo and original node graph.
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
    layout = profile['animation_layout']
    player = profile['symbols']['players']['guest_address']
    anim_address = player + layout['animinfo_player_offset']
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

    # Runtime nodes are linked; source node order is established by sequence tags,
    # then independently checked against the original parent graph and positions.
    root, = struct.unpack_from('<I', memory, player+layout['tree_instance_player_offset'])
    fields = layout['node_fields']
    node_by_tag = {header_pointer+n['sequence_info_offset']: n for n in tree['nodes'] if n['node_type'] == 1}
    record_by_index = {r['node_index']: r for r in records}
    stack, seen, poses = [(root, -1)], set(), []
    while stack:
        address, parent = stack.pop()
        if not address or address % 4 or address+layout['runtime_node_size'] > RAM_SIZE or address in seen:
            raise ValueError('invalid runtime animation node graph')
        if len(seen) >= 16384:
            raise ValueError('animation node graph policy exceeded')
        seen.add(address)
        word = lambda name: struct.unpack_from('<I', memory, address+fields[name])[0]
        node = node_by_tag.get(word('sequence_table_pointer'))
        if not node or node['parent'] != parent or word('node_type') != 1:
            raise ValueError('runtime/source node hierarchy differs')
        if word('cache_pointer') != record_by_index[node['index']]['ee_address']:
            raise ValueError('node/cache binding differs')
        initial = memory[address+fields['initial_position']:address+fields['initial_position']+12]
        if initial != struct.pack('<3f', *node['initial_position']):
            raise ValueError('runtime initial translation differs from main animation tree')
        matrix = word('matrix_pointer')
        if matrix % 4 or not matrix or matrix+layout['local_capture_bytes'] > RAM_SIZE:
            raise ValueError('invalid node matrix pointer')
        poses.append(dict(node_index=node['index'], node_label=node['label'], runtime_node_ee_address=address,
                          matrix_ee_address=matrix, initial_position_raw_hex=initial.hex()))
        if word('sibling_pointer'):
            stack.append((word('sibling_pointer'), parent))
        if word('child_pointer'):
            stack.append((word('child_pointer'), node['index']))
    if len(poses) != len(records):
        raise ValueError('runtime node graph does not cover source cache set')

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
        local = []
        for pose in poses:
            pointer, = struct.unpack('<I', read(pose['runtime_node_ee_address']+fields['matrix_pointer'], 4))
            if pointer != pose['matrix_ee_address']:
                raise ValueError('node matrix binding changed')
            local.append(dict(**pose, local_raw_hex=read(pointer, layout['local_capture_bytes']).hex()))
        return dict(animinfo_ee_address=anim_address, animinfo_raw_hex=info.hex(), caches=result, poses=local,
                    clock_globals={name: read(profile['symbols'][name]['guest_address'],4).hex() for name in layout['global_symbols']},
                    source_sha256=metadata['source_sha256'],
                    scope='fixture-bound SOR caches, player AnimInfo and local node matrices; world/render transforms excluded')
    regions = [(anim_address, memory[anim_address:anim_address+64])]
    regions.extend((profile['symbols'][name]['guest_address'], memory[profile['symbols'][name]['guest_address']:profile['symbols'][name]['guest_address']+4]) for name in layout['global_symbols'])
    for pose in poses:
        address = pose['runtime_node_ee_address']
        regions.append((address, memory[address:address+layout['runtime_node_size']]))
        address = pose['matrix_ee_address']
        regions.append((address, memory[address:address+layout['local_capture_bytes']]))
    return capture, regions
