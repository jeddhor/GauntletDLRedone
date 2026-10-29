#!/usr/bin/env python3
"""Read the current level's AUDS stage record from the paused reference.

  reference_stage.py --out private/reports/NEW.json

Read-only PINE (pine_probe's client): CurWorld, level_data 0x23e158 (the
LEVL record LoadWorldData chose), its code (+8), AUDS index (+0x5a) and
stage pointer (+0x64, set by ResolveWorldData 0x161738), world_data
0x23e150's AUDS base (+0x28) and the stage record's bank (+0), world-sound
index (+0x10), name line (+0x14, ResolveWorldData's AudioFindSound of
"S_<code>NAME") and stream (+0x18). Checks the serial and a code probe
first; refuses output outside private/.
"""
import argparse
import hashlib
import json
import pathlib
import socket
import struct

from pine_probe import Pine, configured_socket

LEVEL_DATA, WORLD_DATA, CUR_WORLD = 0x23e158, 0x23e150, 0x23e180


def text(words):
    raw = b''.join(struct.pack('<I', w) for w in words)
    return raw.split(b'\0')[0].decode('latin1')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--socket', type=pathlib.Path, default=configured_socket())
    p.add_argument('--out', type=pathlib.Path, required=True)
    args = p.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('reference evidence must stay in private/')
    profile = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    elf = pathlib.Path('private/disc/SLUS_200.47').read_bytes()
    if hashlib.sha256(elf).hexdigest() != profile['executable']['sha256']:
        raise ValueError('local ELF identity differs from profile')
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(args.socket))
        pine = Pine(connection)
        if pine.text(12) != 'SLUS-20047':
            raise ValueError('connected reference serial differs')
        # AudioEnterNextStage 0x15df80's first four words, as loaded.
        seg = next(s for s in profile['load_segments'] if s['virtual_address'] <= 0x15df80 < s['virtual_address'] + s['file_size'])
        off = seg['file_offset'] + 0x15df80 - seg['virtual_address']
        if struct.pack('<4I', *pine.read_ee32([0x15df80 + 4 * i for i in range(4)])) != elf[off:off + 16]:
            raise ValueError('loaded code probe differs')
        status = pine.status()
        level, world, cur_world = pine.read_ee32([LEVEL_DATA, WORLD_DATA, CUR_WORLD])
        lw = pine.read_ee32([level + 8, level + 0x58, level + 0x64])
        auds_base = pine.read_ee32([world + 0x28])[0]
        stage = lw[2]
        sw = pine.read_ee32([stage + 4 * i for i in range(15)])
        status_after = pine.status()
    report = dict(schema_version=1, source_profile=profile['profile_id'], address_space='EE physical RAM',
                  coherence='paused diagnostic' if status == status_after == 1 else 'live non-atomic diagnostic',
                  cur_world=cur_world, level_data=hex(level), level_code=text([lw[0]]),
                  levl_5a=struct.unpack('<h', struct.pack('<I', lw[1])[2:])[0], stage_pointer=hex(stage),
                  auds_base=hex(auds_base), stage_index=(stage - auds_base) / 0x3c,
                  stage_bank=text(sw[0:4]), stage_word_10=struct.unpack('<h', struct.pack('<I', sw[4])[:2])[0],
                  stage_name_line=hex(sw[5]), stage_stream=text(sw[6:10]))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open('x') as f:
        json.dump(report, f, indent=1)
    print(json.dumps(report))


if __name__ == '__main__':
    main()
