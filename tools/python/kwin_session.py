#!/usr/bin/env python3
"""Run one command inside a `kwin_wayland --virtual --xwayland` session on
the real GPU (RADV), as title_start_check.py does, and wait for it.

  kwin_session.py LOG -- COMMAND [ARG ...]

The command sees the session's Xwayland DISPLAY (WAYLAND_DISPLAY unset:
the client is X11), the RADV Vulkan driver, and the caller's environment
otherwise (pass GDL_* test variables, WINEPREFIX, … that way). Its output
goes to LOG; the exit status is the command's. Nothing reaches the user's
desktop session.
"""
import os
import pathlib
import shlex
import subprocess
import sys

RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'


def main():
    if len(sys.argv) < 4 or sys.argv[2] != '--':
        raise SystemExit(__doc__)
    log = pathlib.Path(sys.argv[1]).resolve()
    cmd = sys.argv[3:]
    status = log.with_suffix('.status')
    status.unlink(missing_ok=True)
    session = log.with_suffix('.session.sh')
    session.write_text(f"""#!/bin/bash
unset WAYLAND_DISPLAY
export VK_DRIVER_FILES={RADV} VK_ICD_FILENAMES={RADV}
cd {shlex.quote(os.getcwd())}
{' '.join(shlex.quote(c) for c in cmd)} > {shlex.quote(str(log))} 2>&1
echo $? > {shlex.quote(str(status))}
""")
    session.chmod(0o755)
    env = dict(os.environ)
    env.pop('WAYLAND_DISPLAY', None)
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', f'gdl-run-{os.getpid()}', '--width', '1600', '--height', '1000',
                    '--no-lockscreen', '--exit-with-session', str(session)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    code = int(status.read_text().strip()) if status.exists() else 1
    raise SystemExit(code)


if __name__ == '__main__':
    main()
