#!/usr/bin/env python3
"""Title Start → select screen check on the real GPU (playtest 2026-10-28).

  title_start_check.py TAG [--trials N] [--binary PATH]

In a `kwin_wayland --virtual --xwayland` output on the real GPU (RADV), each
trial starts gdl_play at Press Start (GDL_NO_ATTRACT, its own empty saves,
audio on into ALSA's null device: HOME is the run's, whose .asoundrc makes
the default PCM `type null` — nothing reaches the speakers), a virtual uinput gamepad (GDL_GAMEPAD_ONLY, as platform_checks.py)
presses cross twice (Press Start → the title → Start) and GDL_UI_TRACE
records the screens. A trial passes when the select screen is still up six
seconds later. At the real GPU's frame rate a rendered frame can fall
between the select screen's creation and its first 30 Hz update; before the
fix, front_system's game-over check ended the game there (5 of 6 trials on
the RX 580): the select closed after a few frames and the Tower ran with
GAME OVER over its intro scroll (2 of 6 with GDL_NO_AUDIO). The software renderer on the Xvfb steps
at least one update per frame, so the menu flows never showed it.

--attract (playtest 2026-10-28, "Start does nothing on Press Start"): the
client boots as the player's does (the attract sequence: the SONY screen,
then the movies), and only the pad's START is pressed, three times, three
seconds apart; a trial passes when the title menu (Title { at: 0 }) comes up.
--xbox gives the virtual pad the player's controller's identity and
capabilities (Microsoft Xbox Series S|X Controller 045e:0b12 v0509 on USB,
xpad: KEY_RECORD and the BTN_ buttons, 0..1023 triggers), so gilrs maps it
through the same SDL mapping as the real one. --overlay (with --attract):
after the title menu the pad goes Options → Display (d-pad, cross) and the
client saves its own frames (GDL_SHOT_FRAMES; window imports of a
GPU-presented window can be stale). --wayland keeps the client on kwin's
Wayland socket (the player's desktop) instead of Xwayland; --settings FILE
is passed as GDL_SETTINGS (read, never written). --wait-title (with
--attract): no press until the attract sequence reaches Press Start by
itself (the movies played out), then START.

Report: private/reports/title-start-TAG.json; runs in private/runs/title-start-TAG/.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'


def trial(run, binary, ui, e, attract=False, only='gdl-test-pad', opts=()):
    run.mkdir(parents=True)
    saves = run / 'saves'
    saves.mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(saves)
    (run / 'config.json').write_text(json.dumps(cfg))
    trace = run / 'ui.trace'
    home = run / 'home'
    home.mkdir()
    (home / '.asoundrc').write_text('pcm.!default { type null }\nctl.!default { type hw card 0 }\n')
    env = dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV, HOME=str(home), GDL_NO_ATTRACT='1',
               GDL_GAMEPAD_ONLY=only, GDL_UI_TRACE=str(trace))
    env.pop('WAYLAND_DISPLAY', None)
    if attract:
        env.pop('GDL_NO_ATTRACT')
    if 'wayland' in opts:
        env['WAYLAND_DISPLAY'] = os.environ['GDL_KWIN_SOCKET']
    if os.environ.get('GDL_CHECK_SETTINGS'):
        env['GDL_SETTINGS'] = os.environ['GDL_CHECK_SETTINGS']
    if 'overlay' in opts:
        (run / 'frames').mkdir()
        env['GDL_SHOT_DIR'] = str(run / 'frames')
        env['GDL_SHOT_FRAMES'] = ','.join(str(f) for f in range(60, 6000, 60))
    client = subprocess.Popen([str(binary), str(run / 'config.json')], cwd=ROOT, env=env,
                              stdout=open(run / 'client.log', 'w'), stderr=subprocess.STDOUT)

    def cross(button=e.BTN_SOUTH):
        ui.write(e.EV_KEY, button, 1)
        ui.syn()
        time.sleep(0.15)
        ui.write(e.EV_KEY, button, 0)
        ui.syn()

    def shot(label):
        win = subprocess.run(['xdotool', 'search', '--name', 'INTERIM native build'], capture_output=True, text=True).stdout.split()
        if win:
            subprocess.run(['import', '-window', win[0], str(run / f'{label}.png')], capture_output=True)

    if attract:
        try:
            for _ in range(240):
                time.sleep(0.5)
                if trace.exists() and trace.read_text().strip():
                    break
            time.sleep(4)
            if 'wait-title' in opts:
                for _ in range(600):
                    last = trace.read_text().splitlines()[-1] if trace.exists() else ''
                    if 'PressStart' in last and 'attract' not in last:
                        break
                    time.sleep(0.5)
                time.sleep(2)
            for k in range(3):
                shot(f'before{k}')
                cross(e.BTN_START)
                time.sleep(3)
            shot('after')
            if 'overlay' in opts:
                def hat(axis, v):
                    ui.write(e.EV_ABS, axis, v)
                    ui.syn()
                    time.sleep(0.15)
                    ui.write(e.EV_ABS, axis, 0)
                    ui.syn()
                    time.sleep(0.6)
                hat(e.ABS_HAT0Y, 1)
                cross()
                time.sleep(1)
                for _ in range(5):
                    hat(e.ABS_HAT0Y, 1)
                cross()
                time.sleep(4)
        finally:
            client.terminate()
            try:
                client.wait(10)
            except subprocess.TimeoutExpired:
                client.kill()
        lines = trace.read_text().splitlines() if trace.exists() else []
        title = any('Title { at: 0 }' in line for line in lines)
        overlay = any(line.endswith('| overlay') for line in lines)
        log = (run / 'client.log').read_text(errors='replace')
        out = {'trace': lines, 'title_menu': title, 'validation_errors': log.count('Validation Error'), 'pass': title and 'Validation Error' not in log}
        if 'overlay' in opts:
            out['overlay_opened'] = overlay
            out['pass'] = out['pass'] and overlay
        return out

    try:
        for _ in range(240):
            time.sleep(0.5)
            if trace.exists() and 'PressStart' in trace.read_text():
                break
        time.sleep(2)
        cross()
        time.sleep(3)
        cross()
        time.sleep(6)
        win = subprocess.run(['xdotool', 'search', '--name', 'INTERIM native build'], capture_output=True, text=True).stdout.split()
        if win:
            subprocess.run(['import', '-window', win[0], str(run / 'after.png')], capture_output=True)
    finally:
        client.terminate()
        try:
            client.wait(10)
        except subprocess.TimeoutExpired:
            client.kill()
    lines = trace.read_text().splitlines() if trace.exists() else []
    opened = any(' select world 13' in line for line in lines)
    still = bool(lines) and ' select world 13' in lines[-1]
    audio = next((line.strip() for line in (run / 'client.log').read_text(errors='replace').splitlines() if line.startswith('audio:')), '')
    return {'audio': audio, 'trace': lines, 'select_opened': opened, 'select_still_up': still, 'pass': opened and still}


def inside(out, binary, trials, attract=False, xbox=False, opts=()):
    from evdev import AbsInfo, UInput, ecodes as e
    cap = {e.EV_KEY: [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR],
           e.EV_ABS: [(e.ABS_X, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_Y, AbsInfo(0, -32768, 32767, 16, 128, 0)),
                      (e.ABS_HAT0X, AbsInfo(0, -1, 1, 0, 0, 0)), (e.ABS_HAT0Y, AbsInfo(0, -1, 1, 0, 0, 0))]}
    only = 'gdl-test-pad'
    if xbox:
        stick, trigger = AbsInfo(0, -32768, 32767, 16, 128, 0), AbsInfo(0, 0, 1023, 0, 0, 0)
        cap = {e.EV_KEY: [e.KEY_RECORD] + cap[e.EV_KEY],
               e.EV_ABS: [(e.ABS_X, stick), (e.ABS_Y, stick), (e.ABS_Z, trigger), (e.ABS_RX, stick), (e.ABS_RY, stick), (e.ABS_RZ, trigger),
                          (e.ABS_HAT0X, AbsInfo(0, -1, 1, 0, 0, 0)), (e.ABS_HAT0Y, AbsInfo(0, -1, 1, 0, 0, 0))]}
        # The SDL mapping names it "Xbox Series Controller" (the player's idle one too).
        ui = UInput(cap, name='Microsoft Xbox Series S|X Controller', vendor=0x045e, product=0x0b12, version=0x0509, bustype=0x03)
        only = 'Xbox Series'
    else:
        # pid.codes' test ids: no SDL mapping renames the device.
        ui = UInput(cap, name='gdl-test-pad', vendor=0x1209, product=0x0001, version=0x100)
    time.sleep(2.5)
    results = []
    try:
        for k in range(trials):
            try:
                results.append(trial(out / f'trial{k}', binary, ui, e, attract, only, opts))
            except Exception as ex:  # a failed trial is reported, the others still run
                results.append({'pass': False, 'error': repr(ex)})
    finally:
        ui.close()
        (out / 'result.json').write_text(json.dumps(results, indent=1) + '\n')


def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--inside':
        return inside(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), int(sys.argv[4]), 'attract' in sys.argv[5:], 'xbox' in sys.argv[5:],
                      tuple(o for o in ('overlay', 'wayland', 'wait-title') if o in sys.argv[5:]))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--trials', type=int, default=6)
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--attract', action='store_true', help='boot through the attract sequence; press only START')
    ap.add_argument('--xbox', action='store_true', help="the virtual pad imitates the player's Xbox Series controller")
    ap.add_argument('--overlay', action='store_true', help='(with --attract) then Options → Display with the pad; frames saved')
    ap.add_argument('--wayland', action='store_true', help="the client on kwin's Wayland socket")
    ap.add_argument('--settings', help='GDL_SETTINGS for the client')
    ap.add_argument('--wait-title', action='store_true', help='(with --attract) press START only once the sequence reaches Press Start')
    a = ap.parse_args()
    if not a.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    report = ROOT / 'private/reports' / f'title-start-{a.tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = ROOT / 'private/runs' / f'title-start-{a.tag}'
    out.mkdir(parents=True, exist_ok=False)
    binary = pathlib.Path(a.binary).resolve()
    session = out / 'session.sh'
    sock = f'gdl-title-{os.getpid()}'
    extra = ' '.join(o for o, on in (('overlay', a.overlay), ('wayland', a.wayland), ('wait-title', a.wait_title)) if on)
    settings = f"export GDL_CHECK_SETTINGS='{pathlib.Path(a.settings).resolve()}'" if a.settings else ''
    session.write_text(f"""#!/bin/bash
