"""Synthetic transport tests; no emulator or retail bytes required."""
import struct
import unittest
from pine_probe import Pine


class FragmentedConnection:
    def __init__(self, response):
        self.response = bytearray(response)
        self.sent = bytearray()

    def sendall(self, data):
        self.sent.extend(data)

    def recv(self, length):
        data = bytes(self.response[:min(length, 2)])
        del self.response[:len(data)]
        return data


def reply(payload, status=0):
    return struct.pack('<I', len(payload) + 5) + bytes([status]) + payload


class PineTests(unittest.TestCase):
    def test_partial_framing_and_batched_reads(self):
        connection = FragmentedConnection(reply(struct.pack('<II', 123, 456)))
        self.assertEqual(Pine(connection).read_ee32([0x100000, 0x100004]), [123, 456])
        self.assertEqual(connection.sent, struct.pack('<I', 14) + b'\x02\x00\x00\x10\x00\x02\x04\x00\x10\x00')

    def test_bounds_failure_disconnect_and_malformed_string(self):
        pine = Pine(FragmentedConnection(b''))
        for addresses in [[1], [32*1024*1024], list(range(0, 513*4, 4))]:
            with self.assertRaises(ValueError):
                pine.read_ee32(addresses)
        for response in [b'\x05', struct.pack('<I', 999999), reply(b'', 255), reply(b'\x05\0\0\0x\0')]:
            with self.assertRaises(ValueError):
                Pine(FragmentedConnection(response)).text(8)


if __name__ == '__main__':
    unittest.main()
