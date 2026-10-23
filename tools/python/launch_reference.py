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
from virtual_display import environment as virtual_environment


def settings(path):
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read(path)
    return parser


# The memory cards the reference normally uses (the user's; never written by
# a scenario). A scratch card replaces slot 1 only for card experiments.
USER_CARD_SLOT1 = 'Mcd001.ps2'


# Pads behind a multitap in port 1 (PCSX2 [Pad3]/[Pad4]/[Pad5] = port 0
# slots B/C/D, sioConvertPortAndSlotToPad; the game's joy index port × 4 +
# slot, serve_mtap 0x103d90), on keys no other pad or hotkey uses. Slot B
# (joy 1) stays empty: with pad 2 (joy 4) as player 2, ReadControls 0x104b60
# reads joy_remap[k] or port k for k = 0..3, i.e. ports 0, 4, 2, 3 — joy 1
# is never read and cannot join (multitap reference, 2026-10-23).
MULTITAP_PADS = {
    'Pad5': {'Up': '5', 'Down': '6', 'Left': '7', 'Right': '8', 'Cross': '9', 'Square': '0',
             'Triangle': 'Minus', 'Circle': 'Equal', 'Start': 'BracketLeft', 'Select': 'BracketRight'},
    'Pad4': {'Up': 'Home', 'Down': 'End', 'Left': 'Delete', 'Right': 'PageDown', 'Cross': 'Comma', 'Square': 'Slash',
             'Triangle': 'Backslash', 'Circle': 'PageUp', 'Start': 'Semicolon', 'Select': 'Apostrophe'},
}


def configure(root, slot, start_paused, diagnostic_hotkeys=False, card1=None, multitap=False):
    path = root / 'private/reference/config/PCSX2/inis/PCSX2.ini'
    if not path.is_file():
        raise ValueError('missing established private reference configuration')
    config = settings(path)
    # Slot 1: the user's card unless a scratch card is named for this launch
    # (left untouched when the configuration has no card section).
    if card1 is not None or config.has_section('MemoryCards'):
        if not config.has_section('MemoryCards'):
            config.add_section('MemoryCards')
        config['MemoryCards']['Slot1_Filename'] = card1 or USER_CARD_SLOT1
        # With a scratch card, slot 2 (the user's second card) is unplugged.
        config['MemoryCards']['Slot2_Enable'] = 'false' if card1 else 'true'
    config['EmuCore']['EnablePINE'] = 'true'
    config['EmuCore']['PINESlot'] = str(slot)
    config['UI']['StartPaused'] = 'true' if start_paused else 'false'
    # Avoid a UI-selector dependency when collecting subsequent states by hotkey.
    config['EmuCore']['UseSavestateSelector'] = 'false'
    if diagnostic_hotkeys:
        if not config.has_section('Hotkeys'):
            config.add_section('Hotkeys')
        config['Hotkeys']['FrameAdvance'] = 'Keyboard/F10'
        config['Hotkeys']['LoadStateFromSlot1'] = 'Keyboard/F7'
        config['Hotkeys']['SaveStateToSlot2'] = 'Keyboard/F11'
    if not config.has_section('Pad'):
        config.add_section('Pad')
    config['Pad']['MultitapPort1'] = 'true' if multitap else 'false'
    if not config.has_section('Pad3'):
        config.add_section('Pad3')
    config['Pad3']['Type'] = 'None'
    for name, keys in MULTITAP_PADS.items():
        if not config.has_section(name):
            config.add_section(name)
        if multitap:
            config[name]['Type'] = 'DualShock2'
            for button, key in keys.items():
                config[name][button] = f'Keyboard/{key}'
        else:
            config[name]['Type'] = 'None'
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
    p.add_argument('--diagnostic-hotkeys', action='store_true', help='private F10 advance, F7 load slot 1, F11 save slot 2')
    p.add_argument('--multitap', action='store_true',
                   help='a multitap in port 1 with pads 3 and 4 on keys (MULTITAP_PADS); pad 2 stays in port 2')
    p.add_argument('--scratch-card1', help='a NEW card file name (scratch-*.ps2) for slot 1 instead of the user\'s card; '
                   'PCSX2 creates it unformatted; it must not exist yet')
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
    card1 = None
    if args.scratch_card1:
        name = args.scratch_card1
        if not (name.startswith('scratch-') and name.endswith('.ps2')) or '/' in name:
            raise ValueError('scratch card must be a bare scratch-*.ps2 name')
        if (root / 'private/reference/config/PCSX2/memcards' / name).exists():
            raise ValueError('scratch card exists: choose a new name (cards are never reused or overwritten)')
        card1 = name
    ini = configure(root, args.slot, state is not None, args.diagnostic_hotkeys, card1, args.multitap)
    env = virtual_environment()
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
                   start_paused=state is not None, diagnostic_hotkeys=args.diagnostic_hotkeys, card1=card1 or USER_CARD_SLOT1,
                   multitap=args.multitap,
                   display=env['DISPLAY'], presentation_driver='Mesa software / lavapipe for Xvfb', command=command)
    (root / 'private/reports/reference-launch.json').write_text(json.dumps(context, indent=2) + '\n')
    with (root / 'private/reports/reference-stdout.log').open('w') as output:
        process = subprocess.Popen(command, env=env, cwd=root, stdout=output, stderr=subprocess.STDOUT)
        (root / 'private/reports/reference.pid').write_text(str(process.pid) + '\n')
        raise SystemExit(process.wait())


if __name__ == '__main__':
    main()
