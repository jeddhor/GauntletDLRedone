#!/usr/bin/env python3
"""Drive the running PCSX2 reference live (research only).

  reference_driver.py serve TAG     wait for queued steps (40 idle minutes)
  reference_driver.py send TAG 'STEPS'

Steps: ["key", "Up+K", seconds] holds PCSX2 pad bindings (Up/Down/Left/Right,
K cross, I triangle, L circle, J square, Return start, BackSpace select,
Q L1, E R1, 1 L2, 3 R2),
["pause"] / ["resume"] toggle through PCSX2's TogglePause hotkey (Space)
according to PINE's status, ["wait", s], ["shot", label] (the render window),
["status"] (PINE status, game_mode), ["dump", label, address, size]
(EE bytes to dump-label.bin), ["gsdump"] (a single-frame GS dump through
the Tools menu — Alt+T, then the item's mnemonic G — while running; waits
for the complete file in the private snaps directory). No other key can be
sent: the
function-key hotkeys (savestate saves among them) are never pressed. Frames
and output go to private/runs/reference-TAG/. Use only with the private
reference launched by launch_reference.py; slot files are not touched.
"""
import compression.zstd
import json
import os
import pathlib
import socket
import subprocess
import sys
import time

from pine_probe import Pine, configured_socket
from virtual_display import environment

ROOT = pathlib.Path(__file__).resolve().parents[2]
PAD_KEYS = {'Up', 'Down', 'Left', 'Right', 'K', 'I', 'L', 'J', 'Return', 'BackSpace', 'Q', 'E', '1', '3'}


def run_dir(tag):
    if not tag.replace('-', '').isalnum():
        raise ValueError('tag must be alphanumeric')
    d = ROOT / 'private' / 'runs' / f'reference-{tag}'
    d.mkdir(parents=True, exist_ok=True)
    return d


def symbols():
    profile = json.loads((ROOT / 'profiles/ps2/slus-20047-reference.json').read_text())
    s = profile['symbols']
    return {k: s[k]['guest_address'] for k in ('game_mode', 'CurWorld') if k in s}


