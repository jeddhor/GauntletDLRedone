#!/usr/bin/env python3
"""Fixture-qualified enemy observations for reference_trace.py --enemies.

Retains, per vblank step, the complete enemies array (25 × 0x3b0 bytes at
the `enemies` symbol), newlib's rand state (_impure_ptr → +0x58; main
reseeds it with srand(pbGetCPUTime() >> 3) every loop) and the globals the
native do_enemies reads beyond reference_player.py's (with the Items and
WorldBase pointers that map the generator +0x2a4 and floor +0x2ac links, the
WorldInfo block and the generator item records themselves), as raw bytes.
Addresses come from the executable's own symbol table. Research only.
"""
import struct

from reference_player import elf_symbols

ENEMY_GLOBALS = ['BossIndex', 'SuicideBomberIndex', 'CurWorld', 'level_data', 'Items', 'WorldBase', 'frame_main']
ENEMY_SIZE = 0x3b0
ENEMY_COUNT = 25


def enemy_probe(memory):
    sym = elf_symbols({'enemies', '_impure_ptr', 'WorldInfo'} | set(ENEMY_GLOBALS))
    base, impure = sym['enemies'], sym['_impure_ptr']
    reent, = struct.unpack_from('<I', memory, impure)
    if not 0x100000 <= reent < 0x2000000 or reent % 4:
        raise ValueError('_impure_ptr outside main memory')
    size = ENEMY_SIZE * ENEMY_COUNT

    def chunks(read, address, n):
        return b''.join(read(address + o, min(2048, n - o)) for o in range(0, n, 2048))

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
        gens = {}
        for k in range(ENEMY_COUNT):
            g, = struct.unpack_from('<I', raw, k * ENEMY_SIZE + 0x2a4)
            if g and items <= g < items + 0x110 * 4096 and (g - items) % 0x110 == 0:
                gens[str((g - items) // 0x110)] = read(g, 0x110).hex()
        out['generator_items'] = gens
        return out

    regions = [(base + o, memory[base + o:base + min(size, o + 2048)]) for o in range(0, size, 2048)]
    return capture, regions
