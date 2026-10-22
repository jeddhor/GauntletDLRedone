#!/usr/bin/env python3
"""VU micro-mode disassembler (research only).

Decodes 64-bit VU instruction pairs (lower word first, upper word second)
following the opcode tables of the pinned PCSX2 checkout
(pcsx2/pcsx2/DebugTools/DisVUmicro.h, DisVUops.h). Input: a micro memory
image (e.g. a savestate's vu1MicroMem.bin, 16 KiB). Output: one line per
pair with the pair index, the upper and lower instructions and the I/E/M/D/T
bits. Branch targets are pair indices (pc + 1 + imm11); LQ/SQ/ILW/ISW
offsets are quadword (VU data memory) units.
"""
import argparse
import pathlib
import struct


def ft(c):
    return (c >> 16) & 31


def fs(c):
    return (c >> 11) & 31


def fd(c):
    return (c >> 6) & 31


def dest(c):
    return ''.join(ch for ch, b in (('x', 24), ('y', 23), ('z', 22), ('w', 21)) if (c >> b) & 1)


def imm11(c):
    v = c & 0x7ff
    return v - 0x800 if v & 0x400 else v


def imm15(c):
    return ((c >> 10) & 0x7800) | (c & 0x7ff)


def imm5(c):
    v = (c >> 6) & 31
    return v - 32 if v & 16 else v


LANES = 'xyzw'


def vf(i):
    return f'vf{i:02d}'


def vi(i):
    return f'vi{i & 15:02d}'


def upper(c):
    op = c & 0x3f
    d = dest(c)
    bc = 'xyzw'[c & 3]
    if op < 0x30:
        names = {0x00: 'ADD', 0x04: 'SUB', 0x08: 'MADD', 0x0c: 'MSUB', 0x10: 'MAX', 0x14: 'MINI', 0x18: 'MUL'}
        base = op & ~3
        if base in names:
            return f'{names[base]}{bc}.{d} {vf(fd(c))}, {vf(fs(c))}, {vf(ft(c))}{bc}'
        table = {0x1c: ('MULq', 'q'), 0x1d: ('MAXi', 'i'), 0x1e: ('MULi', 'i'), 0x1f: ('MINIi', 'i'),
                 0x20: ('ADDq', 'q'), 0x21: ('MADDq', 'q'), 0x22: ('ADDi', 'i'), 0x23: ('MADDi', 'i'),
                 0x24: ('SUBq', 'q'), 0x25: ('MSUBq', 'q'), 0x26: ('SUBi', 'i'), 0x27: ('MSUBi', 'i')}
        if op in table:
            n, r = table[op]
            return f'{n}.{d} {vf(fd(c))}, {vf(fs(c))}, {r.upper()}'
        three = {0x28: 'ADD', 0x29: 'MADD', 0x2a: 'MUL', 0x2b: 'MAX', 0x2c: 'SUB', 0x2d: 'MSUB', 0x2e: 'OPMSUB', 0x2f: 'MINI'}
        if op in three:
            return f'{three[op]}.{d} {vf(fd(c))}, {vf(fs(c))}, {vf(ft(c))}'
        return f'?upper {c:08x}'
    if op >= 0x3c:
        k = ((op & 3) << 5) | fd(c)  # table FD_xx index by fd
        row = op & 3
        i = fd(c)
        tables = [
            ['ADDAx', 'SUBAx', 'MADDAx', 'MSUBAx', 'ITOF0', 'FTOI0', 'MULAx', 'MULAq', 'ADDAq', 'SUBAq', 'ADDA', 'SUBA'],
            ['ADDAy', 'SUBAy', 'MADDAy', 'MSUBAy', 'ITOF4', 'FTOI4', 'MULAy', 'ABS', 'MADDAq', 'MSUBAq', 'MADDA', 'MSUBA'],
            ['ADDAz', 'SUBAz', 'MADDAz', 'MSUBAz', 'ITOF12', 'FTOI12', 'MULAz', 'MULAi', 'ADDAi', 'SUBAi', 'MULA', 'OPMULA'],
            ['ADDAw', 'SUBAw', 'MADDAw', 'MSUBAw', 'ITOF15', 'FTOI15', 'MULAw', 'CLIP', 'MADDAi', 'MSUBAi', None, 'NOP'],
        ]
        del k
        n = tables[row][i] if i < 12 else None
        if n is None:
            return f'?upperFD {c:08x}'
        if n == 'NOP':
            return 'NOP'
        if n[:4] in ('ITOF', 'FTOI') or n == 'ABS':
            return f'{n}.{d} {vf(ft(c))}, {vf(fs(c))}'
        if n == 'CLIP':
            return f'CLIPw.xyz {vf(fs(c))}, {vf(ft(c))}w'
        if n[-1] in 'xyzw' and n[:-1] in ('ADDA', 'SUBA', 'MADDA', 'MSUBA', 'MULA'):
            return f'{n}.{d} ACC, {vf(fs(c))}, {vf(ft(c))}{n[-1]}'
        if n[-1] in 'qi':
            return f'{n}.{d} ACC, {vf(fs(c))}, {n[-1].upper()}'
        return f'{n}.{d} ACC, {vf(fs(c))}, {vf(ft(c))}'
    return f'?upper {c:08x}'


