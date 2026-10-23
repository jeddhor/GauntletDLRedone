#!/usr/bin/env python3
"""GS local memory of a stock PCSX2 GS dump (research only, never runtime code).

The dump's state blob is GSState::Freeze (STATE_VERSION 9): 425 bytes of
registers and transfer state, then the 4 MiB local memory (offset found in
the data: block 0x2300's per-frame CLUT words 0x00ffffff, 0x01ffffff sit at
0x1a9 + 0x230000; textures re-uploaded in the frame equal what memory held). Reads PSMCT32 and
PSMT8 pixels through the GS block/column swizzles (block and column tables as
in PCSX2's GSTables.cpp), so textures and CLUTs that are resident before the
captured frame — not uploaded in it — can be compared with source records.

  gs_vram.py DUMP.gs.zst NEW_REPORT.json
      every PSMT8 draw in the frame: its texture (TBP0, TBW, TW × TH) read
      from memory and matched to TEXDEF records of formats 48/66/130 (index
      bytes), and its CLUT (CBP, CPSM 0 = PSMCT32, CSM1) summarised.
"""
import argparse
import collections
import compression.zstd
import glob
import hashlib
import json
import os
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from gs_draws import fields_prim, fields_tex0, walk  # noqa: E402
from inspect_gs_dump import parse  # noqa: E402

VRAM_OFFSET = 425
VRAM_SIZE = 4 * 1024 * 1024
BLOCK32 = [[0, 1, 4, 5, 16, 17, 20, 21], [2, 3, 6, 7, 18, 19, 22, 23],
           [8, 9, 12, 13, 24, 25, 28, 29], [10, 11, 14, 15, 26, 27, 30, 31]]
BLOCK8 = BLOCK32
COLUMN32 = [[0, 1, 4, 5, 8, 9, 12, 13], [2, 3, 6, 7, 10, 11, 14, 15], [16, 17, 20, 21, 24, 25, 28, 29],
            [18, 19, 22, 23, 26, 27, 30, 31], [32, 33, 36, 37, 40, 41, 44, 45], [34, 35, 38, 39, 42, 43, 46, 47],
            [48, 49, 52, 53, 56, 57, 60, 61], [50, 51, 54, 55, 58, 59, 62, 63]]
COLUMN8 = [
    [0, 4, 16, 20, 32, 36, 48, 52, 2, 6, 18, 22, 34, 38, 50, 54],
    [8, 12, 24, 28, 40, 44, 56, 60, 10, 14, 26, 30, 42, 46, 58, 62],
    [33, 37, 49, 53, 1, 5, 17, 21, 35, 39, 51, 55, 3, 7, 19, 23],
    [41, 45, 57, 61, 9, 13, 25, 29, 43, 47, 59, 63, 11, 15, 27, 31],
    [96, 100, 112, 116, 64, 68, 80, 84, 98, 102, 114, 118, 66, 70, 82, 86],
    [104, 108, 120, 124, 72, 76, 88, 92, 106, 110, 122, 126, 74, 78, 90, 94],
    [65, 69, 81, 85, 97, 101, 113, 117, 67, 71, 83, 87, 99, 103, 115, 119],
    [73, 77, 89, 93, 105, 109, 121, 125, 75, 79, 91, 95, 107, 111, 123, 127],
    [128, 132, 144, 148, 160, 164, 176, 180, 130, 134, 146, 150, 162, 166, 178, 182],
    [136, 140, 152, 156, 168, 172, 184, 188, 138, 142, 154, 158, 170, 174, 186, 190],
    [161, 165, 177, 181, 129, 133, 145, 149, 163, 167, 179, 183, 131, 135, 147, 151],
    [169, 173, 185, 189, 137, 141, 153, 157, 171, 175, 187, 191, 139, 143, 155, 159],
    [224, 228, 240, 244, 192, 196, 208, 212, 226, 230, 242, 246, 194, 198, 210, 214],
    [232, 236, 248, 252, 200, 204, 216, 220, 234, 238, 250, 254, 202, 206, 218, 222],
    [193, 197, 209, 213, 225, 229, 241, 245, 195, 199, 211, 215, 227, 231, 243, 247],
    [201, 205, 217, 221, 233, 237, 249, 253, 203, 207, 219, 223, 235, 239, 251, 255]]


def vram(raw):
    size, = struct.unpack_from('<I', raw, 4)
    state_size, = struct.unpack_from('<I', raw, 12)
    state = raw[8 + size:8 + size + state_size]
    if struct.unpack_from('<I', state, 0)[0] != 9 or len(state) < VRAM_OFFSET + VRAM_SIZE:
        raise ValueError('unsupported GS state version or size')
    return state[VRAM_OFFSET:VRAM_OFFSET + VRAM_SIZE]


def ct32(mem, bp, bw, x, y):
    bn = bp + (y >> 5) * bw * 32 + (x >> 6) * 32 + BLOCK32[(y >> 3) & 3][(x >> 3) & 7]
    a = ((bn << 6) + COLUMN32[y & 7][x & 7]) * 4 % VRAM_SIZE
    return struct.unpack_from('<I', mem, a)[0]


