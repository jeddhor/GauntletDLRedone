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
from reference_snapshot import describe, fixture_regions
from virtual_display import environment

KEYS = ['Up', 'Down', 'Left', 'Right', 'K', 'J', 'I', 'L']


def schedule_segments(path, steps, keys, limit=120):
    if path:
        with path.open('rb') as file:
            raw = file.read(16*1024+1)
        if len(raw)>16*1024:
            raise ValueError('schedule input exceeds policy')
        segments=json.loads(raw)
    else:
        segments=[dict(steps=steps, keys=keys)]
    if not isinstance(segments, list) or not 1 <= len(segments) <= 16:
        raise ValueError('schedule must contain 1..16 segments')
    result = []
    for segment in segments:
        if not isinstance(segment, dict) or set(segment) != {'steps', 'keys'}:
            raise ValueError('segment needs only steps and keys')
        n, held = segment['steps'], segment['keys']
        if type(n) is not int or not 1 <= n <= 120 or not isinstance(held, list) or any(k not in KEYS for k in held):
            raise ValueError('invalid step count/key list')
        result.append(dict(steps=n, keys=list(dict.fromkeys(held))))
    if sum(s['steps'] for s in result) > limit:
        raise ValueError(f'schedule exceeds {limit}-vblank policy')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--steps', type=int, default=12)
    parser.add_argument('--keys', nargs='*', choices=KEYS, default=[])
    parser.add_argument('--schedule', type=pathlib.Path, help='JSON list of bounded steps/keys segments')
    parser.add_argument('--animation', action='store_true', help='capture fixture-qualified SOR animation cache records')
    parser.add_argument('--movement', action='store_true', help='capture raw slot-0 player record, controls and camera block')
    parser.add_argument('--stdcam', action='store_true', help='capture StdCamera, pb camera block, window record and camera inputs')
    parser.add_argument('--player', action='store_true', help='capture player 0, PlayerControl, window, StdCamera and player globals')
    parser.add_argument('--enemies', action='store_true', help='capture the enemies array, the rand state and enemy globals')
    parser.add_argument('--leave-held', action='store_true',
                        help='diagnostic: keep final keys held after the last step for a following debugger stop. '
                             'Release them with keyup while the render window has focus; releases delivered to '
                             'the debugger window leave PCSX2 input stale (observed 2026-10-15)')
    parser.add_argument('--slot', type=int, choices=(1, 2), default=1,
                        help='savestate slot: 1 the Poison Fields fixture, 2 the Tower new game')
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    # StdCam traces need longer walks to cross transmitter zones (600 bound).
    segments = schedule_segments(args.schedule, args.steps, args.keys, 600 if args.stdcam or args.player or args.enemies else 120)
    step_keys = [s['keys'] for s in segments for _ in range(s['steps'])]
    if not 1 <= args.steps <= 120 or not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('trace must be private and bounded to 1..120 vblanks')
    if args.out.exists():
        raise ValueError('existing trace is preserved; choose a new output')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    fixture = pathlib.Path(f'private/reference/config/PCSX2/sstates/SLUS-20047 (15030637).0{args.slot}.p2s')
    frozen = pathlib.Path('private/reference/captures/' + ('poison-fields-original.p2s' if args.slot == 1 else 'tower-newgame-v1.p2s'))
    fixture_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
    if fixture_hash != hashlib.sha256(frozen.read_bytes()).hexdigest():
        raise ValueError(f'private slot {args.slot} differs from preserved fixture')
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
            regions = fixture_regions(memory, profile)
            capture_animation = None
            if args.animation:
                from reference_animation import cache_probe
                capture_animation, pose_regions = cache_probe(memory, profile)
                regions.extend(pose_regions)
            capture_stdcam = None
            if args.stdcam:
                from reference_stdcam import stdcam_probe
                capture_stdcam, stdcam_regions = stdcam_probe(memory)
                regions.extend(stdcam_regions)
            capture_player = None
            if args.player:
                from reference_player import player_probe
                capture_player, player_regions = player_probe(memory)
                regions.extend(player_regions)
            capture_enemies = None
            if args.enemies:
                from reference_enemies import enemy_probe
                capture_enemies, enemy_regions = enemy_probe(memory)
                regions.extend(enemy_regions)
            capture_movement = None
            if args.movement:
                from reference_movement import movement_probe
                capture_movement, movement_regions = movement_probe(memory, profile)
                regions.extend(movement_regions)
            # PINE queues the load asynchronously; wait for all fixture fields.
            pine.request(b'\x0a' + bytes([args.slot]))
            deadline = time.monotonic()+5
            while True:
                observation = describe(read, profile)
                if pine.status() == 1 and observation == baseline and all(read(a, len(b)) == b for a, b in regions):
                    break
                if time.monotonic() >= deadline:
                    raise ValueError('fixture reload did not settle')
                time.sleep(0.02)
            samples = [dict(step=0, observation=observation)]
            if capture_animation:
                samples[0]['animation'] = capture_animation(read)
            if capture_movement:
                samples[0]['movement'] = capture_movement(read)
            if capture_stdcam:
                samples[0]['stdcam'] = capture_stdcam(read)
            if capture_player:
                samples[0]['player'] = capture_player(read)
            if capture_enemies:
                samples[0]['enemies'] = capture_enemies(read)
            for step, wanted in enumerate(step_keys, start=1):
                for key in list(held):
                    if key not in wanted:
                        xdo('keyup', key)
                        held.remove(key)
                for key in wanted:
                    if key not in held:
                        xdo('keydown', key)
                        held.append(key)
                before = observation['scalars']['vb_counter']['raw_u32']
                xdo('key', '--delay', '20', 'F10')
                deadline = time.monotonic()+5
                resend = time.monotonic()+1.5
                while True:
                    after, = pine.read_ee32([profile['symbols']['vb_counter']['guest_address']])
                    if pine.status() == 1 and after != before:
                        break
                    if step == 1 and time.monotonic() >= resend:
                        # As reference_screenshot.py: the first step after a
                        # state load can be dropped (seen with slot 2); a
                        # double step is still refused below.
                        xdo('key', '--delay', '20', 'F10')
                        resend = deadline
                    if time.monotonic() >= deadline:
                        raise ValueError(f'frame step {step} did not settle')
                    time.sleep(0.01)
                if (after-before)&0xffffffff != 1:
                    raise ValueError(f'frame step {step} advanced more than one vblank')
                observation = describe(read, profile)
                if pine.status() != 1:
                    raise ValueError('reference resumed during paused sampling')
                samples.append(dict(step=step, held_keys=list(wanted), observation=observation))
                if capture_animation:
                    samples[-1]['animation'] = capture_animation(read)
                if capture_movement:
                    samples[-1]['movement'] = capture_movement(read)
                if capture_stdcam:
                    samples[-1]['stdcam'] = capture_stdcam(read)
                if capture_player:
                    samples[-1]['player'] = capture_player(read)
                if capture_enemies:
                    samples[-1]['enemies'] = capture_enemies(read)
            report = dict(schema_version=2, source_profile=source['profile_id'], fixture_sha256=fixture_hash,
                          launch=launch, transport='Xvfb keyboard F10 + paused PINE; one verified vblank per step',
                          alignment='emulator vblank boundary, not a guaranteed guest-update edge',
                          reload_barrier='sampled fields and complete animation pool; animation probes also match source-bound nodes/local matrices to frozen fixture',
                          held_keys=args.keys, input_schedule=segments, samples=samples)
            with args.out.open('x') as file:
                json.dump(report, file, indent=2, allow_nan=False)
                file.write('\n')
            print(f'{len(step_keys)} verified vblank steps; schedule={segments}')
            if args.leave_held:
                print(f'keys intentionally left held: {held}')
                held.clear()
        finally:
            for key in reversed(held):
                xdo('keyup', key)


if __name__ == '__main__':
    main()
