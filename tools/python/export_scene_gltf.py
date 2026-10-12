#!/usr/bin/env python3
"""Export native static-scene research reports as glTF with provenance sidecars.

Materials are explicitly diagnostic: unlit/opaque/double sided. Original GS
blending and lightmaps are not translated. No collision/gameplay is synthesized.
"""
import argparse
import hashlib
import json
import os
import pathlib
import shutil
import struct
import tempfile


def read_report(path):
    if path.stat().st_size > 128 * 1024 * 1024:
        raise ValueError('input report exceeds policy')
    return json.loads(path.read_text())


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('objects', type=pathlib.Path)
    p.add_argument('world', type=pathlib.Path)
    p.add_argument('texture_export', type=pathlib.Path)
    p.add_argument('out', type=pathlib.Path)
    args = p.parse_args()
    if not args.out.resolve().is_relative_to(pathlib.Path('private').resolve()) or args.out.exists():
        raise ValueError('choose a new output directory within private/')
    objects = read_report(args.objects)
    world = read_report(args.world)
    textures = read_report(args.texture_export / 'manifest.json')
    args.out.parent.mkdir(parents=True, exist_ok=True)
    stage = pathlib.Path(tempfile.mkdtemp(prefix='.gdl-gltf-', dir=args.out.parent))
    try:
        document = dict(asset=dict(version='2.0', generator='gdl native report exporter 0.1.0'),
                        extensionsUsed=['KHR_materials_unlit'], scenes=[dict(nodes=[])], scene=0,
                        nodes=[], meshes=[], materials=[], images=[], textures=[],
                        samplers=[dict(magFilter=9728, minFilter=9728, wrapS=10497, wrapT=10497)],
                        buffers=[], bufferViews=[], accessors=[])
        data = bytearray()
        def accessor(values, components, integer=False):
            count = len(values)
            if not count:
                raise ValueError('empty primitive attribute')
            start = len(data)
            if integer:
                data.extend(struct.pack('<'+'I'*count, *values))
            else:
                for v in values:
                    if len(v) != components:
                        raise ValueError('attribute component count differs')
                    data.extend(struct.pack('<'+'f'*components, *v))
            view = len(document['bufferViews'])
            document['bufferViews'].append(dict(buffer=0, byteOffset=start, byteLength=len(data)-start,
                                                target=34963 if integer else 34962))
            a = dict(bufferView=view, componentType=5125 if integer else 5126,
                     count=count, type='SCALAR' if integer else f'VEC{components}')
            if not integer and components == 3:
                a.update(min=[min(v[c] for v in values) for c in range(3)],
                         max=[max(v[c] for v in values) for c in range(3)])
            document['accessors'].append(a)
            return len(document['accessors'])-1
        image_cache = {}
        material_indices = {}
        undecoded = []
        bitmaps = objects['objects']['bitmap_records']
        for b in bitmaps:
            candidates = [t for t in textures['table']['bitmaps'] if
                          (t['texture_offset'], t['format'], t['width'], t['height']) ==
                          (b['texture_offset'], b['format'], b['width'], b['height'])]
            image_index = None
            if len(candidates) == 1 and b['flags'] & 0x20 == 0:
                t = candidates[0]
                outcome = next(o for o in textures['outcomes'] if o['bitmap_index'] == t['index'])
                if outcome['png']:
                    if t['index'] not in image_cache:
                        source_png = args.texture_export / outcome['png']
                        if source_png.resolve().parent != args.texture_export.resolve():
                            raise ValueError('texture export path escaped cache')
                        name = f'bitmap-{t["index"]:06}.png'
                        shutil.copyfile(source_png, stage / name)
                        image_cache[t['index']] = len(document['images'])
                        document['images'].append(dict(uri=name, extras=dict(texdef_index=t['index'],
                            png_sha256=hashlib.sha256(source_png.read_bytes()).hexdigest())))
                    image_index = image_cache[t['index']]
            material = dict(name=f'object-bitmap-{b["index"]:06}', alphaMode='OPAQUE', doubleSided=True,
                            extensions=dict(KHR_materials_unlit={}),
                            pbrMetallicRoughness=dict(baseColorFactor=[1, 1, 1, 1], metallicFactor=0,
                                                     roughnessFactor=1),
                            extras=dict(source_bitmap_index=b['index'], raw_record=b['raw'],
                                        diagnostic='opaque base/vertex color; GS blend and lightmaps unverified'))
            if image_index is not None:
                tex = len(document['textures'])
                document['textures'].append(dict(sampler=0, source=image_index))
                material['pbrMetallicRoughness']['baseColorTexture'] = dict(index=tex)
            else:
                undecoded.append(b['index'])
                material['pbrMetallicRoughness']['baseColorFactor'] = [1, 0, 0.8, 1]
            material_indices[b['index']] = len(document['materials'])
            document['materials'].append(material)
        by_object = {}
        unsupported = []
        for row in objects['outcomes']:
            if row['error'] is not None:
                unsupported.append(dict(object=row['object'], part=row['part'], error=row['error']))
                continue
            part = objects['objects']['objects'][row['object']]['parts'][row['part']]
            attributes = {k: [] for k in ['positions', 'normals', 'uvs', 'lightmap_uvs', 'colors']}
            indices = []
            has_lightmap = all(s['lightmap_uvs'] for s in row['geometry']['strips'])
            for s in row['geometry']['strips']:
                base = len(attributes['positions'])
                indices.extend(i + base for i in s['indices'])
                for k in attributes:
                    if k == 'colors' and not s[k]:
                        attributes[k].extend([[1.0]*4 for _ in s['positions']])
                    else:
                        attributes[k].extend(s[k])
            if not indices:
                unsupported.append(dict(object=row['object'], part=row['part'], error='no emitted triangles'))
                continue
            n = len(attributes['positions'])
            if any(len(attributes[k]) != n for k in ['normals', 'uvs', 'colors']):
                raise ValueError('native attribute counts differ before glTF export')
            attrs = dict(POSITION=accessor(attributes['positions'], 3),
                         NORMAL=accessor(attributes['normals'], 3),
                         TEXCOORD_0=accessor(attributes['uvs'], 2),
                         COLOR_0=accessor(attributes['colors'], 4))
            if has_lightmap:
                attrs['TEXCOORD_1'] = accessor(attributes['lightmap_uvs'], 2)
            primitive = dict(attributes=attrs, indices=accessor(indices, 1, integer=True), mode=4,
                             extras=dict(source_part_index=part['index'], source_file_offset=part['file_offset'],
                                         raw_descriptor=part['raw_descriptor'], lightmap_index=part['lightmap_index']))
            if part['texture_index'] in material_indices:
                primitive['material'] = material_indices[part['texture_index']]
            by_object.setdefault(row['object'], []).append(primitive)
        mesh_indices = {}
        for index, primitives in sorted(by_object.items()):
            mesh_indices[index] = len(document['meshes'])
            document['meshes'].append(dict(name=f'object-{index:06}', primitives=primitives,
                extras=dict(source_object_index=index, raw_record=objects['objects']['objects'][index]['raw'])))
        names = {d['label']: d['object_index'] for d in objects['objects']['definitions']}
        unmatched = []
        for placement in world['candidate_placements']:
            node = world['world']['nodes'][placement['node_index']]
            obj = names.get(node['label'])
            if obj not in mesh_indices:
                unmatched.append(dict(node=node['index'], label=node['label']))
                continue
            index = len(document['nodes'])
            document['nodes'].append(dict(name=f'{node["index"]}:{node["label"]}', mesh=mesh_indices[obj],
                translation=placement['summed_position'], extras=dict(source_node_index=node['index'],
                candidate_parent=placement['parent'], raw_record=node['raw'])))
            document['scenes'][0]['nodes'].append(index)
        document['buffers'].append(dict(uri='scene.bin', byteLength=len(data)))
        (stage / 'scene.bin').write_bytes(data)
        (stage / 'scene.gltf').write_text(json.dumps(document, indent=2, allow_nan=False)+'\n')
        sidecar = dict(schema_version=1, objects_sha256=objects['source_sha256'],
                       worlds_sha256=world['source_sha256'], texdef_sha256=textures['texdef_sha256'],
                       textures_sha256=textures['textures_sha256'],
                       coordinate_convention='original XYZ retained; candidate summed next/child translations',
                       scope='static diagnostic scene; GS/VU semantics, alpha/lightmaps/collision/animation unaccepted',
                       unsupported_geometry=unsupported, undecoded_material_records=undecoded,
                       nodes_without_model=unmatched,
                       report_sha256={str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in
                                      [args.objects, args.world, args.texture_export/'manifest.json']})
        (stage / 'provenance.json').write_text(json.dumps(sidecar, indent=2)+'\n')
        if args.out.exists():
            raise ValueError('output appeared during export; existing data preserved')
        os.rename(stage, args.out)
        print(f'{len(document["nodes"])} nodes; {len(document["meshes"])} meshes; {len(document["images"])} source PNGs')
    finally:
        if stage.exists():
            shutil.rmtree(stage)


if __name__ == '__main__':
    main()
