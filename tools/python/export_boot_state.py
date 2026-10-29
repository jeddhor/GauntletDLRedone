#!/usr/bin/env python3
"""Write the client's built-in new-game start state (M21).

  export_boot_state.py NEW.json

The native client starts every game from a start-state template (build_game
overwrites the players in play, the classes, the level and the session
values it carries). Until M21 the template was always the private capture's
export (export_game_state.py, poison-fields-game-v6: globals, extra values,
camera rest pose, enemy load tables, four player records). An installation
has no capture, so the client embeds this sanitised copy: the same measured
values, with player 1's record (the captured character, always replaced by
the selected one) replaced by player 2's — the original's record for an
unused slot (30 non-zero init bytes) — with its slot number (+0x0, +0x4) 0. Nothing here is recovered from the
original's boot code; the values are MEASURED (labelled in the output).
"""
import hashlib
import json
import pathlib
import sys

SOURCE = pathlib.Path('private/reference/start-states/poison-fields-game-v6.json')


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    out = pathlib.Path(sys.argv[1])
    raw = SOURCE.read_bytes()
    g = json.loads(raw)
    if g['schema_version'] != 6:
        raise SystemExit('unexpected start-state schema')
    # The unused slots' records differ only in +0x0 and +0x4, the slot
    # number; player 1's replacement gets 0 there.
    def slot(rec, k):
        b = bytearray(bytes.fromhex(rec))
        b[0:4] = k.to_bytes(4, 'little')
        b[4:8] = k.to_bytes(4, 'little')
        return b.hex()
    unused = g['players'][1]
    if any(g['players'][k] != slot(unused, k) for k in (2, 3)):
        raise SystemExit('the unused player records differ beyond their slot number; review before embedding')
    g['players'][0] = slot(unused, 0)
    g['label'] = ('MEASURED new-game start state (M21): globals, extra values, camera rest pose and enemy load tables of '
                  'the Poison Fields fixture; player records are the original\'s unused-slot record (player 1\'s captured '
                  'character removed; build_game replaces the players in play). Not recovered from boot code.')
    g['derived_from'] = {'file': str(SOURCE), 'sha256': hashlib.sha256(raw).hexdigest(), 'tool': 'tools/python/export_boot_state.py'}
    out.write_text(json.dumps(g, separators=(',', ':')) + '\n')
    print(out, out.stat().st_size)


if __name__ == '__main__':
    main()
