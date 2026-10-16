#!/usr/bin/env python3
"""Bounded R5900 disassembler for jump-table targets Ghidra leaves unrecovered.

Decodes a common MIPS-I/II/III integer and COP1 single subset; anything else
prints as `.word` and must be checked in Ghidra, never guessed. Reads the
user's SLUS_200.47 from private/disc by guest address.
"""
import argparse
import pathlib
import struct

ELF = pathlib.Path('private/disc/SLUS_200.47')
R = ['zero', 'at', 'v0', 'v1', 'a0', 'a1', 'a2', 'a3', 't0', 't1', 't2', 't3', 't4', 't5', 't6', 't7',
     's0', 's1', 's2', 's3', 's4', 's5', 's6', 's7', 't8', 't9', 'k0', 'k1', 'gp', 'sp', 's8', 'ra']


def segments(b):
    phoff, = struct.unpack_from('<I', b, 0x1c)
    n, = struct.unpack_from('<H', b, 0x2c)
    return [struct.unpack_from('<8I', b, phoff + 32 * i) for i in range(n)]


def read(b, segs, addr, size):
    for t, off, vaddr, _, filesz, *_ in segs:
        if t == 1 and vaddr <= addr and addr + size <= vaddr + filesz:
            return b[off + addr - vaddr: off + addr - vaddr + size]
    raise ValueError(f'{addr:#x} outside loaded segments')


def decode(w, pc):
    op, rs, rt, rd, sa, fn = w >> 26, (w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31, (w >> 6) & 31, w & 63
    imm = w & 0xffff
    simm = imm - 0x10000 if imm & 0x8000 else imm
    br = pc + 4 + (simm << 2)
    if w == 0:
        return 'nop'
    if op == 0:
        names = {0x21: 'addu', 0x23: 'subu', 0x24: 'and', 0x25: 'or', 0x26: 'xor', 0x27: 'nor', 0x2a: 'slt',
                 0x2b: 'sltu', 0x2d: 'daddu', 0x2f: 'dsubu', 0x0a: 'movz', 0x0b: 'movn'}
        if fn in names:
            return f'{names[fn]} {R[rd]},{R[rs]},{R[rt]}'
        if fn in (0, 2, 3):
            return f'{["sll", "", "srl", "sra"][fn]} {R[rd]},{R[rt]},{sa:#x}'
        if fn == 8:
            return f'jr {R[rs]}'
        if fn == 9:
            return f'jalr {R[rd]},{R[rs]}'
        if fn == 0x18:
            return f'mult {R[rd]},{R[rs]},{R[rt]}'
    if op in (2, 3):
        return f'{["j", "jal"][op - 2]} {(pc & 0xf0000000) | ((w & 0x3ffffff) << 2):#010x}'
    if op in (4, 5, 0x14, 0x15):
        n = {4: 'beq', 5: 'bne', 0x14: 'beql', 0x15: 'bnel'}[op]
        return f'{n} {R[rs]},{R[rt]},{br:#010x}'
    if op in (6, 7, 0x16, 0x17):
        n = {6: 'blez', 7: 'bgtz', 0x16: 'blezl', 0x17: 'bgtzl'}[op]
        return f'{n} {R[rs]},{br:#010x}'
    if op == 1:
        n = {0: 'bltz', 1: 'bgez', 2: 'bltzl', 3: 'bgezl'}.get(rt)
        if n:
            return f'{n} {R[rs]},{br:#010x}'
    imms = {8: 'addi', 9: 'addiu', 0xa: 'slti', 0xb: 'sltiu', 0xc: 'andi', 0xd: 'ori', 0xe: 'xori', 0x19: 'daddiu'}
    if op in imms:
        v = imm if op in (0xc, 0xd, 0xe) else simm
        return f'{imms[op]} {R[rt]},{R[rs]},{v:#x}'
    if op == 0xf:
        return f'lui {R[rt]},{imm:#x}'
    mem = {0x20: 'lb', 0x21: 'lh', 0x23: 'lw', 0x24: 'lbu', 0x25: 'lhu', 0x28: 'sb', 0x29: 'sh', 0x2b: 'sw',
           0x37: 'ld', 0x3f: 'sd', 0x1e: 'lq', 0x1f: 'sq', 0x31: 'lwc1', 0x39: 'swc1'}
    if op in mem:
        t = f'f{rt}' if op in (0x31, 0x39) else R[rt]
        return f'{mem[op]} {t},{simm:#x}({R[rs]})'
    if op == 0x11:
        if rs == 0:
            return f'mfc1 {R[rt]},f{rd}'
        if rs == 4:
            return f'mtc1 {R[rt]},f{rd}'
        if rs == 8:
            n = {0: 'bc1f', 1: 'bc1t', 2: 'bc1fl', 3: 'bc1tl'}.get(rt)
            return f'{n} {br:#010x}'
        if rs == 0x10:
            fs, ft, fd = rd, rt, sa
            ops = {0: 'add.S', 1: 'sub.S', 2: 'mul.S', 3: 'div.S', 0x28: 'max.S', 0x29: 'min.S'}
            if fn in ops:
                return f'{ops[fn]} f{fd},f{fs},f{ft}'
            one = {4: 'sqrt.S', 5: 'abs.S', 6: 'mov.S', 7: 'neg.S', 0x24: 'cvt.w.S'}
            if fn in one:
                return f'{one[fn]} f{fd},f{fs}' if fn != 4 else f'sqrt.S f{fd},f{ft}'
            cmp = {0x32: 'c.eq.S', 0x34: 'c.lt.S', 0x36: 'c.le.S'}
            if fn in cmp:
                return f'{cmp[fn]} f{fs},f{ft}'
        if rs == 0x14 and fn == 0x20:
            return f'cvt.s.W f{sa},f{rd}'
    return f'.word {w:#010x}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('start', type=lambda s: int(s, 0))
    parser.add_argument('end', type=lambda s: int(s, 0))
    parser.add_argument('--table', type=lambda s: int(s, 0), help='also print N words of a jump table at START')
    args = parser.parse_args()
    if not 0 < args.end - args.start <= 0x4000:
        raise SystemExit('range policy: 1..16 KiB')
    b = ELF.read_bytes()
    segs = segments(b)
    if args.table:
        words = struct.unpack(f'<{args.table}I', read(b, segs, args.start, 4 * args.table))
        for i, w in enumerate(words):
            print(f'[{i}] {w:#010x}')
        return
    data = read(b, segs, args.start, args.end - args.start)
    for i in range(0, len(data), 4):
        w, = struct.unpack_from('<I', data, i)
        print(f'{args.start + i:08x}\t{decode(w, args.start + i)}')


if __name__ == '__main__':
    main()
