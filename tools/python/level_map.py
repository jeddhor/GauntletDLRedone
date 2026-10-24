#!/usr/bin/env python3
"""Top-down diagnostic map of a level (M14 traversal tooling; DEBUG, not a
fidelity input).

  level_map.py REPORT.json NEW_MAP.png [--explore EXPLORE_REPORT.json]
               [--y-range LO HI]

REPORT.json is a game_run.py report made with --navdump NAV.json and
--itemdump (the nav dump path is read from the report's directory as given
on the command line: pass --nav NAV.json). Floors are dots coloured by
height (every column's floors; the bot graph's reachable nodes from the
start drawn larger), with the progress items over them: exits (red stars),
gates (blue squares), keys (gold circles), triggers (magenta triangles,
labelled with their slot), the realm's legend item (cyan diamond); the start
(green cross) and, with --explore, the search's progress timeline (orange)
and its frontier (red cross).
"""
import argparse
import json
import pathlib
import struct

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt  # noqa: E402

KINDS = {9: ('exit', '*', 'red', 220), 7: ('gate', 's', 'blue', 70), 5: ('trigger', '^', 'magenta', 70)}


def items(report):
    d = report['item_dump']
    defs = [bytes.fromhex(h) for h in d['definitions']]
    out = []
    for i, h in enumerate(d['records']):
        rec = bytes.fromhex(h)
        k = struct.unpack_from('<I', rec, 0)[0]
        if k >= len(defs):
            continue
        kind, sub = struct.unpack_from('<ii', defs[k], 0)
        pos = struct.unpack_from('<3f', rec, 0x40)
        out.append((i, kind, sub, pos))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('report', type=pathlib.Path)
    ap.add_argument('out', type=pathlib.Path)
    ap.add_argument('--nav', type=pathlib.Path, required=True)
    ap.add_argument('--explore', type=pathlib.Path)
    ap.add_argument('--y-range', type=float, nargs=2)
    a = ap.parse_args()
    if a.out.exists():
        raise SystemExit(f'{a.out} exists')
    report = json.loads(a.report.read_text())
    nav = json.loads(a.nav.read_text())
    fig, ax = plt.subplots(figsize=(14, 14), dpi=90)
    allf = nav['all_floors']
    ys = [p[1] for p in allf] or [0, 1]
    lo, hi = a.y_range or (min(ys), max(ys))
    sc = ax.scatter([p[0] for p in allf], [p[2] for p in allf], c=[p[1] for p in allf], s=1, cmap='viridis', vmin=lo, vmax=hi, alpha=0.35)
    reach = [nav['pts'][k] for k in nav['reachable']]
    ax.scatter([p[0] for p in reach], [p[2] for p in reach], c=[p[1] for p in reach], s=5, cmap='viridis', vmin=lo, vmax=hi)
    fig.colorbar(sc, ax=ax, shrink=0.6, label='floor height (y)')
    for i, kind, sub, p in items(report):
        if kind in KINDS:
            name, m, col, size = KINDS[kind]
        elif kind == 1 and sub == 2:
            name, m, col, size = 'key', 'o', 'gold', 60
        elif kind == 1 and sub == 0xd:
            name, m, col, size = 'legend', 'D', 'cyan', 120
        else:
            continue
        ax.scatter([p[0]], [p[2]], marker=m, c=col, s=size, edgecolors='black', linewidths=0.6, zorder=5)
        ax.annotate(f'{i}' + (f'/{sub}' if kind == 5 else '') + f' y{p[1]:.0f}', (p[0], p[2]), fontsize=6, xytext=(3, 3), textcoords='offset points', zorder=6)
    s = nav['start']
    ax.scatter([s[0]], [s[2]], marker='x', c='green', s=200, linewidths=3, zorder=7)
    if a.explore:
        x = json.loads(a.explore.read_text())
        x = x.get('explore', x)
        tl = [t['pos'] for t in x.get('timeline', []) if 'pos' in t]
        if tl:
            ax.plot([p[0] for p in tl], [p[2] for p in tl], '-o', c='orange', ms=4, lw=1, zorder=6)
        f = x.get('frontier')
        if f:
            ax.scatter([f['pos'][0]], [f['pos'][2]], marker='x', c='red', s=250, linewidths=3, zorder=8)
            (bx0, _, bz0), (bx1, _, bz1) = f['box_at_max']
            ax.add_patch(plt.Rectangle((bx0, bz0), bx1 - bx0, bz1 - bz0, fill=False, ec='red', lw=1, ls='--', zorder=8))
    ax.set_aspect('equal')
    ax.invert_yaxis()
    ax.set_xlabel('x')
    ax.set_ylabel('z')
    ax.set_title(f"{a.report.name} (DEBUG map: bot graph and progress items)")
    fig.savefig(a.out, bbox_inches='tight')
    print(a.out)


if __name__ == '__main__':
    main()
