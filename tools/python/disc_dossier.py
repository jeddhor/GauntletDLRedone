#!/usr/bin/env python3
"""Read-only ISO9660/ELF inventory; independently compare against extracted files.

No timestamps or absolute developer paths in the deterministic manifest.
Supports this single-volume, 2048-byte, non-interleaved ISO profile. Rejects
multi-extent records rather than silently reporting an incomplete file.
"""
import argparse
import hashlib
import json
import pathlib
import struct


def digest(stream, length):
    h = hashlib.sha256()
    while length:
        block = stream.read(min(length, 1024 * 1024))
        if not block:
            raise ValueError("truncated extent")
        h.update(block)
        length -= len(block)
    return h.hexdigest()


def both32(buf, offset):
    a = struct.unpack_from('<I', buf, offset)[0]
    b = struct.unpack_from('>I', buf, offset + 4)[0]
    if a != b:
        raise ValueError("ISO dual-endian field disagrees")
    return a


def elf_report(data):
    if data[:7] != b'\x7fELF\x01\x01\x01':
        raise ValueError("expected little-endian ELF32")
    header = struct.unpack_from('<HHIIIIIHHHHHH', data, 16)
    typ, machine, version, entry, phoff, shoff, flags, ehsize, phsize, phnum, shsize, shnum, stridx = header
    if ehsize != 52 or phsize != 32 or shsize != 40 or stridx >= shnum:
        raise ValueError("unsupported ELF header layout")
    if phoff + phnum * phsize > len(data) or shoff + shnum * shsize > len(data):
        raise ValueError("ELF tables exceed file")
    raw = [struct.unpack_from('<10I', data, shoff + i * shsize) for i in range(shnum)]
    _, _, _, _, off, size, *_ = raw[stridx]
    names = data[off:off + size]
    sections = []
    for row in raw:
        name, stype, sflags, addr, offset, length, link, info, align, entsize = row
        if stype != 8 and offset + length > len(data):
            raise ValueError("ELF section exceeds file")
        label = names[name:].split(b'\0', 1)[0].decode('ascii')
        sections.append(dict(name=label, type=stype, flags=sflags, virtual_address=addr,
                             file_offset=offset, size=length, link=link, info=info,
                             alignment=align, entry_size=entsize))
    segments = []
    for i in range(phnum):
        t, off, va, pa, fs, ms, pf, al = struct.unpack_from('<8I', data, phoff + i * phsize)
        if off + fs > len(data) or fs > ms:
            raise ValueError("ELF segment exceeds file/memory")
        segments.append(dict(type=t, file_offset=off, virtual_address=va, physical_address=pa,
                             file_size=fs, memory_size=ms, flags=pf, alignment=al))
    symbols = []
    for section in sections:
        if section['type'] != 2:
            continue
        strings = sections[section['link']]
        table = data[strings['file_offset']:strings['file_offset'] + strings['size']]
        if section['entry_size'] != 16 or section['size'] % 16:
            raise ValueError("bad ELF symbol stride")
        for pos in range(section['file_offset'], section['file_offset'] + section['size'], 16):
            name, value, size, info, other, ndx = struct.unpack_from('<IIIBBH', data, pos)
            symbols.append(dict(name=table[name:].split(b'\0', 1)[0].decode('ascii'),
                                guest_address=value, size=size, type=info & 15,
                                binding=info >> 4, section_index=ndx))
    return dict(type=typ, machine=machine, version=version, entry_point=entry, flags=flags,
                sections=sections, segments=segments, symbols=symbols)


def inspect(iso, extracted):
    size = iso.stat().st_size
    files = []
    visited = set()
    with iso.open('rb') as stream:
        sha = digest(stream, size)
        stream.seek(16 * 2048)
        pvd = stream.read(2048)
        if pvd[:7] != b'\x01CD001\x01' or struct.unpack_from('<H', pvd, 128)[0] != 2048:
            raise ValueError("unsupported ISO primary descriptor or block size")
        if both32(pvd, 80) * 2048 != size:
            raise ValueError("volume size differs from image")

        def walk(record, prefix):
            if len(visited) > 4096:
                raise ValueError("directory count limit exceeded")
            extent, directory_length = both32(record, 2), both32(record, 10)
            if extent in visited:
                raise ValueError("directory cycle/alias")
            visited.add(extent)
            if extent * 2048 + directory_length > size or directory_length > 16 * 1024 * 1024:
                raise ValueError("invalid directory extent")
            stream.seek(extent * 2048)
            directory = stream.read(directory_length)
            pos = 0
            while pos < directory_length:
                n = directory[pos]
                if not n:
                    pos = (pos // 2048 + 1) * 2048
                    continue
                r = directory[pos:pos + n]
                if n < 34 or len(r) != n or 33 + r[32] > n or pos % 2048 + n > 2048:
                    raise ValueError("invalid directory record")
                pos += n
                raw_name = r[33:33 + r[32]]
                if raw_name in (b'\0', b'\1'):
                    continue
                if r[26] or r[27] or r[25] & 128:
                    raise ValueError("unsupported interleaved/multi-extent file")
                name = raw_name.decode('ascii').split(';')[0]
                if '/' in name or '\\' in name or name in ('.', '..'):
                    raise ValueError("unsafe ISO name")
                path = prefix + name
                if r[25] & 2:
                    walk(r, path + '/')
                else:
                    start, length = both32(r, 2), both32(r, 10)
                    if start * 2048 + length > size:
                        raise ValueError("file extent exceeds image")
                    stream.seek(start * 2048)
                    h = digest(stream, length)
                    local = extracted / path
                    with local.open('rb') as src:
                        if local.stat().st_size != length or digest(src, length) != h:
                            raise ValueError(f"extraction mismatch: {path}")
                    files.append(dict(path=path, size=length, sha256=h, lba=start,
                                      iso_file_offset=start * 2048))
        walk(pvd[156:190], '')
    files.sort(key=lambda row: row['path'])
    if len({row['path'] for row in files}) != len(files):
        raise ValueError("duplicate ISO paths")
    actual = sorted(p.relative_to(extracted).as_posix() for p in extracted.rglob('*') if p.is_file())
    if actual != [r['path'] for r in files]:
        raise ValueError("extraction has missing or extra files")
    return dict(schema_version=1, image=dict(filename=iso.name, size=size, sha256=sha),
                filesystem='ISO9660', sector_size=2048,
                volume=pvd[40:72].decode('ascii').rstrip(),
                system=pvd[8:40].decode('ascii').rstrip(),
                publisher=pvd[318:446].decode('ascii').rstrip(),
                preparer=pvd[446:574].decode('ascii').rstrip(),
                files=files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('iso', type=pathlib.Path)
    parser.add_argument('--disc', type=pathlib.Path, default=pathlib.Path('private/disc'))
    parser.add_argument('--out', type=pathlib.Path, default=pathlib.Path('private/reports'))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    manifest = inspect(args.iso, args.disc)
    elf = elf_report((args.disc / 'SLUS_200.47').read_bytes())
    for name, value in [('disc-manifest.json', manifest), ('elf-metadata.json', elf)]:
        (args.out / name).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
    print(f"Verified {len(manifest['files'])} extracted files against ISO extents")
    print(f"ELF: {len(elf['sections'])} sections, {len(elf['symbols'])} symbols")


if __name__ == '__main__':
    main()
