#!/usr/bin/env python3
"""Per-draw GS state summary of a stock PCSX2 GS dump (research only).

Walks the PATH GIF streams tracking the GS registers that select material
behaviour (PRIM from tags with PRE, TEX0_1, TEX1_1, CLAMP_1, ALPHA_1,
TEST_1, FBA_1, FOGCOL, TEXA, PABE, ZBUF_1) and records, per GIF tag that
emits vertices, the active state plus vertex colour/Q/fog statistics and
the XYZ bounds. Output is a private JSON report; never runtime code.
"""
import argparse
import collections
import compression.zstd
import json
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from inspect_gs_dump import parse  # noqa: E402  (framing and limits)

REG = {0x06: 'TEX0_1', 0x14: 'TEX1_1', 0x08: 'CLAMP_1', 0x42: 'ALPHA_1', 0x47: 'TEST_1', 0x4a: 'FBA_1',
       0x3d: 'FOGCOL', 0x3b: 'TEXA', 0x49: 'PABE', 0x4e: 'ZBUF_1', 0x4c: 'FRAME_1', 0x00: 'PRIM',
       0x1b: 'PRMODE', 0x1a: 'PRMODECONT', 0x3f: 'TEXFLUSH', 0x18: 'XYOFFSET_1', 0x40: 'SCISSOR_1'}


def fields_prim(v):
    return dict(type=v & 7, IIP=(v >> 3) & 1, TME=(v >> 4) & 1, FGE=(v >> 5) & 1, ABE=(v >> 6) & 1,
                AA1=(v >> 7) & 1, FST=(v >> 8) & 1, CTXT=(v >> 9) & 1, FIX=(v >> 10) & 1)


def fields_tex0(v):
    return dict(TBP0=v & 0x3fff, TBW=(v >> 14) & 63, PSM=(v >> 20) & 63, TW=(v >> 26) & 15, TH=(v >> 30) & 15,
                TCC=(v >> 34) & 1, TFX=(v >> 35) & 3, CBP=(v >> 37) & 0x3fff, CPSM=(v >> 51) & 15)


def fields_alpha(v):
    return dict(A=v & 3, B=(v >> 2) & 3, C=(v >> 4) & 3, D=(v >> 6) & 3, FIX=(v >> 32) & 255)


def fields_test(v):
    return dict(ATE=v & 1, ATST=(v >> 1) & 7, AREF=(v >> 4) & 255, AFAIL=(v >> 12) & 3, DATE=(v >> 14) & 1,
                ZTE=(v >> 16) & 1, ZTST=(v >> 17) & 3)


