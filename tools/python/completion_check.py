#!/usr/bin/env python3
"""Completion persistence check (M15) on the native client.

  completion_check.py TAG STATE.json

STATE.json is a game-run end state (a chained campaign's last level).
gdl_tools state-save stores its player 0 save region as native save slot 1
(DEBUG tooling: the state stands in for the walk from the level end to the
Tower). gdl_play then loads that slot (GDL_TEST_LOAD_SLOT, the select
screen's Play), enters the Tower, dumps player 1's progress ("loaded"),
saves from the Tower (PlayerWriteSaveFile, card file 0) and dumps again
("saved"); a second run loads the saved slot ("reloaded"). Checks: the
loaded character's completed-level bytes, runes, shards and gold equal the
end state's, and survive the client's own save and reload. Report:
private/reports/completion-TAG.json.
"""
import json
import os
import pathlib
import struct
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import scenarios  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS = os.environ.get('GDL_TOOLS', str(ROOT / 'target' / 'release' / 'gdl_tools'))


def expected(state):
    d = json.loads(pathlib.Path(state).read_text())
    r = bytes.fromhex(d['players'][0])
    cls = struct.unpack_from('<I', r, 0xc)[0]
    waves = [r[0x182c + 0xe * c:0x182c + 0xe * c + 0xe].hex() for c in range(16)]
    gold = struct.unpack_from('<i', r, 0xcec + 0xb4 * cls + 0x30)[0]
    return dict(waves=waves, runes=struct.unpack_from('<H', r, 0x1a26)[0], shards=struct.unpack_from('<H', r, 0x1a24)[0], gold=gold, cls=cls)


def main():
    if len(sys.argv) != 3 or not sys.argv[1].replace('-', '').isalnum():
        raise SystemExit(__doc__)
    tag, state = sys.argv[1], sys.argv[2]
    report = ROOT / 'private' / 'reports' / f'completion-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    saves = ROOT / 'private' / 'runs' / f'completion-{tag}' / 'saves'
    if saves.exists():
        raise SystemExit(f'{saves} exists')
    save_report = ROOT / 'private' / 'reports' / f'completion-{tag}-statesave.json'
    subprocess.run([TOOLS, 'state-save', state, str(saves), '1', str(save_report)], cwd=ROOT, check=True)
    want = expected(state)
    T, UP, CROSS = scenarios.TRIANGLE, scenarios.UP, scenarios.CROSS
    sc = dict(level=None, load_slot=1, updates=900,
              actions=[{"at": 400, "dump": "loaded"}, {"at": 410, "save": 0}, {"at": 420, "dump": "saved"}],
              menu=scenarios.presses(T, 30, 12, 30),
              then=dict(level=None, load_slot=1, updates=500, actions=[{"at": 400, "dump": "reloaded"}], menu=scenarios.presses(T, 30, 12, 30)),
              checks=[("loaded", "world", "==", 13), ("loaded", f"waves.{want['cls']}", "==", want['waves'][want['cls']]),
                      ("loaded", "runes", "==", want['runes']), ("loaded", "shards", "==", want['shards']), ("loaded", "gold", "==", want['gold']),
                      ("saved", "waves", "==", "loaded.waves"), ("reloaded", "world", "==", 13),
                      ("reloaded", "waves", "==", "loaded.waves"), ("reloaded", "runes", "==", "loaded.runes"),
                      ("reloaded", "shards", "==", "loaded.shards")])
    out = scenarios.run(f'completion-{tag}', 'load', sc, saves_from=saves)
    out.update(state=state, statesave=str(save_report.relative_to(ROOT)), expected=want,
               scope='M15 completion persistence: the end state stored as a native save (DEBUG tooling), loaded, saved and reloaded by gdl_play')
    with report.open('x') as f:
        json.dump(out, f, indent=1)
    print('PASS' if out['passed'] else 'FAIL', [(c['dump'], c['field'], c['value'], c['op'], c['expected_value']) for c in out['checks'] if not c['ok']])


if __name__ == '__main__':
    main()
