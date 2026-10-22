#!/usr/bin/env python3
"""PCSX2 state slots of the virtual reference and their preserved captures.

Slots 1..8 are bound to fixed captures (reference-setup.md). Slot 9 is a
staging slot: a preserved capture is copied into it for a run, and it may be
overwritten only while it holds a byte-identical copy of some preserved
capture (nothing unpreserved is ever lost). Slots 1 and 2 are never written.
"""
import hashlib
import pathlib
import shutil

CAPTURES = pathlib.Path('private/reference/captures')
SSTATES = pathlib.Path('private/reference/config/PCSX2/sstates')
SLOTS = {1: 'poison-fields-original.p2s', 2: 'tower-newgame-v1.p2s', 3: 'w2l1-newgame-v1.p2s',
         4: 'w1l1-newgame-v1.p2s', 5: 'w11l1-newgame-v1.p2s', 6: 'w7l1-newgame-v1.p2s', 7: 'w11l1-newgame-v2.p2s',
         8: 'w2l6-boss-v1.p2s'}
STAGE_SLOT = 9


def state_file(slot):
    return SSTATES / f'SLUS-20047 (15030637).{slot:02d}.p2s'


def _sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stage_is_preserved():
    """True when slot 9 is absent or a byte-identical copy of a preserved capture."""
    target = state_file(STAGE_SLOT)
    if not target.exists():
        return True
    digest = _sha(target)
    return any(_sha(p) == digest for p in CAPTURES.glob('*.p2s'))


def capture_path(name):
    if not name or '/' in name or '\\' in name or not name.endswith('.p2s'):
        raise ValueError('capture must be a bare .p2s name under private/reference/captures')
    path = CAPTURES / name
    if not path.is_file():
        raise ValueError(f'{path}: no such preserved capture')
    return path


def stage(name):
    """Copy a preserved capture into slot 9; returns (slot, capture path)."""
    path = capture_path(name)
    if not stage_is_preserved():
        raise ValueError('slot 9 holds a state that is not a preserved capture')
    shutil.copyfile(path, state_file(STAGE_SLOT))
    return STAGE_SLOT, path


def resolve(slot, staged):
    """(slot, preserved capture) for --slot N or --stage NAME, with the slot file checked."""
    if staged:
        slot, frozen = stage(staged)
    else:
        if slot not in SLOTS:
            raise ValueError(f'slot {slot} is not bound to a capture')
        frozen = CAPTURES / SLOTS[slot]
    if _sha(state_file(slot)) != _sha(frozen):
        raise ValueError(f'private slot {slot} differs from its preserved capture')
    return slot, frozen
