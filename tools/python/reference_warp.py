#!/usr/bin/env python3
"""Reach a new reference state from a preserved savestate (research only).

Reloads a preserved slot (1: the Poison Fields fixture, 2: the Tower new
game, 3..7 the warped level-start captures of reference-setup.md) in the
paused virtual reference, optionally writes player 0's position
(the record's matrix translation +0x50 and its node's local translation
+0x30 — a guest write, so the resulting state is labelled as warped),
optionally writes skip_wave (0x10000 + wave: do_exit's own wave skip, the next
exit goes to that wave; also a labelled guest write) or a critter's health
(CritterInsts[k] +0x4f4; labelled guest write), steps a
bounded key schedule one verified vblank at a time (F10), reports CurWorld,
game_mode and player 0's position after every segment, and can save the state
to a new PCSX2 slot (PINE MsgSaveState) copied to a new private capture. The
preserved slots 1 and 2 and existing files are never written.
--sample-filename reads wad_last_filename (GetWadInfo 0x1d47f8 formats every
archive request there before hashing it) after every vblank and logs each
change: a runtime request map at vblank granularity (requests made within
one vblank are seen only as the last of them).
--gs-dump saves a single-frame GS dump at the end through the Tools menu
(Alt+T, then the item's mnemonic G — no function-key hotkey) and one more
verified vblank; the new file in the private snaps directory is logged.
"""
import argparse
import compression.zstd
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
from reference_slots import SLOTS, STAGE_SLOT, resolve, stage_is_preserved, state_file
from virtual_display import environment

