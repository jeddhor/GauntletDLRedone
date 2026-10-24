#!/usr/bin/env python3
"""Facing of drawn and ADC-suppressed strip triangles in a stock PCSX2 GS
dump (research only, never runtime code).

For every PATH3 GIF tag drawing a triangle strip, each triangle whose three
vertices lie inside the window X0..X1 × Y0..Y1 (GS primitive coordinates,
1/16 dropped) gets its screen-space winding, alternated by vertex parity
from the tag's first vertex, and is counted as drawn (the closing vertex
has no ADC) or suppressed (ADC). A tag whose drawn and suppressed
triangles each have one winding is "opposite" when the two differ and
"same" when they agree; tags holding several joined strips (parity restarts
inside the tag) land in "mixed". Opposite ≫ same means the VU program
suppresses one facing.

  gs_cull.py DUMP.gs.zst X0 X1 Y0 Y1
"""
import collections
import compression.zstd
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from gs_draws import REG  # noqa: E402
from inspect_gs_dump import parse  # noqa: E402  (framing and limits)


def streams_of(raw):
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
        else:
            break
    return streams


def strip_tags(stream):
    """(vertices [(x, y, adc)]) per packed GIF tag drawing a triangle strip."""
    state = {}
    cursor = 0
    while cursor + 16 <= len(stream):
        low, layout = struct.unpack_from('<QQ', stream, cursor)
        cursor += 16
        loops, pre, prim = low & 0x7fff, (low >> 46) & 1, (low >> 47) & 0x7ff
        mode = (low >> 58) & 3
        count = (low >> 60) & 15 or 16
        if pre:
            state['PRIM'] = prim
        if mode != 0:
            cursor += ((loops * count * 8 + 15) // 16) * 16 if mode == 1 else loops * 16
            continue
        vs = []
        for i in range(loops * count):
            reg = (layout >> ((i % count) * 4)) & 15
            lo, hi = struct.unpack_from('<QQ', stream, cursor + 16 * i)
            if reg == 14:
                name = REG.get(hi & 255)
                if name:
                    state[name] = lo
            elif reg in (4, 5):
                vs.append(((lo & 0xffff) / 16, ((lo >> 32) & 0xffff) / 16, (hi >> 47) & 1))
        cursor += loops * count * 16
        if len(vs) >= 3 and ((state.get('PRIM') or 0) & 7) == 4:
            yield vs


def main():
    if len(sys.argv) != 6:
        raise SystemExit(__doc__)
    x0, x1, y0, y1 = (float(v) for v in sys.argv[2:6])
    raw = compression.zstd.ZstdDecompressor().decompress(pathlib.Path(sys.argv[1]).read_bytes(), max_length=64 * 1024 * 1024 + 1)
    parse(raw)  # validates the dump framing and identity
    tags = collections.Counter()
    triangles = collections.Counter()
    for stream in streams_of(raw).values():
        for vs in strip_tags(bytes(stream)):
            c = collections.Counter()
            for k in range(2, len(vs)):
                tri = vs[k - 2:k + 1]
                if not all(x0 <= x <= x1 and y0 <= y <= y1 for x, y, _ in tri):
                    continue
                (ax, ay, _), (bx, by, _), (cx, cy, _) = tri
                area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay)
                if area == 0:
                    continue
                facing = (area > 0) ^ (k % 2 == 1)
                c[('suppressed' if vs[k][2] else 'drawn', facing)] += 1
            if not c:
                continue
            triangles.update(c)
            drawn = {f for (w, f) in c if w == 'drawn'}
            suppressed = {f for (w, f) in c if w == 'suppressed'}
            if len(drawn) == 1 and len(suppressed) == 1:
                tags['opposite' if drawn != suppressed else 'same'] += 1
            elif len(drawn) == 1 and not suppressed:
                tags['drawn only'] += 1
            elif len(suppressed) == 1 and not drawn:
                tags['suppressed only'] += 1
            else:
                tags['mixed'] += 1
    print('tags', dict(tags))
    print('triangles', {f'{w} {"+" if f else "-"}': n for (w, f), n in sorted(triangles.items())})


if __name__ == '__main__':
    main()
