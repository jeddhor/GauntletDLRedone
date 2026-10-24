#!/usr/bin/env python3
"""Software replay of a stock PCSX2 GS dump's draws (research only, never
runtime code).

Plays the dump's PATH1..3 GIF data in event order through a small model of
the GS: PACKED/REGLIST/IMAGE tags, the vertex queue (triangles, strips,
fans, sprites; ADC), STQ perspective texturing with the TEX1 LOD and
bilinear/mipmap filters, CLUT lookup, TFX, the alpha and destination-alpha
tests, ZTE/ZTST against a 24-bit depth buffer, ALPHA_1 blending with
COLCLAMP, DIMX dithering, FBA and FBMSK, and writes into PSMCT16S/PSMCT32
frame buffers. The model's frame and depth buffers are linear arrays per
FBP/ZBP (a draw that samples one as a texture is reported, not modelled).
Textures come from the dump's own uploads (each host-to-local IMAGE
transfer kept linearly by DBP/DPSM, local-to-local copies followed), so no
block swizzle is needed for them; textures resident before the dump are
read from the frozen local memory for PSMT8/PSMCT32 (gs_vram.py) and
reported as unavailable otherwise.

  gs_replay.py DUMP.gs.zst NEW_PREFIX [--window X0 Y0 X1 Y1] [--skip TBP,...]
               [--only-frame N]

Writes NEW_PREFIX-frameK.png (the FBP the frame drew most into, 640×448
window coordinates) and NEW_PREFIX.json (per frame: draw/triangle counts,
skipped draws by reason, region statistics). --skip drops draws whose
TEX0 TBP0 is listed (decimal), for per-component contributions. Approximate
where the GS is not modelled (noted in the report's scope): pixel sampling
at integer coordinates with a top-left rule, bilinear weights in floating
point, LOD per pixel.
"""
import argparse
import collections
import compression.zstd
import json
import math
import pathlib
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from gs_vram import VRAM_OFFSET, VRAM_SIZE, ct32, t8  # noqa: E402
from inspect_gs_dump import parse  # noqa: E402  (framing and limits)

BPP = {0x00: 32, 0x01: 24, 0x02: 16, 0x0a: 16, 0x13: 8, 0x14: 4, 0x30: 32, 0x31: 24, 0x32: 16, 0x3a: 16}
W, H = 640, 448
MAX_TRIANGLES = 2_000_000


def f32(bits):
    return struct.unpack('<f', struct.pack('<I', bits & 0xffffffff))[0]


def s12(v):
    v &= 0xfff
    return v - 0x1000 if v & 0x800 else v


class Upload:
    """A linear texel store for one (DBP, DPSM): uploads write rectangles into it."""

    def __init__(self, dbw, psm):
        self.dbw, self.psm = dbw, psm
        self.texels = np.zeros((0, 0), np.uint32)
        self.serial = -1

    def put(self, x, y, w, h, values, serial):
        need_h, need_w = y + h, x + w
        if need_h > self.texels.shape[0] or need_w > self.texels.shape[1]:
            grown = np.zeros((max(need_h, self.texels.shape[0]), max(need_w, self.texels.shape[1])), np.uint32)
            grown[:self.texels.shape[0], :self.texels.shape[1]] = self.texels
            self.texels = grown
        self.texels[y:y + h, x:x + w] = values.reshape(h, w)
        self.serial = serial