# Pad 2 (private ini [Pad2], 2026-10-23): Y/B/V/N up/down/left/right, M cross,
# U square, O triangle, P circle, Z start, X select, R L1, C R1.
KEYS = ['Up', 'Down', 'Left', 'Right', 'K', 'J', 'I', 'L', 'Return', 'Y', 'B', 'V', 'N', 'M', 'U', 'O', 'P', 'Z', 'X', 'R', 'C']
# Multitap slots D and C (launch_reference.py --multitap: [Pad5] joy 3, [Pad4] joy 2;
# xdotool names of MULTITAP_PADS):
# up, down, left, right, cross, square, triangle, circle, start, select.
KEYS += ['5', '6', '7', '8', '9', '0', 'minus', 'equal', 'bracketleft', 'bracketright']
KEYS += ['Home', 'End', 'Delete', 'Next', 'comma', 'slash', 'backslash', 'Prior', 'semicolon', 'apostrophe']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--slot', type=int, choices=sorted(SLOTS), default=0)
    parser.add_argument('--stage', help='a preserved capture name to start from through the staging slot 9')
    parser.add_argument('--pos', type=float, nargs=3, help='player 0 position written after the reload (guest write)')
    parser.add_argument('--pos2', type=float, nargs=3, help='player 2 (second record) position written after the reload (guest write)')
    parser.add_argument('--pos3', type=float, nargs=3, help='player 3 (third record) position written after the reload (guest write)')
    parser.add_argument('--pos4', type=float, nargs=3, help='player 4 (fourth record) position written after the reload (guest write)')
    parser.add_argument('--skip-wave', type=lambda v: int(v, 0), help="skip_wave written after the reload: 0x10000 + world << 8 | record (do_exit's wave skip; guest write)")
    parser.add_argument('--poke', action='append', default=[], metavar='SYMBOL+OFF=WORD',
                        help='a 32-bit guest write after the reload: an executable symbol plus a hex offset, '
                             'the word hex (0x…) or a float (f:1.5); labelled warped, e.g. players+0x154=f:60')
    parser.add_argument('--critter-health', nargs=2, metavar=('INDEX', 'VALUE'),
                        help='CritterInsts[INDEX] +0x4f4 health written after the reload (a live instance; guest write)')
    parser.add_argument('--until-wave', type=lambda v: int(v, 0),
                        help='stop after the first segment that ends with this wave (world << 8 | record) in play '
                             '(game_mode 0x4010, player 0 state 1)')
    parser.add_argument('--schedule', type=pathlib.Path, required=True, help='JSON list of {steps, keys} segments')
    parser.add_argument('--save-slot', type=int, help='PCSX2 slot (3..9) for the state at the end; must not exist')
    parser.add_argument('--capture', help='new file name under private/reference/captures for the saved state')
    parser.add_argument('--shot', type=pathlib.Path, help='private screenshot at the end')
    parser.add_argument('--report', type=pathlib.Path, required=True, help='new private JSON report')
    parser.add_argument('--sample-filename', action='store_true', help='log wad_last_filename changes after every vblank')
    parser.add_argument('--gs-dump', action='store_true', help='save a single-frame GS dump at the end (Tools menu)')
    parser.add_argument('--sample-clock', action='store_true', help='log game_mode, AFrame, vb_elapsed, SPF and Time after every vblank')
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
        free = not state_file(args.save_slot).exists() or (args.save_slot == STAGE_SLOT and stage_is_preserved())
        if not 3 <= args.save_slot <= 9 or args.save_slot in SLOTS or not free:
            raise ValueError('save slot must be 3..9, unbound, and empty (or the staging slot holding a preserved copy)')
        if not args.capture:
            raise ValueError('--save-slot needs --capture')
        capture = pathlib.Path('private/reference/captures') / args.capture
        if capture.exists() or capture.suffix != '.p2s':
            raise ValueError('capture must be a new .p2s name')
    launch = json.loads(pathlib.Path('private/reports/reference-launch.json').read_text())
    env = environment()
    if launch.get('display') != env['DISPLAY'] or not launch.get('diagnostic_hotkeys'):
        raise ValueError('reference was not launched with virtual-display stepping bindings')
    if not args.stage and args.slot not in SLOTS:
        raise ValueError('give --slot N or --stage CAPTURE')
    args.slot, frozen = resolve(args.slot, args.stage)
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047-reference.json').read_text())
    vb_counter = profile['symbols']['vb_counter']['guest_address']
    last_name = profile['symbols']['wad_last_filename']['guest_address']
    clock = {k: profile['symbols'][k]['guest_address'] for k in ('game_mode', 'AFrame', 'vb_elapsed', 'SPF', 'Time')}
    sym = elf_symbols({'players', 'CurWorld', 'CurLevel', 'game_mode', 'skip_wave', 'CritterInsts', 'NumCritterInsts'})

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
        # Players 2..4 (records players + 0x2b00 × (n − 1)), the same two writes.
        for n, pos in ((2, args.pos2), (3, args.pos3), (4, args.pos4)):
            if not pos:
                continue
            pn = sym['players'] + 0x2b00 * (n - 1)
            noden = word(pn + 0x80)
            if not (noden and noden % 16 == 0 and 0x100000 <= noden < 0x2000000):
                raise ValueError(f'player {n} node pointer outside main memory')
            for k, v in enumerate(pos):
                bits = struct.unpack('<I', struct.pack('<f', v))[0]
                write32(pn + 0x50 + 4 * k, bits)
                write32(noden + 0x30 + 4 * k, bits)
            log.append(dict(write={f'pos{n}': pos, 'node': f'{noden:08x}'}))
        if args.skip_wave is not None:
            write32(sym['skip_wave'], args.skip_wave)
            log.append(dict(write=dict(skip_wave=f'{args.skip_wave:#x}')))
        for poke in args.poke:
            target, value = poke.split('=', 1)
            name, _, off = target.partition('+')
            addr = elf_symbols({name})[name] + (int(off, 16) if off else 0)
            word_v = struct.unpack('<I', struct.pack('<f', float(value[2:])))[0] if value.startswith('f:') else int(value, 16)
            write32(addr, word_v)
            log.append(dict(write=dict(poke=poke, address=f'{addr:08x}', word=f'{word_v:08x}')))
        if args.critter_health:
            k, v = int(args.critter_health[0]), float(args.critter_health[1])
            inst = sym['CritterInsts'] + 0xb70 * k
            if not 0 <= k < min(16, word(sym['NumCritterInsts'])) or word(inst + 4) == 0 or not 0.0 < v:
                raise ValueError('critter-health needs a live instance and a positive value')
            write32(inst + 0x4f4, struct.unpack('<I', struct.pack('<f', v))[0])
            log.append(dict(write=dict(critter_health=[k, v])))
        def filename():
            raw = b''.join(w.to_bytes(4, 'little') for w in pine.read_ee32([last_name + 4 * k for k in range(64)]))
            return raw.split(b'\0', 1)[0].decode('latin-1')
        def advance(step):
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
        seen = filename() if args.sample_filename else None
        if args.sample_filename:
            log.append(dict(step=0, filename=seen))
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
                    advance(step)
                    if args.sample_clock:
                        w = dict(zip(clock, pine.read_ee32(list(clock.values()))))
                        f32 = lambda v: struct.unpack('<f', struct.pack('<I', v))[0]
                        log.append(dict(step=step, game_mode=hex(w['game_mode']), aframe=w['AFrame'], vb_elapsed=w['vb_elapsed'],
                                        spf=f32(w['SPF']), time=f32(w['Time'])))
                    if args.sample_filename and (name := filename()) != seen:
                        seen = name
                        log.append(dict(step=step, filename=name, cur_world=word(sym['CurWorld']), game_mode=word(sym['game_mode'])))
                obs = observe(step)
                log.append(obs)
                if args.until_wave is not None and (obs['cur_world'] << 8 | obs['cur_level'] & 0xff) == args.until_wave \
                        and obs['game_mode'] == 0x4010 and obs['state'] == 1:
                    break
        finally:
            for key in held:
                xdo('keyup', key)
        if args.gs_dump:
            snaps = pathlib.Path('private/reference/config/PCSX2/snaps')
            before = set(snaps.glob('*.gs.zst'))
            xdo('key', '--delay', '50', 'alt+t')
            time.sleep(0.5)
            xdo('key', '--delay', '50', 'g')
            time.sleep(0.5)
            # The dump starts with the next frame and is closed at the vsync
            # after it: advance until the zstd stream is complete.
            new = []
            for _ in range(8):
                step += 1
                advance(step)
                time.sleep(1.0)
                new = sorted(set(snaps.glob('*.gs.zst')) - before)
                if new:
                    d = compression.zstd.ZstdDecompressor()
                    try:
                        d.decompress(new[0].read_bytes(), max_length=64 * 1024 * 1024 + 1)
                    except compression.zstd.ZstdError:
                        continue
                    if d.eof:
                        break
            else:
                raise ValueError('the GS dump did not complete within 8 vblanks')
            log.append(dict(step=step, gs_dump=str(new[0]), sha256=hashlib.sha256(new[0].read_bytes()).hexdigest()))
        if args.save_slot is not None:
            target = state_file(args.save_slot)
            if target.exists():
                # Only the staging slot can exist here, holding a preserved copy.
                if args.save_slot != STAGE_SLOT or not stage_is_preserved():
                    raise ValueError('save slot changed during the run')
                target.unlink()
            pine.request(bytes([0x09, args.save_slot]))
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
        json.dump(dict(source_slot=args.slot, source_capture=frozen.name, warped=bool(args.pos) or bool(args.pos2) or bool(args.pos3) or bool(args.pos4) or bool(args.poke) or args.skip_wave is not None,
                       schedule=segments, log=log), f, indent=1)
    for entry in log:
        print(entry)


if __name__ == '__main__':
    main()
