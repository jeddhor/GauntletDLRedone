#!/usr/bin/env python3
"""Installing a newer package over an older installation (M21).

  update_check.py TAG OLD_PACKAGE_DIR NEW_PACKAGE_DIR ISO

In a `kwin_wayland --virtual --xwayland` session on the real GPU, with a
fresh home under /tmp (HOME and XDG_*; ALSA into the null device; no
gamepads): the old package's installer installs from the ISO (`--batch`);
the installed old game plays a TEST character (welcome scrolls closed by
triangle), saves it to card slot 0 and stores changed settings (TEST
actions) — the player's state an update must keep; the new package's
installer installs over the same folder; then the new game starts from
slot 0 (GDL_TEST_LOAD_SLOT=1: the store numbers card file k as k + 1). Checked: the saves folder (saves, backups,
settings.json) byte-identical across the update; the installed programs
equal the new package's; `gdl_import verify` none differing; no staging or
previous-data folder left; the new game loads the old save and runs 300
updates. The home is deleted afterwards.

Report: private/reports/update-TAG.json; logs in private/runs/update-TAG/.
"""
import hashlib
import json
import os
import pathlib
import shlex
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]


def tree(path):
    return {str(p.relative_to(path)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(path.rglob('*')) if p.is_file()} if path.exists() else {}


def top(path):
    """The SHA-256 of the files directly in a folder (an installation's programs)."""
    return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(path.iterdir()) if p.is_file()} if path.exists() else {}