def unpack_image(data, psm, w, h):
    n = w * h
    if BPP[psm] == 32:
        return np.frombuffer(data[:4 * n], '<u4').astype(np.uint32)
    if BPP[psm] == 24:
        b = np.frombuffer(data[:3 * n], np.uint8).reshape(n, 3).astype(np.uint32)
        return b[:, 0] | b[:, 1] << 8 | b[:, 2] << 16
    if BPP[psm] == 16:
        return np.frombuffer(data[:2 * n], '<u2').astype(np.uint32)
    if BPP[psm] == 8:
        return np.frombuffer(data[:n], np.uint8).astype(np.uint32)
    b = np.frombuffer(data[:(n + 1) // 2], np.uint8)
    return np.stack([b & 15, b >> 4], 1).reshape(-1)[:n].astype(np.uint32)


class GS:
    def __init__(self, mem, window, skip):
        self.mem = mem
        self.window = window
        self.skip = skip
        self.r = collections.defaultdict(int)
        self.r[0x1a] = 1  # PRMODECONT AC
        self.q = 1.0
        self.st = (0.0, 0.0)
        self.rgbaq = 0x80808080
        self.queue = []
        self.cache = {}
        self.uploads = {}
        self.serial = 0
        self.trx = None
        self.frames = {}
        self.zbufs = {}
        self.stats = collections.Counter()
        self.fb_use = collections.Counter()
        self.unavailable = collections.Counter()

    # --- registers -------------------------------------------------------
    def write(self, reg, value):
        self.r[reg] = value
        if reg == 0x00:
            self.queue = []
        elif reg == 0x01:
            self.rgbaq = value
        elif reg == 0x53 and value & 3 == 0:
            b, p, rr = self.r[0x50], self.r[0x51], self.r[0x52]
            dpsm = (b >> 56) & 63
            if dpsm not in BPP:
                raise ValueError(f'unknown DPSM {dpsm:#x}')
            self.trx = dict(dbp=(b >> 32) & 0x3fff, dbw=(b >> 48) & 63, psm=dpsm, x=(p >> 32) & 0x7ff, y=(p >> 48) & 0x7ff,
                            w=rr & 0xfff, h=(rr >> 32) & 0xfff, data=bytearray())
            self.trx['need'] = self.trx['w'] * self.trx['h'] * BPP[dpsm] // 8
        elif reg == 0x53 and value & 3 == 2:
            self.local_copy()

    def image(self, data):
        t = self.trx
        if t is None:
            self.stats['image_without_trx'] += 1
            return
        t['data'] += data
        if len(t['data']) >= t['need']:
            vals = unpack_image(bytes(t['data']), t['psm'], t['w'], t['h'])
            u = self.uploads.setdefault((t['dbp'], t['psm']), Upload(t['dbw'], t['psm']))
            self.serial += 1
            u.put(t['x'], t['y'], t['w'], t['h'], vals, self.serial)
            self.trx = None
            self.stats['uploads'] += 1

    def local_copy(self):
        b, p, rr = self.r[0x50], self.r[0x51], self.r[0x52]
        sbp, spsm, dbp, dpsm = b & 0x3fff, (b >> 24) & 63, (b >> 32) & 0x3fff, (b >> 56) & 63
        ssx, ssy, dsx, dsy = p & 0x7ff, (p >> 16) & 0x7ff, (p >> 32) & 0x7ff, (p >> 48) & 0x7ff
        w, h = rr & 0xfff, (rr >> 32) & 0xfff
        src = self.uploads.get((sbp, spsm))
        if src is None or spsm != dpsm or src.texels.shape[0] < ssy + h or src.texels.shape[1] < ssx + w:
            self.stats['local_copy_unmodelled'] += 1
            return
        u = self.uploads.setdefault((dbp, dpsm), Upload((b >> 48) & 63, dpsm))
        self.serial += 1
        u.put(dsx, dsy, w, h, src.texels[ssy:ssy + h, ssx:ssx + w].reshape(-1).copy(), self.serial)
        self.stats['local_copies'] += 1

    # --- vertices --------------------------------------------------------
    def vertex(self, x, y, z, kick, adc, fog=0):
        c = self.rgbaq
        st = self.st
        uv = self.r[0x03]
        self.queue.append(dict(x=x, y=y, z=z, rgba=(c & 255, (c >> 8) & 255, (c >> 16) & 255, (c >> 24) & 255),
                               s=st[0], t=st[1], q=self.q, u=(uv & 0x3fff) / 16.0, v=((uv >> 16) & 0x3fff) / 16.0))
        prim = self.r[0x00] & 7
        need = {0: 1, 1: 2, 2: 2, 3: 3, 4: 3, 5: 3, 6: 2}.get(prim, 99)
        if len(self.queue) < need:
            return
        if not kick or adc:
            if prim in (3, 6):
                if len(self.queue) == need:
                    self.queue = []
            elif prim == 4:
                self.queue = self.queue[-2:]
            elif prim == 5:
                self.queue = [self.queue[0], self.queue[-1]]
            return
        if prim in (3, 4, 5):
            tri = self.queue[-3:] if prim != 5 else [self.queue[0], self.queue[-2], self.queue[-1]]
            self.draw_triangle(tri)
            if prim == 3:
                self.queue = []
            elif prim == 4:
                self.queue = self.queue[-2:]
            else:
                self.queue = [self.queue[0], self.queue[-1]]
        elif prim == 6:
            self.draw_sprite(self.queue[-2:])
            self.queue = []
        else:
            self.stats['points_lines_skipped'] += 1
            self.queue = []

    # --- draw state ------------------------------------------------------
    def ctx(self):
        attrs = self.r[0x00] if self.r[0x1a] & 1 else self.r[0x1b]
        prim = (self.r[0x00] & 7) | (attrs & 0x7f8)
        c = (prim >> 9) & 1
        return prim, dict(tex0=self.r[0x06 + c], clamp=self.r[0x08 + c], tex1=self.r[0x14 + c], mip1=self.r[0x34 + c],
                          mip2=self.r[0x36 + c], alpha=self.r[0x42 + c], test=self.r[0x47 + c], fba=self.r[0x4a + c],
                          frame=self.r[0x4c + c], zbuf=self.r[0x4e + c], xyoff=self.r[0x18 + c], scissor=self.r[0x40 + c])

    def buffers(self, s):
        fbp, fbw, fpsm, fbmsk = s['frame'] & 0x1ff, (s['frame'] >> 16) & 63, (s['frame'] >> 24) & 63, s['frame'] >> 32
        zbp, zpsm, zmsk = s['zbuf'] & 0x1ff, (s['zbuf'] >> 24) & 15, (s['zbuf'] >> 32) & 1
        if fpsm not in (0x00, 0x0a, 0x02):
            return None
        fb = self.frames.setdefault((fbp, fpsm), np.zeros((H, W), np.uint32))
        zb = self.zbufs.setdefault(zbp, np.zeros((H, W), np.uint32))
        self.fb_use[(fbp, fpsm)] += 1
        return fb, fpsm, fbmsk, zb, zmsk, {0: 0xffffffff, 1: 0xffffff, 2: 0xffff, 0xa: 0xffff}.get(zpsm, 0xffffffff)

    def texture(self, tbp, psm, tbw, tw, th):
        u = self.uploads.get((tbp, psm))
        if u is not None:
            if u.texels.shape[0] < th or u.texels.shape[1] < tw:
                self.stats['texture_upload_smaller_than_tex0'] += 1
            t = u.texels
            if t.shape[0] < th or t.shape[1] < tw:
                pad = np.zeros((max(th, t.shape[0]), max(tw, t.shape[1])), np.uint32)
                pad[:t.shape[0], :t.shape[1]] = t
                t = pad
            return t[:th, :tw]
        if psm == 0x13:
            return np.array([[t8(self.mem, tbp, tbw, x, y) for x in range(tw)] for y in range(th)], np.uint32)
        if psm == 0x00:
            return np.array([[ct32(self.mem, tbp, tbw, x, y) for x in range(tw)] for y in range(th)], np.uint32)
        return None

    def clut(self, cbp, cpsm, csm, csa, psm):
        u = self.uploads.get((cbp, cpsm))
        if cpsm not in (0, 2) or csm != 0:
            return None
        if cpsm == 2 and u is None:
            # PSMCT16 CLUTs resident before the dump: the CT16 swizzle is
            # not modelled.
            return None
        if psm == 0x13:
            idx = np.arange(256)
            xs, ys = (idx & 7) | ((idx & 0x10) >> 1), ((idx & 8) >> 3) | ((idx >> 5) << 1)
            if u is not None and u.texels.shape[0] >= 16 and u.texels.shape[1] >= 16:
                return u.texels[ys, xs]
            return np.array([ct32(self.mem, cbp, 1, int(x), int(y)) for x, y in zip(xs, ys)], np.uint32)
        if psm == 0x14:
            if csa:
                return None
            idx = np.arange(16)
            if u is not None and u.texels.shape[0] >= 2 and u.texels.shape[1] >= 8:
                return u.texels[idx >> 3, idx & 7]
            return np.array([ct32(self.mem, cbp, 1, int(i & 7), int(i >> 3)) for i in idx], np.uint32)
        return None

    def rgba_texture(self, tex0, level_tbp=None, level=0):
        key = (tex0 & ((1 << 61) - 1), level_tbp, level)
        tbp0 = tex0 & 0x3fff if level_tbp is None else level_tbp[0]
        psm0, cbp0, cpsm0 = (tex0 >> 20) & 63, (tex0 >> 37) & 0x3fff, (tex0 >> 51) & 15
        serials = (getattr(self.uploads.get((tbp0, psm0)), 'serial', -1), getattr(self.uploads.get((cbp0, cpsm0)), 'serial', -1))
        hit = self.cache.get(key)
        if hit is not None and hit[0] == serials:
            return hit[1]
        t = self.rgba_texture_uncached(tex0, level_tbp, level)
        self.cache[key] = (serials, t)
        return t

    def rgba_texture_uncached(self, tex0, level_tbp=None, level=0):
        tbp = tex0 & 0x3fff if level_tbp is None else level_tbp[0]
        tbw = (tex0 >> 14) & 63 if level_tbp is None else level_tbp[1]
        psm, tw, th = (tex0 >> 20) & 63, 1 << ((tex0 >> 26) & 15), 1 << ((tex0 >> 30) & 15)
        tw, th = max(1, tw >> level), max(1, th >> level)
        idx = self.texture(tbp, psm, tbw, tw, th)
        if idx is None:
            return None
        if psm in (0x13, 0x14):
            pal = self.clut((tex0 >> 37) & 0x3fff, (tex0 >> 51) & 15, (tex0 >> 55) & 1, (tex0 >> 56) & 31, psm)
            if pal is None:
                return None
            c = pal[np.minimum(idx, len(pal) - 1)]
            if (tex0 >> 51) & 15 == 2:
                c = self.expand16(c)
        elif psm == 0x00:
            c = idx
        else:
            return None
        return np.stack([c & 255, (c >> 8) & 255, (c >> 16) & 255, c >> 24], -1).astype(np.float64)

    def expand16(self, c):
        """RGBA5551 → 32-bit through TEXA (TA0, AEM, TA1)."""
        texa = self.r[0x3b]
        ta0, aem, ta1 = texa & 255, (texa >> 15) & 1, (texa >> 32) & 255
        r, g, b = (c & 31) << 3, ((c >> 5) & 31) << 3, ((c >> 10) & 31) << 3
        a = np.where(c & 0x8000, ta1, np.where(aem & ((c & 0x7fff) == 0), 0, ta0))
        return (r | g << 8 | b << 16 | a.astype(np.uint32) << 24).astype(np.uint32)

    # --- rasterisation ---------------------------------------------------
    def setup(self, verts):
        prim, s = self.ctx()
        tex0 = s['tex0']
        tbp = tex0 & 0x3fff
        if (prim >> 4) & 1 and tbp in self.skip:
            self.stats['skipped_by_request'] += 1
            return None
        bufs = self.buffers(s)
        if bufs is None:
            self.stats['frame_psm_unmodelled'] += 1
            return None
        return prim, s, bufs

    def draw_triangle(self, v):
        self.stats['triangles'] += 1
        if self.stats['triangles'] > MAX_TRIANGLES:
            raise ValueError('triangle count policy')
        st = self.setup(v)
        if st is None:
            return
        prim, s, bufs = st
        ox, oy = (s['xyoff'] & 0xffff), ((s['xyoff'] >> 32) & 0xffff)
        xs = [(p['x'] - ox) for p in v]
        ys = [(p['y'] - oy) for p in v]
        area = (xs[1] - xs[0]) * (ys[2] - ys[0]) - (xs[2] - xs[0]) * (ys[1] - ys[0])
        if area == 0:
            return
        sc = s['scissor']
        x0 = max(math.ceil(min(xs) / 16), sc & 0x7ff, self.window[0])
        x1 = min(math.floor(max(xs) / 16), (sc >> 16) & 0x7ff, self.window[2] - 1, W - 1)
        y0 = max(math.ceil(min(ys) / 16), (sc >> 32) & 0x7ff, self.window[1])
        y1 = min(math.floor(max(ys) / 16), (sc >> 48) & 0x7ff, self.window[3] - 1, H - 1)
        if x1 < x0 or y1 < y0:
            return
        px, py = np.meshgrid(np.arange(x0, x1 + 1) * 16, np.arange(y0, y1 + 1) * 16)
        flat = v[2]['rgba']
        if area < 0:
            order = [0, 2, 1]
            xs, ys, v = [xs[i] for i in order], [ys[i] for i in order], [v[i] for i in order]
            area = -area
        inside = np.ones(px.shape, bool)
        bary = []
        for i in range(3):
            a, b = (i + 1) % 3, (i + 2) % 3
            e = (xs[b] - xs[a]) * (py - ys[a]) - (ys[b] - ys[a]) * (px - xs[a])
            # top-left rule: pixels on a top or left edge are drawn.
            top_left = (ys[b] - ys[a] < 0) or (ys[b] == ys[a] and xs[b] - xs[a] > 0)
            inside &= (e > 0) | ((e == 0) & top_left)
            bary.append(e / area)
        if not inside.any():
            return
        w = [b[inside] for b in bary]
        gy, gx = np.nonzero(inside)
        gx, gy = gx + x0, gy + y0

        def lerp(key):
            return w[0] * v[0][key] + w[1] * v[1][key] + w[2] * v[2][key]
        iip = (prim >> 3) & 1
        if iip:
            col = np.stack([lerp_rgba(w, v, k) for k in range(4)], -1)
        else:
            col = np.tile(np.array(flat, np.float64), (len(gx), 1))
        z = np.clip(lerp('z'), 0, 0xffffffff)
        tex = None
        if (prim >> 4) & 1:
            if (prim >> 8) & 1:
                uu, vv, q = lerp('u'), lerp('v'), None
            else:
                q = lerp('q')
                qs = np.where(q == 0, 1e-30, q)
                uu, vv = lerp('s') / qs, lerp('t') / qs
            tex = (uu, vv, q)
        self.shade(gx, gy, col, z, tex, prim, s, bufs, fst=(prim >> 8) & 1)

    def draw_sprite(self, v):
        self.stats['sprites'] += 1
        st = self.setup(v)
        if st is None:
            return
        prim, s, bufs = st
        ox, oy = (s['xyoff'] & 0xffff), ((s['xyoff'] >> 32) & 0xffff)
        a, b = v
        ax, ay, bx, by = a['x'] - ox, a['y'] - oy, b['x'] - ox, b['y'] - oy
        if bx < ax:
            a, b, ax, bx = b, a, bx, ax
        lo_y, hi_y = (ay, by) if ay <= by else (by, ay)
        sc = s['scissor']
        x0 = max(math.ceil(ax / 16), sc & 0x7ff, self.window[0])
        x1 = min(math.ceil(bx / 16) - 1, (sc >> 16) & 0x7ff, self.window[2] - 1, W - 1)
        y0 = max(math.ceil(lo_y / 16), (sc >> 32) & 0x7ff, self.window[1])
        y1 = min(math.ceil(hi_y / 16) - 1, (sc >> 48) & 0x7ff, self.window[3] - 1, H - 1)
        if x1 < x0 or y1 < y0:
            return
        gx, gy = np.meshgrid(np.arange(x0, x1 + 1), np.arange(y0, y1 + 1))
        gx, gy = gx.reshape(-1), gy.reshape(-1)
        col = np.tile(np.array(b['rgba'], np.float64), (len(gx), 1))
        z = np.full(len(gx), float(b['z']))
        tex = None
        if (prim >> 4) & 1:
            fx = (gx * 16 - ax) / max(bx - ax, 1)
            fy = (gy * 16 - ay) / ((by - ay) or 1)
            if (prim >> 8) & 1:
                uu = a['u'] + fx * (b['u'] - a['u'])
                vv = a['v'] + fy * (b['v'] - a['v'])
                q = None
            else:
                sa, sb = a['s'] / (a['q'] or 1e-30), b['s'] / (b['q'] or 1e-30)
                ta, tb = a['t'] / (a['q'] or 1e-30), b['t'] / (b['q'] or 1e-30)
                uu, vv = sa + fx * (sb - sa), ta + fy * (tb - ta)
                q = np.full(len(gx), b['q'])
            tex = (uu, vv, q)
        self.shade(gx, gy, col, z, tex, prim, s, bufs, fst=(prim >> 8) & 1)

    def sample(self, s, uu, vv, q, fst):
        """Texel colours (N × 4 floats) for normalised (STQ) or texel (UV) coordinates."""
        tex0, tex1, clamp = s['tex0'], s['tex1'], s['clamp']
        tw, th = 1 << ((tex0 >> 26) & 15), 1 << ((tex0 >> 30) & 15)
        lcm, mxl, mmag, mmin, l_, k = tex1 & 1, (tex1 >> 2) & 7, (tex1 >> 5) & 1, (tex1 >> 6) & 7, (tex1 >> 19) & 3, s12(tex1 >> 32) / 16.0
        n = len(uu)
        if lcm or q is None:
            lod = np.full(n, k)
        else:
            lod = np.log2(1.0 / np.maximum(np.abs(q), 1e-30)) * (1 << l_) + k
        out = np.zeros((n, 4))
        magnify = lod <= 0
        groups = [(magnify, 0, 0.0, mmag)]
        if (~magnify).any():
            lodc = np.clip(lod, 0, mxl)
            if mmin in (0, 1) or mxl == 0:
                groups.append((~magnify, 0, 0.0, mmin & 1))
            else:
                # nearest-mip modes (2, 4) take the nearest level; linear-mip
                # modes (3, 5) blend floor(LOD) and the next.
                base = (np.floor(lodc) if mmin in (3, 5) else np.floor(lodc + 0.5)).astype(int)
                frac = lodc - base if mmin in (3, 5) else np.zeros(n)
                for lv in range(mxl + 1):
                    sel = (~magnify) & (base == lv)
                    if sel.any():
                        groups.append((sel, lv, frac, 1 if mmin in (4, 5) else 0))
        for sel, lv, frac, linear in groups:
            if not sel.any():
                continue
            c0 = self.sample_level(s, uu[sel], vv[sel], lv, linear, fst, tw, th, clamp)
            if c0 is None:
                return None
            if isinstance(frac, np.ndarray) and (frac[sel] > 0).any() and lv < mxl:
                c1 = self.sample_level(s, uu[sel], vv[sel], lv + 1, linear, fst, tw, th, clamp)
                if c1 is None:
                    return None
                f = frac[sel][:, None]
                c0 = c0 * (1 - f) + c1 * f
            out[sel] = c0
        return out

    def sample_level(self, s, uu, vv, lv, linear, fst, tw, th, clamp):
        tex0 = s['tex0']
        if lv == 0:
            t = self.rgba_texture(tex0)
        else:
            m = s['mip1'] if lv <= 3 else s['mip2']
            j = (lv - 1) % 3
            t = self.rgba_texture(tex0, (((m >> (20 * j)) & 0x3fff), ((m >> (20 * j + 14)) & 63)), lv)
        if t is None:
            self.unavailable[(tex0 & 0x3fff, (tex0 >> 20) & 63, lv)] += 1
            return None
        lh, lw = t.shape[0], t.shape[1]
        x = uu * (lw if not fst else 1.0 / (1 << lv))
        y = vv * (lh if not fst else 1.0 / (1 << lv))
        wms, wmt = clamp & 3, (clamp >> 2) & 3
        if wms > 1 or wmt > 1:
            self.stats['region_clamp_as_clamp'] += 1
            wms, wmt = min(wms, 1), min(wmt, 1)

        def wrap(c, size, mode):
            c = np.floor(c).astype(np.int64)
            return np.clip(c, 0, size - 1) if mode == 1 else np.mod(c, size)
        if linear:
            x, y = x - 0.5, y - 0.5
            fx, fy = (x - np.floor(x))[:, None], (y - np.floor(y))[:, None]
            x0, y0 = wrap(x, lw, wms), wrap(y, lh, wmt)
            x1, y1 = wrap(np.floor(x) + 1, lw, wms), wrap(np.floor(y) + 1, lh, wmt)
            return (t[y0, x0] * (1 - fx) * (1 - fy) + t[y0, x1] * fx * (1 - fy) + t[y1, x0] * (1 - fx) * fy + t[y1, x1] * fx * fy)
        return t[wrap(y, lh, wmt), wrap(x, lw, wms)]

    def shade(self, gx, gy, col, z, tex, prim, s, bufs, fst):
        fb, fpsm, fbmsk, zb, zmsk, zmax = bufs
        test = s['test']
        zte, ztst = (test >> 16) & 1, (test >> 17) & 3
        zi = np.minimum(z, zmax).astype(np.uint32)
        keep = np.ones(len(gx), bool)
        if zte:
            cur = zb[gy, gx]
            if ztst == 0:
                keep[:] = False
            elif ztst == 2:
                keep &= zi >= cur
            elif ztst == 3:
                keep &= zi > cur
        rgba = col
        if tex is not None:
            uu, vv, q = tex
            t = self.sample(s, uu, vv, q, fst)
            if t is None:
                self.stats['draws_texture_unavailable'] += 1
                return
            tex0 = s['tex0']
            tcc, tfx = (tex0 >> 34) & 1, (tex0 >> 35) & 3
            t = np.floor(t)
            if tfx == 0:
                c = np.minimum(np.floor(col[:, :3] * t[:, :3] / 128), 255)
                a = np.minimum(np.floor(col[:, 3] * t[:, 3] / 128), 255) if tcc else col[:, 3]
            elif tfx == 1:
                c, a = t[:, :3], (t[:, 3] if tcc else col[:, 3])
            else:
                c = np.minimum(np.floor(col[:, :3] * t[:, :3] / 128) + col[:, 3:4], 255)
                a = (np.minimum(col[:, 3] + t[:, 3], 255) if tfx == 2 else t[:, 3]) if tcc else col[:, 3]
            rgba = np.concatenate([c, a[:, None]], 1)
        else:
            rgba = np.floor(col)
        a = rgba[:, 3]
        write_fb = np.ones(len(gx), bool)
        write_z = np.full(len(gx), not zmsk)
        if test & 1:
            atst, aref, afail = (test >> 1) & 7, (test >> 4) & 255, (test >> 12) & 3
            ok = [np.zeros(len(a), bool), np.ones(len(a), bool), a < aref, a <= aref, a == aref, a >= aref, a > aref, a != aref][atst]
            if afail == 0:
                keep &= ok
            elif afail == 1:
                write_z &= ok
            elif afail == 2:
                write_fb &= ok
            else:
                write_z &= ok
        cd = fb[gy, gx]
        if fpsm == 0x00:
            dr, dg, db, da = cd & 255, (cd >> 8) & 255, (cd >> 16) & 255, cd >> 24
            dabit = None
        else:
            dr, dg, db = (cd & 31) << 3, ((cd >> 5) & 31) << 3, ((cd >> 10) & 31) << 3
            dabit = (cd >> 15) & 1
            da = dabit * 0x80
        if (test >> 14) & 1:
            datm = (test >> 15) & 1
            bit = dabit if dabit is not None else (da >> 7) & 1
            keep &= bit == datm
        dc = np.stack([dr, dg, db], 1).astype(np.float64)
        sc = rgba[:, :3]
        out = sc.copy()
        if (prim >> 6) & 1:
            al = s['alpha']
            A, B, C, D, fix = al & 3, (al >> 2) & 3, (al >> 4) & 3, (al >> 6) & 3, (al >> 32) & 255
            sel = [sc, dc, np.zeros_like(sc)]
            cf = [a, da.astype(np.float64), np.full(len(a), float(fix))][C][:, None]
            blended = np.floor((sel[A] - sel[B]) * cf / 128) + sel[D]
            if self.r[0x49] & 1:
                use = a >= 128
                out = np.where(use[:, None], blended, sc)
            else:
                out = blended
        dthe = self.r[0x45] & 1
        if dthe and fpsm != 0x00:
            dimx = self.r[0x44]
            dv = np.array([[s3((dimx >> (16 * yy + 4 * xx)) & 7) for xx in range(4)] for yy in range(4)])
            out = out + dv[gy & 3, gx & 3][:, None]
        if self.r[0x46] & 1:
            out = np.clip(out, 0, 255)
        else:
            out = np.mod(out, 256)
        out = out.astype(np.uint32)
        abit = np.full(len(gx), 1 if (s['fba'] & 1) else 0, np.uint32) | (a.astype(np.uint32) >> 7 & 1)
        if fpsm == 0x00:
            av = np.where(s['fba'] & 1, a.astype(np.uint32) | 0x80, a.astype(np.uint32)) & 255
            new = out[:, 0] | out[:, 1] << 8 | out[:, 2] << 16 | av << 24
            mask = fbmsk & 0xffffffff
        else:
            new = (out[:, 0] >> 3) | (out[:, 1] >> 3) << 5 | (out[:, 2] >> 3) << 10 | (abit & 1) << 15
            m = fbmsk & 0xffffffff
            mask = ((m >> 3) & 31) | ((m >> 11) & 31) << 5 | ((m >> 19) & 31) << 10 | ((m >> 31) & 1) << 15
        sel = keep & write_fb
        merged = (cd & mask) | (new & ~np.uint32(mask) & (0xffffffff if fpsm == 0 else 0xffff))
        fb[gy[sel], gx[sel]] = merged[sel]
        selz = keep & write_z
        zb[gy[selz], gx[selz]] = zi[selz]
        self.stats['pixels'] += int(sel.sum())


def s3(v):
    return v - 8 if v & 4 else v


def lerp_rgba(w, v, k):
    return w[0] * v[0]['rgba'][k] + w[1] * v[1]['rgba'][k] + w[2] * v[2]['rgba'][k]


def gif(gs, stream, state):
    """Consume complete GIF tags of one path; `state` keeps a partial tag's remainder."""
    buf = state['buf'] + stream
    cursor = 0
    while True:
        if state.get('tag') is None:
            if cursor + 16 > len(buf):
                break
            low, layout = struct.unpack_from('<QQ', buf, cursor)
            loops, pre, prim, mode = low & 0x7fff, (low >> 46) & 1, (low >> 47) & 0x7ff, (low >> 58) & 3
            count = (low >> 60) & 15 or 16
            size = loops * count * 16 if mode == 0 else ((loops * count * 8 + 15) // 16) * 16 if mode == 1 else loops * 16
            if cursor + 16 + size > len(buf) and mode != 2:
                break
            cursor += 16
            if pre and mode == 0:
                gs.write(0x00, prim)
            gs.q = 1.0
            state['tag'] = (loops, count, layout, mode, size)
        loops, count, layout, mode, size = state['tag']
        if mode == 2:
            take = min(size, len(buf) - cursor)
            gs.image(buf[cursor:cursor + take])
            cursor += take
            if take < size:
                state['tag'] = (loops, count, layout, mode, size - take)
                break
            state['tag'] = None
            continue
        if mode == 1:
            for i in range(loops * count):
                reg = (layout >> ((i % count) * 4)) & 15
                value, = struct.unpack_from('<Q', buf, cursor + 8 * i)
                ad(gs, reg, value)
        else:
            for i in range(loops * count):
                reg = (layout >> ((i % count) * 4)) & 15
                lo, hi = struct.unpack_from('<QQ', buf, cursor + 16 * i)
                packed(gs, reg, lo, hi)
        cursor += size
        state['tag'] = None
    state['buf'] = buf[cursor:]


def ad(gs, reg, value):
    """REGLIST register data (the A+D data layout of register `reg`)."""
    if reg == 0x01:
        gs.rgbaq = value
        gs.q = f32(value >> 32)
    elif reg == 0x02:
        gs.st = (f32(value), f32(value >> 32))
    elif reg in (0x04, 0x05, 0x0c, 0x0d):
        xyz(gs, reg, value)
    elif reg not in (0x0e, 0x0f):
        gs.write(reg, value)


def xyz(gs, reg, value):
    x, y = value & 0xffff, (value >> 16) & 0xffff
    z = (value >> 32) & 0xffffff if reg in (0x04, 0x0c) else value >> 32
    gs.vertex(x, y, z, reg in (0x04, 0x05), False)


def packed(gs, reg, lo, hi):
    if reg == 0x0:
        gs.write(0x00, lo & 0x7ff)
    elif reg == 0x1:
        r, g, b, a = lo & 255, (lo >> 32) & 255, hi & 255, (hi >> 32) & 255
        gs.rgbaq = r | g << 8 | b << 16 | a << 24
    elif reg == 0x2:
        gs.st = (f32(lo), f32(lo >> 32))
        gs.q = f32(hi)
    elif reg == 0x3:
        gs.r[0x03] = (lo & 0x3fff) | ((lo >> 32) & 0x3fff) << 16
    elif reg == 0x4:
        gs.vertex(lo & 0xffff, (lo >> 32) & 0xffff, (hi >> 4) & 0xffffff, not (hi >> 47) & 1, (hi >> 47) & 1, (hi >> 36) & 255)
    elif reg == 0x5:
        gs.vertex(lo & 0xffff, (lo >> 32) & 0xffff, hi & 0xffffffff, not (hi >> 47) & 1, (hi >> 47) & 1)
    elif reg == 0xc:
        gs.vertex(lo & 0xffff, (lo >> 32) & 0xffff, (hi >> 4) & 0xffffff, False, (hi >> 47) & 1, (hi >> 36) & 255)
    elif reg == 0xd:
        gs.vertex(lo & 0xffff, (lo >> 32) & 0xffff, hi & 0xffffffff, False, (hi >> 47) & 1)
    elif reg in (0x6, 0x7, 0x8, 0x9):
        gs.write(reg, lo)
    elif reg == 0xa:
        pass
    elif reg == 0xe:
        a = hi & 255
        if a in (0x04, 0x05, 0x0c, 0x0d):
            xyz(gs, a, lo)
        elif a == 0x01:
            gs.rgbaq = lo
            gs.q = f32(lo >> 32)
        elif a == 0x02:
            gs.st = (f32(lo), f32(lo >> 32))
        else:
            gs.write(a, lo)
    elif reg == 0xf:
        pass
    else:
        gs.stats[f'packed_reg_{reg}_unmodelled'] += 1


def to_rgb(fb, psm):
    if psm == 0x00:
        return np.stack([fb & 255, (fb >> 8) & 255, (fb >> 16) & 255], -1).astype(np.uint8)
    return np.stack([(fb & 31) << 3, ((fb >> 5) & 31) << 3, ((fb >> 10) & 31) << 3], -1).astype(np.uint8)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('dump', type=pathlib.Path)
    ap.add_argument('prefix')
    ap.add_argument('--window', type=int, nargs=4, default=[0, 0, W, H])
    ap.add_argument('--skip', default='')
    ap.add_argument('--stats-window', type=int, nargs=4, help='statistics region (default: the window)')
    a = ap.parse_args()
    prefix = pathlib.Path(a.prefix)
    if not prefix.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('outputs go under private/')
    out_json = prefix.with_name(prefix.name + '.json')
    if out_json.exists():
        raise ValueError(f'{out_json} exists')
    raw = compression.zstd.ZstdDecompressor().decompress(a.dump.read_bytes(), max_length=64 * 1024 * 1024 + 1)
    parse(raw)
    size, = struct.unpack_from('<I', raw, 4)
    state_size, = struct.unpack_from('<I', raw, 12)
    state = raw[8 + size:8 + size + state_size]
    mem = state[VRAM_OFFSET:VRAM_OFFSET + VRAM_SIZE]
    skip = {int(x) for x in a.skip.split(',') if x}
    gs = GS(mem, a.window, skip)
    cursor = 8 + size + state_size + 8192
    paths = {p: dict(buf=b'', tag=None) for p in range(4)}
    frames = []
    from PIL import Image
    sw = a.stats_window or a.window
    while cursor < len(raw):
        kind = raw[cursor]
        cursor += 1
        if kind == 0:
            path = raw[cursor]
            n, = struct.unpack_from('<I', raw, cursor + 1)
            cursor += 5
            gif(gs, raw[cursor:cursor + n], paths[path])
            cursor += n
        elif kind == 1:
            cursor += 1
            if gs.fb_use:
                (fbp, fpsm), _ = gs.fb_use.most_common(1)[0]
                img = to_rgb(gs.frames[(fbp, fpsm)], fpsm)
                k = len(frames)
                png = prefix.with_name(f'{prefix.name}-frame{k}.png')
                if png.exists():
                    raise ValueError(f'{png} exists')
                Image.fromarray(img).save(png)
                reg = img[sw[1]:sw[3], sw[0]:sw[2]].astype(np.int64)
                summed = reg.sum(-1).reshape(-1)
                frames.append(dict(frame=k, fbp=fbp, psm=fpsm, png=str(png), stats=dict(gs.stats), fb_use={f'{p}:{q}': c for (p, q), c in gs.fb_use.items()},
                                   unavailable=[[t, p, lv, c] for (t, p, lv), c in gs.unavailable.most_common(20)],
                                   region=sw, mean_rgb=[round(float(reg[..., i].mean()), 3) for i in range(3)],
                                   summed_p99=float(np.percentile(summed, 99)), summed_p50=float(np.percentile(summed, 50))))
                gs.stats = collections.Counter()
                gs.fb_use = collections.Counter()
                gs.unavailable = collections.Counter()
        elif kind == 2:
            cursor += 4
        elif kind == 3:
            cursor += 8192
    out_json.write_text(json.dumps(dict(dump=str(a.dump), window=a.window, skip=sorted(skip), frames=frames,
                                        scope='research software replay; approximations listed in tools/python/gs_replay.py'), indent=1) + '\n')
    for f in frames:
        print(f['frame'], f['fbp'], f['mean_rgb'], f['summed_p99'], dict(f['stats']))


if __name__ == '__main__':
    main()
