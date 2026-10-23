#!/usr/bin/env python3
"""Fixture-qualified record-based player observations for reference_trace.py --player.

Retains, per vblank step, player slot 0's complete record (0x2b00), the four
PlayerControl records, the MboxWinPtr window block (+0x90/+0x98 camera
forward), the StdCamera record (CameraLimitPlayerDpos), the player node's local
matrix and parent link (+0x80 → node +0x00/+0x74) and the globals the
native do_players/PlayerMotion/DoPlayerAction read, all as raw bytes.
Addresses come from the executable's own symbol table. Never a runtime
dependency.
"""
import pathlib
import struct

ELF = pathlib.Path('private/disc/SLUS_200.47')
GLOBALS = ['Time', 'SPF', 'ATime', 'AnimTimeScale', 'vb_elapsed', 'AuxLevels', 'game_mode', 'cam_wait',
           'start_of_stage', 'NumVisibleEnemies', 'StopTime', 'EnemyShrink', 'QuestItemState', 'BossType',
           'BossActive', 'WldBtm', 'paused', 'help', 'trig_cam_active', 'NumActivePlayers', 'skip_wave',
           'quit_level', 'auto_exit_flag', 'someone_wants_to_exit', 'firstgetidx', 'disable_player_controls',
           # Camera state main's dispatch reads (boss camera, transmitters, DoShake).
           'BossCameraMode', 'BossCam', 'BossCamMode', 'CurTransmitter', 'Cur3DTransmitter', 'shaking', 'shake_type',
           'shake_priority', 'shake_delay', 'shake_count', 'shake_rad', 'ZOOM_FRAMES', 'resetStdCam', 'WorldStartTime',
           # The boss completion: DoGoodWizard's state and the auto exit.
           'GoodWizard', 'good_wiz_timer', 'good_wiz_alpha', 'good_wiz_speech_idx', 'good_wiz_speech_frame',
           'good_wiz_speech_pause', 'good_wiz_yaw', 'good_wiz_plyr_attn', 'all_rune_stones', 'auto_exit_timer',
           'BossIsDead', 'BossIsDying', 'optmenu_disable']


def elf_symbols(names):
    b = ELF.read_bytes()
    shoff, = struct.unpack_from('<I', b, 0x20)
    shnum, = struct.unpack_from('<H', b, 0x30)
    secs = [struct.unpack_from('<10I', b, shoff + 40 * i) for i in range(shnum)]
    sym = next(s for s in secs if s[1] == 2)
    strtab = secs[sym[6]]
    found = {}
    for i in range(sym[5] // 16):
        nm, val, size, _info, _other, _shn = struct.unpack_from('<IIIBBH', b, sym[4] + 16 * i)
        n = b[strtab[4] + nm:b.index(b'\0', strtab[4] + nm)].decode()
        if n in names:
            found[n] = val
    missing = set(names) - set(found)
    if missing:
        raise ValueError(f'symbols not in the executable: {sorted(missing)}')
    return found


def player_probe(memory):
    sym = elf_symbols(set(GLOBALS) | {'players', 'PlayerControl', 'MboxWinPtr', 'StdCamera', 'Options', 'PlayerRootNode', 'Cam',
                                      'BossCamera', 'BossCamData', 'good_wiz_objgrp'})
    player, control = sym['players'], sym['PlayerControl']
    win, = struct.unpack_from('<I', memory, sym['MboxWinPtr'])
    # player1: the second record (players stride 0x2b00), for co-op traces.
    blocks = {'player0': (player, 0x2b00), 'player1': (player + 0x2b00, 0x2b00), 'control': (control, 0xf0), 'win': (win, 0xb0),
              'stdcam': (sym['StdCamera'], 0x1b0), 'options': (sym['Options'], 0x30),
              # Cam[0]'s matrix rows (Cam +0x10..+0x3c) and eye (+0x40..+0x4c).
              'cam0': (sym['Cam'] + 0x10, 0x40),
              # The boss camera record (0x1b0, the StdCam layout).
              'bosscam': (sym['BossCamera'], 0x1b0),
              # good_wiz_objgrp: +0x00 the wizard node's matrix, +0x40 and
              # +0x50 the attention point, +0x60 the node.
              'good_wiz_objgrp': (sym['good_wiz_objgrp'], 0x70)}
    # The level's BCAM record (BossCamData, 0x54 bytes) when one is bound.
    bcam, = struct.unpack_from('<I', memory, sym['BossCamData'])
    if bcam and bcam % 4 == 0 and 0x100000 <= bcam < 0x2000000 - 0x54:
        blocks['bcam'] = (bcam, 0x54)
    blocks.update({g: (sym[g], 4) for g in GLOBALS})
    if any(a % 4 or n % 4 or n <= 0 for a, n in blocks.values()):
        raise ValueError('player blocks must be aligned')

    def chunks(read, address, size):
        return b''.join(read(address + o, min(2048, size - o)) for o in range(0, size, 2048))

    def capture(read):
        pointer, = struct.unpack('<I', read(sym['MboxWinPtr'], 4))
        if pointer != win:
            raise ValueError('MboxWinPtr binding changed')
        out = {name: chunks(read, a, n).hex() for name, (a, n) in blocks.items()}
        # The player's node (player +0x80 → UpdateObjWorldMat's target): its
        # local matrix (+0x00..+0x40) and parent link (+0x74).
        node, = struct.unpack('<I', read(player + 0x80, 4))
        if node and node % 16 == 0 and node + 0x80 <= 32 * 1024 * 1024:
            raw = read(node, 0x80)
            out['node'] = raw[:0x40].hex()
            out['node_parent'] = raw[0x74:0x78].hex()
            out['player_root_node'] = read(sym['PlayerRootNode'], 4).hex()
        return out

    regions = []
    for name in ('player0', 'control', 'win', 'stdcam'):
        a, n = blocks[name]
        regions += [(a + o, memory[a + o:a + min(n, o + 2048)]) for o in range(0, n, 2048)]
    return capture, regions
