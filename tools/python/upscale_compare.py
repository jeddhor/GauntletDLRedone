#!/usr/bin/env python3
"""Texture upscaler comparison (graphics plan step 7, Checkpoint B).

  upscale_compare.py pack METHOD OUT_DIR DUMP_DIR [DUMP_DIR ...] [--jobs N]
  upscale_compare.py sheet OUT PICKS.tsv RUN_DIR PACK_DIR [PACK_DIR ...] [--rows N]
  upscale_compare.py ncnn-check DUMP_DIR NCNN_DIR WORK_DIR STEM [STEM ...]
  upscale_compare.py page RUN_DIR

Research tooling for a NATIVE ENHANCEMENT experiment: nothing here ships,
and its outputs (upscaled retail textures) stay in private/.

`pack` upscales every texture of GDL_TEXTURE_DUMP directories
(<stem>.png and index.tsv: stem, TEXDEF label, size, record flags) 4× into
OUT_DIR as a texture pack (Settings::texture_pack), with OUT_DIR/
manifest.json (method, parameters, tool hashes, per-texture seconds).
METHOD:
  nearest         pixel replication (control: the pack path, no new detail)
  catmull         Catmull-Rom (Pillow BICUBIC: Keys cubic, a = -0.5)
  xbrz            xBRZ 1.9 (private/upstream/upscalers/xbrz, GPL-3.0),
                  scale 4, default ScalerCfg, ARGB mode
  esrgan-general  Real-ESRGAN realesr-general-x4v3, mixed 0.5/0.5 with
                  realesr-general-wdn-x4v3 (inference_realesrgan.py's
                  default --denoise_strength 0.5; utils.py dni)
  esrgan-general-dn1  realesr-general-x4v3 alone (--denoise_strength 1:
                  the strongest denoising; dni is skipped)
  esrgan-anime    Real-ESRGAN realesr-animevideov3
Real-ESRGAN v0.3.0 (BSD-3-Clause; private/upstream/upscalers) runs here as
float32 numpy (its SRVGGNetCompact: conv3×3 + PReLU, num_conv 32 / 16,
PixelShuffle 4, plus the nearest-upsampled input), the alpha channel
through the network as grey (the default --alpha_upsampler realesrgan),
output clamped to 0..1 and rounded; `ncnn-check` compares it with the
upstream realesrgan-ncnn-vulkan program (animevideov3-x4).

Our choices, the same for every method (not upstream behaviour):
  - edges: each axis is padded before scaling and cropped after, wrapped
    where the record repeats and replicated where it clamps (flags
    0x4 / 0x8, texfilter::sampler), so tiling surfaces stay seamless;
  - fully transparent texels (cut-outs: alpha test rejected; blended:
    alpha 0) take their colour from the nearest kept texels first, so no
    dark fringes grow into the upscaled edges;
  - alpha classes stay as the client reads them (lib.rs materials_for):
    opaque stays opaque, a cut-out's alpha is thresholded at half, blended
    alpha is upscaled with the colour.

`page` writes RUN_DIR/index.html, a local viewer (never published: the
images are retail-derived) of the in-game frames RUN_DIR/scenes.json
lists, switchable between packs at native size, the sheets
RUN_DIR/sheet-*.png and the RUN_DIR/numbers.json table.

`sheet` draws PICKS.tsv rows (level, TEXDEF label, category; the level's
texture from RUN_DIR/dump-<level>) with one column per pack (the
original's texels repeated 4× first) on a checkerboard, ROWS rows per
page (OUT-1.png, OUT-2.png, ...).
"""
import argparse
import collections
import hashlib
import io
import json
import os
import pathlib
import pickle
import subprocess
import sys
import time
import zipfile

os.environ.setdefault('OPENBLAS_NUM_THREADS', '1')
os.environ.setdefault('OMP_NUM_THREADS', '1')
import numpy as np  # noqa: E402
from PIL import Image, ImageDraw, ImageFont  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
UP = ROOT / 'private/upstream/upscalers'
WEIGHTS = UP / 'realesrgan-weights'
XBRZ_SRC = UP / 'xbrz/src'
XBRZ_BIN = UP / 'xbrz/xbrz_cli'
SCALE = 4
PAD = 12

