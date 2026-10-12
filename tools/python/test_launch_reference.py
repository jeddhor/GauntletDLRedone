"""Synthetic private-reference configuration tests; no emulator or BIOS needed."""
import pathlib
import tempfile
import unittest
from launch_reference import configure, settings


class ReferenceConfigurationTests(unittest.TestCase):
    def test_global_ipc_and_backups_preserve_unrelated_preferences(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            ini = root / 'private/reference/config/PCSX2/inis/PCSX2.ini'
            game = root / 'private/reference/config/PCSX2/gamesettings/SLUS-20047_15030637.ini'
            ini.parent.mkdir(parents=True)
            game.parent.mkdir(parents=True)
            original = '[UI]\nStartPaused=false\nTheme=keep\n[EmuCore]\nEnablePINE=false\nPINESlot=28011\n[EmuCore/GS]\nRenderer=13\n'
            game_original = '[EmuCore]\nEnablePINE=true\nPINESlot=28012\n[EmuCore/GS]\nupscale_multiplier=2\n'
            ini.write_text(original)
            game.write_text(game_original)
            configure(root, 28011, True)
            result = settings(ini)
            self.assertTrue(result.getboolean('EmuCore', 'EnablePINE'))
            self.assertEqual(result.getint('EmuCore', 'PINESlot'), 28011)
            self.assertTrue(result.getboolean('UI', 'StartPaused'))
            self.assertEqual(result['UI']['Theme'], 'keep')
            self.assertEqual(result['EmuCore/GS']['Renderer'], '13')
            self.assertFalse(settings(game).has_option('EmuCore', 'EnablePINE'))
            self.assertFalse(settings(game).has_option('EmuCore', 'PINESlot'))
            self.assertEqual(settings(game)['EmuCore/GS']['upscale_multiplier'], '2')
            self.assertEqual(ini.with_name('PCSX2.before-pine.ini').read_text(), original)
            self.assertEqual(game.with_suffix('.before-pine.ini').read_text(), game_original)
            configure(root, 28012, False)
            self.assertEqual(ini.with_name('PCSX2.before-pine.ini').read_text(), original)
            self.assertEqual(game.with_suffix('.before-pine.ini').read_text(), game_original)


if __name__ == '__main__':
    unittest.main()
