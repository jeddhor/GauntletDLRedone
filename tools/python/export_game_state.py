#!/usr/bin/env python3
"""Export the v5 native game start state from the user's preserved save.

Captured (not recovered) values: player records, session globals, camera rest
pose and view-plane constants, per-type enemy load tables. Guest pointers in
player records are cleared or converted to native indices; the list of
converted fields is written into the report. Items and enemies are not
copied: the native game loads the level's items fresh (a labelled interim
start) and starts with no enemies. Output stays in ignored private/.
"""
import hashlib
import json
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reference_snapshot import frozen_memory  # noqa: E402

FIXTURE = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
OUT = pathlib.Path('private/reference/start-states/poison-fields-game-v5.json')
PLAYERS, PLAYER_SIZE, OBJECTS = 0x221550, 0x2b00, 0x11e4cb8
# Pointer fields of the player record: (offset, conversion).
POINTERS = [(0x80, 'clear'), (0x90, 'clear'), (0x690, 'clear'), (0x694, 'clear'), (0x6a0, 'clear'),
            (0x6a4, 'clear'), (0x6a8, 'clear'), (0x6ac, 'clear'), (0x6b4, 'clear'), (0x708, 'clear'),
            (0x768, 'clear'), (0x880, 'clear'), (0x884, 'clear'), (0x888, 'clear'),
            (0x89c, 'object'), (0x8a0, 'object')]
GLOBALS = dict(time=(0x20f904, 'f'), spf=(0x20f908, 'f'), itime=(0x20f910, 'i'), vb_elapsed=(0x20f91c, 'i'),
               game_mode=(0x20f978, 'I'), cur_world=(0x23e180, 'i'), cur_level=(0x23e184, 'i'),
               boss_type=(0x25d2c4, 'i'), boss_index=(0x25d2c8, 'i'), boss_active=(0x25d190, 'i'),
               boss_is_dead=(0x25d194, 'i'), good_wizard=(0x23e28c, 'i'), quest_item_state=(0x23e210, 'I'),
               world_bottom=(0x23e21c, 'f'), aux_levels=(0x210788, 'I'), options=(0x20f948, 'i'),
               options_4=(0x20f94c, 'I'), num_active_players=(0x20f988, 'i'), num_in_game_players=(0x20f98c, 'i'),
               num_virtual_players=(0x20f990, 'i'), num_visible_enemies=(0x233594, 'i'), global_958=(0x20f958, 'i'),
               enemy_shrink=(0x22015c, 'f'), secret_world=(0x23e228, 'i'), max_keys_per_player=(0x23e1a4, 'i'),
               max_potions_per_player=(0x23e1a8, 'i'), trig_cam_active=(0x25d53c, 'i'), start_of_stage=(0x22016c, 'i'),
               end_of_stage=(0x220170, 'i'), cam_wait=(0x220168, 'i'), paused=(0x20f8fc, 'i'), help=(0x20f984, 'i'),
               skip_wave=(0x22011c, 'i'), quit_level=(0x21edb4, 'i'), demo_mode=(0x20ea88, 'i'),
               active_players=(0x220114, 'i'), auto_exit_flag=(0x220130, 'i'), someone_wants_to_exit=(0x220134, 'i'),
               firstgetidx=(0x221538, 'i'), it=(0x22c154, 'i'), rune_disp_timer=(0x22c4fc, 'i'),
               wave_timer=(0x220120, 'f'), wave_complete=(0x220128, 'i'), max_level_coins=(0x23ccac, 'i'),
               pvp_mode=(0x21edd4, 'i'), atime=(0x20f914, 'f'), anim_time_scale=(0x2bc3a0, 'f'))
EXTRA = dict(item_active_dist=(0x234364, 'f'), mode_950=(0x20f950, 'i'), globals_954=(0x20f954, 'i'),
             start_enemyinfo=(0x23e1ac, 'i'), random_item_num=(0x23c76c, 'I'), vb_counter=(0x2a1c78, 'I'),
             mode_970=(0x20f970, 'i'), stop_time=(0x220158, 'i'), boss_is_dying=(0x25d198, 'i'),
             newcam_mode=(0x21f130, 'i'), max_enemy_radius=(0x249044, 'f'), max_item_radius=(0x249048, 'f'),
             boss_crit_inst=(0x25d2c0, 'i'), target_cone_mode=(0x21edcc, 'i'))


def main():
    if OUT.exists():
        raise SystemExit(f'{OUT} exists; choose a new version')
    m = frozen_memory(FIXTURE)
    val = lambda a, t: struct.unpack_from('<' + t, m, a)[0]
    players = []
    for k in range(4):
        rec = bytearray(m[PLAYERS + PLAYER_SIZE * k: PLAYERS + PLAYER_SIZE * (k + 1)])
        for off, how in POINTERS:
            p = struct.unpack_from('<I', rec, off)[0]
            v = 0
            if how == 'object' and p:
                v = (p - OBJECTS) // 60 + 1
            struct.pack_into('<I', rec, off, v)
        players.append(rec.hex())
    globals_ = {k: val(a, t) for k, (a, t) in GLOBALS.items()}
    globals_['camera_90'] = val(val(0x2a1550, 'I') + 0x90, 'f')
    globals_['camera_98'] = val(val(0x2a1550, 'I') + 0x98, 'f')
    extra = {k: val(a, t) for k, (a, t) in EXTRA.items()}
    view = val(val(0x25d870, 'I') + 4, 'I')
    std = val(0x21f0dc, 'I')
    camera = dict(basis=[list(struct.unpack_from('<3f', m, 0x2bc740 + 16 * r)) for r in range(3)],
                  distance=val(0x2bc834, 'f'), target=list(struct.unpack_from('<3f', m, std + 0xa4)),
                  planes=list(struct.unpack_from('<6f', m, view + 0x60)), newcam_mode=extra['newcam_mode'],
                  view_matrix=[list(struct.unpack_from('<4f', m, view + 0x210 + 16 * r)) for r in range(4)])
    enemy_load = dict(model=list(struct.unpack_from('<45i', m, 0x233650)),
                      loaded=list(struct.unpack_from('<45i', m, 0x233708)),
                      genfx=list(struct.unpack_from('<45i', m, 0x233fd8)))
    # PlayerData[k] -> class record; offsets read by ported code so far.
    classes = []
    for k in range(4):
        c = val(0x2342a0 + 4 * k, 'I')
        classes.append(None if c == 0 else dict(powerup_time_58=val(c + 0x58, 'f'),
                                                missile_origin_5c=list(struct.unpack_from('<3f', m, c + 0x5c)),
                                                missile_origin_158=list(struct.unpack_from('<3f', m, c + 0x158)),
                                                raw_hex=m[c:c + 0x200].hex()))
    report = dict(schema_version=5, classes=classes, source_fixture=str(FIXTURE),
                  fixture_sha256=hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
                  label='INTERIM GAME START: player records/globals captured from the user save; '
                        'items loaded fresh, no enemies; spawn rules unrecovered',
                  players=players, pointer_fields=[[hex(o), h] for o, h in POINTERS], globals=globals_,
                  extra=extra, camera=camera, enemy_load=enemy_load)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open('x') as f:
        json.dump(report, f, indent=1)
        f.write('\n')
    print(f'wrote {OUT}')


if __name__ == '__main__':
    main()
