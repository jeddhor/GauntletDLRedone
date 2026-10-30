#!/usr/bin/env python3
"""Private-data audit of the whole Git history (before a push; M21).

  history_audit.py TAG

Every blob reachable from any ref (tools/git-local.sh rev-list --objects
--all) gets release_audit.py's checks: a SHA-256 equal to a private file
(disc, WAD entries, ISO, captures, start states, BIOS), equal to one of the
user's saves (read only), a disc-format or private/target*/runs/reports
path, the identity strings (this machine's user folders, the repository's
mount path, the global git configuration's name, e-mail address and its
local part; raw and UTF-16LE), and credential patterns (private keys,
GitHub/AWS/Slack tokens). Every commit and annotated tag (author, committer
and tagger lines, messages) gets the identity check too: the repository's
own user.* is its public identity (research/release.md "History rewrite").
Findings name the blob's path and the first commit holding it, or the
commit or tag; identity strings are reported by index, not spelled out.
Report: private/reports/history-audit-TAG.json.
"""
import collections
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys

import release_audit as ra

ROOT = pathlib.Path(__file__).resolve().parents[2]
GIT = ['bash', str(ROOT / 'tools/git-local.sh')]
SECRETS = re.compile(rb'BEGIN [A-Z ]*PRIVATE KEY|gh[opsu]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|xox[abprs]-[A-Za-z0-9-]{10,}')


def git(*args, data=None):
    return subprocess.run(GIT + list(args), input=data, capture_output=True, check=True).stdout


def main():
    if len(sys.argv) != 2 or not sys.argv[1].replace('-', '').isalnum():
        raise SystemExit(__doc__)
    report = ROOT / 'private/reports' / f'history-audit-{sys.argv[1]}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    known = ra.private_hashes()
    saves_dir = pathlib.Path(os.environ.get('XDG_DATA_HOME') or pathlib.Path.home() / '.local/share') / 'gdl-native/SLUS-20047/saves'
    saves = {ra.file_sha(p) for p in saves_dir.glob('*') if p.is_file()} if saves_dir.exists() else set()
    needles = ra.identity_needles()
    paths = collections.defaultdict(set)
    for line in git('rev-list', '--objects', '--all').decode().splitlines():
        oid, _, name = line.partition(' ')
        if name:
            paths[oid].add(name)
    kinds = git('cat-file', '--batch-check=%(objectname) %(objecttype) %(objectsize)', data='\n'.join(paths).encode() + b'\n')
    blobs = [(o, int(s)) for o, t, s in (k.split() for k in kinds.decode().splitlines()) if t == 'blob']
    first = {}  # blob -> the oldest commit holding it (for the report)
    for commit in git('rev-list', '--reverse', '--all').decode().split():
        for line in git('ls-tree', '-r', commit).decode().splitlines():
            first.setdefault(line.split()[2], commit[:7])
    findings = []
    for p in sorted(set().union(*paths.values())):
        if ra.NAME_PATTERNS.search(p.lower()) or ra.PRIVATE_PATH.search(p):
            findings.append({'kind': 'name', 'path': p})
    cat = subprocess.Popen(GIT + ['cat-file', '--batch'], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    for oid, size in blobs:
        cat.stdin.write(oid.encode() + b'\n')
        cat.stdin.flush()
        cat.stdout.readline()
        data = cat.stdout.read(size)
        cat.stdout.read(1)
        where = {'path': sorted(paths[oid])[0], 'blob': oid, 'first_commit': first.get(oid)}
        digest = hashlib.sha256(data).hexdigest()
        if digest in known:
            findings.append({'kind': 'game data', **where, 'equals': known[digest]})
        if digest in saves:
            findings.append({'kind': 'save', **where})
        for i, (_, needle) in enumerate(needles):
            if needle in data:
                findings.append({'kind': 'identity', **where, 'needle': i, 'count': data.count(needle)})
        if SECRETS.search(data):
            findings.append({'kind': 'credential pattern', **where})
    tags = [o for o, t in (line.split() for line in git('for-each-ref', '--format=%(objectname) %(objecttype)', 'refs/tags').decode().splitlines()) if t == 'tag']
    for oid in git('rev-list', '--all').decode().split() + tags:
        cat.stdin.write(oid.encode() + b'\n')
        cat.stdin.flush()
        _, kind, size = cat.stdout.readline().split()
        data = cat.stdout.read(int(size))
        cat.stdout.read(1)
        for i, (_, needle) in enumerate(needles):
            if needle in data:
                findings.append({'kind': f'identity in {kind.decode()}', 'object': oid[:7], 'needle': i, 'count': data.count(needle)})
    cat.stdin.close()
    cat.wait()
    head = git('rev-parse', 'HEAD').decode().strip()
    head_paths = set(git('ls-tree', '-r', '--name-only', 'HEAD').decode().splitlines())
    head_blobs = {line.split()[2] for line in git('ls-tree', '-r', 'HEAD').decode().splitlines()}
    for f in findings:
        if f['kind'] == 'name':
            f['at_head'] = f['path'] in head_paths
        elif 'object' in f:
            f['at_head'] = head.startswith(f['object'])
        else:
            f['at_head'] = f['blob'] in head_blobs
    result = {'scope': 'every blob reachable from any ref', 'head': head, 'commits': len(git('rev-list', '--all').split()),
              'blobs': len(blobs), 'private_hashes': len(known), 'save_files': len(saves), 'identity_needles': len(needles),
              'findings': findings, 'at_head': [f for f in findings if f['at_head']],
              'pass_at_head': not any(f['at_head'] for f in findings), 'pass': not findings}
    report.write_text(json.dumps(result, indent=1) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k not in ('findings', 'scope')}, indent=1))
    for f in findings:
        print(f)
    print(report)


if __name__ == '__main__':
    main()
