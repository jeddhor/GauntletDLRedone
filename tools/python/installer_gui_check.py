#!/usr/bin/env python3
"""Drive the Qt installer's pages on the private Xvfb (M21).

  installer_gui_check.py TAG PACKAGE_DIR [--mount ISO]

A fresh home (HOME, XDG_* under private/runs/installer-gui-TAG/home: no
shortcut or save reaches the user's own folders) and GDL_NO_AUDIO for the
game it may start. With --mount the ISO is attached read-only through
udisks (udisksctl loop-setup, mount) like a desktop's disc, so the disc page
must find it as a drive; it is unmounted and detached afterwards. Keys go
to the installer's window (xdotool; the default button is Next/Finish):
welcome → disc (waits for the probe) → destination (the default folder) →
install (waits for the end) → finish (start the game), a screenshot of
each page. With --cancel the installation is cancelled two seconds in
(Escape, then Yes): nothing of it may remain (no data/, no staging
directory). Report: private/reports/installer-gui-TAG.json.
"""
import json
import pathlib
import re
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/python'))
from virtual_display import environment  # noqa: E402


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        raise SystemExit(__doc__)
    tag, package = args[0], pathlib.Path(args[1]).resolve()
    iso = pathlib.Path(args[args.index('--mount') + 1]).resolve() if '--mount' in args else None
    cancel = '--cancel' in args
    run = ROOT / 'private/runs' / f'installer-gui-{tag}'
    report = ROOT / 'private/reports' / f'installer-gui-{tag}.json'
    if run.exists() or report.exists():
        raise SystemExit(f'{run} or {report} exists')
    home = run / 'home'
    home.mkdir(parents=True)
    env = environment()
    for k in list(env):
        if k.startswith('GDL_'):
            env.pop(k)
    env.update(HOME=str(home), XDG_DATA_HOME=str(home / '.local/share'), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_CACHE_HOME=str(home / '.cache'), XDG_DESKTOP_DIR=str(home / 'Desktop'), GDL_NO_AUDIO='1', QT_QPA_PLATFORM='xcb')
    xenv = dict(DISPLAY=env['DISPLAY'], XAUTHORITY=env['XAUTHORITY'], PATH='/usr/bin:/bin')
    loop = mount = None
    out = dict(package=str(package), shots=[], steps=[])

    def x(*a):
        return subprocess.run(['xdotool', *a], env=xenv, capture_output=True, text=True).stdout.strip()

    def shot(label):
        win = x('search', '--onlyvisible', '--name', 'Dark Legacy — Installer')
        if win:
            path = run / f'{len(out["shots"]):02d}-{label}.png'
            subprocess.run(['import', '-window', win.split()[0], str(path)], env=xenv, capture_output=True)
            out['shots'].append(path.name)

    def key(k):
        win = x('search', '--onlyvisible', '--name', 'Dark Legacy — Installer').split()
        if win:
            x('windowactivate', '--sync', win[0])
            x('key', '--window', win[0], k)

    game = None
    try:
        if iso:
            r = subprocess.run(['udisksctl', 'loop-setup', '-r', '-f', str(iso)], capture_output=True, text=True, check=True)
            loop = re.search(r'(/dev/loop\d+)', r.stdout).group(1)
            time.sleep(2)
            r = subprocess.run(['udisksctl', 'mount', '-b', loop], capture_output=True, text=True)
            m = re.search(r' at (.+?)\.?$', r.stdout.strip())
            mount = m.group(1) if m else None
            if mount is None:  # the desktop may have mounted it already
                mount = subprocess.run(['findmnt', '-n', '-o', 'TARGET', loop], capture_output=True, text=True).stdout.strip() or None
            out['mount'] = dict(loop=loop, at=mount)
        proc = subprocess.Popen([str(package / 'GauntletDarkLegacy-Installer')], cwd=str(home), env=env,
                                stdout=open(run / 'installer.log', 'w'), stderr=subprocess.STDOUT)
        for _ in range(60):
            time.sleep(0.5)
            if x('search', '--onlyvisible', '--name', 'Dark Legacy — Installer'):
                break
        time.sleep(1.5)
        shot('welcome')
        key('Return')
        time.sleep(6)  # drive scan and probe
        shot('disc')
        key('Return')
        time.sleep(2)
        shot('destination')
        key('Return')
        t0 = time.time()
        shot('installing')
        dest = home / 'GauntletDarkLegacy'
        if cancel:
            time.sleep(2)
            key('Escape')
            time.sleep(1)
            shot('cancel-question')
            key('alt+y')
            time.sleep(6)
            shot('cancelled')
            out['cancelled_left'] = sorted(p.name for p in dest.iterdir()) if dest.exists() else []
            out['data_present'] = (dest / 'data').exists()
            out['staging_left'] = sorted(p.name for p in dest.glob('.gdl-*')) if dest.exists() else []
            key('Escape')
            time.sleep(1)
            subprocess.run(['kill', str(proc.pid)])
            return

        while time.time() - t0 < 600 and not (dest / 'install.json').exists():
            time.sleep(1)
        out['install_seconds'] = round(time.time() - t0, 1)
        time.sleep(3)
        shot('finished')
        key('Return')  # Finish with "Start the game now"
        time.sleep(25)
        game = subprocess.run(['pgrep', '-f', str(dest / 'gdl_play')], capture_output=True, text=True).stdout.split()
        out['game_started'] = bool(game)
        win = x('search', '--name', 'Gauntlet: Dark Legacy - INTERIM')
        if win:
            path = run / f'{len(out["shots"]):02d}-game.png'
            subprocess.run(['import', '-window', win.split()[0], str(path)], env=xenv, capture_output=True)
            out['shots'].append(path.name)
        out['installer_exit'] = proc.wait(timeout=30)
        out['installed'] = (dest / 'install.json').exists()
        out['shortcuts'] = sorted(str(p.relative_to(home)) for p in home.rglob('gauntlet-dark-legacy.desktop'))
    finally:
        for pid in game or []:
            subprocess.run(['kill', pid])
        if loop:
            if mount:
                subprocess.run(['udisksctl', 'unmount', '-b', loop], capture_output=True)
            subprocess.run(['udisksctl', 'loop-delete', '-b', loop], capture_output=True)
        report.write_text(json.dumps(out, indent=1) + '\n')
    print(json.dumps(out, indent=1))


if __name__ == '__main__':
    main()