def walk(stream):
    state = {}
    draws = []
    cursor = 0
    while cursor + 16 <= len(stream):
        low, layout = struct.unpack_from('<QQ', stream, cursor)
        cursor += 16
        loops, eop, pre, prim = low & 0x7fff, (low >> 15) & 1, (low >> 46) & 1, (low >> 47) & 0x7ff
        mode = (low >> 58) & 3
        count = (low >> 60) & 15 or 16
        if pre:
            state['PRIM'] = prim
        size = loops * count * 16 if mode == 0 else ((loops * count * 8 + 15) // 16) * 16 if mode == 1 else loops * 16
        if mode != 0:
            if mode == 1:
                regs = [(layout >> (4 * i)) & 15 for i in range(count)]
                verts = sum(1 for _ in range(loops) for r in regs if r in (4, 5, 0xc, 0xd))
                if verts:
                    draws.append(dict(state={k: state.get(k) for k in ('PRIM', 'TEX0_1', 'ALPHA_1', 'TEST_1', 'FBA_1', 'TEX1_1', 'CLAMP_1', 'FOGCOL', 'TEXA', 'PABE')},
                                      vertices=verts, mode='reglist'))
            cursor += size
            continue
        colours, qs, fogs, xyz = [], [], [], []
        verts = 0
        for i in range(loops * count):
            reg = (layout >> ((i % count) * 4)) & 15
            lo, hi = struct.unpack_from('<QQ', stream, cursor + 16 * i)
            if reg == 14:
                name = REG.get(hi & 255)
                if name:
                    state[name] = lo
                elif (hi & 255) == 0:
                    state['PRIM'] = lo
            elif reg == 1:
                r, g, b, a = lo & 255, (lo >> 32) & 255, hi & 255, (hi >> 32) & 255
                colours.append((r, g, b, a))
            elif reg == 2:
                q, = struct.unpack('<f', struct.pack('<I', hi & 0xffffffff))
                qs.append(q)
            elif reg in (4, 5):
                verts += 1
                x, y = lo & 0xffff, (lo >> 32) & 0xffff
                if reg == 4:
                    z = (hi >> 4) & 0xffffff
                    fogs.append((hi >> 36) & 255)
                else:
                    z = hi & 0xffffffff
                xyz.append((x / 16.0, y / 16.0, z))
        cursor += loops * count * 16
        if verts:
            def stat(vals):
                return [min(vals), max(vals)] if vals else None
            draws.append(dict(
                state={k: state.get(k) for k in ('PRIM', 'TEX0_1', 'ALPHA_1', 'TEST_1', 'FBA_1', 'TEX1_1', 'CLAMP_1', 'FOGCOL', 'TEXA', 'PABE')},
                vertices=verts,
                rgba=[stat([c[k] for c in colours]) for k in range(4)] if colours else None,
                fog=stat(fogs), q=stat(qs),
                screen=[stat([v[0] for v in xyz]), stat([v[1] for v in xyz])],
                first_xyz=xyz[:3]))
    return draws


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('dump', type=pathlib.Path)
    ap.add_argument('out', type=pathlib.Path)
    a = ap.parse_args()
    if not a.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or a.out.exists():
        raise ValueError('choose a new private output file')
    raw = compression.zstd.ZstdDecompressor().decompress(a.dump.read_bytes(), max_length=64 * 1024 * 1024 + 1)
    parse(raw)  # validates the dump framing and identity
    # Re-read the concatenated streams (parse keeps only summaries).
    size, = struct.unpack_from('<I', raw, 4)
    state_size, = struct.unpack_from('<I', raw, 12)
    cursor = 8 + size + state_size + 8192
    streams = collections.defaultdict(bytearray)
    while cursor < len(raw):
        kind = raw[cursor]
        cursor += 1
        if kind == 0:
            path = raw[cursor]
            n, = struct.unpack_from('<I', raw, cursor + 1)
            cursor += 5
            streams[path].extend(raw[cursor:cursor + n])
            cursor += n
        elif kind == 1:
            cursor += 1
        elif kind == 2:
            cursor += 4
        elif kind == 3:
            cursor += 8192
    report = {}
    for path, s in streams.items():
        draws = walk(bytes(s))
        groups = collections.Counter()
        for d in draws:
            st = d['state']
            prim = fields_prim(st['PRIM'] or 0)
            key = json.dumps(dict(prim={k: prim[k] for k in ('type', 'TME', 'FGE', 'ABE', 'IIP')},
                                  alpha=fields_alpha(st['ALPHA_1'] or 0) if prim['ABE'] else None,
                                  tex=({k: fields_tex0(st['TEX0_1'] or 0)[k] for k in ('PSM', 'TCC', 'TFX')} if prim['TME'] else None),
                                  test=fields_test(st['TEST_1'] or 0)), sort_keys=True)
            groups[key] += d['vertices']
        report[str(path)] = dict(draws=len(draws), groups=[dict(state=json.loads(k), vertices=v) for k, v in groups.most_common()],
                                 sample=draws[:40], fogcol=[d['state']['FOGCOL'] for d in draws[:1]])
    with a.out.open('x') as f:
        json.dump(report, f, indent=1)
    for path, r in report.items():
        print('path', path, 'draws', r['draws'])
        for g in r['groups'][:25]:
            print(g['vertices'], g['state'])


if __name__ == '__main__':
    main()