def main():
    if len(sys.argv) != 5:
        raise SystemExit(__doc__)
    tag = sys.argv[1]
    old, new, iso = (pathlib.Path(a).resolve() for a in sys.argv[2:5])
    if not tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    report = ROOT / 'private/reports' / f'update-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = ROOT / 'private/runs' / f'update-{tag}'
    out.mkdir(parents=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix=f'gdl-update-{tag}-'))
    home = work / 'home'
    home.mkdir()
    (home / '.asoundrc').write_text('pcm.!default { type null }\nctl.!default { type hw card 0 }\n')
    dest = home / 'GauntletDarkLegacy'
    saves = home / '.local/share/gdl-native/SLUS-20047/saves'
    (out / 'menu.json').write_text(json.dumps([[30 + 30 * k, 32 + 30 * k, [0xc]] for k in range(8)]))
    # The settings action is applied on the rendered frame at its update; a
    # frame may step two updates, so it is repeated (it is idempotent).
    (out / 'actions-old.json').write_text(json.dumps([{'at': 200, 'save': 0}]
                                                     + [{'at': u, 'settings': {'widescreen': True, 'msaa': 2}} for u in range(205, 216)]))
    q = lambda p: shlex.quote(str(p))
    env = (f'HOME={shlex.quote(str(home))} XDG_DATA_HOME={shlex.quote(str(home / ".local/share"))} '
           f'XDG_CONFIG_HOME={shlex.quote(str(home / ".config"))} XDG_CACHE_HOME={shlex.quote(str(home / ".cache"))}')
    game = (f'GDL_TEST_AUTOSTART=1 GDL_NO_ATTRACT=1 GDL_NO_GAMEPADS=1 GDL_MENU_SCRIPT={shlex.quote(str(out / "menu.json"))}')
    # Two sessions, so that the saves are fingerprinted between the installs.
    first, second = out / 'session-1.sh', out / 'session-2.sh'
    first.write_text(f'''#!/bin/bash
unset WAYLAND_DISPLAY
cd {q(work)}
env {env} {q(old / 'GauntletDarkLegacy-Installer')} --batch --source {q(iso)} --dest {q(dest)} > {q(out / 'install-old.log')} 2>&1
echo $? > {q(out / 'install-old.exit')}
cd {q(dest)}
env {env} {game} GDL_TEST_ACTIONS={q(out / 'actions-old.json')} GDL_EXIT_AFTER=300 \\
    GDL_STATE_OUT={q(out / 'state-old.txt')} ./gdl_play > {q(out / 'game-old.log')} 2>&1
echo $? > {q(out / 'game-old.exit')}
''')
    second.write_text(f'''#!/bin/bash
unset WAYLAND_DISPLAY
cd {q(work)}
env {env} {q(new / 'GauntletDarkLegacy-Installer')} --batch --source {q(iso)} --dest {q(dest)} > {q(out / 'install-new.log')} 2>&1
echo $? > {q(out / 'install-new.exit')}
cd {q(dest)}
env {env} {game} GDL_TEST_LOAD_SLOT=1 GDL_EXIT_AFTER=300 GDL_UI_TRACE={q(out / 'ui-new.trace')} \\
    GDL_STATE_OUT={q(out / 'state-new.txt')} ./gdl_play > {q(out / 'game-new.log')} 2>&1
echo $? > {q(out / 'game-new.exit')}
''')
    for f in (first, second):
        f.chmod(0o755)
    kw = [sys.executable, str(ROOT / 'tools/python/kwin_session.py')]
    subprocess.run(kw + [str(out / 'kwin-1.log'), '--', str(first)], timeout=1200)
    before = tree(saves)
    old_programs = top(dest)
    subprocess.run(kw + [str(out / 'kwin-2.log'), '--', str(second)], timeout=1200)
    after_saves = tree(saves)
    installed = top(dest)
    package = top(new / 'payload')
    programs = {n: installed.get(n) == package.get(n) for n in ('gdl_play', 'gdl_import')}
    verify = subprocess.run([str(dest / 'gdl_import'), 'verify', str(dest / 'data')], capture_output=True, text=True).stdout.strip()
    try:
        verify = json.loads(verify.splitlines()[-1])
    except (ValueError, IndexError):
        pass
    leftovers = sorted(p.name for p in dest.iterdir() if p.name.startswith('.gdl'))
    read = lambda n: (out / n).read_text().strip() if (out / n).exists() else None
    settings = json.loads((saves / 'settings.json').read_text()) if (saves / 'settings.json').exists() else {}
    trace = (read('ui-new.trace') or '').splitlines()
    result = {
        'scope': 'a newer Linux package installed over an older installation (actual disc image; real GPU through a virtual kwin session)',
        'old_package': str(old), 'new_package': str(new),
        'install_old_exit': read('install-old.exit'), 'game_old_exit': read('game-old.exit'), 'updates_old': len((read('state-old.txt') or '').splitlines()),
        'saves_before': before, 'saves_after_equal': before == after_saves and bool(before),
        'settings_kept': settings.get('widescreen') is True and settings.get('msaa') == 2,
        'install_new_exit': read('install-new.exit'), 'install_new_last': (read('install-new.log') or '').splitlines()[-3:],
        'old_programs_replaced': {n: old_programs.get(n) != installed.get(n) for n in ('gdl_play', 'gdl_import')},
        'programs_equal_new_package': programs, 'verify': verify, 'leftovers': leftovers,
        'game_new_exit': read('game-new.exit'), 'updates_new': len((read('state-new.txt') or '').splitlines()),
        'new_game_trace_head': trace[:4],
    }
    result['pass'] = (result['install_old_exit'] == '0' and result['game_old_exit'] == '0' and result['saves_after_equal']
                      and result['settings_kept'] and result['install_new_exit'] == '0' and all(programs.values())
                      and isinstance(verify, dict) and verify.get('differing') == [] and not leftovers
                      and result['game_new_exit'] == '0' and result['updates_new'] == 300)
    report.write_text(json.dumps(result, indent=1) + '\n')
    shutil.rmtree(work, ignore_errors=True)
    print(json.dumps({k: v for k, v in result.items() if k not in ('saves_before', 'scope')}, indent=1))
    print(report)


if __name__ == '__main__':
    main()
