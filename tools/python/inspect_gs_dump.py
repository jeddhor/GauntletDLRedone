#!/usr/bin/env python3
"""Bounded stock PCSX2 GS-dump framing/GIF A+D diagnostics, never runtime code.

The dump contains private retail graphics evidence. Register observations are
not yet material bindings or an accepted native rendering implementation.
"""
import argparse
import collections
import compression.zstd
import hashlib
import json
import pathlib
import struct


def parse(data, keep_streams=False):
    def region(p, n):
        if p < 0 or n < 0 or p+n > len(data):
            raise ValueError(f'truncated dump range at {p:#x}')
        return data[p:p+n]
    if len(data) > 64*1024*1024 or struct.unpack('<I', region(0, 4))[0] != 0xffffffff:
        raise ValueError('unsupported/over-limit GS dump')
    size, = struct.unpack('<I', region(4, 4))
    fields = struct.unpack('<9I', region(8, 36))
    names = ['state_version', 'state_size', 'serial_offset', 'serial_size', 'crc',
             'screenshot_width', 'screenshot_height', 'screenshot_offset', 'screenshot_size']
    header = dict(zip(names, fields, strict=True))
    region(8, size)
    for offset, count in [(header['serial_offset'], header['serial_size']),
                          (header['screenshot_offset'], header['screenshot_size'])]:
        if offset < 36 or offset+count > size:
            raise ValueError('GS secondary-header range invalid')
    serial = region(8+header['serial_offset'], header['serial_size']).decode('ascii')
    if serial != 'SLUS-20047' or header['crc'] != 0x15030637:
        raise ValueError('GS dump differs from reference build')
    state = region(8+size, header['state_size'])
    cursor = 8+size+header['state_size']
    registers = region(cursor, 8192)
    cursor += 8192
    events = []
    streams = collections.defaultdict(bytearray)
    while cursor < len(data):
        at = cursor
        kind = region(cursor, 1)[0]
        cursor += 1
        if kind == 0:
            path = region(cursor, 1)[0]
            n, = struct.unpack('<I', region(cursor+1, 4))
            cursor += 5
            if path > 3 or n > 16*1024*1024:
                raise ValueError('transfer path/size policy')
            payload = region(cursor, n)
            cursor += n
            streams[path].extend(payload)
            events.append(dict(offset=at, type='transfer', path=path, bytes=n))
        elif kind == 1:
            field = region(cursor, 1)[0]
            cursor += 1
            events.append(dict(offset=at, type='vsync', field=field))
        elif kind == 2:
            n, = struct.unpack('<I', region(cursor, 4))
            cursor += 4
            events.append(dict(offset=at, type='read_fifo', bytes=n))
        elif kind == 3:
            region(cursor, 8192)
            cursor += 8192
            events.append(dict(offset=at, type='register_snapshot'))
        else:
            raise ValueError(f'unknown event {kind} at {at:#x}')
        if len(events) > 1_000_000:
            raise ValueError('GS event count policy')
    decoded = []
    for path, stream in sorted(streams.items()):
        cursor = 0
        counts = collections.Counter()
        writes = []
        tags = 0
        while cursor < len(stream):
            if cursor+16 > len(stream):
                raise ValueError('partial GIF tag across concatenated transfers')
            low, layout = struct.unpack_from('<QQ', stream, cursor)
            cursor += 16
            loops = low & 0x7fff
            mode = (low >> 58) & 3
            count = (low >> 60) & 15 or 16
            tags += 1
            if tags > 250_000:
                raise ValueError('GIF tag count policy')
            payload_size = loops*count*16 if mode == 0 else ((loops*count*8+15)//16)*16 if mode == 1 else loops*16
            if cursor+payload_size > len(stream):
                raise ValueError('GIF payload exceeds captured stream')
            if mode == 0:
                for i in range(loops*count):
                    reg = (layout >> ((i % count)*4)) & 15
                    counts[reg] += 1
                    if reg == 14:
                        value, address = struct.unpack_from('<QQ', stream, cursor+i*16)
                        writes.append(dict(stream_offset=cursor+i*16, register=address & 255,
                                           value_hex=f'{value:016x}'))
                        if len(writes) > 250_000:
                            raise ValueError('GIF register-write count policy')
            cursor += payload_size
        decoded.append(dict(path=path, bytes=len(stream), tags=tags,
                            packed_register_counts=dict(counts), ad_writes=writes))
    report = dict(header=header, serial=serial, state_sha256=hashlib.sha256(state).hexdigest(),
                  initial_registers_sha256=hashlib.sha256(registers).hexdigest(), events=events,
                  gif_streams=decoded, scope='framing and raw register observations; material/primitive bindings unverified')
    if keep_streams:
        report['streams'] = streams
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('dump', type=pathlib.Path)
    p.add_argument('out', type=pathlib.Path)
    args = p.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('choose a new private output file')
    if args.dump.stat().st_size > 64*1024*1024:
        raise ValueError('compressed GS dump exceeds policy')
    compressed = args.dump.read_bytes()
    decoder = compression.zstd.ZstdDecompressor()
    raw = decoder.decompress(compressed, max_length=64*1024*1024+1)
    if len(raw) > 64*1024*1024 or not decoder.eof:
        raise ValueError('incomplete/over-limit decompressed GS dump')
    report = parse(raw)
    report.update(dump_sha256=hashlib.sha256(compressed).hexdigest(), decoded_bytes=len(raw))
    with args.out.open('x') as file:
        json.dump(report, file, indent=2)
        file.write('\n')
    print(f'{len(report["events"])} framed events; {len(report["gif_streams"])} GIF paths')


if __name__ == '__main__':
    main()
