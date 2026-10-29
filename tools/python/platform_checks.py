#!/usr/bin/env python3
"""M20 platform checks of the native client on this machine.

  platform_checks.py TAG [--binary PATH] [--width W --height H]

The G1 test character (GDL_TEST_AUTOSTART, GDL_DEBUG_LEVEL=G1; health
writes keep it alive, DEBUG), scratch saves under the run directory.

In a `kwin_wayland --virtual --xwayland` output on the real GPU (RADV, as
perf_bench.py):
  pad         a virtual gamepad (uinput "gdl-test-pad", its own vendor and
              product ids; the client sees only it: GDL_GAMEPAD_ONLY)
              presses cross and holds the d-pad up; it is unplugged, the
              game must go on; plugged in again, cross must attack again
  resize      the window resized to 1600×900 and 800×900: frames continue
              and the picture keeps the PS2 frame's shape (black borders)
  fullscreen  the fullscreen setting turned on and off while playing
              (GDL_TEST_ACTIONS "settings", the path F11 takes): the window
              must cover the output, then not
  full-disk   settings.json.tmp links to /dev/full before the start; a
              settings change's write must fail with a message and the game
              go on
On the private Xvfb (no window manager; lavapipe), because Xwayland in the
virtual kwin session drops XTEST keyboard input:
  focus       W held on the client, then another window (xmessage) takes
              the focus with W still down: the player must stop (the
              client releases keys on focus loss) and frames continue

Report private/reports/platform-checks-TAG.json (binary SHA-256, each
check's observations and verdict); screenshots and logs in
private/runs/platform-TAG/. Actual disc, Linux; the uinput pad is a
virtual device, not a physical controller.
"""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'
# Player 1's actions (+0x1e0) seen while cross was held on the keyboard in
# G1 (private/runs/client-m19-bind1, 2026-10-28): the attack chain.
ATTACKS = {0x27, 0x28, 0x2a, 0x5c, 0x5f, 0x61}


def sha256(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


# Update numbers of the TEST actions (after the pad and resize checks).
FULL_ON, FULL_OFF, FPS_ON = 1500, 1700, 1900


class Session:
    """The client, its frame log and xdotool on one display."""

    def __init__(self, run, binary, env=None):
        self.run = run
        self.binary = binary
        self.log = run / 'frames.log'
        self.env = env or dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV)

    def start(self):
        cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
        saves = self.run / 'saves'
        saves.mkdir(exist_ok=True)
        cfg['saves'] = str(saves)
        (self.run / 'config.json').write_text(json.dumps(cfg))
        # full-disk: the settings write's temporary file is /dev/full.
        tmp = saves / 'settings.json.tmp'
        if not tmp.is_symlink():
            tmp.symlink_to('/dev/full')
        actions = [{'at': u, 'health': 5000} for u in range(200, 4000, 200)]
        actions += [{'at': FULL_ON, 'settings': {'fullscreen': True}}, {'at': FULL_OFF, 'settings': {'fullscreen': False}},
                    {'at': FPS_ON, 'settings': {'show_fps': True}}]
        (self.run / 'actions.json').write_text(json.dumps(actions))
        env = dict(self.env, GDL_NO_AUDIO='1', GDL_GAMEPAD_ONLY='gdl-test-pad', GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1',
                   GDL_FRAME_LOG=str(self.log), GDL_TEST_ACTIONS=str(self.run / 'actions.json'))
        env.pop('WAYLAND_DISPLAY', None)
        self.client = subprocess.Popen([str(self.binary), str(self.run / 'config.json')], cwd=ROOT, env=env,
                                       stdout=open(self.run / 'client.log', 'w'), stderr=subprocess.STDOUT)
        self.win = ''
        for _ in range(120):
            time.sleep(1)
            ids = self.x('search', '--name', '^Gauntlet: Dark Legacy$').split()
            if ids:
                self.win = ids[0]
            if self.win and self.updates() >= 150:
                return True
        return False

    def x(self, *a):
        return subprocess.run(['xdotool', *a], capture_output=True, text=True, env=self.env).stdout.strip()

    def wait_update(self, n, limit=120):
        for _ in range(limit * 4):
            if n <= self.updates() or not self.alive():
                return True
            time.sleep(0.25)
        return False

    def rows(self):
        try:
            return [line.split() for line in self.log.read_text().splitlines() if line.strip()]
        except FileNotFoundError:
            return []

    def updates(self):
        r = self.rows()
        return int(r[-1][3]) if r else 0

    def alive(self):
        return self.client.poll() is None

    def body(self):
        r = self.rows()
        return (float(r[-1][12]), float(r[-1][14])) if r and len(r[-1]) > 14 else (0.0, 0.0)

    def actions_since(self, n):
        return {int(r[4]) for r in self.rows()[n:]}

    def focus(self):
        self.x('windowfocus', '--sync', self.win)
        time.sleep(0.4)

    def shot(self, name):
        out = self.run / f'{name}.png'
        subprocess.run(['import', '-window', self.win, str(out)], capture_output=True, env=self.env)
        return out

    def geometry(self):
        g = self.x('getwindowgeometry', self.win)
        for line in g.splitlines():
            if 'Geometry:' in line:
                w, h = line.split(':')[1].strip().split('x')
                return int(w), int(h)
        return None

    def stop(self):
        if self.alive():
            self.client.terminate()
            try:
                self.client.wait(10)
            except subprocess.TimeoutExpired:
                self.client.kill()