XBRZ_CLI = r'''
// xBRZ command line for upscale_compare.py: stdin "W H" + W*H*4 bytes
// (B, G, R, A per pixel: xbrz ARGB on little-endian), stdout the 4x image.
#include "xbrz.h"
#include <cstdio>
#include <vector>
int main() {
    int w = 0, h = 0;
    if (std::scanf("%d %d", &w, &h) != 2 || w <= 0 || h <= 0 || w > 4096 || h > 4096) return 2;
    std::getchar();
    std::vector<uint32_t> src(size_t(w) * h), trg(size_t(w) * h * 16);
    if (std::fread(src.data(), 4, src.size(), stdin) != src.size()) return 3;
    xbrz::scale(4, src.data(), trg.data(), w, h, xbrz::ColorFormat::argb);
    std::fwrite(trg.data(), 4, trg.size(), stdout);
    return 0;
}
'''


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


# ---------------------------------------------------------------- Real-ESRGAN

def load_pth(path):
    """A PyTorch zip checkpoint's tensors as numpy arrays (no torch; a
    restricted unpickler that only rebuilds float tensors and dicts)."""
    z = zipfile.ZipFile(path)
    pkl = next(n for n in z.namelist() if n.endswith('/data.pkl'))
    prefix = pkl[:-len('data.pkl')]
    dtypes = {'FloatStorage': np.float32, 'HalfStorage': np.float16, 'DoubleStorage': np.float64}
    cache = {}

    def rebuild(storage, offset, size, stride, *_):
        item = storage.itemsize
        return np.lib.stride_tricks.as_strided(storage[offset:], shape=tuple(size), strides=[s * item for s in stride]).copy()

    class Unpickler(pickle.Unpickler):
        def find_class(self, module, name):
            if (module, name) == ('torch._utils', '_rebuild_tensor_v2'):
                return rebuild
            if (module, name) == ('collections', 'OrderedDict'):
                return collections.OrderedDict
            if module == 'torch' and name in dtypes:
                return dtypes[name]
            raise pickle.UnpicklingError(f'refused {module}.{name}')

        def persistent_load(self, pid):
            kind, dtype, key, _location, _numel = pid
            if kind != 'storage':
                raise pickle.UnpicklingError(f'refused storage {kind}')
            if key not in cache:
                cache[key] = np.frombuffer(z.read(f'{prefix}data/{key}'), dtype=dtype)
            return cache[key]

    return Unpickler(io.BytesIO(z.read(pkl))).load()


def esrgan_params(method):
    """(layers, num_conv): the state dict upstream's RealESRGANer loads."""
    if method == 'esrgan-anime':
        net = load_pth(WEIGHTS / 'realesr-animevideov3.pth')
        num_conv = 16
    elif method == 'esrgan-general-dn1':
        net = load_pth(WEIGHTS / 'realesr-general-x4v3.pth')
        num_conv = 32
    else:
        # utils.py dni: net_a['params'][k] = w0 * a + w1 * b with
        # dni_weight [0.5, 0.5]; RealESRGANer then prefers 'params_ema'
        # when the mixed checkpoint has it.
        a = load_pth(WEIGHTS / 'realesr-general-x4v3.pth')
        b = load_pth(WEIGHTS / 'realesr-general-wdn-x4v3.pth')
        for k, v in a['params'].items():
            a['params'][k] = (np.float32(0.5) * v + np.float32(0.5) * b['params'][k]).astype(np.float32)
        net = a
        num_conv = 32
    sd = net['params_ema'] if 'params_ema' in net else net['params']
    layers = []
    for i in range(2 * num_conv + 3):
        if i % 2 == 0:
            layers.append(('conv', sd[f'body.{i}.weight'].astype(np.float32), sd[f'body.{i}.bias'].astype(np.float32)))
        else:
            layers.append(('prelu', sd[f'body.{i}.weight'].astype(np.float32), None))
    return layers, sorted(net.keys())


