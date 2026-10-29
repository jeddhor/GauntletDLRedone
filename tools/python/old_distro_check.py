#!/usr/bin/env python3
"""The Linux package on an older distribution (M21; research/release.md).

  old_distro_check.py TAG PACKAGE_TAR ISO [--image IMAGE]

In a `kwin_wayland --virtual --xwayland` session on the real GPU, the
package archive is unpacked and used inside gdl-runtime-focal
(tools/linux/Containerfile.test: Ubuntu 20.04, runtime libraries and its
own Mesa Vulkan drivers only; no network; the GPU's /dev/dri; the session's
X11 socket): unresolved libraries (ldd; the bundled libraries and plugins with lib/
first, as the installer's inherited DT_RPATH resolves them) of every ELF file; a `--batch`
install from the ISO (read-only) into a fresh home, then `gdl_import
verify`; the wizard on the X display for 8 s (a screenshot from outside);
the installed game started with no argument (TEST autostart, its own home,
no audio and no gamepads; triangle presses close the welcome scrolls), 600 updates with per-update simulation hashes
and two frames. The home lives in /tmp on the host (tmpfs), so the same
installation's game then runs on the host for the hash comparison; it is
deleted afterwards.

Report: private/reports/old-distro-TAG.json; evidence in
private/runs/old-distro-TAG/.
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
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'
GAME_ENV = 'GDL_TEST_AUTOSTART=1 GDL_NO_ATTRACT=1 GDL_NO_GAMEPADS=1 GDL_NO_AUDIO=1 GDL_EXIT_AFTER=600'

INSIDE = r'''#!/bin/bash
# In the container: /pkg.tar.gz, /iso/game.iso, /out (evidence), /home/t.
set -u
export HOME=/home/t XDG_DATA_HOME=/home/t/.local/share XDG_CONFIG_HOME=/home/t/.config XDG_CACHE_HOME=/home/t/.cache
mkdir -p /home/t/pkg && tar -C /home/t/pkg -xzf /pkg.tar.gz
P=$(echo /home/t/pkg/GauntletDarkLegacy-*)
cat /etc/os-release | grep PRETTY > /out/os.txt; ldd --version | head -1 >> /out/os.txt
find "$P" -type f | while read -r f; do
  if [ "$(head -c 4 "$f" | od -An -tx1 | tr -d ' \n')" = 7f454c46 ]; then
    echo "$f" >> /out/elf-files.txt
    # Libraries and plugins resolve through the installer's DT_RPATH
    # ($ORIGIN/lib, inherited by its whole dependency tree): lib/ first.
    case "$f" in "$P"/lib/*|"$P"/plugins/*) LD_LIBRARY_PATH="$P/lib" ldd "$f" ;; *) ldd "$f" ;; esac 2>&1 | grep 'not found' | sed "s|^|$f: |"
  fi
done > /out/ldd-missing.txt
"$P/GauntletDarkLegacy-Installer" --batch --source /iso/game.iso --dest /home/t/GauntletDarkLegacy > /out/install.log 2>&1
echo $? > /out/install.exit
/home/t/GauntletDarkLegacy/gdl_import verify /home/t/GauntletDarkLegacy/data > /out/verify.log 2>&1
"$P/GauntletDarkLegacy-Installer" > /out/wizard.log 2>&1 &
W=$!
sleep 8
if kill -0 $W 2>/dev/null; then echo up > /out/wizard.state; else echo exited > /out/wizard.state; fi
touch /out/wizard-shot
sleep 3
kill $W 2>/dev/null; wait $W 2>/dev/null
cd /home/t/GauntletDarkLegacy
env __GAME_ENV__ GDL_MENU_SCRIPT=/out/menu.json GDL_STATE_OUT=/out/state-inside.txt GDL_SHOT_DIR=/out/frames GDL_SHOT_FRAMES=300,600 \
    GDL_UI_TRACE=/out/ui-inside.trace ./gdl_play > /out/game-inside.log 2>&1
echo $? > /out/game-inside.exit
'''


def main():
    args = sys.argv[1:]
    if len(args) < 3:
        raise SystemExit(__doc__)
    tag, tar, iso = args[0], pathlib.Path(args[1]).resolve(), pathlib.Path(args[2]).resolve()
    image = args[args.index('--image') + 1] if '--image' in args else 'gdl-runtime-focal:1'
    if not tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    report = ROOT / 'private/reports' / f'old-distro-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = ROOT / 'private/runs' / f'old-distro-{tag}'
    out.mkdir(parents=True)
    (out / 'frames').mkdir()
    home = pathlib.Path(tempfile.mkdtemp(prefix=f'gdl-old-distro-{tag}-'))
    # The Tower's welcome scrolls pause the updates: triangle closes them
    # (ticks outside play), as in the boot-state equivalence runs.
    (out / 'menu.json').write_text(json.dumps([[30 + 30 * k, 32 + 30 * k, [0xc]] for k in range(8)]))
    inside = out / 'inside.sh'
    inside.write_text(INSIDE.replace('__GAME_ENV__', GAME_ENV))
    inside.chmod(0o755)
    podman = ['podman', 'run', '--rm', '--network=none', '--userns=keep-id', '--group-add', 'keep-groups', '--device', '/dev/dri',
              '-v', '/tmp/.X11-unix:/tmp/.X11-unix:ro', '-e', 'DISPLAY',
              '-v', f'{tar}:/pkg.tar.gz:ro', '-v', f'{iso}:/iso/game.iso:ro', '-v', f'{out}:/out', '-v', f'{home}:/home/t',
              '-v', f'{inside}:/inside.sh:ro']
    # The session's X authority file, when it has one.
    cmd = ' '.join(shlex.quote(a) for a in podman) + ' ${XAUTHORITY:+-v "$XAUTHORITY":/xauth:ro -e XAUTHORITY=/xauth} ' \
        + ' '.join(shlex.quote(a) for a in (image, 'bash', '/inside.sh'))
    session = out / 'session.sh'
    session.write_text(f'''#!/bin/bash
unset WAYLAND_DISPLAY
{cmd} > {shlex.quote(str(out / 'podman.log'))} 2>&1 &
C=$!
for i in $(seq 1 600); do [ -e {shlex.quote(str(out / 'wizard-shot'))} ] && break; kill -0 $C 2>/dev/null || break; sleep 0.5; done
W=$(xdotool search --name 'Installer$' 2>/dev/null | head -1)
[ -n "$W" ] && import -window "$W" {shlex.quote(str(out / 'wizard.png'))}
wait $C
''')
    session.chmod(0o755)
    env = {k: v for k, v in os.environ.items() if k != 'WAYLAND_DISPLAY'}
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', f'gdl-old-{os.getpid()}', '--width', '1600', '--height', '1000',
                    '--no-lockscreen', '--exit-with-session', str(session)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=1800)
    # The same installation's game on the host (glibc of this system).
    inst = home / 'GauntletDarkLegacy'
    host_state = out / 'state-host.txt'
    if (inst / 'gdl_play').exists():
        hhome = home / 'host-home'
        hhome.mkdir()
        (hhome / '.asoundrc').write_text('pcm.!default { type null }\n')
        game = out / 'host.sh'
        game.write_text(f'''#!/bin/bash
cd {shlex.quote(str(inst))}
env HOME={shlex.quote(str(hhome))} XDG_DATA_HOME={shlex.quote(str(hhome / '.local/share'))} XDG_CONFIG_HOME={shlex.quote(str(hhome / '.config'))} \\
    VK_DRIVER_FILES={RADV} VK_ICD_FILENAMES={RADV} {GAME_ENV} GDL_MENU_SCRIPT={shlex.quote(str(out / 'menu.json'))} GDL_STATE_OUT={shlex.quote(str(host_state))} ./gdl_play > {shlex.quote(str(out / 'game-host.log'))} 2>&1
''')
        game.chmod(0o755)
        subprocess.run([sys.executable, str(ROOT / 'tools/python/kwin_session.py'), str(out / 'host-kwin.log'), '--', str(game)], timeout=900)
    read = lambda name: (out / name).read_text().strip() if (out / name).exists() else None
    verify = read('verify.log')
    try:
        verify = json.loads(verify.splitlines()[-1]) if verify else None
    except ValueError:
        pass
    inside_hashes = (read('state-inside.txt') or '').splitlines()
    host_hashes = (read('state-host.txt') or '').splitlines()
    frames = sorted(p.name for p in (out / 'frames').iterdir())
    result = {
        'scope': 'the Linux package on Ubuntu 20.04 (container; real GPU through a virtual kwin session; actual disc image)',
        'image': image, 'package': str(tar), 'package_sha256': hashlib.sha256(tar.read_bytes()).hexdigest(),
        'os': read('os.txt'), 'elf_files': len((read('elf-files.txt') or '').splitlines()),
        'ldd_missing': (read('ldd-missing.txt') or '').splitlines(),
        'install_exit': read('install.exit'), 'install_last': (read('install.log') or '').splitlines()[-3:], 'verify': verify,
        'wizard': read('wizard.state'), 'wizard_screenshot': (out / 'wizard.png').exists(),
        'game_exit': read('game-inside.exit'), 'updates_inside': len(inside_hashes), 'frames': frames,
        'updates_host': len(host_hashes), 'hashes_equal': bool(inside_hashes) and inside_hashes == host_hashes,
        'first_difference': next((i for i, (a, b) in enumerate(zip(inside_hashes, host_hashes)) if a != b), None),
    }
    result['pass'] = (result['elf_files'] > 0 and not result['ldd_missing'] and result['install_exit'] == '0' and isinstance(verify, dict)
                      and verify.get('differing') == [] and result['wizard'] == 'up' and result['game_exit'] == '0'
                      and result['updates_inside'] == 600 and result['hashes_equal'])
    report.write_text(json.dumps(result, indent=1) + '\n')
    shutil.rmtree(home, ignore_errors=True)
    print(json.dumps({k: result[k] for k in ('os', 'elf_files', 'ldd_missing', 'install_exit', 'verify', 'wizard', 'game_exit', 'updates_inside', 'hashes_equal', 'pass')}))
    print(report)


if __name__ == '__main__':
    main()