def check_pad(s):
    from evdev import AbsInfo, UInput, ecodes as e
    cap = {e.EV_KEY: [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR],
           e.EV_ABS: [(e.ABS_X, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_Y, AbsInfo(0, -32768, 32767, 16, 128, 0)),
                      (e.ABS_RX, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_RY, AbsInfo(0, -32768, 32767, 16, 128, 0)),
                      (e.ABS_Z, AbsInfo(0, 0, 255, 0, 0, 0)), (e.ABS_RZ, AbsInfo(0, 0, 255, 0, 0, 0)),
                      (e.ABS_HAT0X, AbsInfo(0, -1, 1, 0, 0, 0)), (e.ABS_HAT0Y, AbsInfo(0, -1, 1, 0, 0, 0))]}

    def plug():
        # pid.codes' test ids: no SDL mapping renames the device.
        ui = UInput(cap, name='gdl-test-pad', vendor=0x1209, product=0x0001, version=0x100)
        time.sleep(2.5)
        return ui

    def press(ui, code, times=3):
        mark = len(s.rows())
        for _ in range(times):
            ui.write(e.EV_KEY, code, 1)
            ui.syn()
            time.sleep(0.25)
            ui.write(e.EV_KEY, code, 0)
            ui.syn()
            time.sleep(0.5)
        time.sleep(1.0)
        return sorted(hex(a) for a in s.actions_since(mark))

    out = {}
    ui = plug()
    out['actions_first'] = press(ui, e.BTN_SOUTH)
    before = s.body()
    ui.write(e.EV_ABS, e.ABS_HAT0Y, -1)
    ui.syn()
    time.sleep(1.5)
    ui.write(e.EV_ABS, e.ABS_HAT0Y, 0)
    ui.syn()
    time.sleep(0.5)
    after = s.body()
    out['dpad_moved'] = round(((after[0] - before[0]) ** 2 + (after[1] - before[1]) ** 2) ** 0.5, 3)
    ui.close()
    u0 = s.updates()
    time.sleep(3)
    out['updates_while_unplugged'] = s.updates() - u0
    out['alive_after_unplug'] = s.alive()
    ui = plug()
    out['actions_after_replug'] = press(ui, e.BTN_SOUTH)
    ui.close()
    attacked = lambda acts: any(int(a, 16) in ATTACKS for a in acts)
    out['pass'] = attacked(out['actions_first']) and out['dpad_moved'] > 0.5 and out['alive_after_unplug'] and out['updates_while_unplugged'] > 30 and attacked(out['actions_after_replug'])
    return out


def check_focus(s):
    out = {}
    s.focus()
    s.x('keydown', 'w')
    time.sleep(1.2)
    a = s.body()
    time.sleep(0.6)
    b = s.body()
    out['moving_step'] = round(((b[0] - a[0]) ** 2 + (b[1] - a[1]) ** 2) ** 0.5, 3)
    other = subprocess.Popen(['xmessage', '-geometry', '300x100+10+10', 'focus thief'], env=s.env)
    time.sleep(1.5)
    for w in s.x('search', '--class', 'xmessage').split():
        s.x('windowfocus', '--sync', w)
    time.sleep(1.5)
    u0, c = s.updates(), s.body()
    time.sleep(1.5)
    d = s.body()
    out['step_after_focus_loss'] = round(((d[0] - c[0]) ** 2 + (d[1] - c[1]) ** 2) ** 0.5, 3)
    out['updates_while_unfocused'] = s.updates() - u0
    s.x('keyup', 'w')
    other.terminate()
    s.focus()
    out['pass'] = out['moving_step'] > 0.3 and out['step_after_focus_loss'] < 0.05 and out['updates_while_unfocused'] > 20 and s.alive()
    return out


