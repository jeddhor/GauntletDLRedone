"""Synthetic process-identity guard checks; no display or emulator access."""
import pathlib
import tempfile
import unittest
from virtual_display import server_identity


class ServerIdentityTests(unittest.TestCase):
    def test_xvfb_authentication_and_start_time_are_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);process=root/'123';process.mkdir()
            args=['/usr/bin/Xvfb',':191','-screen','0','1280x1024x24','-auth','/tmp/test-Xauthority']
            process.joinpath('cmdline').write_bytes(('\0'.join(args)+'\0').encode())
            process.joinpath('stat').write_text('123 (Xvfb) '+' '.join(['S']+['0']*18+['777']+['0']*3))
            context=dict(server_pid=123,server_start_ticks='777',display=':191',authority='/tmp/test-Xauthority')
            self.assertEqual(server_identity(context,root),123)
            for field,value in [('server_start_ticks','778'),('display',':0'),('authority','/tmp/other-authority')]:
                with self.assertRaises(ValueError):
                    server_identity(dict(context,**{field:value}),root)
            args[0]='/usr/bin/Xwayland'
            process.joinpath('cmdline').write_bytes(('\0'.join(args)+'\0').encode())
            with self.assertRaises(ValueError):
                server_identity(context,root)


if __name__=='__main__':
    unittest.main()