def conv3(x, w, b):
    c, h, wd = x.shape
    xp = np.pad(x, ((0, 0), (1, 1), (1, 1)))
    cols = np.empty((c, 9, h, wd), np.float32)
    for dy in range(3):
        for dx in range(3):
            cols[:, dy * 3 + dx] = xp[:, dy:dy + h, dx:dx + wd]
    out = w.reshape(w.shape[0], c * 9) @ cols.reshape(c * 9, h * wd)
    out += b[:, None]
    return out.reshape(-1, h, wd)


def srvgg(x, layers):
    """SRVGGNetCompact.forward for one (3, H, W) float32 image."""
    out = x
    for kind, w, b in layers:
        if kind == 'conv':
            out = conv3(out, w, b)
        else:
            out = np.where(out >= 0, out, w[:, None, None] * out)
    c, h, wd = out.shape
    r = SCALE
    out = out.reshape(c // (r * r), r, r, h, wd).transpose(0, 3, 1, 4, 2).reshape(c // (r * r), h * r, wd * r)
    return out + x.repeat(r, 1).repeat(r, 2)


_LAYERS = None


def esrgan_rgb(rgb, layers):
    """(H, W, 3) 0..1 → (4H, 4W, 3) 0..1 (clamped, as post_process)."""
    out = srvgg(np.ascontiguousarray(rgb.transpose(2, 0, 1), np.float32), layers)
    return np.clip(out, 0, 1).transpose(1, 2, 0)


# ---------------------------------------------------------------- methods

def build_xbrz():
    if XBRZ_BIN.exists():
        return
    src = XBRZ_BIN.with_suffix('.cpp')
    src.write_text(XBRZ_CLI)
    subprocess.run(['g++', '-O2', '-std=c++20', '-I', str(XBRZ_SRC), str(src), str(XBRZ_SRC / 'xbrz.cpp'), '-o', str(XBRZ_BIN), '-lpthread'], check=True)


def scale_xbrz(rgba8):
    h, w, _ = rgba8.shape
    bgra = rgba8[:, :, [2, 1, 0, 3]].tobytes()
    out = subprocess.run([str(XBRZ_BIN)], input=f'{w} {h}\n'.encode() + bgra, capture_output=True, check=True).stdout
    return np.frombuffer(out, np.uint8).reshape(h * SCALE, w * SCALE, 4)[:, :, [2, 1, 0, 3]]


def scale_catmull(chan):
    h, w = chan.shape
    im = Image.fromarray(chan.astype(np.float32), 'F').resize((w * SCALE, h * SCALE), Image.Resampling.BICUBIC)
    return np.asarray(im, np.float32)


def fill_clear(rgb, kept, wrap_u, wrap_v):
    """Colours of texels outside `kept` from the mean of kept 8-neighbours,
    repeated outward until every texel has one."""
    rgb = rgb.copy()
    have = kept.copy()
    if not have.any():
        return rgb
    while not have.all():
        acc = np.zeros_like(rgb)
        cnt = np.zeros(have.shape, np.float32)
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dy == 0 and dx == 0:
                    continue
                hv = shift(have.astype(np.float32), dy, dx, wrap_u, wrap_v)
                acc += shift(rgb * have[:, :, None], dy, dx, wrap_u, wrap_v) * 1.0
                cnt += hv
        new = (~have) & (cnt > 0)
        rgb[new] = acc[new] / cnt[new][:, None]
        have |= new
    return rgb


def shift(a, dy, dx, wrap_u, wrap_v):
    """a moved by (dy, dx): wrapped or edge-replicated per axis."""
    p = pad_axes(a, wrap_u, wrap_v, 1)
    h, w = a.shape[:2]
    return p[1 - dy:1 - dy + h, 1 - dx:1 - dx + w]


def pad_axes(a, wrap_u, wrap_v, n=PAD):
    extra = [(0, 0)] * (a.ndim - 2)
    a = np.pad(a, [(n, n), (0, 0)] + extra, mode='wrap' if wrap_v else 'edge')
    return np.pad(a, [(0, 0), (n, n)] + extra, mode='wrap' if wrap_u else 'edge')


def upscale(method, rgba8, flags):
    """One dumped texture (H, W, 4; alpha 0..255 = GS alpha doubled) → the
    4× RGBA8 pack image."""
    wrap_u, wrap_v = not flags & 0x4, not flags & 0x8
    a = rgba8[:, :, 3]
    opaque = bool((a == 255).all())
    binary = bool(((a <= 2) | (a == 255)).all())
    rgb = rgba8[:, :, :3].astype(np.float32) / 255
    if not opaque:
        rgb = fill_clear(rgb, a > 2 if binary else a > 0, wrap_u, wrap_v)
    alpha = a.astype(np.float32) / 255
    h, w = a.shape
    prgb, palpha = pad_axes(rgb, wrap_u, wrap_v), pad_axes(alpha, wrap_u, wrap_v)
    if method == 'nearest':
        out_rgb = prgb.repeat(SCALE, 0).repeat(SCALE, 1)
        out_a = palpha.repeat(SCALE, 0).repeat(SCALE, 1)
    elif method == 'catmull':
        out_rgb = np.stack([scale_catmull(prgb[:, :, c]) for c in range(3)], 2)
        out_a = scale_catmull(palpha)
    elif method == 'xbrz':
        q = np.concatenate([np.round(prgb * 255), np.round(palpha * 255)[:, :, None]], 2).astype(np.uint8)
        o = scale_xbrz(q).astype(np.float32) / 255
        out_rgb, out_a = o[:, :, :3], o[:, :, 3]
    else:
        out_rgb = esrgan_rgb(prgb, _LAYERS)
        if opaque:
            out_a = np.ones(out_rgb.shape[:2], np.float32)
        else:
            g = esrgan_rgb(np.repeat(palpha[:, :, None], 3, 2), _LAYERS)
            out_a = 0.299 * g[:, :, 0] + 0.587 * g[:, :, 1] + 0.114 * g[:, :, 2]
    c = PAD * SCALE
    out_rgb, out_a = out_rgb[c:c + h * SCALE, c:c + w * SCALE], out_a[c:c + h * SCALE, c:c + w * SCALE]
    if opaque:
        out_a = np.ones_like(out_a)
    elif binary:
        out_a = (out_a >= 0.5).astype(np.float32)
    rgb8 = np.round(np.clip(out_rgb, 0, 1) * 255).astype(np.uint8)
    a8 = np.round(np.clip(out_a, 0, 1) * 255).astype(np.uint8)
    return np.concatenate([rgb8, a8[:, :, None]], 2)


def read_index(dumps):
    rows = {}
    for d in dumps:
        for line in (pathlib.Path(d) / 'index.tsv').read_text().splitlines():
            stem, label, size, flags = line.split('\t')
            rows.setdefault(stem, {'stem': stem, 'label': label, 'size': size, 'flags': int(flags, 16), 'dump': str(d)})
    return rows


def _init(method):
    global _LAYERS
    if method.startswith('esrgan'):
        _LAYERS = esrgan_params(method)[0]


def _work(job):
    method, src, dst, flags = job
    t = time.perf_counter()
    rgba = np.asarray(Image.open(src).convert('RGBA'))
    out = upscale(method, rgba, flags)
    tmp = pathlib.Path(dst).with_suffix('.png.tmp')
    Image.fromarray(out, 'RGBA').save(tmp, format='PNG', compress_level=1)
    os.replace(tmp, dst)
    return pathlib.Path(dst).stem, time.perf_counter() - t


def cmd_pack(args):
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    rows = read_index(args.dumps)
    if args.method == 'xbrz':
        build_xbrz()
    jobs = [(args.method, str(pathlib.Path(r['dump']) / f'{s}.png'), str(out / f'{s}.png'), r['flags'])
            for s, r in sorted(rows.items()) if not (out / f'{s}.png').exists()]
    import multiprocessing
    t0 = time.perf_counter()
    times = {}
    with multiprocessing.Pool(args.jobs, initializer=_init, initargs=(args.method,)) as pool:
        for stem, sec in pool.imap_unordered(_work, jobs, chunksize=4):
            times[stem] = round(sec, 4)
    tools = {}
    if args.method == 'xbrz':
        tools = {'xBRZ_1.9.zip': sha256(UP / 'xbrz/xBRZ_1.9.zip')}
    elif args.method == 'esrgan-anime':
        tools = {'realesr-animevideov3.pth': sha256(WEIGHTS / 'realesr-animevideov3.pth')}
    elif args.method == 'esrgan-general':
        tools = {n: sha256(WEIGHTS / n) for n in ('realesr-general-x4v3.pth', 'realesr-general-wdn-x4v3.pth')}
    elif args.method == 'esrgan-general-dn1':
        tools = {'realesr-general-x4v3.pth': sha256(WEIGHTS / 'realesr-general-x4v3.pth')}
    manifest = {'method': args.method, 'scale': SCALE, 'pad': PAD, 'tools': tools, 'textures': len(rows), 'new': len(jobs),
                'cpu_jobs': args.jobs, 'wall_seconds': round(time.perf_counter() - t0, 1), 'texture_seconds': times,
                'dumps': [str(d) for d in args.dumps]}
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=1))
    print(args.method, len(jobs), 'textures in', manifest['wall_seconds'], 's')