cd '{ROOT}'
unset WAYLAND_DISPLAY
export GDL_KWIN_SOCKET='{sock}'
{settings}
python3 '{pathlib.Path(__file__).resolve()}' --inside '{out}' '{binary}' {a.trials} {'attract' if a.attract else 'select'} {'xbox' if a.xbox else 'test'} {extra} > '{out / "driver.log"}' 2>&1
""")
    session.chmod(0o755)
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', sock, '--width', '1920' if a.wayland else '1600', '--height', '1080' if a.wayland else '1000',
                    '--no-lockscreen', '--exit-with-session', str(session)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=(420 if a.wait_title else 120) + 60 * a.trials)
    results = json.loads((out / 'result.json').read_text()) if (out / 'result.json').exists() else []
    import hashlib
    report.write_text(json.dumps({
        'scope': ('boot through the attract sequence, pad START only → title menu' if a.attract else 'title Start → select screen') + ' (actual disc, real GPU through a virtual kwin session, Linux; virtual uinput gamepad; audio into ALSA null)',
        'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'run': str(out),
        'passed': sum(1 for r in results if r.get('pass')), 'trials': len(results), 'results': results,
    }, indent=1) + '\n')
    print(f"{sum(1 for r in results if r.get('pass'))}/{len(results)} trials " + ('reached the title menu with START' if a.attract else 'kept the select screen'))
    print(report)


if __name__ == '__main__':
    main()
