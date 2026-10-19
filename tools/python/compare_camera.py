#!/usr/bin/env python3
"""Check the native StdCam port against a frozen reference capture.

Extracts the StdCamera record (0x2bc740), the pb camera block (pbg+4), the
window records, the four player records, the Transmitters table and the
level's CAMS record from the capture's EE RAM, then runs
`gdl_tools camera-check`, which (1) rebuilds the transmitters from the
level's WORLDS locators and derives the CAMS bounds (exact checks), then
(2) recomputes the pb matrices from the record's eye and direction and
(3) runs one UpdateCam on the captured state. (2) and (3) are informational:
a frozen capture holds the pb block computed from the previous update's eye
(the one-update lag measured in trace-stdcam-g1-v1), so they cannot match;
stdcam-replay over a vblank trace is the update check.
"""
import argparse
import json
import pathlib
import struct
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reference_snapshot import frozen_memory  # noqa: E402
from run_play import resolve, ELF  # noqa: E402

STDCAM, PBG, MBOXWIN, LEVEL_DATA = 0x2bc740, 0x25d870, 0x2a1550, 0x23e158
PLAYERS, PLAYER_SIZE = 0x221550, 0x2b00
TRANSMITTERS, NUM_TRANSMITTERS, CUR3D = 0x234500, 0x238500, 0x21f120
GLOBALS = {'num_in_game_players': 0x20f98c, 'vb_elapsed': 0x20f91c, 'paused': 0x20f8fc,
           'help': 0x20f984, 'game_mode': 0x20f978, 'cur_world': 0x23e180, 'zoom_frames': 0x21f0e4,
           'cam_wait': 0x220168}


def u32(m, a):
    return struct.unpack_from('<I', m, a)[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=pathlib.Path)
    parser.add_argument('--worlds', required=True, help='WORLDS package label, e.g. LEVELS/LEVELG1/WORLDS.PS2')
    parser.add_argument('--wdata', required=True, help='WDATA package label')
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('report must be a new file under private/')
    m = frozen_memory(args.capture)
    pbg = u32(m, PBG)
    pb4, pb10, win = u32(m, pbg + 4), u32(m, pbg + 0x10), u32(m, MBOXWIN)
    level = u32(m, LEVEL_DATA)
    cams = u32(m, level + 0x60)
    count = u32(m, NUM_TRANSMITTERS)
    if not 0 < count <= 0x100:
        raise ValueError('transmitter count outside the table')
    spf, = struct.unpack_from('<f', m, 0x20f908)
    fixture = dict(
        level_code=m[level + 8:level + 12].split(b'\0')[0].decode(),
        std=m[STDCAM:STDCAM + 0x1b0].hex(), pb=m[pb4:pb4 + 0x3b0].hex(), pb10=m[pb10:pb10 + 0x48].hex(),
        win=m[win:win + 0x1c0].hex(), cams=m[cams:cams + 0x6c].hex(),
        transmitters=m[TRANSMITTERS:TRANSMITTERS + 0x40 * count].hex(), cur3d=struct.unpack_from('<i', m, CUR3D)[0],
        players=[m[PLAYERS + PLAYER_SIZE * k:PLAYERS + PLAYER_SIZE * (k + 1)].hex() for k in range(4)],
        spf=spf, **{k: struct.unpack_from('<i', m, a)[0] for k, a in GLOBALS.items()})
    worlds, wdata = resolve([args.worlds, args.wdata])
    scratch = args.out.with_suffix('.fixture.json')
    if scratch.exists():
        raise ValueError('fixture scratch exists')
    scratch.write_text(json.dumps(fixture))
    try:
        result = subprocess.run(['target/release/gdl_tools', 'camera-check', str(scratch), str(worlds), str(wdata), str(ELF)],
                                check=True, capture_output=True, text=True, timeout=120).stdout
    finally:
        scratch.unlink()
    report = json.loads(result)
    report.update(capture=str(args.capture), worlds=args.worlds, wdata=args.wdata,
                  scope='actual disc: one native UpdateCam on a frozen capture; not a trace')
    with args.out.open('x') as f:
        json.dump(report, f, indent=1)
        f.write('\n')
    print(json.dumps({k: v for k, v in report.items() if k.endswith('mismatches') or k in ('level_code', 'quiet')}))


if __name__ == '__main__':
    main()
