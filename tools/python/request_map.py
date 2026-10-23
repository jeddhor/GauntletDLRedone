#!/usr/bin/env python3
"""Runtime archive request map (M04) from reference_warp.py --sample-filename
reports: each sampled GetWadInfo name (wad_last_filename) with its archive
entry (wad-independent.json dictionary names) and how gdl_play loads it
(play-config.json path, catalog label, or skipped when the archive has no
entry). Usage: request_map.py NEW_REPORT.json (inputs are the private reports
named below).
"""
import json
import re
import sys

R = 'private/reports/'
w = json.load(open(R + 'wad-independent.json'))
by_name = {}
for x in w['entries']:
    for n in x['proposed_names']:
        by_name[n.upper()] = x['index']
cfg = json.load(open(R + 'play-config.json'))
# gdl_play reads these by label through the catalog (package_entry / catalog.read).
BY_LABEL = ('SELECT/', 'FONTS/', 'SHPDATA/SHOP.WAD', 'TEXT/', 'STATIC/', 'TITLE/')
used = {int(m) for m in re.findall(r'private/wad/(\d{6})-', json.dumps(cfg))}
runs = [('requests-G1exit-tower-v2.json', 'G1 exit -> tally (idle)'), ('requests-tally-tower-v1.json', 'tally -> shop -> select Done -> Tower start')]
out = dict(schema_version=2, supersedes='level-requests-tower-v1.json (its native column knew only config paths)', source='reference_warp.py --sample-filename (wad_last_filename after every vblank; GetWadInfo 0x1d47f8)',
           granularity='one sample per vblank: requests within one vblank show only the last', runs=[])
for f, label in runs:
    log = json.load(open(R + f))['log']
    reqs = []
    for e in log:
        if 'filename' not in e or e['step'] == 0:
            continue
        name = e['filename'].replace('\\', '/').upper()
        idx = by_name.get(name)
        reqs.append(dict(step=e['step'], cur_world=e['cur_world'], game_mode=hex(e['game_mode']), request=e['filename'],
                         archive_index=idx, outcome='entry' if idx is not None else 'no archive entry (lookup returns 0)',
                         native_client=('config path' if idx in used else 'catalog label' if any(name.startswith(l) for l in BY_LABEL) else 'not loaded')
                         if idx is not None else 'skipped (no archive entry)'))
    out['runs'].append(dict(report=f, label=label, requests=reqs))
with open(sys.argv[1], 'x') as fh:
    json.dump(out, fh, indent=1)
for r in out['runs']:
    for q in r['requests']:
        print(q['step'], q['request'], q['archive_index'], q['native_client'])
