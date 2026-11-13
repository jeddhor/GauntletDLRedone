#!/usr/bin/env python3
"""The SPIR-V the renderer hands the Vulkan driver, validated.

  spirv_check.py TAG [--skip-build]

NVIDIA's shader compiler refuses invalid SPIR-V that Mesa (RADV, lavapipe)
accepts: naga 27 passed a binding array's element to a function's texture
parameter as a loaded handle (gs.wgsl bspline_r), and the GTX 1080 lost its
device on the first 3D scene (research/rendering.md "NVIDIA"). This machine
cannot see that, so this check looks at the SPIR-V itself:

1. a copy of the locked wgpu-hal with a DIAGNOSTIC patch (every pipeline
   stage's SPIR-V written to GDL_SPV_DUMP; a failed render pipeline's label
   and stages on stderr) under private/diag-patch, and gdl_play built against
   it (cargo --config patch; target-spirv; Cargo.lock restored afterwards);
2. that build on the private Xvfb with lavapipe (client_driver.py), every
   graphics option on: the title, Start, the select screen; then a TEST
   character in G1 (GDL_TEST_AUTOSTART, GDL_DEBUG_LEVEL) hitting secret
   wall 7 (GDL_TEST_ACTIONS damage_item), so the hit spark's Z-modifier
   variant (gs.wgsl DEPTH_MOD, frag_depth) is compiled too;
3. spirv-val --target-env vulkan1.3 on every stage written.

Two findings occur in naga's output of shaders every driver tested compiles
(the GTX 1080 included): explicit layout decorations on Function or
Workgroup variables (VUID-StandaloneSpirv-None-10684) and atomics with
relaxed memory semantics (VUID-StandaloneSpirv-MemorySemantics-10871). They
are counted, not failed; any other finding fails. Stages writing FragDepth
are counted (spirv-dis). Results:
private/runs/spirv-check-TAG/result.json. Research only.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
KNOWN = ('VUID-StandaloneSpirv-None-10684', 'VUID-StandaloneSpirv-MemorySemantics-10871')
TOOLCHAIN = pathlib.Path.home() / '.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin'


def locked_version(name):
    lock = (ROOT / 'Cargo.lock').read_text()
    m = re.search(r'\[\[package\]\]\nname = "%s"\nversion = "([^"]+)"' % re.escape(name), lock)
    return m.group(1)


def patched_hal():
    """private/diag-patch/wgpu-hal-VERSION with the DIAGNOSTIC patch."""
    version = locked_version('wgpu-hal')
    registry = next((pathlib.Path.home() / '.cargo/registry/src').glob(f'*/wgpu-hal-{version}'))
    out = ROOT / 'private/diag-patch' / f'wgpu-hal-{version}'
    if out.exists():
        shutil.rmtree(out)
    shutil.copytree(registry, out)
    p = out / 'src/vulkan/device.rs'
    s = p.read_text()

    def sub(old, new):
        nonlocal s
        if s.count(old) != 1:
            raise SystemExit(f'wgpu-hal {version}: patch anchor not found once: {old[:60]!r}')
        s = s.replace(old, new)
    spv = """                let spv = {
                    profiling::scope!("naga::spv::write_vec");
                    naga::back::spv::write_vec(&module, &info, options, Some(&pipeline_options))
                }
                .map_err(|e| crate::PipelineError::Linkage(stage_flags, format!("{e}")))?;
"""
    sub(spv, spv + """                if let Some(dir) = std::env::var_os("GDL_SPV_DUMP") {
                    let mut h: u32 = 2166136261;
                    for w in &spv {
                        h = (h ^ *w).wrapping_mul(16777619);
                    }
                    let name = alloc::format!("{}-{:08x}.spv", stage.entry_point, h);
                    let bytes: Vec<u8> = spv.iter().flat_map(|w| w.to_le_bytes()).collect();
                    let _ = std::fs::write(std::path::Path::new(&dir).join(&name), bytes);
                    GDL_STAGES.with(|s| s.borrow_mut().push(name));
                }
