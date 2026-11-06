#!/usr/bin/env python3
"""The native MPEG-2 decoder against FFmpeg, frame by frame, on every movie.

  movie_check.py TAG [--disc private/disc] [--tools PATH] [--wine]
                 [--cpuflags 0|default] [--only NAME ...]

For every MPEG/*.PSS and MPEG2/*.PSS of the disc, `gdl_tools movie-frames`
prints the SHA-256 of each decoded picture's Y, Cb and Cr planes in display
order; FFmpeg decodes the same file to raw yuv420p (one frame per decoded
picture: -fps_mode passthrough) and the script hashes its frames the same
way. The decoder uses FFmpeg's integer "simple" IDCT, so with FFmpeg's C
code (-cpuflags 0, the default here) every frame has to be equal; with
--cpuflags default FFmpeg picks its SIMD IDCT and the counts of differing
frames are only reported. --wine runs a Windows gdl_tools.exe under Wine
(WINEPREFIX from the environment, else /tmp/gdl-wine-movie).

FFmpeg is the reference here only (docs/toolchain.md); the game does not use
it. Results: private/runs/movie-check-TAG/result.json. Research only.
"""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]


def native(tools, path, wine):
    cmd = [str(tools), 'movie-frames', str(path)]
    env = dict(os.environ)
    if wine:
        env.setdefault('WINEPREFIX', '/tmp/gdl-wine-movie')
        pathlib.Path(env['WINEPREFIX']).mkdir(parents=True, exist_ok=True)
        env.setdefault('WINEDEBUG', '-all')
        cmd = ['wine'] + cmd
    t = time.monotonic()
    r = subprocess.run(cmd, capture_output=True, text=True, env=env)
    took = time.monotonic() - t
    if r.returncode != 0:
        return None, r.stderr.strip()[-400:], took
    frames = []
    for line in r.stdout.splitlines():
        n, size, h = line.split()
        frames.append((size, h))
    return frames, r.stderr.strip(), took


def reference(path, cpuflags):
    cmd = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'error']
    if cpuflags != 'default':
        cmd += ['-cpuflags', cpuflags]
    cmd += ['-i', str(path), '-an', '-fps_mode', 'passthrough', '-f', 'rawvideo', '-pix_fmt', 'yuv420p', '-']
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    size = 640 * 480 * 3 // 2
    frames = []
    while True:
        b = p.stdout.read(size)
        if len(b) < size:
            break
        frames.append(hashlib.sha256(b).hexdigest())
    errors = p.stderr.read().decode(errors='replace').strip()
    p.wait()
    return frames, errors, p.returncode


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('tag')
    ap.add_argument('--disc', default=str(ROOT / 'private/disc'))
    ap.add_argument('--tools', default=str(ROOT / 'target/release/gdl_tools'))
    ap.add_argument('--wine', action='store_true')
    ap.add_argument('--cpuflags', default='0')
    ap.add_argument('--only', nargs='*')
    a = ap.parse_args()
    out = ROOT / 'private/runs' / f'movie-check-{a.tag}'
    out.mkdir(parents=True, exist_ok=False)
    disc = pathlib.Path(a.disc)
    files = sorted(list(disc.glob('MPEG/*.PSS')) + list(disc.glob('MPEG2/*.PSS')))
    if a.only:
        files = [f for f in files if f.stem in a.only]
    ffmpeg = subprocess.run(['ffmpeg', '-version'], capture_output=True, text=True).stdout.splitlines()[0]
    rows, fail = [], []
    for f in files:
        mine, note, took = native(a.tools, f, a.wine)
        ref, ref_err, code = reference(f, a.cpuflags)
        name = f'{f.parent.name}/{f.name}'
        row = {'file': name, 'sha256': hashlib.sha256(f.read_bytes()).hexdigest(), 'native_seconds': round(took, 3),
               'native_note': note, 'ffmpeg_frames': len(ref), 'ffmpeg_exit': code, 'ffmpeg_errors': ref_err}
        if mine is None:
            fail.append(f'{name}: native decoder failed: {note}')
            rows.append(row)
            continue
        row['native_frames'] = len(mine)
        sizes = sorted({s for s, _ in mine})
        row['sizes'] = sizes
        differing = [k for k, ((_, h), r) in enumerate(zip(mine, ref)) if h != r]
        row['differing_frames'] = len(differing)
        row['first_differing'] = differing[0] if differing else None
        rows.append(row)
        print(f'{name}: {len(mine)} native, {len(ref)} ffmpeg, {len(differing)} differing, {took:.2f} s', flush=True)
        if a.cpuflags != 'default':
            if len(mine) != len(ref):
                fail.append(f'{name}: {len(mine)} native pictures, {len(ref)} from ffmpeg')
            if differing:
                fail.append(f'{name}: {len(differing)} frames differ, the first {differing[0]}')
            if sizes != ['640x480']:
                fail.append(f'{name}: sizes {sizes}')
    result = {'tag': a.tag, 'tools': a.tools, 'wine': a.wine, 'cpuflags': a.cpuflags, 'ffmpeg': ffmpeg,
              'files': len(files), 'frames': sum(r.get('native_frames', 0) for r in rows),
              'pass': not fail and bool(files), 'failures': fail, 'movies': rows}
    (out / 'result.json').write_text(json.dumps(result, indent=1) + '\n')
    print(json.dumps({k: result[k] for k in ('files', 'frames', 'pass', 'failures')}, indent=1))
    return 0 if result['pass'] else 1


if __name__ == '__main__':
    sys.exit(main())
