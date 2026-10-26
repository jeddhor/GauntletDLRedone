#!/usr/bin/env python3
"""Reload a preserved savestate slot in the paused virtual reference (slot 1:
the Poison Fields fixture, slot 2: the Tower new-game capture), step a
bounded number of vblanks (F10, optional held keys) and save a screenshot of
the virtual display. Research only; no guest writes; outputs stay private.
"""
import argparse
import json
import pathlib
import socket
import subprocess
import time

from pine_probe import Pine, configured_socket
from reference_trace import KEYS, schedule_segments
from virtual_display import environment

from reference_slots import SLOTS, resolve  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--slot', type=int, choices=sorted(SLOTS), default=1)
    parser.add_argument('--stage', help='a preserved capture name to load through the staging slot 9 instead of --slot')
    parser.add_argument('--steps', type=int, default=2)
    parser.add_argument('--keys', nargs='*', choices=KEYS, default=[])
    parser.add_argument('--schedule', type=pathlib.Path,
                        help='JSON list of steps/keys segments (reference_trace.py format; 900 vblanks at most)')
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.steps <= 900 or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('screenshot must be private and bounded to 1..900 vblanks')
    if args.schedule:
        segments = schedule_segments(args.schedule, None, None, limit=900)
    else:
        segments = [dict(steps=args.steps, keys=list(dict.fromkeys(args.keys)))]
    if args.out.exists():
        raise ValueError('existing capture is preserved; choose a new output')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    args.slot, frozen = resolve(args.slot, args.stage)
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    vb_counter = profile['symbols']['vb_counter']['guest_address']

    def xdo(*a):
        return subprocess.run(['xdotool', *a], env=env, check=True, capture_output=True, text=True, timeout=5).stdout
    pid = pathlib.Path('private/reports/reference.pid').read_text().strip()
    windows = [s for s in xdo('search', '--onlyvisible', '--pid', pid).splitlines() if s.isdigit()]
    render = [w for w in windows if xdo('getwindowname', w).strip() == 'pcsx2-qt']
    if len(render) != 1:
        raise ValueError('reference render window is ambiguous')
    # The capture is of the screen: no other visible window may cover the
    # render window (Xvfb has no compositor; gdl_play shares the display).
    def rect(w):
        g = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', w).splitlines() if '=' in line)
        return int(g['X']), int(g['Y']), int(g['X']) + int(g['WIDTH']), int(g['Y']) + int(g['HEIGHT'])
    rx0, ry0, rx1, ry1 = rect(render[0])
    covering = []
    for w in xdo('search', '--onlyvisible', '--name', '.').split():
        if w in windows or not w.isdigit():
            continue
        x0, y0, x1, y1 = rect(w)
        if x0 < rx1 and rx0 < x1 and y0 < ry1 and ry0 < y1 and 1 < x1 - x0 and 1 < y1 - y0:
            covering.append(int(w))
    # Another window over it (e.g. a gdl_play window left on the display):
    # raise the reference's own top-level windows (Xvfb has no window
    # manager), then require each of them above every covering window in
    # the root's stacking order (xwininfo lists the top-most first).
    def stacking():
        out = subprocess.run(['xwininfo', '-root', '-children'], env=env, check=True, capture_output=True, text=True, timeout=5).stdout
        return [int(line.split()[0], 16) for line in out.splitlines() if line.strip().startswith('0x')]
    own = [w for w in stacking() if str(w) in windows]
    for w in reversed(own):
        xdo('windowraise', str(w))
    xdo('windowraise', render[0])
    if covering:
        order = stacking()
        top = [order.index(w) for w in own if w in order]
        for w in covering:
            if w in order and (not top or order.index(w) < max(top)):
                raise ValueError(f'window {w} ({xdo("getwindowname", str(w)).strip()}) stays above the reference render window')
    xdo('windowfocus', '--sync', render[0])
    held = []
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047' or pine.status() != 1:
            raise ValueError('expected supported paused reference')
        pine.request(bytes([0x0a, args.slot]))
        time.sleep(3.0)
        try:
            for segment in segments:
                for key in [k for k in held if k not in segment['keys']]:
                    xdo('keyup', key)
                    held.remove(key)
                for key in [k for k in segment['keys'] if k not in held]:
                    xdo('keydown', key)
                    held.append(key)
                for step in range(segment['steps']):
                    before, = pine.read_ee32([vb_counter])
                    xdo('key', '--delay', '20', 'F10')
                    deadline = time.monotonic() + 5
                    resend = time.monotonic() + 1.5
                    while True:
                        after, = pine.read_ee32([vb_counter])
                        if pine.status() == 1 and after != before:
                            break
                        if time.monotonic() >= resend:
                            # The first step after a state load can be dropped.
                            xdo('key', '--delay', '20', 'F10')
                            resend = deadline
                        if time.monotonic() >= deadline:
                            raise ValueError(f'frame step {step} did not settle')
                        time.sleep(0.01)
        finally:
            for key in held:
                xdo('keyup', key)
    time.sleep(0.3)
    subprocess.run(['import', '-window', 'root', str(args.out)], env=env, check=True)
    print(f'{sum(s["steps"] for s in segments)} vblanks from slot {args.slot} ({frozen.name}); schedule {segments}; {args.out}')


if __name__ == '__main__':
    main()
