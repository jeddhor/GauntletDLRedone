#!/usr/bin/env python3
"""Private-data audit of a release folder (M21; research/release.md).

  release_audit.py TAG OUT_DIR

OUT_DIR is tools/package.sh's output: the package folders, their archives
(.tar.gz, .zip), the source archive and library-source/. Every file of the
package folders and every member of the archives (the source archive's
included) is checked for:

  game data   a SHA-256 equal to a file of the disc (private/disc), an
              extracted WAD entry (private/wad), the ISO, a reference
              capture or memory card (private/reference/captures), a start
              state (private/reference/start-states) or the BIOS
              (private/reports/reference-bios-hashes.json); a disc-format
              name (.PSS, .IRX, .ELF, SLUS_*, WAD.BIN, .p2s, .ps2 card, BIOS)
  saves       a file equal to one in the user's save folder, read only
              ($XDG_DATA_HOME/gdl-native/SLUS-20047/saves), or named save*.json
  identity    the user's name in a path or e-mail address (raw, UTF-16LE),
              this machine's mount and home paths
  private     a member path under private/, target*/ or a reports/runs folder

library-source/ holds third-party source archives: listed with their
SHA-256 (docs/toolchain.md records each one's provenance), not unpacked.
Report: private/reports/release-audit-TAG.json.
"""
import hashlib
import io
import json
import os
import pathlib
import re
import sys
import tarfile
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
NAME_PATTERNS = re.compile(r'(\.pss|\.irx|\.elf|\.p2s|\.iso|\.mec|\.nvm|\.rom0|/wad\.bin|/slus_[0-9.]+|^slus_[0-9.]+|^wad\.bin)$', re.I)
PRIVATE_PATH = re.compile(r'(^|/)(private|target[^/]*|runs|reports)/')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 22), b''):
            h.update(block)
    return h.hexdigest()


def private_hashes():
    known = {}
    for base in ('private/disc', 'private/wad', 'private/reference/captures', 'private/reference/start-states'):
        for p in (ROOT / base).rglob('*'):
            if p.is_file():
                known[file_sha(p)] = str(p.relative_to(ROOT))
    for iso in ROOT.glob('*.iso'):
        known[file_sha(iso)] = iso.name
    bios = ROOT / 'private/reports/reference-bios-hashes.json'
    if bios.exists():
        for name, digest in json.loads(bios.read_text()).items():
            known[digest] = f'BIOS {name}'
    return known


def identity_needles():
    user = os.environ.get('USER') or pathlib.Path.home().name
    raw = {f'/home/{user}', f'/run/media/{user}', f'C:\\Users\\{user}', f'C:/Users/{user}', str(ROOT)}
    needles = []
    for n in sorted(raw):
        needles.append((n, n.encode()))
        needles.append((n + ' (UTF-16LE)', n.encode('utf-16-le')))
    return needles


def members(path):
    """(name, bytes) of every file in a package folder or archive."""
    if path.is_dir():
        for p in sorted(path.rglob('*')):
            if p.is_file():
                yield str(p.relative_to(path.parent)), p.read_bytes()
    elif path.name.endswith('.tar.gz'):
        with tarfile.open(path) as t:
            for m in t.getmembers():
                if m.isfile():
                    yield m.name, t.extractfile(m).read()
    elif path.name.endswith('.zip'):
        with zipfile.ZipFile(path) as z:
            for i in z.infolist():
                if not i.is_dir():
                    yield i.filename, z.read(i)


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    tag, out = sys.argv[1], pathlib.Path(sys.argv[2]).resolve()
    report = ROOT / 'private/reports' / f'release-audit-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    known = private_hashes()
    saves_dir = pathlib.Path(os.environ.get('XDG_DATA_HOME') or pathlib.Path.home() / '.local/share') / 'gdl-native/SLUS-20047/saves'
    saves = {file_sha(p): p.name for p in saves_dir.glob('*') if p.is_file()} if saves_dir.exists() else {}
    needles = identity_needles()
    findings, scanned = [], {}
    targets = sorted(p for p in out.iterdir() if p.is_dir() and p.name != 'library-source' or p.name.endswith(('.tar.gz', '.zip')))
    for target in targets:
        n = 0
        for name, data in members(target):
            n += 1
            digest = sha(data)
            where = f'{target.name}: {name}'
            if digest in known:
                findings.append({'kind': 'game data', 'file': where, 'equals': known[digest]})
            if digest in saves:
                findings.append({'kind': 'save', 'file': where, 'equals': saves[digest]})
            if NAME_PATTERNS.search(name.lower()) or re.search(r'(^|/)save\d*\.json$', name):
                findings.append({'kind': 'name', 'file': where})
            if PRIVATE_PATH.search(name.split('/', 1)[-1] if '/' in name else name):
                findings.append({'kind': 'private path', 'file': where})
            for label, needle in needles:
                if needle in data:
                    findings.append({'kind': 'identity', 'file': where, 'contains': label, 'count': data.count(needle)})
        scanned[target.name] = n
    lib = out / 'library-source'
    library = {p.name: file_sha(p) for p in sorted(lib.iterdir())} if lib.exists() else {}
    result = {'scope': 'release folder private-data audit', 'out': str(out), 'private_hashes': len(known), 'save_files': len(saves),
              'identity_needles': [label for label, _ in needles], 'scanned': scanned, 'library_source': library,
              'findings': findings, 'pass': not findings}
    report.write_text(json.dumps(result, indent=1) + '\n')
    print(json.dumps({k: result[k] for k in ('private_hashes', 'save_files', 'scanned', 'pass')}, indent=1))
    for f in findings[:40]:
        print(f)
    print(report)


if __name__ == '__main__':
    main()
