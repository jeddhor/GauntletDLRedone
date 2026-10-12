#!/usr/bin/env python3
"""Bounded vblank-stepped reference experiments inside the private Xvfb.

These pauses are emulator-frame boundaries, not guaranteed game-update edges.
Only the copied fixture in private slot 1 is loaded. No state/EE writes occur.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import struct
import subprocess
import time

from pine_probe import Pine, configured_socket
from reference_snapshot import describe
from virtual_display import environment


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--steps', type=int, default=12)
    parser.add_argument('--keys', nargs='*', choices=['Up', 'Down', 'Left', 'Right', 'K', 'J', 'I', 'L'], default=[])
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.steps <= 120 or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('trace must be private and bounded to 1..120 vblanks')
    if args.out.exists():
        raise ValueError('existing trace is preserved; choose a new output')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    fixture = pathlib.Path('private/reference/config/PCSX2/sstates/SLUS-20047 (15030637).01.p2s')
    frozen = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
    fixture_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
    if fixture_hash != hashlib.sha256(frozen.read_bytes()).hexdigest():
        raise ValueError('private slot 1 differs from preserved fixture')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    source = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    elf = pathlib.Path('private/disc/SLUS_200.47').read_bytes()
    if hashlib.sha256(elf).hexdigest() != source['executable']['sha256']:
        raise ValueError('local ELF identity differs')
    def xdo(*args):
        return subprocess.run(['xdotool', *args], env=env, check=True, capture_output=True, text=True, timeout=5).stdout
    pid = pathlib.Path('private/reports/reference.pid').read_text().strip()
    windows = [s for s in xdo('search', '--onlyvisible', '--pid', pid).splitlines() if s.isdigit()]
    render = [w for w in windows if xdo('getwindowname', w).strip() == 'pcsx2-qt']
    if len(render) != 1:
        raise ValueError('reference render window is ambiguous')
    xdo('windowfocus', '--sync', render[0])
    if xdo('getwindowfocus').strip() != render[0]:
        raise ValueError('virtual reference did not acquire keyboard focus')
    held = []
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047' or pine.status() != 1:
            raise ValueError('expected supported paused reference')
        def read(address, size):
            if address % 4 or size % 4 or size > 2048:
                raise ValueError('unaligned/over-limit trace read')
            return struct.pack('<' + 'I' * (size//4), *pine.read_ee32(list(range(address, address+size, 4))))
        for name in ('main', 'ClockOncePerFrame', 'GetPlayerDMotion', 'GetWadInfo'):
            address = profile['symbols'][name]['guest_address']
            segment = next(s for s in source['load_segments'] if s['virtual_address'] <= address and
                           address+16 <= s['virtual_address']+s['file_size'])
            offset = address-segment['virtual_address']+segment['file_offset']
            if read(address, 16) != elf[offset:offset+16]:
                raise ValueError('reference code differs before tracing')
        try:
            from reference_snapshot import frozen_memory
            memory = frozen_memory(frozen)
            baseline = describe(lambda a, n: memory[a:a+n], profile)
            # PINE queues the load asynchronously; wait for all fixture fields.
            pine.request(b'\x0a\x01')
            deadline = time.monotonic()+5
            while True:
                observation = describe(read, profile)
                if pine.status() == 1 and observation == baseline:
                    break
                if time.monotonic() >= deadline:
                    raise ValueError('fixture reload did not settle')
                time.sleep(0.02)
            samples = [dict(step=0, observation=observation)]
            for key in args.keys:
                xdo('keydown', key)
                held.append(key)
            for step in range(1, args.steps+1):
                before = observation['scalars']['vb_counter']['raw_u32']
                xdo('key', '--delay', '20', 'F10')
                deadline = time.monotonic()+5
                while True:
                    after, = pine.read_ee32([profile['symbols']['vb_counter']['guest_address']])
                    if pine.status() == 1 and after != before:
                        break
                    if time.monotonic() >= deadline:
                        raise ValueError(f'frame step {step} did not settle')
                    time.sleep(0.01)
                if (after-before)&0xffffffff != 1:
                    raise ValueError(f'frame step {step} advanced more than one vblank')
                observation = describe(read, profile)
                if pine.status() != 1:
                    raise ValueError('reference resumed during paused sampling')
                samples.append(dict(step=step, observation=observation))
            report = dict(schema_version=1, source_profile=source['profile_id'], fixture_sha256=fixture_hash,
                          launch=launch, transport='Xvfb keyboard F10 + paused PINE; one verified vblank per step',
                          alignment='emulator vblank boundary, not a guaranteed guest-update edge',
                          held_keys=args.keys, samples=samples)
            with args.out.open('x') as file:
                json.dump(report, file, indent=2, allow_nan=False)
                file.write('\n')
            print(f'{args.steps} verified vblank steps; keys={args.keys}')
        finally:
            for key in reversed(held):
                xdo('keyup', key)


if __name__ == '__main__':
    main()
