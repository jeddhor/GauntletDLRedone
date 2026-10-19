#!/usr/bin/env python3
"""Reload the preserved slot-1 fixture in the paused virtual reference, step a
bounded number of vblanks (F10, optional held keys) and save a screenshot of
the virtual display. Research only; no guest writes; outputs stay private.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import subprocess
import time

from pine_probe import Pine, configured_socket
from virtual_display import environment

KEYS = ['Up', 'Down', 'Left', 'Right', 'K', 'J', 'I', 'L']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--steps', type=int, default=2)
    parser.add_argument('--keys', nargs='*', choices=KEYS, default=[])
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.steps <= 600 or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('screenshot must be private and bounded to 1..600 vblanks')
    if args.out.exists():
        raise ValueError('existing capture is preserved; choose a new output')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    fixture = pathlib.Path('private/reference/config/PCSX2/sstates/SLUS-20047 (15030637).01.p2s')
    frozen = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
    if hashlib.sha256(fixture.read_bytes()).digest() != hashlib.sha256(frozen.read_bytes()).digest():
        raise ValueError('private slot 1 differs from preserved fixture')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    vb_counter = profile['symbols']['vb_counter']['guest_address']

    def xdo(*a):
        return subprocess.run(['xdotool', *a], env=env, check=True, capture_output=True, text=True, timeout=5).stdout
    pid = pathlib.Path('private/reports/reference.pid').read_text().strip()
    windows = [s for s in xdo('search', '--onlyvisible', '--pid', pid).splitlines() if s.isdigit()]
    render = [w for w in windows if xdo('getwindowname', w).strip() == 'pcsx2-qt']
    if len(render) != 1:
        raise ValueError('reference render window is ambiguous')
    xdo('windowraise', render[0])
    xdo('windowfocus', '--sync', render[0])
    held = []
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047' or pine.status() != 1:
            raise ValueError('expected supported paused reference')
        pine.request(b'\x0a\x01')
        time.sleep(1.0)
        try:
            for key in args.keys:
                xdo('keydown', key)
                held.append(key)
            for step in range(args.steps):
                before, = pine.read_ee32([vb_counter])
                xdo('key', '--delay', '20', 'F10')
                deadline = time.monotonic() + 5
                while True:
                    after, = pine.read_ee32([vb_counter])
                    if pine.status() == 1 and after != before:
                        break
                    if time.monotonic() >= deadline:
                        raise ValueError(f'frame step {step} did not settle')
                    time.sleep(0.01)
        finally:
            for key in held:
                xdo('keyup', key)
    time.sleep(0.3)
    subprocess.run(['import', '-window', 'root', str(args.out)], env=env, check=True)
    print(f'{args.steps} vblanks from the slot-1 fixture; keys {args.keys}; {args.out}')


if __name__ == '__main__':
    main()