def lower(c, pc):
    top = c >> 25
    d = dest(c)
    if top == 0x00:
        return f'LQ.{d} {vf(ft(c))}, {imm11(c)}({vi(fs(c))})'
    if top == 0x01:
        return f'SQ.{d} {vf(fs(c))}, {imm11(c)}({vi(ft(c))})'
    if top == 0x04:
        return f'ILW.{d} {vi(ft(c))}, {imm11(c)}({vi(fs(c))})'
    if top == 0x05:
        return f'ISW.{d} {vi(ft(c))}, {imm11(c)}({vi(fs(c))})'
    if top == 0x08:
        return f'IADDIU {vi(ft(c))}, {vi(fs(c))}, {imm15(c)}'
    if top == 0x09:
        return f'ISUBIU {vi(ft(c))}, {vi(fs(c))}, {imm15(c)}'
    flags = {0x10: 'FCEQ', 0x11: 'FCSET', 0x12: 'FCAND', 0x13: 'FCOR', 0x14: 'FSEQ', 0x15: 'FSSET', 0x16: 'FSAND',
             0x17: 'FSOR', 0x18: 'FMEQ', 0x1a: 'FMAND', 0x1b: 'FMOR', 0x1c: 'FCGET'}
    if top in flags:
        return f'{flags[top]} {vi(ft(c))}, {vi(fs(c))}, {c & 0xffffff:#x}'
    br = {0x20: 'B', 0x21: 'BAL', 0x28: 'IBEQ', 0x29: 'IBNE', 0x2c: 'IBLTZ', 0x2d: 'IBGTZ', 0x2e: 'IBLEZ', 0x2f: 'IBGEZ'}
    if top in br:
        t = pc + 1 + imm11(c)
        return f'{br[top]} {vi(ft(c))}, {vi(fs(c))} -> {t}'
    if top == 0x24:
        return f'JR {vi(fs(c))}'
    if top == 0x25:
        return f'JALR {vi(ft(c))}, {vi(fs(c))}'
    if top == 0x40:
        op = c & 0x3f
        if op == 0x30:
            return f'IADD {vi(fd(c))}, {vi(fs(c))}, {vi(ft(c))}'
        if op == 0x31:
            return f'ISUB {vi(fd(c))}, {vi(fs(c))}, {vi(ft(c))}'
        if op == 0x32:
            return f'IADDI {vi(ft(c))}, {vi(fs(c))}, {imm5(c)}'
        if op == 0x34:
            return f'IAND {vi(fd(c))}, {vi(fs(c))}, {vi(ft(c))}'
        if op == 0x35:
            return f'IOR {vi(fd(c))}, {vi(fs(c))}, {vi(ft(c))}'
        if op >= 0x3c:
            row = op & 3
            t3 = [
                {0x0c: 'MOVE', 0x0d: 'LQI', 0x0e: 'DIV', 0x0f: 'MTIR', 0x10: 'RNEXT', 0x19: 'MFP', 0x1a: 'XTOP', 0x1b: 'XGKICK',
                 0x1c: 'ESADD', 0x1d: 'EATANxy', 0x1e: 'ESQRT', 0x1f: 'ESIN'},
                {0x0c: 'MR32', 0x0d: 'SQI', 0x0e: 'SQRT', 0x0f: 'MFIR', 0x10: 'RGET', 0x1a: 'XITOP', 0x1c: 'ERSADD',
                 0x1d: 'EATANxz', 0x1e: 'ERSQRT', 0x1f: 'EATAN'},
                {0x0d: 'LQD', 0x0e: 'RSQRT', 0x0f: 'ILWR', 0x10: 'RINIT', 0x1c: 'ELENG', 0x1d: 'ESUM', 0x1e: 'ERCPR', 0x1f: 'EEXP'},
                {0x0d: 'SQD', 0x0e: 'WAITQ', 0x0f: 'ISWR', 0x10: 'RXOR', 0x1c: 'ERLENG', 0x1e: 'WAITP'},
            ][row]
            n = t3.get(fd(c))
            fsf, ftf = LANES[(c >> 21) & 3], LANES[(c >> 23) & 3]
            if n is None:
                return f'?lowerT3 {c:08x}'
            if n == 'MOVE':
                return 'NOP' if fs(c) == 0 and ft(c) == 0 else f'MOVE.{d} {vf(ft(c))}, {vf(fs(c))}'
            if n in ('MR32',):
                return f'{n}.{d} {vf(ft(c))}, {vf(fs(c))}'
            if n in ('LQI', 'LQD'):
                return f'{n}.{d} {vf(ft(c))}, ({vi(fs(c))}{"++" if n == "LQI" else "--"})'
            if n in ('SQI', 'SQD'):
                return f'{n}.{d} {vf(fs(c))}, ({vi(ft(c))}{"++" if n == "SQI" else "--"})'
            if n in ('DIV', 'RSQRT'):
                return f'{n} Q, {vf(fs(c))}{fsf}, {vf(ft(c))}{ftf}'
            if n == 'SQRT':
                return f'SQRT Q, {vf(ft(c))}{ftf}'
            if n == 'MTIR':
                return f'MTIR {vi(ft(c))}, {vf(fs(c))}{fsf}'
            if n == 'MFIR':
                return f'MFIR.{d} {vf(ft(c))}, {vi(fs(c))}'
            if n in ('ILWR', 'ISWR'):
                return f'{n}.{d} {vi(ft(c))}, ({vi(fs(c))})'
            if n in ('XGKICK',):
                return f'XGKICK {vi(fs(c))}'
            if n in ('XTOP', 'XITOP'):
                return f'{n} {vi(ft(c))}'
            if n == 'MFP':
                return f'MFP.{d} {vf(ft(c))}, P'
            if n in ('WAITQ', 'WAITP'):
                return n
            if n in ('ESADD', 'ERSADD', 'ELENG', 'ERLENG', 'EATANxy', 'EATANxz', 'ESUM'):
                return f'{n} P, {vf(fs(c))}'
            if n in ('ERCPR', 'ESQRT', 'ERSQRT', 'ESIN', 'EATAN', 'EEXP'):
                return f'{n} P, {vf(fs(c))}{fsf}'
            if n in ('RINIT', 'RXOR'):
                return f'{n} R, {vf(fs(c))}{fsf}'
            if n in ('RGET', 'RNEXT'):
                return f'{n}.{d} {vf(ft(c))}, R'
            return n
        return f'?lowerOP {c:08x}'
    return f'?lower {c:08x}'


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('micro', type=pathlib.Path)
    ap.add_argument('--start', type=int, default=0, help='first pair index')
    ap.add_argument('--count', type=int, default=None)
    a = ap.parse_args()
    b = a.micro.read_bytes()
    if len(b) > 64 * 1024 or len(b) % 8:
        raise ValueError('expected a VU micro memory image')
    n = len(b) // 8
    end = n if a.count is None else min(n, a.start + a.count)
    for pc in range(a.start, end):
        lo, up = struct.unpack_from('<II', b, pc * 8)
        bits = ''.join(ch for ch, s in (('I', 31), ('E', 30), ('M', 29), ('D', 28), ('T', 27)) if (up >> s) & 1)
        u = upper(up & 0x07ffffff | (up & 0x00000000))
        if (up >> 31) & 1:
            low = f'LOI {struct.unpack("<f", struct.pack("<I", lo))[0]!r}'
        else:
            low = lower(lo, pc)
        print(f'{pc:4d} {pc * 8:04x}  {u:<40} {low:<40} {bits}')


if __name__ == '__main__':
    main()
