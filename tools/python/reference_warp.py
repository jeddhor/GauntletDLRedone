#!/usr/bin/env python3
"""Reach a new reference state from a preserved savestate (research only).

Reloads a preserved slot (1: the Poison Fields fixture, 2: the Tower new
game, 3..7 the warped level-start captures of reference-setup.md) in the
paused virtual reference, optionally writes player 0's position
(the record's matrix translation +0x50 and its node's local translation
+0x30 — a guest write, so the resulting state is labelled as warped),
optionally writes skip_wave (0x10000 + wave: do_exit's own wave skip, the next
exit goes to that wave; also a labelled guest write), steps a
bounded key schedule one verified vblank at a time (F10), reports CurWorld,
game_mode and player 0's position after every segment, and can save the state
to a new PCSX2 slot (PINE MsgSaveState) copied to a new private capture. The
preserved slots 1 and 2 and existing files are never written.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import socket
import struct
import subprocess
import time

from pine_probe import Pine, configured_socket
from reference_player import elf_symbols
from virtual_display import environment

KEYS = ['Up', 'Down', 'Left', 'Right', 'K', 'J', 'I', 'L', 'Return']
SLOTS = {1: 'poison-fields-original.p2s', 2: 'tower-newgame-v1.p2s', 3: 'w2l1-newgame-v1.p2s',
         4: 'w1l1-newgame-v1.p2s', 5: 'w11l1-newgame-v1.p2s', 6: 'w7l1-newgame-v1.p2s', 7: 'w11l1-newgame-v2.p2s',
         8: 'w2l6-boss-v1.p2s'}
SSTATES = pathlib.Path('private/reference/config/PCSX2/sstates')


def state_file(slot):
    return SSTATES / f'SLUS-20047 (15030637).{slot:02d}.p2s'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--slot', type=int, choices=sorted(SLOTS), required=True)
    parser.add_argument('--pos', type=float, nargs=3, help='player 0 position written after the reload (guest write)')
    parser.add_argument('--skip-wave', type=lambda v: int(v, 0), help="skip_wave written after the reload: 0x10000 + world << 8 | record (do_exit's wave skip; guest write)")
    parser.add_argument('--schedule', type=pathlib.Path, required=True, help='JSON list of {steps, keys} segments')
    parser.add_argument('--save-slot', type=int, help='PCSX2 slot (3..9) for the state at the end; must not exist')
    parser.add_argument('--capture', help='new file name under private/reference/captures for the saved state')
    parser.add_argument('--shot', type=pathlib.Path, help='private screenshot at the end')
    parser.add_argument('--report', type=pathlib.Path, required=True, help='new private JSON report')
    args = parser.parse_args()
    if args.skip_wave is not None and not 0x10100 <= args.skip_wave <= 0x10dff:
        raise ValueError('skip_wave must be 0x10000 + a wave of worlds 1..13')
    segments = json.loads(args.schedule.read_text())
    if not isinstance(segments, list) or not 1 <= len(segments) <= 64:
        raise ValueError('schedule must contain 1..64 segments')
    for s in segments:
        if set(s) != {'steps', 'keys'} or type(s['steps']) is not int or not 1 <= s['steps'] <= 600 or any(k not in KEYS for k in s['keys']):
            raise ValueError('invalid segment')
    if sum(s['steps'] for s in segments) > 12000:
        raise ValueError('schedule exceeds the 12000-vblank policy')
    for p in (args.report, args.shot):
        if p and (p.exists() or not p.resolve().is_relative_to(pathlib.Path('private').resolve())):
            raise ValueError(f'{p}: outputs must be new private files')
    capture = None
    if args.save_slot is not None:
        if not 3 <= args.save_slot <= 9 or args.save_slot in SLOTS or state_file(args.save_slot).exists():
            raise ValueError('save slot must be 3..9 and not exist yet')
        if not args.capture:
            raise ValueError('--save-slot needs --capture')
        capture = pathlib.Path('private/reference/captures') / args.capture
        if capture.exists() or capture.suffix != '.p2s':
            raise ValueError('capture must be a new .p2s name')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    fixture, frozen = state_file(args.slot), pathlib.Path('private/reference/captures') / SLOTS[args.slot]
    if hashlib.sha256(fixture.read_bytes()).digest() != hashlib.sha256(frozen.read_bytes()).digest():
        raise ValueError(f'private slot {args.slot} differs from its preserved capture')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    vb_counter = profile['symbols']['vb_counter']['guest_address']
    sym = elf_symbols({'players', 'CurWorld', 'CurLevel', 'game_mode', 'skip_wave'})

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
    log = []
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047' or pine.status() != 1:
            raise ValueError('expected supported paused reference')

        def word(a):
            return pine.read_ee32([a])[0]

        def fl(a):
            return struct.unpack('<f', struct.pack('<I', word(a)))[0]

        def write32(a, v):
            if a % 4 or not 0x100000 <= a < 0x2000000:
                raise ValueError('write outside main memory')
            pine.request(b'\x06' + struct.pack('<II', a, v))

        def observe(step):
            p = sym['players']
            return dict(step=step, cur_world=word(sym['CurWorld']), cur_level=word(sym['CurLevel']),
                        game_mode=word(sym['game_mode']), pos=[fl(p + 0x50), fl(p + 0x54), fl(p + 0x58)],
                        state=word(p + 0xfc))
        pine.request(bytes([0x0a, args.slot]))
        time.sleep(3.0)
        log.append(observe(0))
        if args.pos:
            p = sym['players']
            node = word(p + 0x80)
            if not (node and node % 16 == 0 and 0x100000 <= node < 0x2000000):
                raise ValueError('player node pointer outside main memory')
            for k, v in enumerate(args.pos):
                bits = struct.unpack('<I', struct.pack('<f', v))[0]
                write32(p + 0x50 + 4 * k, bits)
                write32(node + 0x30 + 4 * k, bits)
            log.append(dict(write=dict(pos=args.pos, node=f'{node:08x}')))
        if args.skip_wave is not None:
            write32(sym['skip_wave'], args.skip_wave)
            log.append(dict(write=dict(skip_wave=f'{args.skip_wave:#x}')))
        step = 0
        try:
            for seg in segments:
                for key in list(held):
                    if key not in seg['keys']:
                        xdo('keyup', key)
                        held.remove(key)
                for key in seg['keys']:
                    if key not in held:
                        xdo('keydown', key)
                        held.append(key)
                for _ in range(seg['steps']):
                    step += 1
                    before = word(vb_counter)
                    xdo('key', '--delay', '20', 'F10')
                    deadline = time.monotonic() + 5
                    resend = time.monotonic() + 1.5
                    while True:
                        after = word(vb_counter)
                        if pine.status() == 1 and after != before:
                            break
                        if time.monotonic() >= resend:
                            xdo('key', '--delay', '20', 'F10')
                            resend = deadline
                        if time.monotonic() >= deadline:
                            raise ValueError(f'frame step {step} did not settle')
                        time.sleep(0.01)
                log.append(observe(step))
        finally:
            for key in held:
                xdo('keyup', key)
        if args.save_slot is not None:
            pine.request(bytes([0x09, args.save_slot]))
            target = state_file(args.save_slot)
            deadline = time.monotonic() + 30
            last = -1
            while time.monotonic() < deadline:
                if target.exists() and target.stat().st_size == last and last > 0:
                    break
                last = target.stat().st_size if target.exists() else -1
                time.sleep(1.0)
            if not target.exists():
                raise ValueError('PCSX2 did not write the save slot')
            shutil.copyfile(target, capture)
            log.append(dict(saved=str(capture), sha256=hashlib.sha256(capture.read_bytes()).hexdigest(), slot=args.save_slot))
    if args.shot:
        time.sleep(0.3)
        subprocess.run(['import', '-window', 'root', str(args.shot)], env=env, check=True)
    with args.report.open('x') as f:
        json.dump(dict(source_slot=args.slot, source_capture=SLOTS[args.slot], warped=bool(args.pos) or args.skip_wave is not None,
                       schedule=segments, log=log), f, indent=1)
    for entry in log:
        print(entry)


if __name__ == '__main__':
    main()
