#!/usr/bin/env python3
"""Compare a native geometry report with separately pinned gdl_tools decoding.

The unchanged private LGPL decoder runs only in this research harness. It is
not shipped or linked into the native viewer. Its filesystem/conversion helpers
are unused; only its original padding helper is loaded from that helper module.
"""
import argparse
import ast
import hashlib
import importlib
import io
import json
import pathlib
import sys
import types


class Buffer(io.BytesIO):
    def __len__(self):
        return len(self.getbuffer())


def decoder(root):
    for name in ('gdl', 'gdl.compilation', 'gdl.compilation.g3d', 'gdl.compilation.g3d.serialization'):
        module = types.ModuleType(name)
        module.__path__ = [str(root.joinpath(*name.split('.')))]
        sys.modules[name] = module
    helper_path = root / 'gdl/compilation/util.py'
    tree = ast.parse(helper_path.read_text())
    padding = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'calculate_padding')
    helper = types.ModuleType('gdl.compilation.g3d.util')
    exec(compile(ast.Module(body=[padding], type_ignores=[]), str(helper_path), 'exec'), helper.__dict__)
    sys.modules[helper.__name__] = helper
    return importlib.import_module('gdl.compilation.g3d.serialization.model_vif')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('native_report', type=pathlib.Path)
    parser.add_argument('out', type=pathlib.Path)
    args = parser.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()):
        raise ValueError('owned-data reports must remain private')
    upstream_root = pathlib.Path('private/upstream/gdl_tools')
    upstream = decoder(upstream_root)
    native = json.loads(args.native_report.read_text())
    results = []
    for row in native['outcomes']:
        if row['error'] is not None:
            results.append(dict(object=row['object'], part=row['part'], skipped=row['error']))
            continue
        part = native['objects']['objects'][row['object']]['parts'][row['part']]
        model = types.SimpleNamespace(verts=[], norms=[], colors=[], uvs=[], lm_uvs=[], tri_lists={}, bnd_rad=0.0)
        payload = bytes(part['raw_container'][8:])
        upstream.import_vif_to_g3d(model, Buffer(payload), headerless=True, stream_len=len(payload))
        expected = dict(positions=model.verts, normals=model.norms, colors=model.colors,
                        uvs=model.uvs, lightmap_uvs=model.lm_uvs)
        maximum = {}
        for field, values in expected.items():
            actual = [v for strip in row['geometry']['strips'] for v in strip[field]]
            if len(actual) != len(values) or any(len(a) != len(b) for a, b in zip(actual, values)):
                raise ValueError(f'attribute count differs: {row["object"]}/{row["part"]}/{field}')
            maximum[field] = max((abs(x-y) for a, b in zip(actual, values) for x, y in zip(a, b)), default=0.0)
            if maximum[field] > 3e-7:
                raise ValueError(f'attribute values differ: {row["object"]}/{row["part"]}/{field}: {maximum[field]}')
        triangles = [i for faces in model.tri_lists.values() for face in faces for i in face[::3]]
        actual = []
        base = 0
        for strip in row['geometry']['strips']:
            actual.extend(i + base for i in strip['indices'])
            base += len(strip['positions'])
        if actual != triangles:
            raise ValueError(f'topology differs: {row["object"]}/{row["part"]}')
        results.append(dict(object=row['object'], part=row['part'], vertex_count=len(model.verts),
                            triangle_count=len(triangles)//3, maximum_absolute_difference=maximum))
    decoder_path = upstream_root / 'gdl/compilation/g3d/serialization/model_vif.py'
    report = dict(source_sha256=native['source_sha256'],
                  native_report_sha256=hashlib.sha256(args.native_report.read_bytes()).hexdigest(),
                  upstream_commit='47799f19ec85fa49af87eec4c64a27ee1fc87d7c',
                  upstream_decoder_sha256=hashlib.sha256(decoder_path.read_bytes()).hexdigest(),
                  scope='independent integer packet comparison; shared format knowledge; not GS/VU consumer validation',
                  results=results)
    with args.out.open('x') as file:
        json.dump(report, file, indent=2)
        file.write('\n')
    print(f'{len(results)} compared packet outcomes')


if __name__ == '__main__':
    main()
