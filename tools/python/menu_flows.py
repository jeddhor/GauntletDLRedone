#!/usr/bin/env python3
"""Menu flow tests (M17) on the native client.

  menu_flows.py TAG [FLOW ...]

Each flow starts gdl_play on the private Xvfb through client_driver.py
(scratch saves, no attract), sends key holds to the client's window
(xdotool; keyboard bindings in research/playtest.md: arrows move, Return
accepts (cross), BackSpace goes back (triangle), Escape opens the pause
menu) and records GDL_UI_TRACE: one line per change of the screen, the
front-end page or the in-game modal state. A flow passes when its expected
patterns (regular expressions) match trace lines in order. Frames are taken
at the flow's "shot" steps. Report: private/reports/menu-flows-TAG.json.
"""
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def k(key, n=1, hold=0.15, gap=0.5):
    out = []
    for _ in range(n):
        out += [["key", key, hold], ["wait", gap]]
    return out


# Front-end keys: Return = cross (accept), BackSpace = triangle (back).
FLOWS = {
    # Press Start → the title menu → Options → Audio and back out again.
    'title-options': dict(
        steps=[["wait", 8]] + k('Return') + [["wait", 1], ["shot", "title"]] + k('Down') + k('Return') + [["shot", "options"]]
        + k('Return') + [["shot", "options-first"]] + k('BackSpace') + k('BackSpace') + [["shot", "back"], ["wait", 1]],
        expect=[r'front .*PressStart', r'front .*Title \{ at: 0 \}', r'front .*Title \{ at: 1 \}', r'front .*Options \{ at: 0',
                r'front .*Title \{ at: 1 \}']),
    # Title → Options → Back: title_options fades out (finish_optmenu) and
    # its background burns (StartFireScroll: title_menu has none).
    'menu-burn': dict(
        steps=[["wait", 8]] + k('Return') + [["wait", 1]] + k('Down') + k('Return') + [["wait", 1], ["shot", "options"]]
        + [["key", "BackSpace", 0.1], ["wait", 0.1], ["shot", "burn1"], ["wait", 0.15], ["shot", "burn2"], ["wait", 0.15], ["shot", "burn3"],
           ["wait", 1.5], ["shot", "after"]],
        expect=[r'front .*Options \{ at: 0', r'front .*Title \{ at: 1 \}.*ending finish 1 burn', r'front .*Title \{ at: 1 \} \| $']),
    # In the Tower: pause → Back: tower_menu fades and burns while the game
    # stays paused; it resumes after the burn.
    'pause-burn': dict(
        steps=[["wait", 8]] + k('Return') + [["wait", 1]] + k('Return') + [["wait", 3]] + k('space', 8, gap=1.5) + [["wait", 12]]
        + k('i', 6, gap=1.5) + [["wait", 2]] + k('Escape') + [["wait", 1], ["shot", "paused"], ["key", "BackSpace", 0.1], ["wait", 0.2], ["shot", "burn"],
           ["wait", 1.5], ["shot", "after"]],
        expect=[r'paused=true Paused \{ tower: true, at: 0', r'paused=false Paused .*ending finish 1 burn', r'paused=false Paused \{ tower: true, at: 0 \} \| [a-z+]* ?world 13 level \d+$']),
    # The screen saver (GDL_DEBUG_SAVER shortens the ten idle minutes): in
    # G1 with no input it starts, the weapons fly; a key ends it.
    'saver': dict(
        env=dict(GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1', GDL_DEBUG_SAVER='300'),
        steps=[["wait", 60], ["shot", "saver1"], ["wait", 10], ["shot", "saver2"], ["wait", 10], ["shot", "saver3"]] + k('Down') + [["wait", 3], ["shot", "after"]],
        expect=[r'playing .*\| saver world 7', r'playing .*\|  world 7']),
    # The stage title (write_stage_info) during the start camera: G1's name;
    # S1 (LEVL +0 flag 4) adds GRAB COINS BEFORE TIME RUNS OUT on its box.
    'stage-title': dict(
        env=dict(GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1'),
        steps=[["wait", 1], ["shot", "t1"], ["wait", 0.5], ["shot", "t2"], ["wait", 0.5], ["shot", "t3"], ["wait", 0.5], ["shot", "t4"], ["wait", 4], ["shot", "t5"]],
        expect=[r'playing .*world 7 level 0']),
    'stage-title-s1': dict(
        env=dict(GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='S1'),
        steps=[["wait", 1], ["shot", "t1"], ["wait", 0.5], ["shot", "t2"], ["wait", 0.5], ["shot", "t3"], ["wait", 0.5], ["shot", "t4"], ["wait", 4], ["shot", "t5"]],
        expect=[r'playing .*world']),
    # The level end's fade (world_update: GlobalInten and the
    # TRANSITION_SCREEN blit over two seconds): G1 with player 1 placed on
    # its exit (GDL_DEBUG_TELEPORT at update 600).
    'level-end-fade': dict(
        env=dict(GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1', GDL_DEBUG_TELEPORT='118.28,86.30,-472.5'),
        steps=[["wait", 20], ["shot", "f1"], ["wait", 0.5], ["shot", "f2"], ["wait", 0.5], ["shot", "f3"], ["wait", 0.5], ["shot", "f4"], ["wait", 0.5], ["shot", "f5"], ["wait", 6], ["shot", "f6"]],
        expect=[r'playing .*world 7 level 0']),
    # Start → the select screen: a new character (default name) → the Tower.
    'new-character': dict(
        steps=[["wait", 8]] + k('Return') + [["wait", 1]] + k('Return') + [["wait", 3], ["shot", "select"]] + k('space', 8, gap=1.5)
        + [["wait", 10], ["shot", "tower"]],
        expect=[r'front .*PressStart', r'front .*Title', r'playing .*\| select world 13', r'playing .*\| (message_box )?world 13|playing .*\| message_box world 13']),
    # Every Options entry opened and closed in turn (the trace names each page).
    'options-all': dict(
        steps=[["wait", 8]] + k('Return') + k('Down') + k('Return')
        + sum(([*k('Down', i), *k('Return'), ["wait", 1], ["shot", f"opt{i}"], *k('BackSpace'), *k('Up', i)] for i in range(6)), [])
        + [["wait", 1]],
        expect=[r'Options \{ at: 0', r'Audio', r'Options \{ at: 1', r'Options \{ at: 2', r'Options \{ at: 3']),
    # In the Tower: the pause menu and its entries.
    'pause-tower': dict(
        steps=[["wait", 8]] + k('Return') + [["wait", 1]] + k('Return') + [["wait", 3]] + k('space', 8, gap=1.5) + [["wait", 12]]
        + k('i', 4, gap=1.5) + [["wait", 2]]
        + sum(([*k('Escape'), ["wait", 1], *k('Down', i), *k('Return'), ["wait", 2], ["shot", f"pause{i}"], *k("i", 2, gap=1.0), ["wait", 1]] for i in range(5)), []),
        expect=[r'playing .*\| select world 13', r'paused=true Paused \{ tower: true, at: 0', r'Paused \{ tower: true, at: 1 \} \| select',
                r'Paused \{ tower: true, at: 2 \} \| select\+shop', r'paused=true Inventory', r'InventoryClose', r'paused=true ConfirmQuit',
                r'paused=false Paused \{ tower: true, at: 4']),
    # In a realm (G1, TEST autostart): pause → Quit Level → "Abort Level?"
    # (quitlevel_question) → Yes → back in the Tower.
    'pause-realm': dict(
        env=dict(GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1'),
        steps=[["wait", 15]] + k('Escape') + [["wait", 1], ["shot", "paused"]] + k('Down') + k('Return') + [["wait", 1], ["shot", "quit"]]
        + k('Down') + k('Return') + [["wait", 15], ["shot", "after"]],
        expect=[r'playing .*world 7 level 0', r'paused=true Paused \{ tower: false, at: 0', r'Paused \{ tower: false, at: 1',
                r'paused=true ConfirmQuit \{ at: 1, tower: false', r'world 13']),
}


def run(tag, name, flow):
    run_tag = f'{tag}-{name}'
    d = ROOT / 'private' / 'runs' / f'client-{run_tag}'
    if d.exists():
        raise SystemExit(f'{d} exists')
    d.mkdir(parents=True)
    steps = d / 'steps.json'
    steps.write_text(json.dumps(flow['steps'] + [["quit"]]))
    trace = d / 'ui.txt'
    env = dict(os.environ, GDL_UI_TRACE=str(trace), **flow.get('env', {}))
    subprocess.run([sys.executable, str(ROOT / 'tools/python/client_driver.py'), 'serve', run_tag, str(steps)], cwd=ROOT, env=env, timeout=1200)
    lines = trace.read_text().splitlines() if trace.exists() else []
    at, matched = 0, []
    for pat in flow['expect']:
        rx = re.compile(pat)
        hit = next((i for i in range(at, len(lines)) if rx.search(lines[i])), None)
        matched.append(dict(pattern=pat, line=None if hit is None else lines[hit]))
        if hit is None:
            break
        at = hit + 1
    passed = len(matched) == len(flow['expect']) and all(m['line'] for m in matched)
    return dict(passed=passed, matched=matched, trace=lines[:200])


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    tag = sys.argv[1]
    names = sys.argv[2:] or list(FLOWS)
    report = ROOT / 'private' / 'reports' / f'menu-flows-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = {n: run(tag, n, FLOWS[n]) for n in names}
    with report.open('x') as f:
        json.dump(out, f, indent=1)
    for n, r in out.items():
        print(n, 'PASS' if r['passed'] else 'FAIL', [m['pattern'] for m in r['matched'] if not m['line']])


if __name__ == '__main__':
    main()
