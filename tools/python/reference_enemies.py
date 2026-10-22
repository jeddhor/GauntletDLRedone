#!/usr/bin/env python3
"""Fixture-qualified enemy observations for reference_trace.py --enemies.

Retains, per vblank step, the complete enemies array (25 × 0x3b0 bytes at
the `enemies` symbol), newlib's rand state (_impure_ptr → +0x58; main
reseeds it with srand(pbGetCPUTime() >> 3) every loop) and the globals the
native do_enemies reads beyond reference_player.py's (with the Items and
WorldBase pointers that map the generator +0x2a4 and floor +0x2ac links, the
WorldInfo block and the generator item records themselves), as raw bytes.
Also the whole Items array (NumItems records of 0x110 bytes), delta-coded:
the first capture holds every record, later ones only the slots whose bytes
changed since the previous capture (do_items flags such as +0xd8 0x4000
"visible" change per frame). Addresses come from the executable's own
symbol table. Research only.
"""
import struct

from reference_player import elf_symbols

ENEMY_GLOBALS = ['BossIndex', 'SuicideBomberIndex', 'CurWorld', 'level_data', 'Items', 'WorldBase', 'frame_main',
                 'sfx_ene_bloodidx']
ENEMY_SIZE = 0x3b0
ENEMY_COUNT = 25


def enemy_probe(memory):
    sym = elf_symbols({'enemies', '_impure_ptr', 'WorldInfo', 'NumItems', 'Effects', 'NumEffects', 'SfxId'} | set(ENEMY_GLOBALS))
    base, impure = sym['enemies'], sym['_impure_ptr']
    reent, = struct.unpack_from('<I', memory, impure)
    if not 0x100000 <= reent < 0x2000000 or reent % 4:
        raise ValueError('_impure_ptr outside main memory')
    size = ENEMY_SIZE * ENEMY_COUNT

    def chunks(read, address, n):
        return b''.join(read(address + o, min(2048, n - o)) for o in range(0, n, 2048))

    seen = set()
    previous = {}
    previous_fx = {}
    previous_obj = {}
    world = {}

    def capture(read):
        pointer, = struct.unpack('<I', read(impure, 4))
        if pointer != reent:
            raise ValueError('_impure_ptr binding changed')
        raw = chunks(read, base, size)
        out = {'enemies': raw.hex(), 'rand_next': read(reent + 0x58, 4).hex()}
        out.update({g: read(sym[g], 4).hex() for g in ENEMY_GLOBALS})
        # WorldInfo (+0x04 objects, +0x68 item definitions) and every item
        # record an enemy's generator link (+0x2a4) points to, by slot.
        out['WorldInfo'] = read(sym['WorldInfo'], 0xa4).hex()
        items, = struct.unpack('<I', read(sym['Items'], 4))
        # Slots an enemy links to (+0x2a4 generator, +0x2a0 blocking item),
        # kept for the rest of the trace once seen (a destroyed generator is
        # no longer linked but its record still changes).
        for k in range(ENEMY_COUNT):
            for o in (0x2a4, 0x2a0):
                g, = struct.unpack_from('<I', raw, k * ENEMY_SIZE + o)
                if g and items <= g < items + 0x110 * 4096 and (g - items) % 0x110 == 0:
                    seen.add((g - items) // 0x110)
        out['generator_items'] = {str(slot): read(items + 0x110 * slot, 0x110).hex() for slot in sorted(seen)}
        count, = struct.unpack('<i', read(sym['NumItems'], 4))
        if not 0 <= count <= 4096:
            raise ValueError('NumItems out of range')
        array = chunks(read, items, 0x110 * count)
        delta = {}
        for slot in range(count):
            r = array[0x110 * slot:0x110 * (slot + 1)]
            if previous.get(slot) != r:
                delta[str(slot)] = r.hex()
                previous[slot] = r
        out['NumItems'] = count
        out['items_delta'] = delta
        fx = chunks(read, sym['Effects'], 64 * 0xf0)
        fx_delta, nodes = {}, {}
        for slot in range(64):
            r = fx[0xf0 * slot:0xf0 * (slot + 1)]
            if previous_fx.get(slot) != r:
                fx_delta[str(slot)] = r.hex()
                previous_fx[slot] = r
            node, tree = struct.unpack_from('<II', r, 0x14)
            if 0x100000 <= node < 0x2000000:
                nodes[str(slot)] = {'node': read(node, 0x80).hex(), 'tree': read(tree, 0x80).hex() if 0x100000 <= tree < 0x2000000 else None}
        out['NumEffects'] = struct.unpack('<i', read(sym['NumEffects'], 4))[0]
        out['SfxId'] = struct.unpack('<i', read(sym['SfxId'], 4))[0]
        out['effects_delta'] = fx_delta
        out['effect_nodes'] = nodes
        objects, = struct.unpack('<I', read(sym['WorldBase'], 4))
        if not world:
            for off in range(0x10, 0x400, 4):
                b = objects - off
                head = read(b, 0x70)
                if struct.unpack_from('<I', head, 4)[0] == off and struct.unpack_from('<I', head, 0x60)[0] >> 4 == 0xf00bab0:
                    world.update(base=b, count=struct.unpack_from('<I', head, 0x68)[0], entries=struct.unpack_from('<I', head, 0x6c)[0],
                                 objects=struct.unpack_from('<I', head, 0)[0])
                    break
            else:
                raise ValueError('WORLDS base not found below WorldBase')
            if not (0 <= world['count'] <= 1024 and 0 < world['objects'] <= 8192):
                raise ValueError('WORLDS header out of range')
        out['world_anims'] = read(world['base'] + world['entries'], 16 * world['count']).hex() if world['count'] else ''
        raw_obj = chunks(read, objects, 60 * world['objects'])
        obj_delta = {}
        for k in range(world['objects']):
            r = raw_obj[60 * k:60 * (k + 1)]
            if previous_obj.get(k) != r:
                obj_delta[str(k)] = r.hex()
                previous_obj[k] = r
        out['objects_delta'] = obj_delta
        return out

    regions = [(base + o, memory[base + o:base + min(size, o + 2048)]) for o in range(0, size, 2048)]
    return capture, regions
