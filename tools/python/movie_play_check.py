#!/usr/bin/env python3
"""The attract sequence's movies play in the real client, without FFmpeg.

  movie_play_check.py TAG [--binary PATH] [--wine] [--seconds N]
                      [--window WxH] [--ffmpeg PATH]

On the private Xvfb with lavapipe (virtual_display.py; software rendering,
the real GPU and the desktop are not involved), gdl_play boots as the
player's does (the SONY screen, then the attract movies) with its own HOME
and saves, no audio, and PATH set to an empty folder, so no FFmpeg can be
found. GDL_DEBUG_ATTRACT=FILE writes the attract state every rendered frame,
each line with the client's seconds (a file: the Windows build's stderr is
not captured under Wine). --wine runs the Windows build
(target-win) in a Wine virtual desktop (WGPU_BACKEND=vulkan, WINEPREFIX
/tmp/gdl-wine-movie), whose PATH has no FFmpeg either. The window is
640x448 (GDL_WINDOW) by default: lavapipe draws the full-size window at a
few frames a second, and the client's clock takes at most 0.25 s a frame.
--ffmpeg sets GDL_FFMPEG for a build from before the native decoder (1.3.0
and older), for comparison.

Runs until the first two attract movies (MIDWAY, OPENING) have ended.
Passes when each starts ("playing"; "none" is a movie that could not
start), plays for its length (its picture count at 30 fps, within 15 %) and
ends by itself, and the client's own frames (GDL_SHOT_FRAMES) taken while
they play are not black and change.
Results: private/runs/movie-play-TAG/result.json. Research only.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from virtual_display import environment  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
SHOTS = list(range(15, 3000, 15))


def picture_count(name):
    r = subprocess.run([str(ROOT / 'target/release/gdl_tools'), 'movie-frames', str(ROOT / 'private/disc/MPEG2' / f'{name.upper()}.PSS')],
                       capture_output=True, text=True, check=True)
    return len(r.stdout.splitlines())


def shot_stats(path):
    """Mean of a frame's RGB values and a 32×24 thumbnail of it."""
    from PIL import Image, ImageStat
    im = Image.open(path).convert('RGB')
    return round(sum(ImageStat.Stat(im).mean) / 3, 2), list(im.resize((32, 24)).getdata())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary')
    ap.add_argument('--wine', action='store_true')
    ap.add_argument('--seconds', type=float, default=150)
    ap.add_argument('--window', default='640x448')
    ap.add_argument('--ffmpeg')
    a = ap.parse_args()
    run = ROOT / 'private/runs' / f'movie-play-{a.tag}'
    run.mkdir(parents=True, exist_ok=False)
    (run / 'saves').mkdir()
    (run / 'home').mkdir()
    (run / 'shots').mkdir()
    (run / 'empty-path').mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(run / 'saves')
    (run / 'config.json').write_text(json.dumps(cfg))
    log = run / 'attract.log'
    env = dict(environment(), HOME=str(run / 'home'), GDL_NO_AUDIO='1', GDL_NO_GAMEPADS='1', GDL_DEBUG_ATTRACT=str(log),
               GDL_SHOT_FRAMES=','.join(map(str, SHOTS)), GDL_SHOT_DIR=str(run / 'shots'), GDL_RENDER_FPS='30',
               PATH=str(run / 'empty-path'), RUST_BACKTRACE='1', LP_NUM_THREADS='4', GDL_WINDOW=a.window)
    for k in ('WAYLAND_DISPLAY', 'XDG_DATA_HOME', 'GDL_SETTINGS', 'GDL_BASELINE', 'GDL_NO_ATTRACT', 'GDL_FFMPEG'):
        env.pop(k, None)
    if a.ffmpeg:
        env['GDL_FFMPEG'] = a.ffmpeg
    if a.wine:
        binary = a.binary or str(ROOT / 'target-win/x86_64-pc-windows-gnu/release/gdl_play.exe')
        prefix = pathlib.Path('/tmp/gdl-wine-movie')
        prefix.mkdir(exist_ok=True)
        # As net_check.py: no Mono or Gecko prompts, wgpu on Vulkan
        # (lavapipe through winevulkan), the configuration through Z:.
        env.update(WINEPREFIX=str(prefix), WINEDEBUG='-all', WINEDLLOVERRIDES='mscoree,mshtml=', WGPU_BACKEND='vulkan',
                   PATH=f'{run / "empty-path"}:{os.path.dirname(shutil.which("wine"))}')
        # Wine's own PATH (Windows folders) has no ffmpeg.exe either.
        win = lambda p: 'Z:' + str(p).replace('/', '\\')  # noqa: E731
        env.update(GDL_DEBUG_ATTRACT=win(log), GDL_SHOT_DIR=win(run / 'shots'))
        cmd = ['wine', 'explorer', '/desktop=gdlmovie,1280x960', binary, win(run / 'config.json')]
    else:
        binary = a.binary or str(ROOT / 'target/release/gdl_play')
        cmd = [binary, str(run / 'config.json')]
    def read_lines():
        lines = []
        for ln in log.read_text(errors='replace').splitlines() if log.exists() else []:
            ts, _, rest = ln.partition(' ')
            lines.append((float(ts), rest))
        return lines

    start = time.monotonic()
    with open(run / 'client.log', 'w') as out:
        client = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=out, stderr=subprocess.STDOUT)
    second_end = None
    while time.monotonic() - start < a.seconds and client.poll() is None:
        time.sleep(0.5)
        states = [(n, s) for _, ln in read_lines() if (m := re.match(r'ATTRACT movie (\S+) (\S+)', ln)) for n, s in [m.groups()]]
        ended = {n for n, s in states if s == 'ended'}
        if len(ended) >= 2 and second_end is None:
            second_end = time.monotonic()
        if second_end and time.monotonic() - second_end > 2:
            break
    client.terminate()
    try:
        client.wait(10)
    except subprocess.TimeoutExpired:
        client.kill()
    if a.wine:
        subprocess.run(['/usr/lib/x86_64-linux-gnu/wine/wineserver', '-k'], env=env)
        shutil.rmtree(env['WINEPREFIX'], ignore_errors=True)
    lines = read_lines()
    states = [(ts, m.group(1), m.group(2)) for ts, ln in lines if (m := re.match(r'ATTRACT movie (\S+) (\S+)', ln))]
    fail = []
    movies = []
    for name in dict.fromkeys(n for _, n, _ in states):
        seen = [(ts, s) for ts, n, s in states if n == name]
        playing = [ts for ts, s in seen if s == 'playing']
        row = {'movie': name, 'states': sorted({s for _, s in seen}), 'playing_frames': len(playing)}
        if playing:
            ended = [ts for ts, s in seen if s == 'ended' and ts >= playing[0]]
            row['seconds'] = round((ended[0] if ended else playing[-1]) - playing[0], 2)
            row['ended'] = bool(ended)
            row['pictures'] = picture_count(name)
            row['expected_seconds'] = round(row['pictures'] / 30, 2)
        movies.append(row)
    if len(movies) < 2:
        fail.append(f'{len(movies)} attract movies reached')
    for m in movies[:2]:
        if 'none' in m['states'] or not m['playing_frames']:
            fail.append(f'{m["movie"]} did not start: {m["states"]}')
        elif not m.get('ended'):
            fail.append(f'{m["movie"]} did not end')
        elif abs(m['seconds'] - m['expected_seconds']) > 0.15 * m['expected_seconds']:
            fail.append(f'{m["movie"]} played {m["seconds"]} s, its pictures take {m["expected_seconds"]} s')
    # Frames shot while a movie played (ATTRACT lines come one per frame).
    shots = []
    frame_lines = [ln for _, ln in lines if ln.startswith('ATTRACT ')]
    for k in SHOTS:
        p = run / 'shots' / f'frame-{k:05}.png'
        if k <= len(frame_lines) and p.exists() and (m := re.match(r'ATTRACT movie (\S+) playing', frame_lines[k - 1])):
            mean, values = shot_stats(p)
            shots.append({'frame': k, 'movie': m.group(1), 'mean': mean, 'values': values})
    for name in [m['movie'] for m in movies[:2]]:
        mine = [s for s in shots if s['movie'] == name]
        if len(mine) < 2:
            fail.append(f'{name}: {len(mine)} shots while it played')
        elif max(s['mean'] for s in mine) < 8:
            fail.append(f'{name}: the shots are black: {[s["mean"] for s in mine]}')
        elif all(s['values'] == mine[0]['values'] for s in mine[1:]):
            fail.append(f'{name}: the shots do not change')
    panics = [ln for ln in (run / 'client.log').read_text(errors='replace').splitlines() if 'panicked at' in ln]
    if panics:
        fail.append(f'panics: {panics[:3]}')
    result = {'tag': a.tag, 'binary': binary, 'wine': a.wine, 'path': env['PATH'], 'window': a.window, 'ffmpeg': a.ffmpeg,
              'attract_lines': sum(1 for _, ln in lines if ln.startswith('ATTRACT ')), 'passed': not fail, 'failures': fail,
              'movies': movies, 'shots': [{'frame': s['frame'], 'movie': s['movie'], 'mean': s['mean']} for s in shots]}
    (run / 'result.json').write_text(json.dumps(result, indent=1) + '\n')
    print(json.dumps({k: result[k] for k in ('passed', 'failures', 'movies', 'shots')}, indent=1))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
