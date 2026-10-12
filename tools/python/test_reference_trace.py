"""Synthetic schedule validation; no GUI/emulator/source assets required."""
import json
import pathlib
import tempfile
import unittest
from reference_trace import schedule_segments


class ScheduleTests(unittest.TestCase):
    def test_segments_preserve_boundaries_and_reject_invalid_or_excessive_input(self):
        with tempfile.TemporaryDirectory() as root:
            p = pathlib.Path(root) / 'schedule.json'
            value = [dict(steps=40, keys=['Right', 'Right']), dict(steps=26, keys=['Down'])]
            p.write_text(json.dumps(value))
            self.assertEqual(schedule_segments(p, 12, []), [dict(steps=40, keys=['Right']), dict(steps=26, keys=['Down'])])
            for value in [[], [dict(steps=121, keys=[])], [dict(steps=True, keys=[])],
                          [dict(steps=60, keys=[]), dict(steps=61, keys=[])],
                          [dict(steps=1, keys=['F10'])], [dict(steps=1, keys=[], unknown=1)]]:
                p.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    schedule_segments(p, 12, [])


if __name__ == '__main__':
    unittest.main()
