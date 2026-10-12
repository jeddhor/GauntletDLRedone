#!/usr/bin/env python3
"""Launch the native texture viewer for an observed archive package label."""
import argparse
import hashlib
import json
import pathlib
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('package', nargs='?', default='TITLE', help='dictionary package label, e.g. TITLE or STATIC')
    args = p.parse_args()
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    archive = json.loads(pathlib.Path('private/wad/manifest.json').read_text())
    if proposed['source_sha256'] != archive['source_sha256']:
        raise ValueError('name candidates and extraction refer to different archives')
    paths = []
    for basename in ['TEXDEF.PS2', 'TEXTURES.PS2']:
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
    result = subprocess.run(['target/release/gdl_client', *map(str, paths)])
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
