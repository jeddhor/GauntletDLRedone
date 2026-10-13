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


def server_identity(context, proc_root=pathlib.Path('/proc')):
    """Verify the actual authenticated Xvfb server, even after wrapper reparenting."""
    pid = context['server_pid']
    process = proc_root/str(pid)
    args = process.joinpath('cmdline').read_bytes().rstrip(b'\0').decode().split('\0')
    if pathlib.Path(args[0]).name != 'Xvfb' or context['display'] not in args:
        raise ValueError('recorded server is not the isolated Xvfb')
    if '-auth' not in args or args[args.index('-auth')+1] != context['authority']:
        raise ValueError('Xvfb authentication binding differs')
    # /proc stat's comm field can contain spaces; fields after ')' start at3.
    start_ticks = process.joinpath('stat').read_text().rsplit(')',1)[1].split()[19]
    if start_ticks != context['server_start_ticks']:
        raise ValueError('Xvfb server PID was reused')
    return pid


def record_server(context):
    matches=[]
    for process in pathlib.Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            args=process.joinpath('cmdline').read_bytes().rstrip(b'\0').decode().split('\0')
            if pathlib.Path(args[0]).name == 'Xvfb' and context['display'] in args and '-auth' in args and args[args.index('-auth')+1] == context['authority']:
                ticks=process.joinpath('stat').read_text().rsplit(')',1)[1].split()[19]
                matches.append((int(process.name),ticks))
        except (OSError,UnicodeError,IndexError):
            continue
    if len(matches)!=1:
        raise ValueError('authenticated Xvfb server identity is ambiguous')
    context['server_pid'],context['server_start_ticks']=matches[0]
    server_identity(context)


def environment():
    path = pathlib.Path('private/reference/xvfb-session.json')
    context = json.loads(path.read_text())
    pid = context['holder_pid']
    if not pathlib.Path(f'/proc/{pid}').is_dir():
        raise ValueError('virtual display session is no longer running')
    if 'virtual_display.py' not in pathlib.Path(f'/proc/{pid}/cmdline').read_bytes().decode(errors='replace'):
        raise ValueError('virtual display holder identity differs')
    if 'server_pid' in context:
        server_identity(context)
    else:
        status = pathlib.Path(f'/proc/{pid}/status').read_text()
        parent = next(line.split()[1] for line in status.splitlines() if line.startswith('PPid:'))
        if b'xvfb-run' not in pathlib.Path(f'/proc/{parent}/cmdline').read_bytes():
            raise ValueError('display holder was not started by xvfb-run; desktop fallback refused')
        record_server(context)
    if not pathlib.Path(context['authority']).is_file():
        raise ValueError('Xvfb authority file is missing')
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
    record_server(context)
    path = pathlib.Path('private/reference/xvfb-session.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(context, indent=2) + '\n')
    print(f'Private virtual display ready: {display}', flush=True)
    while True:
        signal.pause()


if __name__ == '__main__':
    main()