def t8(mem, bp, bw, x, y):
    bn = bp + (y >> 6) * (bw >> 1) * 32 + (x >> 7) * 32 + BLOCK8[(y >> 4) & 3][(x >> 4) & 7]
    return mem[((bn << 8) + COLUMN8[y & 15][x & 15]) % VRAM_SIZE]


def clut256(mem, cbp):
    """CSM1 PSMCT32 CLUT: entry i at x = (i & 7) | (i & 0x10) >> 1, y = (i & 8) >> 3 | (i >> 5) << 1."""
    return [ct32(mem, cbp, 1, (i & 7) | ((i & 0x10) >> 1), ((i & 8) >> 3) | ((i >> 5) << 1)) for i in range(256)]


def records():
    names = json.load(open('private/reports/wad-independent.json'))['entries']
    files = {int(os.path.basename(p)[:6]): p for p in glob.glob('private/wad/*.dat')}
    pkgs = collections.defaultdict(dict)
    for x in names:
        for n in x['proposed_names']:
            u = n.upper()
            for kind in ('TEXDEF.PS2', 'TEXTURES.PS2'):
                if u.endswith('/' + kind):
                    pkgs[u[:-len(kind) - 1]][kind] = files[x['index']]
    out = collections.defaultdict(list)
    for pkg, f in sorted(pkgs.items()):
        if len(f) != 2:
            continue
        td = pathlib.Path(f['TEXDEF.PS2']).read_bytes()
        tx = pathlib.Path(f['TEXTURES.PS2']).read_bytes()
        count, defs, bitmaps = struct.unpack_from('<III', td, 0)
        for k in range(min(count, 100_000)):
            b = bitmaps + 64 * k
            fmt = td[b + 8]
            w, h, off, size = struct.unpack_from('<HHII', td, b + 12)
            pal = {48: 512, 66: 1024, 130: 0}.get(fmt)
            if pal is None or off + pal + w * h > len(tx):
                continue
            label = td[defs + 36 * k:defs + 36 * k + 30].split(b'\0')[0].decode('latin-1')
            idx = tx[off + pal:off + pal + w * h]
            out[hashlib.sha256(idx).hexdigest()].append(dict(package=pkg, record=k, label=label, format=fmt, width=w, height=h))
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('dump', type=pathlib.Path)
    p.add_argument('out', type=pathlib.Path)
    a = p.parse_args()
    if not a.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or a.out.exists():
        raise ValueError('choose a new private output file')
    raw = compression.zstd.ZstdDecompressor().decompress(a.dump.read_bytes(), max_length=64 * 1024 * 1024 + 1)
    framed = parse(raw, keep_streams=True)
    mem = vram(raw)
    index = records()
    seen = {}
    for path, stream in sorted(framed['streams'].items()):
        for d in walk(bytes(stream)):
            st = d['state']
            if not fields_prim(st['PRIM'] or 0)['TME']:
                continue
            t = fields_tex0(st['TEX0_1'] or 0)
            if t['PSM'] != 0x13:
                continue
            key = (t['TBP0'], t['TBW'], t['TW'], t['TH'], t['CBP'], t['CPSM'])
            if key in seen:
                seen[key]['vertices'] += d['vertices']
                continue
            w, h = 1 << t['TW'], 1 << t['TH']
            idx = bytes(t8(mem, t['TBP0'], t['TBW'], x, y) for y in range(h) for x in range(w))
            clut = clut256(mem, t['CBP']) if t['CPSM'] == 0 else None
            seen[key] = dict(path=path, tbp0=t['TBP0'], tbw=t['TBW'], width=w, height=h, cbp=t['CBP'], cpsm=t['CPSM'],
                             tcc=t['TCC'], tfx=t['TFX'], vertices=d['vertices'],
                             texture_matches=index.get(hashlib.sha256(idx).hexdigest(), [])[:8],
                             index_values=sorted(set(idx))[:24],
                             clut_first=[f'{c:08x}' for c in clut[:8]] if clut else None,
                             clut_used=({str(i): f'{clut[i]:08x}' for i in sorted(set(idx))[:24]} if clut else None))
    report = dict(schema_version=1, dump=str(a.dump), dump_sha256=hashlib.sha256(a.dump.read_bytes()).hexdigest(),
                  scope='PSMT8 draws: resident texture and CLUT read from the dump\'s initial GS memory', draws=list(seen.values()))
    with a.out.open('x') as f:
        json.dump(report, f, indent=1)
    for r in report['draws']:
        print(r['tbp0'], r['width'], r['height'], r['cbp'], [(m['package'], m['label'], m['format']) for m in r['texture_matches'][:2]], r['index_values'][:6], r['clut_first'][:2] if r['clut_first'] else None)


if __name__ == '__main__':
    main()
