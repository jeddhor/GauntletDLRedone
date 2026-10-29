#!/usr/bin/env python3
"""Check that a Linux package's installer runs on its bundled Qt alone (M21).

  qt_bundle_check.py TAG PACKAGE_DIR --batch ISO
  qt_bundle_check.py TAG PACKAGE_DIR --wayland

The installer runs in a bubblewrap sandbox in which the system has no Qt:
every libQt6 library, every library the package bundles (lib/) and Qt's
plugin folder are masked (empty files, an empty folder), so loading any of
them from the system fails. LD_DEBUG=libs records each object loaded; the
check requires Qt Core, Gui, Widgets and the platform plugin from the
package and nothing from a masked path.

--batch ISO   `--batch` install from ISO into a fresh home under /tmp,
              without a display (batch mode opens no window, so no
              platform plugin is loaded); the installation is deleted
              afterwards.
--wayland     the wizard in a `kwin_wayland --virtual` session with
              QT_QPA_PLATFORM=wayland for 8 seconds, once in the sandbox
              (where it is loaded from) and once outside it (kwin gives
              sandboxed clients no outputs): it must stay up, and outside
              the sandbox log nothing.

Report: private/reports/qt-bundle-TAG.json. installer_gui_check.py
--qt-sandbox drives the wizard's pages in the same sandbox.
"""
import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
LIBDIR = pathlib.Path('/usr/lib/x86_64-linux-gnu')


def sandbox(package, keep=()):
    """bwrap arguments hiding the system's Qt and the bundled libraries
    (except the sonames in KEEP)."""
    real = {p.resolve() for p in LIBDIR.glob('libQt6*.so*') if p.exists()}
    for f in (package / 'lib').iterdir():
        if (LIBDIR / f.name).exists() and f.name not in keep:
            real.add((LIBDIR / f.name).resolve())
    args = ['bwrap', '--dev-bind', '/', '/', '--die-with-parent', '--tmpfs', str(LIBDIR / 'qt6')]
    for r in sorted(real):
        args += ['--ro-bind', '/dev/null', str(r)]
    return args, len(real)


def loaded(debug_dir, exe):
    """Objects the dynamic loader initialised (LD_DEBUG=libs) in the
    processes of EXE (not the importer or the game it starts)."""
    objs = set()
    for f in pathlib.Path(debug_dir).glob('ld.*'):
        text = f.read_text(errors='replace')
        if not re.search(rf'initialize program: {re.escape(str(exe))}$', text, re.M):
            continue
        for line in text.splitlines():
            m = re.search(r'calling init: (\S+)', line)
            if m:
                objs.add(m.group(1))
    return objs


def verdict(package, debug_dir, platform):
    objs = loaded(debug_dir, package / 'GauntletDarkLegacy-Installer')
    bundled = {f.name for f in (package / 'lib').iterdir()}
    pkg = str(package) + '/'
    from_system = sorted(o for o in objs if not o.startswith(pkg) and (
        pathlib.Path(o).name in bundled or 'libQt6' in o or '/qt6/' in o))
    need = ['lib/libQt6Core.so.6', 'lib/libQt6Gui.so.6', 'lib/libQt6Widgets.so.6'] + ([f'plugins/platforms/{platform}'] if platform else [])
    missing = [n for n in need if pkg + n not in objs]
    return dict(loaded_from_package=sorted(o[len(pkg):] for o in objs if o.startswith(pkg)),
                loaded_from_system_instead=from_system, required_not_loaded=missing,
                ok=not from_system and not missing)


