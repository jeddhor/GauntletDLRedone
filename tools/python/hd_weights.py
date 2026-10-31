#!/usr/bin/env python3
"""The HD texture generator's network weights (graphics plan step 8).

  hd_weights.py WEIGHTS_DIR OUT.bin [--model general-dn0.5 | anime]

Reads Real-ESRGAN v0.3.0's released PyTorch checkpoints (BSD-3-Clause,
Copyright (c) 2021 Xintao Wang) with a restricted unpickler (no torch) and
writes the SRVGGNetCompact parameters gdl_client::hd::esrgan loads:

  general-dn0.5  realesr-general-x4v3.pth and realesr-general-wdn-x4v3.pth
                 mixed 0.5 / 0.5 in float32 (inference_realesrgan.py's
                 default --denoise_strength 0.5, utils.py dni; params_ema
                 when the checkpoint has it), num_conv 32 — the user's
                 choice at Checkpoint B;
  anime          realesr-animevideov3.pth, num_conv 16 (only for checking
                 the GPU port against the upstream ncnn program).

Format (little-endian): b"GDLSRVGG", u32 version 1, u32 num_conv, u32
num_feat, u32 scale, then per layer in order body.0 … body.(2·num_conv+2):
a convolution's weight (out, in, 3, 3) and bias (out) as f32, a PReLU's
weight (num_feat) as f32. OUT.json beside it records the sources' SHA-256,
the mixing and the output's SHA-256.
"""
import argparse
import hashlib
import json
import pathlib
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from upscale_compare import load_pth  # noqa: E402

SOURCES = {
    'general-dn0.5': (['realesr-general-x4v3.pth', 'realesr-general-wdn-x4v3.pth'], 32),
    'anime': (['realesr-animevideov3.pth'], 16),
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('weights')
    ap.add_argument('out')
    ap.add_argument('--model', choices=sorted(SOURCES), default='general-dn0.5')
    args = ap.parse_args()
    names, num_conv = SOURCES[args.model]
    paths = [pathlib.Path(args.weights) / n for n in names]
    nets = [load_pth(p) for p in paths]
    net = nets[0]
    if len(nets) == 2:
        # utils.py dni: net_a['params'][k] = w0 * a + w1 * b, dni_weight [0.5, 0.5].
        for k, v in net['params'].items():
            net['params'][k] = (np.float32(0.5) * v + np.float32(0.5) * nets[1]['params'][k]).astype(np.float32)
    sd = net['params_ema'] if 'params_ema' in net else net['params']
    out = bytearray(b'GDLSRVGG' + struct.pack('<4I', 1, num_conv, 64, 4))
    shapes = []
    for i in range(2 * num_conv + 3):
        if i % 2 == 0:
            w, b = sd[f'body.{i}.weight'].astype('<f4'), sd[f'body.{i}.bias'].astype('<f4')
            assert w.shape[2:] == (3, 3), w.shape
            out += w.tobytes() + b.tobytes()
            shapes.append(['conv', list(w.shape)])
        else:
            a = sd[f'body.{i}.weight'].astype('<f4')
            out += a.tobytes()
            shapes.append(['prelu', list(a.shape)])
    dst = pathlib.Path(args.out)
    dst.write_bytes(bytes(out))
    meta = {'model': args.model, 'format': 'GDLSRVGG v1 (tools/python/hd_weights.py)', 'num_conv': num_conv, 'num_feat': 64, 'scale': 4,
            'sources': {n: hashlib.sha256(p.read_bytes()).hexdigest() for n, p in zip(names, paths)},
            'mix': '0.5 / 0.5 float32 (Real-ESRGAN utils.py dni; inference_realesrgan.py --denoise_strength 0.5)' if len(nets) == 2 else None,
            'state_dict': 'params_ema' if 'params_ema' in net else 'params',
            'upstream': 'Real-ESRGAN v0.3.0 release assets v0.2.5.0 (BSD-3-Clause, Copyright (c) 2021, Xintao Wang)',
            'layers': shapes, 'bytes': len(out), 'sha256': hashlib.sha256(out).hexdigest()}
    dst.with_suffix('.json').write_text(json.dumps(meta, indent=1) + '\n')
    print(dst, len(out), meta['sha256'])


if __name__ == '__main__':
    main()