# ---------------------------------------------------------------- sheet

def font(size):
    for p in ('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', '/usr/share/fonts/TTF/DejaVuSans.ttf'):
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    return ImageFont.load_default()


def checker(w, h, n=8):
    y, x = np.mgrid[0:h, 0:w]
    v = np.where(((x // n) + (y // n)) % 2 == 0, 200, 150).astype(np.uint8)
    return np.stack([v, v, v], 2)


def over(rgba):
    a = rgba[:, :, 3:4].astype(np.float32) / 255
    bg = checker(rgba.shape[1], rgba.shape[0]).astype(np.float32)
    return (rgba[:, :, :3] * a + bg * (1 - a)).astype(np.uint8)


def cmd_sheet(args):
    run = pathlib.Path(args.run)
    picks = []
    for line in pathlib.Path(args.picks).read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        level, label, cat = line.split('\t')[:3]
        rows = [r for r in read_index([run / f'dump-{level}']).values() if r['label'] == label]
        if not rows:
            sys.exit(f'{level} {label}: not in the dump')
        r = max(rows, key=lambda r: eval(r['size'].replace('x', '*')))
        picks.append(dict(r, level=level, cat=cat))
    packs = [pathlib.Path(p) for p in args.packs]
    names = ['original (texels ×4)'] + [json.loads((p / 'manifest.json').read_text())['method'] for p in packs]
    cell, head, left = args.cell, 28, 230
    f, small = font(16), font(12)
    for page in range(0, len(picks), args.rows):
        part = picks[page:page + args.rows]
        sheet = Image.new('RGB', (left + cell * len(names), head + cell * len(part)), (24, 24, 24))
        d = ImageDraw.Draw(sheet)
        for i, n in enumerate(names):
            d.text((left + i * cell + 6, 6), n, fill=(255, 255, 255), font=f)
        for j, r in enumerate(part):
            stem = r['stem']
            y = head + j * cell
            d.text((6, y + 6), r['cat'], fill=(255, 220, 120), font=f)
            d.text((6, y + 28), r['label'], fill=(255, 255, 255), font=small)
            d.text((6, y + 44), f"{r['level']}  {r['size']}  flags {r['flags']:#x}", fill=(180, 180, 180), font=small)
            orig = np.asarray(Image.open(pathlib.Path(r['dump']) / f'{stem}.png').convert('RGBA'))
            imgs = [orig.repeat(SCALE, 0).repeat(SCALE, 1)] + [np.asarray(Image.open(p / f'{stem}.png').convert('RGBA')) for p in packs]
            for i, im in enumerate(imgs):
                h, w = im.shape[:2]
                # The 4× image 1:1 when it fits, else its centre crop;
                # small ones repeated up to at least half the cell.
                k = max(1, (cell // 2) // max(h, w))
                if k > 1:
                    im = im.repeat(k, 0).repeat(k, 1)
                    h, w = im.shape[:2]
                y0, x0 = max(0, (h - cell) // 2), max(0, (w - cell) // 2)
                im = im[y0:y0 + cell, x0:x0 + cell]
                sheet.paste(Image.fromarray(over(im)), (left + i * cell, y))
        out = f'{args.out}-{page // args.rows + 1}.png'
        sheet.save(out)
        print(out, sheet.size)


PAGE = """<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Upscaler comparison</title>
<style>
:root{--bg:#161616;--fg:#eee;--dim:#aaa;--acc:#ffd27a;--btn:#2a2a2a;--on:#4a6aa0}
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,sans-serif}
h1{font-size:22px;margin:0 0 4px}h2{font-size:18px;margin:28px 0 8px;color:var(--acc)}
p{max-width:70em;color:var(--dim)}
.bar{display:flex;flex-wrap:wrap;gap:6px;margin:8px 0}
button{background:var(--btn);color:var(--fg);border:1px solid #444;border-radius:6px;padding:6px 10px;font:inherit;cursor:pointer}
button.on{background:var(--on);border-color:#8ab}
.view{overflow:auto;max-height:78vh;border:1px solid #333;background:#000}
.view img{display:block;image-rendering:pixelated}
.view.fit img{width:100%;height:auto}
table{border-collapse:collapse;margin:8px 0}td,th{border:1px solid #333;padding:4px 10px;text-align:right}th{background:#222}
td:first-child,th:first-child{text-align:left}
.sheet{max-width:100%;border:1px solid #333;margin:6px 0}
</style></head><body>
<h1>Upscaler comparison (Checkpoint B)</h1>
<p>Every texture of the Tower, G1, C2 and J5 upscaled 4\u00d7 by each method and loaded as a texture pack,
Enhanced settings, 1920\u00d71080, frozen frames on the RX 580. Number keys switch the method in the scene you last
clicked; the frame keeps its scroll position, so flip back and forth on one spot. \u201cFit\u201d shows the whole frame.</p>
<div id="scenes"></div>
<h2>Texture sheets</h2>
<p>Columns: the original texels (each repeated 4\u00d7), then each method\u2019s 4\u00d7 texture, 1:1 (centre crop when larger than the cell).</p>
<div id="sheets"></div>
<h2>Numbers</h2><div id="numbers"></div>
<script>
const DATA = __DATA__;
let active = null;
function scene(sc) {
  const box = document.createElement('section');
  box.innerHTML = `<h2>${sc.title}</h2>`;
  const bar = document.createElement('div'); bar.className = 'bar';
  const view = document.createElement('div'); view.className = 'view';
  const img = document.createElement('img'); view.appendChild(img);
  const buttons = DATA.methods.map((m, i) => {
    const b = document.createElement('button'); b.textContent = `${i + 1} ${m.name}`;
    b.onclick = () => { set(i); active = set; }; bar.appendChild(b); return b; });
  const fit = document.createElement('button'); fit.textContent = 'Fit';
  fit.onclick = () => { view.classList.toggle('fit'); fit.classList.toggle('on'); }; bar.appendChild(fit);
  function set(i) { img.src = sc.frames[DATA.methods[i].key]; buttons.forEach((b, k) => b.classList.toggle('on', k === i)); }
  view.onclick = () => { active = set; };
  set(0); box.appendChild(bar); box.appendChild(view); return box;
}
const sc = document.getElementById('scenes'); DATA.scenes.forEach(s => sc.appendChild(scene(s)));
document.addEventListener('keydown', e => { const i = Number(e.key) - 1; if (active && i >= 0 && i < DATA.methods.length) active(i); });
const sh = document.getElementById('sheets');
DATA.sheets.forEach(p => { const i = document.createElement('img'); i.src = p; i.className = 'sheet'; i.loading = 'lazy'; sh.appendChild(i); });
const n = DATA.numbers; if (n) { const t = document.createElement('table');
  t.innerHTML = '<tr>' + n.head.map(h => `<th>${h}</th>`).join('') + '</tr>' + n.rows.map(r => '<tr>' + r.map(c => `<td>${c}</td>`).join('') + '</tr>').join('');
  document.getElementById('numbers').appendChild(t);
  if (n.notes) n.notes.forEach(x => { const p = document.createElement('p'); p.textContent = x; document.getElementById('numbers').appendChild(p); }); }
</script></body></html>
"""


def cmd_page(args):
    run = pathlib.Path(args.run)
    data = json.loads((run / 'scenes.json').read_text())
    data['sheets'] = sorted((p.name for p in run.glob('sheet-[0-9]*.png')), key=lambda n: int(n[6:-4]))
    numbers = run / 'numbers.json'
    data['numbers'] = json.loads(numbers.read_text()) if numbers.exists() else None
    (run / 'index.html').write_text(PAGE.replace('__DATA__', json.dumps(data)))
    print(run / 'index.html')


# ---------------------------------------------------------------- ncnn check

def cmd_ncnn(args):
    """Our animevideov3 against upstream realesrgan-ncnn-vulkan (opaque
    RGB textures). ncnn pads each tile by 10 mirrored texels (its output
    matches ours padded that way: interior and borders alike)."""
    global _LAYERS
    _LAYERS = esrgan_params('esrgan-anime')[0]
    rows = read_index([args.dump])
    ncnn = pathlib.Path(args.ncnn)
    tmp = pathlib.Path(args.work)
    tmp.mkdir(parents=True, exist_ok=True)
    for stem in args.stems:
        r = rows[stem]
        rgb = np.asarray(Image.open(pathlib.Path(r['dump']) / f'{stem}.png').convert('RGB'))
        Image.fromarray(rgb).save(tmp / f'{stem}-in.png')
        subprocess.run([str(ncnn / 'realesrgan-ncnn-vulkan'), '-i', str(tmp / f'{stem}-in.png'), '-o', str(tmp / f'{stem}-ncnn.png'),
                        '-n', 'realesr-animevideov3', '-s', '4', '-m', str(ncnn / 'models')], check=True, capture_output=True)
        theirs = np.asarray(Image.open(tmp / f'{stem}-ncnn.png').convert('RGB')).astype(np.float32)
        x = np.pad(rgb.astype(np.float32) / 255, ((10, 10), (10, 10), (0, 0)), mode='reflect')
        ours = np.round(esrgan_rgb(x, _LAYERS) * 255)[40:-40, 40:-40]
        diff = np.abs(theirs - ours)
        print(stem, r['label'], r['size'], f'mean {diff.mean():.3f}/255  max {diff.max():.0f}  ≥2: {(diff >= 2).mean() * 100:.2f} %')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('pack')
    p.add_argument('method', choices=['nearest', 'catmull', 'xbrz', 'esrgan-general', 'esrgan-general-dn1', 'esrgan-anime'])
    p.add_argument('out')
    p.add_argument('dumps', nargs='+')
    p.add_argument('--jobs', type=int, default=os.cpu_count())
    p = sub.add_parser('sheet')
    p.add_argument('out')
    p.add_argument('picks')
    p.add_argument('run')
    p.add_argument('packs', nargs='+')
    p.add_argument('--cell', type=int, default=256)
    p.add_argument('--rows', type=int, default=8)
    p = sub.add_parser('ncnn-check')
    p.add_argument('dump')
    p.add_argument('ncnn')
    p.add_argument('work')
    p.add_argument('stems', nargs='+')
    p = sub.add_parser('page')
    p.add_argument('run')
    args = ap.parse_args()
    {'pack': cmd_pack, 'sheet': cmd_sheet, 'ncnn-check': cmd_ncnn, 'page': cmd_page}[args.cmd](args)


if __name__ == '__main__':
    main()
