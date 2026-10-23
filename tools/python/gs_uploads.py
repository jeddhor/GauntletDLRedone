#!/usr/bin/env python3
"""Texture and CLUT uploads of a stock PCSX2 GS dump matched to TEXDEF records
(M05 research only, never runtime code).

Walks the dump's GIF streams, follows the A+D writes of BITBLTBUF, TRXPOS,
TRXREG and TRXDIR, and collects every host-to-local transfer's IMAGE data.
Each upload is matched by SHA-256 against record-aligned slices of every
TEXDEF/TEXTURES pair in the private archive (dictionary names from
wad-independent.json): the palette, the base level and each mip level as the
native decoder lays them out (palette first, then base indices, then mips).
A slice match means the original uploaded exactly those source bytes; it says
nothing about how the GS samples them. Output is a new private JSON report.

  gs_uploads.py DUMP.gs.zst NEW_REPORT.json
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
from inspect_gs_dump import parse  # noqa: E402  (framing and limits)

# Bits per pixel of host transfers by DPSM.
BPP = {0x00: 32, 0x01: 24, 0x02: 16, 0x0a: 16, 0x13: 8, 0x14: 4, 0x1b: 8, 0x24: 4, 0x2c: 4,
       0x30: 32, 0x31: 24, 0x32: 16, 0x3a: 16}
PSM_NAME = {0x00: 'PSMCT32', 0x01: 'PSMCT24', 0x02: 'PSMCT16', 0x0a: 'PSMCT16S', 0x13: 'PSMT8', 0x14: 'PSMT4',
            0x1b: 'PSMT8H', 0x24: 'PSMT4HL', 0x2c: 'PSMT4HH', 0x30: 'PSMZ32', 0x31: 'PSMZ24', 0x32: 'PSMZ16', 0x3a: 'PSMZ16S'}
# TEXDEF format → (palette bytes, index bits per pixel), as gdl_formats::texdef and objects decode them.
FORMATS = {0: (0, 16), 2: (0, 32), 16: (32, 4), 34: (64, 4), 48: (512, 8), 66: (1024, 8), 146: (0, 4), 130: (0, 8)}
MAX_UPLOADS = 20_000


def uploads(stream, path):
    regs = {}
    out = []
    current = None
    cursor = 0
    while cursor + 16 <= len(stream):
        low, layout = struct.unpack_from('<QQ', stream, cursor)
        cursor += 16
        loops, mode, count = low & 0x7fff, (low >> 58) & 3, (low >> 60) & 15 or 16
        size = loops * count * 16 if mode == 0 else ((loops * count * 8 + 15) // 16) * 16 if mode == 1 else loops * 16
        if cursor + size > len(stream):
            raise ValueError('GIF payload exceeds captured stream')
        if mode == 0:
            for i in range(loops * count):
                if (layout >> ((i % count) * 4)) & 15 != 14:
                    continue
                value, address = struct.unpack_from('<QQ', stream, cursor + i * 16)
                reg = address & 255
                regs[reg] = value
                if reg == 0x53 and value & 3 == 0:
                    b, r, p = regs.get(0x50, 0), regs.get(0x52, 0), regs.get(0x51, 0)
                    dpsm = (b >> 56) & 63
                    rrw, rrh = r & 0xfff, (r >> 32) & 0xfff
                    if dpsm not in BPP:
                        raise ValueError(f'unknown DPSM {dpsm:#x}')
                    current = dict(path=path, dbp=(b >> 32) & 0x3fff, dbw=(b >> 48) & 63, dpsm=PSM_NAME[dpsm],
                                   dsax=(p >> 32) & 0x7ff, dsay=(p >> 48) & 0x7ff, rrw=rrw, rrh=rrh,
                                   expected=rrw * rrh * BPP[dpsm] // 8, data=bytearray())
        elif mode == 2 and current is not None:
            current['data'] += stream[cursor:cursor + size]
            if len(current['data']) >= current['expected']:
                current['data'] = bytes(current['data'][:current['expected']])
                out.append(current)
                current = None
                if len(out) > MAX_UPLOADS:
                    raise ValueError('upload count policy')
        cursor += size
    return out


def record_slices():
    """sha256 → [(package, record, format, slice)] over every TEXDEF/TEXTURES pair."""
    names = json.load(open('private/reports/wad-independent.json'))['entries']
    files = {int(os.path.basename(p)[:6]): p for p in glob.glob('private/wad/*.dat')}
    pkgs = collections.defaultdict(dict)
    for x in names:
        for n in x['proposed_names']:
            u = n.upper()
            for kind in ('TEXDEF.PS2', 'TEXTURES.PS2'):
                if u.endswith('/' + kind):
                    pkgs[u[:-len(kind) - 1]][kind] = files[x['index']]
    index = collections.defaultdict(list)
    textures = {}
    for pkg, f in sorted(pkgs.items()):
        if len(f) != 2:
            continue
        td = pathlib.Path(f['TEXDEF.PS2']).read_bytes()
        tx = pathlib.Path(f['TEXTURES.PS2']).read_bytes()
        textures[pkg] = tx
        count, _, bitmaps = struct.unpack_from('<III', td, 0)
        if count > 100_000 or bitmaps + 64 * count > len(td):
            raise ValueError(f'{pkg}: TEXDEF bounds')
        for k in range(count):
            b = bitmaps + 64 * k
            fmt, mips = td[b + 8], td[b + 10]
            w, h, off, size = struct.unpack_from('<HHII', td, b + 12)
            if fmt not in FORMATS or off + size > len(tx):
                continue
            pal, bits = FORMATS[fmt]
            data = tx[off:off + size]
            cuts = [('palette', 0, pal)] if pal else []
            at = pal
            for level in range(1 + mips):
                n = max(1, w >> level) * max(1, h >> level) * bits // 8
                if at + n > len(data):
                    break
                cuts.append(('base' if level == 0 else f'mip{level}', at, at + n))
                at += n
            for name, a, z in cuts:
                index[hashlib.sha256(data[a:z]).hexdigest()].append(dict(package=pkg, record=k, format=fmt, width=w, height=h, mips=mips, slice=name))
    return index, textures


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('dump', type=pathlib.Path)
    p.add_argument('out', type=pathlib.Path)
    args = p.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('choose a new private output file')
    if args.dump.stat().st_size > 64 * 1024 * 1024:
        raise ValueError('compressed GS dump exceeds policy')
    compressed = args.dump.read_bytes()
    decoder = compression.zstd.ZstdDecompressor()
    raw = decoder.decompress(compressed, max_length=64 * 1024 * 1024 + 1)
    if len(raw) > 64 * 1024 * 1024 or not decoder.eof:
        raise ValueError('incomplete/over-limit decompressed GS dump')
    framed = parse(raw, keep_streams=True)
    ups = []
    for path, stream in sorted(framed['streams'].items()):
        ups += uploads(bytes(stream), path)
    index, textures = record_slices()
    located = {}
    rows = []
    summary = collections.Counter()
    for k, u in enumerate(ups):
        sha = hashlib.sha256(u['data']).hexdigest()
        matches = index.get(sha, [])
        kinds = sorted({m['slice'].rstrip('0123456789') for m in matches})
        if not matches and sha not in located and 1024 <= len(u['data']):
            # Not a TEXDEF slice: where the bytes sit in TEXTURES payloads
            # (object bitmap records point there directly).
            located[sha] = [dict(package=pkg, offset=at) for pkg, tx in sorted(textures.items()) if (at := tx.find(u['data'])) >= 0][:8]
        summary[(u['dpsm'], 'matched' if matches else 'unmatched')] += 1
        rows.append(dict(upload=k, **{x: u[x] for x in ('path', 'dbp', 'dbw', 'dpsm', 'dsax', 'dsay', 'rrw', 'rrh')},
                         bytes=len(u['data']), sha256=sha, slice_kinds=kinds, match_count=len(matches),
                         matches=matches[:8], formats=sorted({m['format'] for m in matches}),
                         textures_offsets=located.get(sha, [])))
    report = dict(schema_version=1, dump=str(args.dump), dump_sha256=hashlib.sha256(compressed).hexdigest(),
                  scope='host-to-local transfers in the captured frame matched to record-aligned TEXDEF slices; GS sampling not compared',
                  uploads=len(ups), summary={f'{a} {b}': n for (a, b), n in sorted(summary.items())}, rows=rows)
    with args.out.open('x') as f:
        json.dump(report, f, indent=1)
        f.write('\n')
    for k, v in report['summary'].items():
        print(k, v)


if __name__ == '__main__':
    main()