def border_columns(png):
    from PIL import Image
    im = Image.open(png).convert('RGB')
    w, h = im.size
    px = im.load()
    side = all(max(px[x, y]) < 8 for x in (0, 2, w - 1, w - 3) for y in range(h // 4, 3 * h // 4, 7))
    return {'size': [w, h], 'black_side_columns': side}


def check_resize(s):
    out = {'sizes': []}
    for w, h in ((1600, 900), (800, 900), (1280, 896)):
        u0 = s.updates()
        s.x('windowsize', '--sync', s.win, str(w), str(h))
        time.sleep(2.0)
        shot = s.shot(f'resize-{w}x{h}')
        out['sizes'].append({'asked': [w, h], 'got': s.geometry(), 'updates': s.updates() - u0, **border_columns(shot)})
    wide = out['sizes'][0]
    out['pass'] = s.alive() and all(z['updates'] > 20 for z in out['sizes']) and wide['black_side_columns']
    return out


def check_fullscreen(s, screen):
    out = {}
    s.wait_update(FULL_ON + 60)
    out['fullscreen_geometry'] = s.geometry()
    s.shot('fullscreen')
    s.wait_update(FULL_OFF + 60)
    out['windowed_geometry'] = s.geometry()
    out['pass'] = s.alive() and out['fullscreen_geometry'] == tuple(screen) and out['windowed_geometry'] not in (None, tuple(screen))
    return out


def check_full_disk(s):
    s.wait_update(FPS_ON + 60)
    log = (s.run / 'client.log').read_text(errors='replace')
    out = {'message': next((l for l in log.splitlines() if l.startswith('settings: not stored')), None),
           'settings_json_exists': (s.run / 'saves/settings.json').exists(), 'alive': s.alive()}
    s.shot('full-disk-f3')
    out['pass'] = bool(out['message']) and not out['settings_json_exists'] and out['alive']
    return out


def inside(run, binary, screen):
    s = Session(run, binary)
    result = {'started': s.start()}
    try:
        if result['started']:
            for name, fn in (('pad', check_pad), ('resize', check_resize),
                             ('fullscreen', lambda s: check_fullscreen(s, screen)), ('full-disk', check_full_disk)):
                try:
                    result[name] = fn(s)
                except Exception as ex:  # a failed check is reported, the others still run
                    result[name] = {'pass': False, 'error': repr(ex)}
        result['alive_at_end'] = s.alive()
    finally:
        s.stop()
        (run / 'result.json').write_text(json.dumps(result, indent=1) + '\n')


def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--inside':
        return inside(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), (int(sys.argv[4]), int(sys.argv[5])))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--width', type=int, default=1600)
    ap.add_argument('--height', type=int, default=1000)
    a = ap.parse_args()
    report = ROOT / 'private/reports' / f'platform-checks-{a.tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    run = ROOT / 'private/runs' / f'platform-{a.tag}'
    run.mkdir(parents=True, exist_ok=False)
    binary = pathlib.Path(a.binary).resolve()
    session = run / 'session.sh'
    session.write_text(f"""#!/bin/bash
cd '{ROOT}'
unset WAYLAND_DISPLAY
python3 '{pathlib.Path(__file__).resolve()}' --inside '{run}' '{binary}' {a.width} {a.height} > '{run / "driver.log"}' 2>&1
""")
    session.chmod(0o755)
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', f'gdl-plat-{os.getpid()}', '--width', str(a.width), '--height', str(a.height),
                    '--no-lockscreen', '--exit-with-session', str(session)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1800)
    result = json.loads((run / 'result.json').read_text()) if (run / 'result.json').exists() else {'started': False}
    # The focus check on the private Xvfb (keyboard input works there).
    sys.path.insert(0, str(ROOT / 'tools/python'))
    from virtual_display import environment
    xrun = run / 'xvfb'
    xrun.mkdir()
    xs = Session(xrun, binary, environment())
    try:
        result['focus'] = check_focus(xs) if xs.start() else {'pass': False, 'error': 'client did not start on the Xvfb'}
    except Exception as ex:
        result['focus'] = {'pass': False, 'error': repr(ex)}
    finally:
        xs.stop()
    uname = subprocess.run(['uname', '-sr'], capture_output=True, text=True).stdout.strip()
    report.write_text(json.dumps({
        'scope': 'M20 platform checks (actual disc, real GPU through a virtual kwin session, Linux; virtual uinput gamepad)',
        'binary': str(binary), 'binary_sha256': sha256(binary), 'os': uname, 'output': [a.width, a.height], 'run': str(run), **result,
    }, indent=1) + '\n')
    for k, v in result.items():
        print(k, v.get('pass') if isinstance(v, dict) else v)
    print(report)


if __name__ == '__main__':
    main()
