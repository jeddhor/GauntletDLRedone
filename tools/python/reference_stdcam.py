#!/usr/bin/env python3
"""Fixture-qualified StdCam observations for reference_trace.py --stdcam.

Retains, per vblank step, the StdCamera record (0x2bc740, 0x1b0 bytes), the
pb camera block (pbg+4, 0x3b0 bytes), the MboxWinPtr window record (0x1c0),
the four player records' camera inputs (+0x50..+0x80, +0xfc, +0x93c) and the
camera globals, all as raw bytes. Never a runtime dependency.
"""
import struct

STDCAM, PBG, MBOXWIN = 0x2bc740, 0x25d870, 0x2a1550
PLAYERS, PLAYER_SIZE = 0x221550, 0x2b00
# Cur3DTransmitter, ZOOM_FRAMES, resetStdCam, cam_wait, start_of_stage,
# vb_elapsed, SPF, Time, game_mode, paused, help, NumInGamePlayers,
# shaking/shake_* are not included (ShakeCamera inactive in the fixture).
GLOBALS = {'Cur3DTransmitter': 0x21f120, 'ZOOM_FRAMES': 0x21f0e4, 'resetStdCam': 0x21f144,
           'cam_wait': 0x220168, 'start_of_stage': 0x22016c, 'vb_elapsed': 0x20f91c, 'SPF': 0x20f908,
           'Time': 0x20f904, 'game_mode': 0x20f978, 'paused': 0x20f8fc, 'help': 0x20f984,
           'NumInGamePlayers': 0x20f98c}


def stdcam_probe(memory):
    pbg, = struct.unpack_from('<I', memory, PBG)
    pb4, = struct.unpack_from('<I', memory, pbg + 4)
    win, = struct.unpack_from('<I', memory, MBOXWIN)
    blocks = {'std': (STDCAM, 0x1b0), 'pb': (pb4, 0x3b0), 'win': (win, 0x1c0)}
    for k in range(4):
        base = PLAYERS + PLAYER_SIZE * k
        blocks[f'player{k}_50'] = (base + 0x50, 0x30)
        blocks[f'player{k}_fc'] = (base + 0xfc, 4)
        blocks[f'player{k}_93c'] = (base + 0x93c, 4)
    for name, address in GLOBALS.items():
        blocks[name] = (address, 4)
    if any(a % 4 or n % 4 or not 0 < n <= 2048 for a, n in blocks.values()):
        raise ValueError('stdcam blocks must be aligned and bounded')

    def capture(read):
        pointer, = struct.unpack('<I', read(PBG, 4))
        if pointer != pbg:
            raise ValueError('pbg binding changed')
        return {name: read(a, n).hex() for name, (a, n) in blocks.items()}

    regions = [(a, memory[a:a + n]) for name, (a, n) in blocks.items() if name in ('std', 'pb', 'win')]
    return capture, regions
