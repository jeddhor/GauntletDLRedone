#!/usr/bin/env python3
"""Export the interim playtest start state from the user's preserved save.

Spawn rules are not recovered, so the labelled playtest starts from the saved
Poison Fields scene. Output stays in ignored private/. Every field cites the
record offset or global it came from; nothing here is a recovered default.
"""
import hashlib
import json
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reference_snapshot import frozen_memory  # noqa: E402

FIXTURE = pathlib.Path('private/reference/captures/poison-fields-original.p2s')
OUT = pathlib.Path('private/reference/start-states/poison-fields-sor-v2.json')
PLAYER, OBJECTS_EE = 0x221550, 0x11e4cb8


def main():
    if OUT.exists():
        raise SystemExit(f'{OUT} exists; choose a new version')
    m = frozen_memory(FIXTURE)
    f = lambda a: struct.unpack_from('<f', m, a)[0]
    u = lambda a: struct.unpack_from('<I', m, a)[0]
    p = lambda o: PLAYER + o
    obj = lambda a: None if a == 0 else (a - OBJECTS_EE) // 60
    camera = u(0x2a1550)
    sim = dict(
        position=list(struct.unpack_from('<3f', m, p(0x50))),
        matrix=[list(struct.unpack_from('<3f', m, p(0x20 + 16 * r))) for r in range(3)],
        matrix_scale=f(p(0x34)), facing=f(p(0x86c)),
        collision_offset_81c=list(struct.unpack_from('<3f', m, p(0x81c))), offset_flags_84=u(p(0x84)),
        floor_y_88c=f(p(0x88c)), floor_object_89c=obj(u(p(0x89c))), flags_8ac=u(p(0x8ac)),
        word_100=struct.unpack_from('<i', m, p(0x100))[0], walking_timer=f(p(0x874)), motion_code=u(p(0x1dc)),
        current_action=u(p(0x1e0)), requested_action=u(p(0x1e4)),
        stats=dict(speed_124=f(p(0x124)), scale_964=f(p(0x964)), scale_96c=f(p(0x96c)),
                   radius_828=f(p(0x828)), height_82c=f(p(0x82c)), turn_968=f(p(0x968))),
        status_93c=struct.unpack_from('<H', m, p(0x93c))[0], state_934=struct.unpack_from('<h', m, p(0x934))[0],
        flags_138=u(p(0x138)), word_80c=u(p(0x80c)), flags_8b0=u(p(0x8b0)),
        attack_state_8c8=struct.unpack_from('<i', m, p(0x8c8))[0],
        camera_90=f(camera + 0x90), camera_98=f(camera + 0x98), spf=f(0x20f908), world_bottom=f(0x23e21c),
        boss_type=struct.unpack_from('<i', m, 0x25d2c4)[0], quest_item_state=u(0x23e210))
    action_map = [list(struct.unpack_from('<Ii', m, p(0x1e8 + 8 * i))) for i in range(150)]
    presentation = dict(
        camera_basis=[list(struct.unpack_from('<3f', m, 0x2bc740 + 16 * r)) for r in range(3)],
        camera_distance=f(0x2bc834), attention_offset_810=list(struct.unpack_from('<3f', m, p(0x810))),
        camera_fov=f(camera + 0x1c),
        action_map=action_map,
        evidence=('StdCamera 0x2bc740 rows (right, up, forward); its position equals target - forward*distance '
                  'with target = position + attention offset +0x810 and distance at 0x2bc834 in the fixture; '
                  'camera_fov is MboxWinPtr+0x1c (axis convention unverified). '
                  'Action map: runtime table at player+0x1e8, (clip, frames) per action, built by DoPlayerAction '
                  'from class data at load; captured, not recovered.'))
    report = dict(schema_version=1, source_fixture=str(FIXTURE),
                  fixture_sha256=hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
                  label='INTERIM PLAYTEST START: player state copied from the user save; spawn rules unrecovered',
                  sim=sim, presentation=presentation)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open('x') as file:
        json.dump(report, file, indent=2)
        file.write('\n')
    print(f'wrote {OUT}')


if __name__ == '__main__':
    main()
