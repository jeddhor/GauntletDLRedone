#!/usr/bin/env python3
"""Compare every Rust outcome against the pinned upstream table reader + Python zlib.

Research-only: requires separately fetched, attributed upstream files in private/.
Retain dictionary candidates separately; a matching hash is not a verified filename.
"""
import argparse
import ast
import hashlib
import json
import pathlib
import sys
import zlib


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--native', type=pathlib.Path, default=pathlib.Path('private/reports/wad-native.json'))
    p.add_argument('--archive', type=pathlib.Path, default=pathlib.Path('private/disc/WAD.BIN'))
    p.add_argument('--upstream', type=pathlib.Path, default=pathlib.Path('private/upstream/gdl_wad_decoder'))
    p.add_argument('--out', type=pathlib.Path, default=pathlib.Path('private/reports/wad-independent.json'))
    args = p.parse_args()
    sys.path.insert(0, str(args.upstream.resolve()))
    source = args.upstream / 'wad_decoder.py'
    module = ast.parse(source.read_text())
    # The upstream script invokes main unconditionally. Keep its actual table
    # reader and imports, but omit that invocation to avoid name-based extraction.
    module.body = [n for n in module.body if not (isinstance(n, ast.Expr) and isinstance(n.value, ast.Call)
                   and isinstance(n.value.func, ast.Name) and n.value.func.id == 'main')]
    namespace = {}
    exec(compile(module, str(source), 'exec'), namespace)
    native = json.loads(args.native.read_text())
    candidates = {}
    for name in namespace['name_list']:
        h = namespace['hash_me'](name.replace('/', '\\').upper())
        candidates.setdefault(h, [])
        if name not in candidates[h]:
            candidates[h].append(name)
    outcomes = []
    with args.archive.open('rb') as archive:
        sha = hashlib.file_digest(archive, 'sha256').hexdigest()
        if sha != native['source_sha256']:
            raise ValueError('archive identity differs')
        archive.seek(0)
        namespace['read_table_of_contents'](archive)
        reference = namespace['file_list']
        if len(reference) != len(native['entries']):
            raise ValueError('entry count differs')
        for index, (entry, observed) in enumerate(zip(reference, native['entries'], strict=True)):
            for upstream, rust in [('uncompressed_file_size', 'decoded_size'), ('file_pointer', 'file_offset'),
                                   ('hash', 'path_hash'), ('compressed_file_size', 'raw_compressed_size')]:
                if entry[upstream] != observed[rust]:
                    raise ValueError(f'entry {index}: table differs at {rust}')
            length = entry['uncompressed_file_size'] if entry['compressed_file_size'] == -1 else entry['compressed_file_size']
            archive.seek(entry['file_pointer'])
            stored = archive.read(length)
            decoded = stored if entry['compressed_file_size'] == -1 else zlib.decompress(stored)
            if len(decoded) != entry['uncompressed_file_size']:
                raise ValueError(f'entry {index}: independent decoded length differs')
            h = hashlib.sha256(decoded).hexdigest()
            if h != observed['decoded_sha256'] or hashlib.sha256(stored).hexdigest() != observed['stored_sha256']:
                raise ValueError(f'entry {index}: content differs')
            outcomes.append(dict(index=index, path_hash=entry['hash'], decoded_sha256=h,
                                 proposed_names=candidates.get(entry['hash'], []),
                                 naming_confidence='dictionary hash match; runtime lookup not verified'))
    result = dict(schema_version=1, source_sha256=sha,
                  upstream=json.loads((args.upstream / 'revision.json').read_text()),
                  upstream_reader_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  all_entries_agree=True, entries=outcomes)
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    named = sum(bool(e['proposed_names']) for e in outcomes)
    print(f'{len(outcomes)} entries agree byte-for-byte; {named} have dictionary candidates; {len(outcomes)-named} unnamed')


if __name__ == '__main__':
    main()
