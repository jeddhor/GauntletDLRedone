#!/usr/bin/env python3
"""Drive the native client (gdl_play) on the private Xvfb with xdotool.

  client_driver.py serve TAG STEPS.json   start gdl_play (scratch saves, no
                                           attract), run STEPS, then serve
                                           queued steps until 'quit' or 40
                                           idle minutes
  client_driver.py send TAG 'STEPS'       queue steps for a running session
                                           and print its output

The client runs with GDL_NO_GAMEPADS, so the machine's physical controllers
do not reach it. Steps: ["key", "a+w", seconds] (XTEST key holds on the client's focused
window), ["wait", seconds], ["shot", label] (an import of the client's own
window), ["quit"]. Only this session's window is used: PCSX2's reference
window shares the display and the game's name, so the window must carry the
INTERIM title and belong to this run's process group, and keys are sent only
while it has the focus. Everything goes to private/runs/client-TAG/ (frames,
log, queue). The client's F2 key prints a DEBUG item dump to the log.
"""
import json
import os
import pathlib
import signal
import subprocess
import sys
import time

from virtual_display import environment

ROOT = pathlib.Path(__file__).resolve().parents[2]


def run_dir(tag):
    if not tag.replace('-', '').isalnum():
        raise ValueError('tag must be alphanumeric')
    d = ROOT / 'private' / 'runs' / f'client-{tag}'
    d.mkdir(parents=True, exist_ok=True)
    return d


def serve(tag, steps):
    d = run_dir(tag)
    q = d / 'queue'
    q.mkdir(exist_ok=True)
    env = environment()
    xenv = dict(os.environ, DISPLAY=env['DISPLAY'], XAUTHORITY=env['XAUTHORITY'])
    saves = d / 'saves'
    saves.mkdir(exist_ok=True)
    log = open(d / 'client.log', 'w')
    p = subprocess.Popen([sys.executable, 'tools/python/run_play.py', '--virtual', '--save-dir', str(saves)],
                         cwd=ROOT, env=dict(os.environ, GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1'), stdout=log, stderr=subprocess.STDOUT,
                         start_new_session=True)

    def x(*a):
        return subprocess.run(['xdotool', *a], env=xenv, capture_output=True, text=True).stdout.strip()

    # Continue an existing run's numbering: frames are never overwritten.
    shots = max((int(f.name[:3]) for f in d.glob('[0-9][0-9][0-9]-*.png')), default=0)

    def run(steps, win, out):
        nonlocal shots
        for st in steps:
            if p.poll() is not None:
                out.append(f'client exited {p.returncode}')
                return False
            if st[0] == 'quit':
                return False
            if st[0] == 'wait':
                time.sleep(st[1])
            elif st[0] == 'key':
                x('windowfocus', '--sync', win)
                if x('getwindowfocus') != win:
                    out.append('focus is not the client window; key skipped')
                    continue
                keys = st[1].split('+')
                for k in keys:
                    x('keydown', k)
                time.sleep(st[2])
                for k in reversed(keys):
                    x('keyup', k)
                time.sleep(0.3)
            elif st[0] == 'shot':
                shots += 1
                o = d / f'{shots:03d}-{st[1]}.png'
                subprocess.run(['import', '-window', win, str(o)], env=xenv)
                out.append(str(o))
            else:
                out.append(f'unknown step {st!r}')
        return True

    try:
        win = ''
        for _ in range(180):
            time.sleep(1)
            for w in x('search', '--name', 'INTERIM native build').split():
                pid = x('getwindowpid', w)
                if pid and os.getpgid(int(pid)) == p.pid:
                    win = w
            if win:
                break
        if not win:
            raise SystemExit('client window not found')
        print('window', win, flush=True)
        out = []
        alive = run(steps, win, out)
        print('\n'.join(out), flush=True)
        idle = time.time()
        while alive and time.time() - idle < 2400:
            cmds = sorted(c for c in q.glob('*.json') if not c.with_suffix('.done').exists())
            if not cmds:
                time.sleep(0.5)
                continue
            out = []
            alive = run(json.loads(cmds[0].read_text()), win, out)
            cmds[0].with_suffix('.done').write_text('\n'.join(out) + '\n')
            idle = time.time()
    finally:
        # Only this run's process group (run_play and its gdl_play child).
        try:
            os.killpg(p.pid, signal.SIGTERM)
            p.wait(10)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            os.killpg(p.pid, signal.SIGKILL)


def send(tag, steps):
    q = run_dir(tag) / 'queue'
    c = q / f'{len(list(q.glob("*.json"))):03d}.json'
    c.write_text(json.dumps(steps))
    done = c.with_suffix('.done')
    t0 = time.time()
    while not done.exists():
        if time.time() - t0 > 600:
            raise SystemExit('timed out')
        time.sleep(0.5)
    print(done.read_text(), end='')


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in ('serve', 'send'):
        raise SystemExit(__doc__)
    steps = json.loads(pathlib.Path(sys.argv[3]).read_text()) if sys.argv[1] == 'serve' else json.loads(sys.argv[3])
    (serve if sys.argv[1] == 'serve' else send)(sys.argv[2], steps)


if __name__ == '__main__':
    main()