""")
    sub("""    ) -> Result<super::RenderPipeline, crate::PipelineError> {
""", """    ) -> Result<super::RenderPipeline, crate::PipelineError> {
        GDL_STAGES.with(|s| s.borrow_mut().clear());
""")
    sub("""                    .create_graphics_pipelines(pipeline_cache, &vk_infos, None)
                    .map_err(|(_, e)| super::map_pipeline_err(e))""", """                    .create_graphics_pipelines(pipeline_cache, &vk_infos, None)
                    .map_err(|(_, e)| {
                        std::eprintln!("GDL pipeline failed: {:?} label {:?} stages {:?}", e, desc.label, GDL_STAGES.with(|s| s.borrow().clone()));
                        super::map_pipeline_err(e)
                    })""")
    sub("use super::{conv, RawTlasInstance};", "use super::{conv, RawTlasInstance};\n\nstd::thread_local! { static GDL_STAGES: core::cell::RefCell<Vec<alloc::string::String>> = const { core::cell::RefCell::new(Vec::new()) }; }")
    p.write_text(s)
    return out


def build(hal):
    lock = ROOT / 'Cargo.lock'
    saved = lock.read_bytes()
    env = dict(os.environ, RUSTC=str(TOOLCHAIN / 'rustc'), CARGO_TARGET_DIR=str(ROOT / 'target-spirv'))
    try:
        subprocess.run([str(TOOLCHAIN / 'cargo'), 'build', '--release', '--offline', '-p', 'gdl_client', '--bin', 'gdl_play',
                        '--config', f'patch.crates-io.wgpu-hal.path="{hal.relative_to(ROOT)}"'], cwd=ROOT, env=env, check=True)
    finally:
        lock.write_bytes(saved)
    return ROOT / 'target-spirv/release/gdl_play'


def drive(tag, binary, dump, settings, steps, extra):
    d = ROOT / 'private/runs' / f'client-{tag}'
    if d.exists():
        shutil.rmtree(d)
    (d / 'saves').mkdir(parents=True)
    (d / 'saves/settings.json').write_text(json.dumps(settings))
    (d / 'steps.json').write_text(json.dumps(steps))
    env = dict(os.environ, GDL_PLAY=str(binary), GDL_SPV_DUMP=str(dump), GDL_UI_TRACE=str(d / 'ui.log'), **extra)
    subprocess.run([sys.executable, 'tools/python/client_driver.py', 'serve', tag, str(d / 'steps.json')], cwd=ROOT, env=env,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=900)
    log = (d / 'client.log').read_text(errors='replace')
    trace = (d / 'ui.log').read_text(errors='replace').splitlines() if (d / 'ui.log').exists() else []
    return {'run': f'client-{tag}', 'pipeline_failures': [ln for ln in log.splitlines() if 'GDL pipeline failed' in ln],
            'panics': [ln for ln in log.splitlines() if 'panicked' in ln], 'last_trace': trace[-1:] if trace else []}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--skip-build', action='store_true')
    a = ap.parse_args()
    run = ROOT / 'private/runs' / f'spirv-check-{a.tag}'
    run.mkdir(parents=True, exist_ok=False)
    dump = run / 'spv'
    dump.mkdir()
    binary = ROOT / 'target-spirv/release/gdl_play' if a.skip_build else build(patched_hal())
    # Every graphics option on (the Enhanced preset and the lighting), 4×
    # MSAA, windowed at the software renderer's size.
    settings = {'msaa': 4, 'fullscreen': False, 'widescreen': True, 'texture_filter': 'sharp', 'sharpen': 40, 'full_colour': True,
                'smooth_lightmaps': True, 'more_lights': True, 'torch_lights': True, 'shadows': True, 'ambient_occlusion': True,
                'bloom': True, 'smaa': False, 'hd_textures': False}
    # The G1 character beside secret wall 7, which it damages every second
    # (its hit sparks carry a Z modifier).
    actions = run / 'g1-actions.json'
    actions.write_text(json.dumps([{'at': 30, 'health': 50000}, {'at': 40, 'teleport': [4.8, 10.0, -53.9]}]
                                  + [{'at': 60 + 30 * k, 'damage_item': [7, 1]} for k in range(20)]))
    runs = [
        drive(f'{a.tag}-select', binary, dump, settings,
              [['wait', 10], ['key', 'Return', 0.15], ['wait', 2], ['key', 'Return', 0.15], ['wait', 25], ['quit']], {}),
        drive(f'{a.tag}-g1', binary, dump, settings, [['wait', 60], ['quit']],
              {'GDL_TEST_AUTOSTART': '1', 'GDL_DEBUG_LEVEL': 'G1', 'GDL_WINDOW': '640x448', 'GDL_TEST_ACTIONS': str(actions)}),
    ]
    stages, findings, known = [], [], {k: 0 for k in KNOWN}
    frag_depth = 0
    for f in sorted(dump.glob('*.spv')):
        dis = subprocess.run(['spirv-dis', str(f)], capture_output=True, text=True)
        frag_depth += 'BuiltIn FragDepth' in dis.stdout
        r = subprocess.run(['spirv-val', '--target-env', 'vulkan1.3', str(f)], capture_output=True, text=True)
        text = (r.stdout + r.stderr).strip()
        stages.append(f.name)
        if r.returncode == 0:
            continue
        hit = next((k for k in KNOWN if k in text), None)
        if hit:
            known[hit] += 1
        else:
            findings.append({'stage': f.name, 'error': text.splitlines()[:3]})
    fail = []
    if len(stages) < 20:
        fail.append(f'only {len(stages)} stages written')
    for r in runs:
        fail += [f'{r["run"]}: {x}' for x in r['pipeline_failures'] + r['panics']]
    fail += [f'{f["stage"]}: {f["error"][0]}' for f in findings]
    result = {'tag': a.tag, 'passed': not fail, 'failures': fail, 'stages': len(stages), 'frag_depth_stages': frag_depth, 'known_naga_patterns': known,
              'findings': findings, 'runs': runs}
    (run / 'result.json').write_text(json.dumps(result, indent=1) + '\n')
    print(json.dumps({k: result[k] for k in ('passed', 'failures', 'stages', 'frag_depth_stages', 'known_naga_patterns')}, indent=1))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
