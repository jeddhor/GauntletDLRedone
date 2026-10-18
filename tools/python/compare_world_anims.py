#!/usr/bin/env python3
"""Compare native world-animation matrices with a capture's node matrices.

Usage: compare_world_anims.py FIXTURE.p2s WORLDS-LABEL NEW_REPORT.json

The capture's matrix was built from the frame before the last advance
(DoWorldAnimSub builds, then advances by SPF*30), so the native evaluation
uses frame - SPF*30 (forward) / + SPF*30 (reverse) and skips entries that
wrapped this update."""
#!/usr/bin/env python3
import sys, struct, json, pathlib, subprocess
sys.path.insert(0, 'tools/python')
from reference_snapshot import frozen_memory
from run_play import resolve
fixture, label, out = sys.argv[1], sys.argv[2], sys.argv[3]
m = frozen_memory(pathlib.Path(fixture))
u = lambda a: struct.unpack_from('<I', m, a)[0]
f = lambda a: struct.unpack_from('<f', m, a)[0]
spf = f(0x20f908)
wi = 0x21f358
objs, ents, n = u(wi + 4), u(wi + 0x8c), u(wi + 0x90)
req, ram = [], []
for k in range(n):
    e = ents + 16 * k
    obj, frames = struct.unpack_from('<hh', m, e)
    flags = struct.unpack_from('<H', m, e + 6)[0]
    fr = f(e + 8)
    if not flags & 1 or struct.unpack_from('<i', m, e + 12)[0] == 0:
        continue
    # Resting at an end (non-looping): built at the clamped frame itself.
    if flags & 2 and fr == 0.0 and not flags & 0x100:
        prev = 0.0
    elif not flags & 2 and fr == frames - 1 and not flags & 0x100:
        prev = fr
    else:
        prev = fr - spf * 30 if not flags & 2 else fr + spf * 30
    if prev < 0 or prev > frames - 1:
        continue
    node = u(objs + 0x3c * obj + 0x28)
    mat = struct.unpack_from('<16f', m, node)
    req.append([k, prev]); ram.append((k, obj, flags, fr, mat))
tmp = pathlib.Path(out).with_suffix('.frames.json'); tmp.write_text(json.dumps(req))
worlds, = resolve([label])
nat = json.loads(subprocess.run(['target/release/gdl_tools', 'world-anim-eval', str(worlds), str(tmp)], capture_output=True, text=True, check=True).stdout)
exact = close = 0; worst = []
for (k, obj, flags, fr, mat), nv in zip(ram, nat):
    if nv['matrix'] is None:
        worst.append((k, 'error', nv['error'])); continue
    flat = [x for row in nv['matrix'] for x in row]
    d = max(abs(a - b) for a, b in zip(flat[:15], mat[:15]) if True)
    same = all(struct.pack('<f', a) == struct.pack('<f', b) for a, b in zip(flat[:12], mat[:12])) and all(struct.pack('<f', a) == struct.pack('<f', b) for a, b in zip(flat[12:15], mat[12:15]))
    exact += same; close += d < 1e-3
    if not same: worst.append((k, obj, hex(flags), round(fr, 3), round(d, 6)))
rep = dict(fixture=fixture, entries=len(ram), exact=exact, within_1e3=close, spf=spf, mismatches=worst)
pathlib.Path(out).write_text(json.dumps(rep, indent=1))
print(json.dumps({k: rep[k] for k in ('entries', 'exact', 'within_1e3')}), worst[:12])
