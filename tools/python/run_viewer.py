#!/usr/bin/env python3
"""Launch a native data inspector for a hash-qualified archive package label."""
import argparse
import hashlib
import json
import pathlib
import subprocess
from virtual_display import environment as virtual_environment


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('package', nargs='?', default='TITLE', help='dictionary package label, e.g. TITLE or STATIC')
    p.add_argument('--scene', action='store_true', help='static world/mesh inspector (e.g. LEVELS/levelG1)')
    p.add_argument('--rig', action='store_true', help='translation-only rig inspector (e.g. PLAYERS/SOR/YEL00)')
    args = p.parse_args()
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    archive = json.loads(pathlib.Path('private/wad/manifest.json').read_text())
    if proposed['source_sha256'] != archive['source_sha256']:
        raise ValueError('name candidates and extraction refer to different archives')
    paths = []
    if args.rig and args.scene:
        raise ValueError('choose either scene or rig mode')
    basenames = ['OBJECTS.PS2', 'ANIM.PS2', 'TEXDEF.PS2', 'TEXTURES.PS2'] if args.rig else ['OBJECTS.PS2', 'WORLDS.PS2', 'TEXDEF.PS2', 'TEXTURES.PS2'] if args.scene else ['TEXDEF.PS2', 'TEXTURES.PS2']
    for basename in basenames:
        target = args.package.upper().rstrip('/') + '/' + basename
        entries = [e for e in proposed['entries'] if target in [n.upper() for n in e['proposed_names']]]
        if len(entries) != 1:
            raise ValueError(f'unknown/ambiguous package label: {target}')
        e = entries[0]
        identity = archive['entries'][e['index']]
        path = pathlib.Path('private/wad') / identity['output_file']
        if hashlib.sha256(path.read_bytes()).hexdigest() != e['decoded_sha256']:
            raise ValueError('decoded source identity differs')
        paths.append(path)
    binary = 'gdl_scene' if args.scene or args.rig else 'gdl_client'
    flags = ['--rig'] if args.rig else []
    result = subprocess.run([f'target/release/{binary}', *flags, *map(str, paths)], env=virtual_environment())
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
