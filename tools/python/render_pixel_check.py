#!/usr/bin/env python3
"""CPU evaluation of the observed GS world formula at chosen pixels.

Ray-casts the reference window camera (tools/python/reference_camera.py
output; 554.27-pixel focal length over the 640×384 viewport, pbProjCalc)
through each pixel into the decoded level (gdl_tools objects-inspect /
world-inspect JSON), finds the nearest placed triangle and evaluates
texel × vertex colour / 128 × lightmap CLUT alpha / 128 (PSMT8 CT32 base,
format-146 lightmap, observed CLUT alpha 8·i) at its interpolated UVs. The
result is compared with a reference screenshot and, optionally, a native one
(both cropped to the 3D viewport and scaled to 640×384). Research only:
static file positions, no item/elevator/animation state, nearest texel.
"""
import argparse
import json
import pathlib

import numpy as np
from PIL import Image


def swizzle(i):
    return (i & ~24) | ((i & 8) << 1) | ((i & 16) >> 1)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--objects', type=pathlib.Path, required=True, help='objects-inspect JSON')
    ap.add_argument('--world', type=pathlib.Path, required=True, help='world-inspect JSON')
    ap.add_argument('--textures', type=pathlib.Path, required=True, help='level TEXTURES.PS2')
    ap.add_argument('--camera', type=pathlib.Path, required=True, help='reference_camera.py JSON')
    ap.add_argument('--reference', type=pathlib.Path, help='640×384 reference viewport PNG')
    ap.add_argument('--native', type=pathlib.Path, help='640×384 native viewport PNG')
    ap.add_argument('pixels', nargs='+', help='x,y in the 640×384 viewport')
    a = ap.parse_args()
    o = json.loads(a.objects.read_text())
    w = json.loads(a.world.read_text())
    tex = a.textures.read_bytes()
    cam = json.loads(a.camera.read_text())
    eye, right, up, fwd = (np.array(cam[k]) for k in ('eye', 'right', 'up', 'forward'))
    recs = {b['index']: b for b in o['objects']['bitmap_records']}
    nodes = w['world']['nodes']
    placed = {p['node_index']: p for p in w['candidate_placements']}
    defs = {d['label']: d['object_index'] for d in o['objects']['definitions']}
    geometry = {}
    for oc in o['outcomes']:
        if oc['geometry']:
            geometry.setdefault(oc['object'], []).append((oc['part'], oc['geometry']['strips']))
    parts = {ob['index']: ob['parts'] for ob in o['objects']['objects']}
    ref = Image.open(a.reference).convert('RGB') if a.reference else None
    nat = Image.open(a.native).convert('RGB') if a.native else None

    def texel(index, u, v):
        b = recs[index]
        if b['format'] != 66:
            return None
        width, height, off = b['width'], b['height'], b['texture_offset']
        i = tex[off + 1024 + (int(v * height) % height) * width + int(u * width) % width]
        p = swizzle(i)
        return tex[off + 4 * p:off + 4 * p + 4]

    def lightmap(index, u, v):
        off = recs[index]['texture_offset']
        i = min(255, max(0, int(v * 256))) * 256 + min(255, max(0, int(u * 256)))
        return ((tex[off + i // 2] >> ((i % 2) * 4)) & 15) * 8

    def expand(c):
        return (c << 2) | (c >> 3)

    for pixel in a.pixels:
        px, py = map(float, pixel.split(','))
        d = fwd + ((px - 320) / 554.27) * right - ((py - 192) / 554.27) * up
        d /= np.linalg.norm(d)
        best = None
        for n in nodes:
            oi = defs.get(n['label'])
            if oi is None or oi not in geometry or n['flags'] & 0x800:
                continue
            t = np.array(placed[n['index']]['summed_position'])
            for part, strips in geometry[oi]:
                for s in strips:
                    pos = np.array(s['positions']) + t
                    for ia, ib, ic in np.array(s['indices']).reshape(-1, 3):
                        v0, v1, v2 = pos[ia], pos[ib], pos[ic]
                        e1, e2 = v1 - v0, v2 - v0
                        h = np.cross(d, e2)
                        det = e1 @ h
                        if abs(det) < 1e-9:
                            continue
                        inv = 1 / det
                        sv = eye - v0
                        bu = inv * (sv @ h)
                        if bu < 0 or bu > 1:
                            continue
                        q = np.cross(sv, e1)
                        bv = inv * (d @ q)
                        if bv < 0 or bu + bv > 1:
                            continue
                        dist = inv * (e2 @ q)
                        if 0 < dist and (best is None or dist < best[0]):
                            best = (dist, n['label'], parts[oi][part], s, (ia, ib, ic), (1 - bu - bv, bu, bv))
        out = dict(pixel=(px, py))
        if best:
            dist, label, part, s, tri, bc = best
            mix = lambda values: sum(bc[k] * np.array(values[i], dtype=float) for k, i in enumerate(tri))
            uv = mix(s['uvs'])
            colour = mix([[expand(p & 31), expand((p >> 5) & 31), expand((p >> 10) & 31)] for p in s['packed_colors']]) if s['packed_colors'] else np.array([128.0] * 3)
            t = texel(part['texture_index'], *uv)
            out.update(node=label, texture=part['texture_index'], lightmap=part['lightmap_index'], distance=round(float(dist), 2))
            if t is not None:
                rgb = [min(255.0, t[k] * colour[k] / 128) for k in range(3)]
                if s['lightmap_uvs'] and part['lightmap_index'] in recs and recs[part['lightmap_index']]['format'] == 146:
                    lm = lightmap(part['lightmap_index'], *mix(s['lightmap_uvs']))
                    out['lightmap_alpha'] = lm
                    rgb = [x * lm / 128 for x in rgb]
                out['expected'] = [round(x) for x in rgb]
        if ref:
            out['reference'] = ref.getpixel((int(px), int(py)))
        if nat:
            out['native'] = nat.getpixel((int(px), int(py)))
        print(json.dumps(out))


if __name__ == '__main__':
    main()
