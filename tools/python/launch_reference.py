#!/usr/bin/env python3
"""Configure and launch the private PCSX2 reference with global PINE enabled.

Run only after saving and exiting the existing reference. Original ISO, BIOS
and user configuration are preserved. A supplied state starts paused.
"""
import argparse
import configparser
import hashlib
import json
import os
import pathlib
import shutil
import subprocess


def settings(path):
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read(path)
    return parser


def configure(root, slot, start_paused):
    path = root / 'private/reference/config/PCSX2/inis/PCSX2.ini'
    if not path.is_file():
        raise ValueError('missing established private reference configuration')
    config = settings(path)
    config['EmuCore']['EnablePINE'] = 'true'
    config['EmuCore']['PINESlot'] = str(slot)
    config['UI']['StartPaused'] = 'true' if start_paused else 'false'
    # Avoid a UI-selector dependency when collecting subsequent states by hotkey.
    config['EmuCore']['UseSavestateSelector'] = 'false'
    backup = path.with_name('PCSX2.before-pine.ini')
    if not backup.exists():
        shutil.copy2(path, backup)
    staged = path.with_suffix('.ini.tmp')
    with staged.open('w') as file:
        config.write(file)
        file.flush()
        os.fsync(file.fileno())
    staged.replace(path)
    # The observed per-game override caused misleading checkbox/slot behavior.
    # Remove only PINE keys, preserving all other game-specific preferences.
    game_path = root / 'private/reference/config/PCSX2/gamesettings/SLUS-20047_15030637.ini'
    if game_path.exists():
        game = settings(game_path)
        if game.has_section('EmuCore'):
            game.remove_option('EmuCore', 'EnablePINE')
            game.remove_option('EmuCore', 'PINESlot')
        game_backup = game_path.with_suffix('.before-pine.ini')
        if not game_backup.exists():
            shutil.copy2(game_path, game_backup)
        game_staged = game_path.with_suffix('.ini.tmp')
        with game_staged.open('w') as file:
            game.write(file)
            file.flush()
            os.fsync(file.fileno())
        game_staged.replace(game_path)
    return path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=pathlib.Path)
    p.add_argument('--slot', type=int, default=28011)
    args = p.parse_args()
    if not 0 <= args.slot <= 65535:
        raise ValueError('PINE slot must be 0..65535')
    root = pathlib.Path(__file__).resolve().parents[2]
    log_path = root / 'private/reports/reference-boot.log'
    # Inspect command lines of the emulator only, never other application data.
    pids = subprocess.run(['pgrep', '-x', 'pcsx2-qt'], capture_output=True, text=True)
    for pid in pids.stdout.split():
        try:
            command = pathlib.Path(f'/proc/{int(pid)}/cmdline').read_bytes().split(b'\0')
        except FileNotFoundError:
            continue
        if os.fsencode(str(log_path)) in command:
            raise ValueError('private reference is still running: save the scene and exit before relaunching')
    state = args.state.resolve() if args.state else None
    if state is not None and (not state.is_file() or not state.is_relative_to(root / 'private')):
        raise ValueError('state must be an existing file inside private/')
    profile = json.loads((root / 'profiles/ps2/slus-20047.json').read_text())
    iso = root / profile['image']['filename']
    with iso.open('rb') as file:
        if hashlib.file_digest(file, 'sha256').hexdigest() != profile['image']['sha256']:
            raise ValueError('disc differs from the established source profile')
    ini = configure(root, args.slot, state is not None)
    env = os.environ.copy()
    env.update(QT_QPA_PLATFORM='xcb', XDG_CONFIG_HOME=str(root / 'private/reference/config'),
               XDG_CACHE_HOME=str(root / 'private/reference/cache'),
               XDG_DATA_HOME=str(root / 'private/reference/data'))
    command = ['/usr/games/pcsx2-qt', '-batch', '-fastboot', '-nofullscreen', '-logfile', str(log_path)]
    if state is not None:
        command.extend(['-statefile', str(state)])
    command.extend(['--', str(iso)])
    context = dict(source_profile=profile['profile_id'], disc_sha256=profile['image']['sha256'],
                   pine_slot=args.slot, ini_sha256=hashlib.sha256(ini.read_bytes()).hexdigest(),
                   state=str(state.relative_to(root)) if state else None,
                   state_sha256=hashlib.sha256(state.read_bytes()).hexdigest() if state else None,
                   start_paused=state is not None, command=command)
    (root / 'private/reports/reference-launch.json').write_text(json.dumps(context, indent=2) + '\n')
    with (root / 'private/reports/reference-stdout.log').open('w') as output:
        process = subprocess.Popen(command, env=env, cwd=root, stdout=output, stderr=subprocess.STDOUT)
        (root / 'private/reports/reference.pid').write_text(str(process.pid) + '\n')
        raise SystemExit(process.wait())


if __name__ == '__main__':
    main()