def serve(tag):
    d = run_dir(tag)
    q = d / 'queue'
    q.mkdir(exist_ok=True)
    env = environment()

    def xdo(*a):
        try:
            return subprocess.run(['xdotool', *a], env=env, capture_output=True, text=True, timeout=5).stdout.strip()
        except subprocess.TimeoutExpired:
            return ''

    pid = (ROOT / 'private/reports/reference.pid').read_text().strip()

    def render_window():
        # PCSX2 recreates its render window (e.g. around movies), so the id is
        # looked up again whenever it is needed.
        windows = [w for w in xdo('search', '--onlyvisible', '--pid', pid).split() if w.isdigit()]
        render = [w for w in windows if xdo('getwindowname', w) == 'pcsx2-qt']
        return render[0] if len(render) == 1 else None
    win = render_window()
    if win is None:
        raise SystemExit('reference render window not found or ambiguous')
    syms = symbols()
    # Continue an existing run's numbering: frames are never overwritten.
    shots = max((int(f.name[:3]) for f in d.glob('[0-9][0-9][0-9]-*.png')), default=0)
    conn = socket.socket(socket.AF_UNIX)
    conn.settimeout(3)
    conn.connect(str(configured_socket()))
    pine = Pine(conn)
    if pine.text(12) != 'SLUS-20047':
        raise SystemExit('reference is not running the supported serial')

    def focus():
        nonlocal win
        win = render_window() or win
        xdo('windowraise', win)
        xdo('windowfocus', '--sync', win)
        return xdo('getwindowfocus') == win

    def status():
        st = pine.status()
        words = dict(zip(syms, pine.read_ee32(list(syms.values())))) if syms else {}
        return dict(pine_status=st, **{k: hex(v) for k, v in words.items()})

    def run(steps, out):
        nonlocal shots
        for st in steps:
            kind = st[0]
            if kind == 'quit':
                return False
            if kind == 'wait':
                time.sleep(st[1])
            elif kind == 'key':
                keys = st[1].split('+')
                if any(k not in PAD_KEYS for k in keys):
                    out.append(f'refused keys {keys}')
                    continue
                if not focus():
                    out.append('focus is not the reference window; key skipped')
                    continue
                for k in keys:
                    xdo('keydown', k)
                time.sleep(st[2])
                for k in reversed(keys):
                    xdo('keyup', k)
                time.sleep(0.2)
            elif kind in ('pause', 'resume'):
                want = 1 if kind == 'pause' else 0
                if (pine.status() == 1) != bool(want) and focus():
                    xdo('key', 'space')
                    time.sleep(0.5)
                out.append(f'{kind}: status {pine.status()}')
            elif kind == 'shot':
                shots += 1
                o = d / f'{shots:03d}-{st[1]}.png'
                if o.exists():
                    out.append(f'{o} exists')
                    continue
                current = render_window()
                if current is None:
                    out.append('no render window for the shot')
                    continue
                try:
                    subprocess.run(['import', '-window', current, str(o)], env=env, timeout=20)
                    out.append(str(o))
                except subprocess.TimeoutExpired:
                    out.append(f'{o}: import timed out')
            elif kind == 'status':
                out.append(json.dumps(status()))
            elif kind == 'gsdump':
                snaps = ROOT / 'private/reference/config/PCSX2/snaps'
                before = set(snaps.glob('*.gs.zst'))
                if pine.status() != 0 or not focus():
                    out.append('gsdump needs the running reference in focus')
                    continue
                xdo('key', '--delay', '50', 'alt+t')
                time.sleep(0.5)
                xdo('key', '--delay', '50', 'g')
                done = None
                deadline = time.time() + 60
                while time.time() < deadline and done is None:
                    time.sleep(1.0)
                    for f in sorted(set(snaps.glob('*.gs.zst')) - before):
                        z = compression.zstd.ZstdDecompressor()
                        try:
                            z.decompress(f.read_bytes(), max_length=64 * 1024 * 1024 + 1)
                        except compression.zstd.ZstdError:
                            continue
                        if z.eof:
                            done = f
                out.append(f'gsdump {done}' if done else 'gsdump did not complete')
            elif kind == 'dump':
                # ["dump", label, address, size]: EE bytes to a new file.
                label, addr, size = st[1], int(str(st[2]), 0), int(str(st[3]), 0)
                if not (0 < size <= 0x100000 and 0 <= addr < 0x2000000 and label.replace('-', '').isalnum()):
                    out.append('refused dump')
                    continue
                o = d / f'dump-{label}.bin'
                if o.exists():
                    out.append(f'{o} exists')
                    continue
                words = []
                for off in range(0, size, 0x100):
                    n = min(0x100, size - off) // 4
                    words += pine.read_ee32([addr + off + 4 * i for i in range(n)])
                o.write_bytes(b''.join(w.to_bytes(4, 'little') for w in words))
                out.append(str(o))
            else:
                out.append(f'unknown step {st!r}')
        return True

    print('window', win, json.dumps(status()), flush=True)
    idle = time.time()
    alive = True
    while alive and time.time() - idle < 2400:
        cmds = sorted(c for c in q.glob('*.json') if not c.with_suffix('.done').exists())
        if not cmds:
            time.sleep(0.5)
            continue
        out = []
        try:
            alive = run(json.loads(cmds[0].read_text()), out)
        finally:
            # Never leave a pad key held.
            for k in PAD_KEYS:
                xdo('keyup', k)
        cmds[0].with_suffix('.done').write_text('\n'.join(out) + '\n')
        idle = time.time()


def send(tag, steps):
    q = run_dir(tag) / 'queue'
    q.mkdir(exist_ok=True)
    c = q / f'{len(list(q.glob("*.json"))):03d}.json'
    c.write_text(json.dumps(steps))
    done = c.with_suffix('.done')
    t0 = time.time()
    while not done.exists():
        if time.time() - t0 > 900:
            raise SystemExit('timed out')
        time.sleep(0.5)
    print(done.read_text(), end='')


def main():
    if len(sys.argv) < 3 or sys.argv[1] not in ('serve', 'send'):
        raise SystemExit(__doc__)
    if sys.argv[1] == 'serve':
        serve(sys.argv[2])
    else:
        send(sys.argv[2], json.loads(sys.argv[3]))


if __name__ == '__main__':
    main()
