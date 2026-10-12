#!/usr/bin/env python3
"""Catalog observed families; invoke the native texture metadata reader for each.

Proposed archive names remain labels. Structural identification of an unnamed
TEXDEF is possible without discarding or renaming its stable entry identity.
"""
import collections
import json
import pathlib
import struct
import subprocess


def main():
    root = pathlib.Path('private/wad')
    manifest = json.loads((root / 'manifest.json').read_text())
    names = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())['entries']
    output = pathlib.Path('private/texture-metadata')
    output.mkdir(exist_ok=True)
    records = []
    families = collections.Counter()
    bitmap_count = 0
    formats = collections.Counter()
    failures = []
    for e, name in zip(manifest['entries'], names, strict=True):
        if e['index'] != name['index']:
            raise ValueError('manifest entry order mismatch')
        path = root / e['output_file']
        label = name['proposed_names'][0] if name['proposed_names'] else ''
        family = pathlib.PurePosixPath(label).name.upper() if label else 'unknown'
        # Retain a structural hypothesis for unnamed records with a bounded,
        # internally consistent TEXDEF header and observed v23 signature.
        if not label and e['decoded_size'] >= 76:
            with path.open('rb') as f:
                count, definitions, bitmaps = struct.unpack('<III', f.read(12))
                if 0 < count <= 100_000 and definitions >= 12 and bitmaps >= 12 \
                        and definitions + count * 36 <= e['decoded_size'] \
                        and bitmaps + count * 64 <= e['decoded_size']:
                    f.seek(bitmaps)
                    if f.read(4) == b'\x17\x00\x0b\xf0':
                        family = 'candidate TEXDEF.PS2 (unnamed)'
        row = dict(identity=e['identity'], index=e['index'], decoded_sha256=e['decoded_sha256'],
                   proposed_names=name['proposed_names'], family=family,
                   decoded_size=e['decoded_size'], semantic_decode='not implemented',
                   runtime_support='not implemented', validation='unverified')
        families[family] += 1
        if family in ('TEXDEF.PS2', 'candidate TEXDEF.PS2 (unnamed)'):
            dest = output / f"{e['index']:06}-{e['path_hash']:08x}"
            if not dest.exists():
                result = subprocess.run(['target/release/gdl_tools', 'texdef-inspect', str(path), str(dest)],
                                        capture_output=True, text=True)
                if result.returncode:
                    failures.append(dict(index=e['index'], error=result.stderr))
                    row['validation'] = 'failed metadata parse'
                    records.append(row)
                    continue
            metadata = json.loads((dest / 'manifest.json').read_text())
            if metadata['texdef_sha256'] != e['decoded_sha256']:
                raise ValueError('stale metadata source identity')
            bitmap_count += len(metadata['table']['bitmaps'])
            for b in metadata['table']['bitmaps']:
                formats[b['format']] += 1
            row['semantic_decode'] = 'bounded v23 metadata; pixel/material semantics incomplete'
            row['validation'] = 'native metadata parse passed'
        records.append(row)
    report = dict(schema_version=1, source_sha256=manifest['source_sha256'],
                  archive_entries=len(records), families=dict(families),
                  texture_records=bitmap_count, texture_formats=dict(formats),
                  texture_metadata_failures=failures, entries=records)
    pathlib.Path('private/reports/asset-catalog.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'{len(records)} entries cataloged; {bitmap_count} texture records; {len(failures)} metadata failures')
    print(json.dumps(dict(formats), sort_keys=True))
    if failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