def main():
    args = sys.argv[1:]
    if len(args) < 3 or not ({'--batch', '--wayland'} & set(args)):
        raise SystemExit(__doc__)
    tag, package = args[0], pathlib.Path(args[1]).resolve()
    report = ROOT / 'private/reports' / f'qt-bundle-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    work = pathlib.Path(tempfile.mkdtemp(prefix=f'gdl-qt-bundle-{tag}-'))
    home, debug = work / 'home', work / 'ld'
    home.mkdir()
    debug.mkdir()
    box, masked = sandbox(package)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('GDL_', 'QT_', 'LD_'))}
    env.update(HOME=str(home), XDG_DATA_HOME=str(home / '.local/share'), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_CACHE_HOME=str(home / '.cache'), GDL_NO_AUDIO='1', LD_DEBUG='libs', LD_DEBUG_OUTPUT=str(debug / 'ld'))
    exe = str(package / 'GauntletDarkLegacy-Installer')
    out = dict(package=str(package), masked_files=masked)
    try:
        if '--batch' in args:
            for k in ('DISPLAY', 'XAUTHORITY', 'WAYLAND_DISPLAY'):
                env.pop(k, None)
            iso = str(pathlib.Path(args[args.index('--batch') + 1]).resolve())
            dest = home / 'GauntletDarkLegacy'
            t0 = time.time()
            r = subprocess.run(box + [exe, '--batch', '--source', iso, '--dest', str(dest)], env=env,
                               capture_output=True, text=True, timeout=900)
            out.update(mode='batch', seconds=round(time.time() - t0, 1), exit=r.returncode,
                       last_lines=(r.stdout + r.stderr).strip().splitlines()[-3:],
                       installed=(dest / 'install.json').exists())
            if out['installed']:
                v = subprocess.run([str(dest / 'gdl_import'), 'verify', str(dest / 'data')], capture_output=True, text=True)
                out['verify'] = json.loads(v.stdout.strip().splitlines()[-1]) if v.stdout.strip() else v.stderr.strip()[-200:]
            out.update(verdict(package, debug, None))
            out['ok'] = (out['ok'] and r.returncode == 0 and out['installed'] and isinstance(out.get('verify'), dict)
                         and out['verify'].get('differing') == [])
        else:
            # Pass 1 in the sandbox: what is loaded from where. Under bwrap,
            # kwin offers the client no outputs and the client reports a
            # broken connection when the sandbox is stopped; the system's Qt
            # does the same there, so pass 2 runs the package unsandboxed
            # (its own Qt first through DT_RPATH and qt.conf) for the log.
            passes = {}
            for name, prefix in (('sandboxed', box), ('unsandboxed', [])):
                d = work / name
                d.mkdir()
                log, state, session = d / 'installer.log', d / 'state', d / 'session.sh'
                session.write_text('#!/bin/bash\n'
                                   f'env LD_DEBUG=libs LD_DEBUG_OUTPUT={shlex.quote(str(d / "ld"))} '
                                   + ' '.join(shlex.quote(a) for a in prefix)
                                   + f' env QT_QPA_PLATFORM=wayland {shlex.quote(exe)} > {shlex.quote(str(log))} 2>&1 &\n'
                                   'p=$!\nsleep 8\n'
                                   f'if kill -0 $p 2>/dev/null; then echo up > {shlex.quote(str(state))}; kill $p; '
                                   f'else echo exited > {shlex.quote(str(state))}; fi\nwait $p\n')
                session.chmod(0o755)
                # Nothing reaches the user's desktop: kwin's own socket only.
                kenv = {k: v for k, v in env.items() if k not in ('LD_DEBUG', 'LD_DEBUG_OUTPUT', 'WAYLAND_DISPLAY', 'DISPLAY')}
                subprocess.run(['kwin_wayland', '--virtual', '--socket', f'gdl-qt-{os.getpid()}-{name}', '--width', '1280',
                                '--height', '900', '--no-lockscreen', '--exit-with-session', str(session)],
                               env=kenv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
                v = verdict(package, d, 'libqwayland.so')
                v['window_state'] = state.read_text().strip() if state.exists() else 'no session'
                v['log'] = log.read_text(errors='replace').strip().splitlines() if log.exists() else []
                v['wayland_shell_from_package'] = 'plugins/wayland-shell-integration/libxdg-shell.so' in v['loaded_from_package']
                passes[name] = v
            out.update(mode='wayland', passes=passes)
            out['ok'] = (all(v['ok'] and v['window_state'] == 'up' and v['wayland_shell_from_package'] for v in passes.values())
                         and passes['unsandboxed']['log'] == [])
    finally:
        shutil.rmtree(work, ignore_errors=True)
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(out, indent=1) + '\n')
    brief = {k: v for k, v in out.items() if k not in ('loaded_from_package', 'passes')}
    for name, v in out.get('passes', {}).items():
        brief[name] = {k: x for k, x in v.items() if k != 'loaded_from_package'}
    print(json.dumps(brief, indent=1))
    raise SystemExit(0 if out.get('ok') else 1)


if __name__ == '__main__':
    main()
