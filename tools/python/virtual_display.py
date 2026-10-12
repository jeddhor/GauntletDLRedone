#!/usr/bin/env python3
"""Hold an authenticated xvfb-run session for private reference/viewer tooling.

Start with xvfb-run -a -n 191 -s '-screen 0 1280x1024x24 -nolisten tcp'
python3 tools/python/virtual_display.py. All child GUI commands must use the
recorded DISPLAY/XAUTHORITY. Never fall back to the user's desktop.
"""
import json
import os
import pathlib
import signal


def environment():
    path = pathlib.Path('private/reference/xvfb-session.json')
    context = json.loads(path.read_text())
    pid = context['holder_pid']
    if not pathlib.Path(f'/proc/{pid}').is_dir():
        raise ValueError('virtual display session is no longer running')
    if 'virtual_display.py' not in pathlib.Path(f'/proc/{pid}/cmdline').read_bytes().decode(errors='replace'):
        raise ValueError('virtual display holder identity differs')
    status = pathlib.Path(f'/proc/{pid}/status').read_text()
    parent = next(line.split()[1] for line in status.splitlines() if line.startswith('PPid:'))
    if b'xvfb-run' not in pathlib.Path(f'/proc/{parent}/cmdline').read_bytes():
        raise ValueError('display holder was not started by xvfb-run; desktop fallback refused')
    env = os.environ.copy()
    env.update(DISPLAY=context['display'], XAUTHORITY=context['authority'],
               QT_QPA_PLATFORM='xcb', LIBGL_ALWAYS_SOFTWARE='1',
               VK_DRIVER_FILES='/usr/share/vulkan/icd.d/lvp_icd.json',
               VK_ICD_FILENAMES='/usr/share/vulkan/icd.d/lvp_icd.json')
    env.pop('WAYLAND_DISPLAY', None)
    return env


def main():
    display = os.environ.get('DISPLAY', '')
    authority = os.environ.get('XAUTHORITY', '')
    if not display or not authority or not pathlib.Path(authority).is_file():
        raise ValueError('run this holder through authenticated xvfb-run')
    if b'xvfb-run' not in pathlib.Path(f'/proc/{os.getppid()}/cmdline').read_bytes():
        raise ValueError('holder must be started by xvfb-run, never on the desktop')
    context = dict(display=display, authority=authority, holder_pid=os.getpid(),
                   purpose='isolated reference and native viewer; no desktop input')
    path = pathlib.Path('private/reference/xvfb-session.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(context, indent=2) + '\n')
    print(f'Private virtual display ready: {display}', flush=True)
    while True:
        signal.pause()


if __name__ == '__main__':
    main()
