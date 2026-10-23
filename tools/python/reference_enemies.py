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
                 'sfx_ene_bloodidx', 'help_flag', 'help_num', 'help_player', 'help_time', 'help_interval',
                 'help_interval_idx', 'help_lv', 'help_testflag', 'RuneDispTimer', 'DoRockFly', 'DidRockFly']
# Critter globals (name: bytes): CritterETypes[9][6] TYPE pointers, the id
# and frame counters, the target bookkeeping and the boss state.
CRITTER_GLOBALS = {'CritterETypes': 9 * 6 * 4, 'CritterNewID': 4, 'CritterCounter': 4, 'CritterNActivePlayers': 4,
                   'CritterPlayerNTargets': 16, 'BossStartPos': 12, 'BossActivateTime': 4, 'BossCamMode': 4,
                   'EnemyRootNode': 4,
                   # ProcessSpewItems' list: 0x20 × {item pointer, velocity}.
                   'NumSpewItems': 4, 'SpewItems': 0x20 * 0x10}
ENEMY_SIZE = 0x3b0
ENEMY_COUNT = 25


def enemy_probe(memory):
    sym = elf_symbols({'enemies', '_impure_ptr', 'WorldInfo', 'NumItems', 'Effects', 'NumEffects', 'SfxId', 'CritterInsts',
                       'NumCritterInsts', 'BossCritInst'} | set(ENEMY_GLOBALS) | set(CRITTER_GLOBALS))
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
    previous_crit = {}
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
        out['world_anims'] = chunks(read, world['base'] + world['entries'], 16 * world['count']).hex() if world['count'] else ''
        raw_obj = chunks(read, objects, 60 * world['objects'])
        obj_delta = {}
        for k in range(world['objects']):
            r = raw_obj[60 * k:60 * (k + 1)]
            if previous_obj.get(k) != r:
                obj_delta[str(k)] = r.hex()
                previous_obj[k] = r
        out['objects_delta'] = obj_delta
        # CritterInsts (16 × 0xb70, delta-coded) with NumCritterInsts and
        # BossCritInst.
        crit = chunks(read, sym['CritterInsts'], 16 * 0xb70)
        crit_delta = {}
        for k in range(16):
            r = crit[0xb70 * k:0xb70 * (k + 1)]
            if previous_crit.get(k) != r:
                crit_delta[str(k)] = r.hex()
                previous_crit[k] = r
        out['critters_delta'] = crit_delta
        out['NumCritterInsts'] = struct.unpack('<i', read(sym['NumCritterInsts'], 4))[0]
        # Each live instance's +0x70 MB node (ProcessCritter's GetWorldMat
        # source): local matrix +0x00..+0x40 and parent link +0x74.
        nodes = {}
        for k in range(min(max(out['NumCritterInsts'], 0), 16)):
            r = crit[0xb70 * k:0xb70 * (k + 1)]
            ty, node = struct.unpack_from('<I', r, 4)[0], struct.unpack_from('<I', r, 0x70)[0]
            if ty and node and node % 16 == 0 and node + 0x80 <= 32 * 1024 * 1024:
                raw = read(node, 0x80)
                nodes[str(k)] = {'node': raw[:0x40].hex(), 'parent': raw[0x74:0x78].hex()}
                # CritterGetNode's table: +0xc0 → +0xbc entries of 0x28
                # bytes, the tree node's MB node pointer first (the
                # chimera's tree has 176 nodes; its heads share the table).
                count, table = struct.unpack_from('<II', r, 0xbc)
                if 0 < count <= 256 and table and table % 4 == 0 and table + 0x28 * count <= 32 * 1024 * 1024:
                    t = chunks(read, table, 0x28 * count)
                    nodes[str(k)]['tree_nodes'] = b''.join(t[0x28 * n:0x28 * n + 4] for n in range(count)).hex()
        out['critter_nodes'] = nodes
        out['BossCritInst'] = read(sym['BossCritInst'], 4).hex()
        out['critter_globals'] = {k: read(sym[k], n).hex() for k, n in CRITTER_GLOBALS.items()}
        return out

    regions = [(base + o, memory[base + o:base + min(size, o + 2048)]) for o in range(0, size, 2048)]
    return capture, regions
