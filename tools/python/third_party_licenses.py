#!/usr/bin/env python3
"""Collect the third-party notices of the shipped programs (M21).

  third_party_licenses.py NEW.txt

The Rust crates compiled into gdl_play and gdl_import for Linux and Windows
(`cargo tree --offline --locked -e normal`, build-time proc-macros left
out), each with its license expression and the license/notice files of its
package in the local Cargo registry (identical texts printed once, with the
crates that carry them); then the name dictionary the shipped catalog uses
(MIT), Real-ESRGAN (BSD-3-Clause, the HD texture network's weights) and
Qt 6 (LGPL-3.0, the installer). Offline; reads only local files.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
REGISTRY = sorted((pathlib.Path.home() / '.cargo/registry/src').glob('*'))
TARGETS = ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-gnu']
NOTICE = re.compile(r'^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)', re.I)


def crates():
    out = {}
    for t in TARGETS:
        r = subprocess.run(['cargo', 'tree', '--offline', '--locked', '-p', 'gdl_client', '-p', 'gdl_install', '--target', t,
                            '-e', 'normal', '--prefix', 'none', '--format', '{p}\t{l}'],
                           cwd=ROOT, capture_output=True, text=True, check=True)
        for line in r.stdout.splitlines():
            line = line.replace(' (*)', '')
            if '\t' not in line or '(proc-macro)' in line:
                continue
            p, lic = line.split('\t', 1)
            parts = p.split()
            if len(parts) < 2 or (len(parts) > 2 and parts[2].startswith('(/')):
                continue  # this workspace's own crates
            out[(parts[0], parts[1].lstrip('v'))] = lic.strip()
    return out


def package_dir(name, version):
    for reg in REGISTRY:
        d = reg / f'{name}-{version}'
        if d.is_dir():
            return d
    return None


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    found = crates()
    texts = {}
    rows = []
    missing = []
    for (name, version), lic in sorted(found.items()):
        d = package_dir(name, version)
        files = sorted(f for f in d.iterdir() if f.is_file() and NOTICE.match(f.name)) if d else []
        if not files:
            missing.append(f'{name} {version}')
        for f in files:
            texts.setdefault(f.read_text(errors='replace').strip(), []).append(f'{name} {version}')
        rows.append(f'{name} {version} — {lic}')
    out = ['THIRD-PARTY NOTICES — Gauntlet: Dark Legacy native port', '=' * 56, '',
           'The programs gdl_play and gdl_import contain the following Rust crates',
           f'({len(rows)} packages; the license chosen where a choice is offered is any',
           'of the listed permissive ones; "Apache-2.0 OR GPL-2.0-only" is taken under',
           'Apache-2.0). Their license and notice files follow the list.', '']
    out += rows
    if missing:
        out += ['', 'Packages without a license file in their package (their license',
                'expression above applies; the standard texts are included below):'] + missing
    out += ['', '', 'LICENSE AND NOTICE TEXTS', '=' * 24]
    for text, owners in sorted(texts.items(), key=lambda kv: kv[1][0]):
        out += ['', '-' * 72, 'Used by: ' + ', '.join(owners), '-' * 72, text]
    dictionary = ROOT / 'private/upstream/gdl_wad_decoder/license'
    out += ['', '', 'NAME DICTIONARY', '=' * 15, '',
            'import/catalog.json names the game archive\'s entries with the name',
            'dictionary of haekb/gdl_wad_decoder (commit 981f4ec9586903257e8f2161e918bbbe5263a6eb),',
            'matched by path hash. Its license:', '', dictionary.read_text().strip()]
    esrgan = ROOT / 'crates/gdl_client/assets/hd/LICENSE-Real-ESRGAN.txt'
    out += ['', '', 'REAL-ESRGAN', '=' * 11, '',
            'gdl_play (HD textures, a native enhancement) contains network weights derived',
            'from Real-ESRGAN v0.3.0\'s released realesr-general-x4v3 and',
            'realesr-general-wdn-x4v3 checkpoints (mixed 0.5 / 0.5), and runs an',
            'independently written implementation of its SRVGGNetCompact network.',
            'Real-ESRGAN: https://github.com/xinntao/Real-ESRGAN. Its license:', '', esrgan.read_text().strip()]
    out += ['', '', 'QT', '=' * 2, '',
            'The installer (GauntletDarkLegacy-Installer) uses the Qt 6 libraries (Qt Core,',
            'Qt Gui, Qt Widgets and their platform plugins), (C) The Qt Company Ltd and',
            'other contributors, under the GNU Lesser General Public License version 3',
            '(LGPL-3.0-only; https://www.gnu.org/licenses/lgpl-3.0.html, extending the GPL',
            'version 3 in LICENSE). The packages carry them beside the installer, unmodified',
            'and dynamically linked; LIBRARY_LICENSES.txt in each package lists those files',
            'and the libraries they need, with their notices, and the Qt source they were',
            'built from is distributed with the release (library-source/). The game and the',
            'importer do not use Qt.']
    pathlib.Path(sys.argv[1]).write_text('\n'.join(out) + '\n')
    print(sys.argv[1], len(rows), 'crates', len(texts), 'distinct texts', len(missing), 'without files')


if __name__ == '__main__':
    main()
