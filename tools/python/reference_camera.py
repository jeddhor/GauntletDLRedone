#!/usr/bin/env python3
"""Read the paused virtual reference's window camera (MboxWinPtr +0x70..+0xac:
rows right, up, forward and the eye) through PINE and write it as JSON for
gdl_play's GDL_DEBUG_CAMERA comparison override. Read-only; private output.
"""
import argparse
import json
import pathlib
import socket
import struct

from pine_probe import Pine, configured_socket
from reference_player import elf_symbols


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('choose a new private output file')
    sym = elf_symbols({'MboxWinPtr'})
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(configured_socket()))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047' or pine.status() != 1:
            raise ValueError('expected supported paused reference')
        win, = pine.read_ee32([sym['MboxWinPtr']])
        if not 0x100000 <= win < 0x2000000:
            raise ValueError('MboxWinPtr outside main memory')
        words = pine.read_ee32(list(range(win + 0x70, win + 0xb0, 4)))
    f = [struct.unpack('<f', struct.pack('<I', w))[0] for w in words]
    out = dict(source='MboxWinPtr +0x70..+0xac (paused reference)', right=f[0:3], up=f[4:7], forward=f[8:11], eye=f[12:15])
    with args.out.open('x') as file:
        json.dump(out, file, indent=1)
    print(json.dumps(out))


if __name__ == '__main__':
    main()
