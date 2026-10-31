//! NATIVE ENHANCEMENT (HD textures, graphics plan step 8): Real-ESRGAN's
//! SRVGGNetCompact (v0.3.0, BSD-3-Clause, Copyright (c) 2021 Xintao Wang)
//! as wgpu compute: a 3×3 convolution with PReLU per body layer, the last
//! convolution's 48 channels pixel-shuffled ×4, plus the nearest-upsampled
//! input, clamped to 0..1 (upstream's forward and post_process; float32 as
//! tools/python/upscale_compare.py, which matches the upstream ncnn program
//! within 0.18 / 255). Weights: tools/python/hd_weights.py's GDLSRVGG file.
use std::error::Error;
use wgpu::util::DeviceExt;

/// The user's choice at Checkpoint B: realesr-general-x4v3 mixed 0.5 / 0.5
/// with realesr-general-wdn-x4v3 (denoise 0.5).
pub const GENERAL_DN05: &[u8] = include_bytes!("../../assets/hd/realesr-general-x4v3-dn05.bin");

/// Parsed GDLSRVGG weights: every layer's floats in one array.
pub struct Weights {
    pub num_conv: usize,
    pub floats: Vec<f32>,
    /// Per convolution: (weight offset, bias offset, PReLU offset or None,
    /// in channels, out channels).
    pub convs: Vec<(u32, u32, Option<u32>, u32, u32)>,
}

impl Weights {
    pub fn parse(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < 24 || &bytes[..8] != b"GDLSRVGG" {
            return Err("not a GDLSRVGG weights file".into());
        }
        let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let (version, num_conv, feat, scale) = (word(8), word(12), word(16), word(20));
        if version != 1 || feat != 64 || scale != 4 || !(1..=64).contains(&num_conv) {
            return Err(format!("GDLSRVGG v{version} conv {num_conv} feat {feat} scale {scale}").into());
        }
        let floats: Vec<f32> = bytes[24..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let mut convs = Vec::new();
        let mut at = 0u32;
        let n = 2 * num_conv + 3;
        for i in (0..n).step_by(2) {
            let (cin, cout) = match i {
                0 => (3, 64),
                _ if i == n - 1 => (64, 48),
                _ => (64, 64),
            };
            let w = at;
            let b = w + cin * cout * 9;
            at = b + cout;
            let a = (i + 1 < n).then(|| {
                let a = at;
                at += 64;
                a
            });
            convs.push((w, b, a, cin, cout));
        }
        if at as usize != floats.len() {
            return Err(format!("GDLSRVGG holds {} floats, the network {at}", floats.len()).into());
        }
        Ok(Self { num_conv, floats, convs })
    }
}

/// The convolution kernel: 8×8 invocations, each 2×2 pixels × 16 output
/// channels (accumulators written out, so they stay in registers); input
/// channels in chunks of 8 through workgroup memory with a one-texel halo;
/// weights read with workgroup-uniform indices from the transposed layout
/// [in][tap][out] (Upscaler::new), 16 contiguous floats per step (one wide
/// scalar load).
fn conv_shader() -> String {
    let mut acc = String::new();
    let mut add = String::new();
    let mut store = String::new();
    for j in 0..16 {
        acc += &format!("    var a{j} = vec4<f32>(0.0);\n");
        add += &format!("                a{j} += weights[wb + {j}u] * v;\n");
        store += &format!("        put(a{j}, g * 16u + {j}u, n, px, py);\n");
    }
    format!(
        r#"
struct Conv {{
    n: u32, cin: u32, cout: u32, h: u32,
    w: u32, w_off: u32, b_off: u32, a_off: u32,
    groups: u32, p0: u32, p1: u32, p2: u32,
}};
@group(0) @binding(0) var<uniform> p: Conv;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> src: array<f32>;
@group(0) @binding(3) var<storage, read_write> dst: array<f32>;

var<workgroup> tile: array<f32, 2592>;

fn put(a: vec4<f32>, o: u32, n: u32, px: u32, py: u32) {{
    let b = weights[p.b_off + o];
    let plane = p.h * p.w;
    for (var k = 0u; k < 4u; k++) {{
        let x = px + k % 2u;
        let y = py + k / 2u;
        if (x < p.w && y < p.h) {{
            var v = a[k] + b;
            if (p.a_off != 0xffffffffu && v < 0.0) {{
                v = weights[p.a_off + o] * v;
            }}
            dst[(n * p.cout + o) * plane + y * p.w + x] = v;
        }}
    }}
}}

@compute @workgroup_size(8, 8, 1)
fn conv(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>, @builtin(local_invocation_index) li: u32) {{
    let n = wg.z / p.groups;
    let g = wg.z % p.groups;
    let x0 = wg.x * 16u;
    let y0 = wg.y * 16u;
    let plane = p.h * p.w;
{acc}    for (var c0 = 0u; c0 < p.cin; c0 += 8u) {{
        for (var i = li; i < 2592u; i += 64u) {{
            let ci = c0 + i / 324u;
            let r = i % 324u;
            let sy = i32(y0 + r / 18u) - 1;
            let sx = i32(x0 + r % 18u) - 1;
            var v = 0.0;
            if (ci < p.cin && 0 <= sy && 0 <= sx && u32(sy) < p.h && u32(sx) < p.w) {{
                v = src[(n * p.cin + ci) * plane + u32(sy) * p.w + u32(sx)];
            }}
            tile[i] = v;
        }}
        workgroupBarrier();
        let cn = min(8u, p.cin - c0);
        for (var ci = 0u; ci < cn; ci++) {{
            for (var k = 0u; k < 9u; k++) {{
                let t = ci * 324u + (lid.y * 2u + k / 3u) * 18u + lid.x * 2u + k % 3u;
                let v = vec4<f32>(tile[t], tile[t + 1u], tile[t + 18u], tile[t + 19u]);
                let wb = p.w_off + ((c0 + ci) * 9u + k) * p.cout + g * 16u;
{add}            }}
        }}
        workgroupBarrier();
    }}
    let px = x0 + lid.x * 2u;
    let py = y0 + lid.y * 2u;
{store}}}
"#
    )
}

const SHADER: &str = r#"
struct Shuffle {
    n: u32, h: u32, w: u32, pad: u32,
    oh: u32, ow: u32, p0: u32, p1: u32,
};
@group(0) @binding(0) var<uniform> s: Shuffle;
@group(0) @binding(1) var<storage, read> last: array<f32>;
@group(0) @binding(2) var<storage, read> input: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

// PixelShuffle(4) of the last convolution plus the nearest-upsampled input,
// clamped, only the crop inside the padding.
@compute @workgroup_size(8, 8, 1)
fn shuffle(@builtin(global_invocation_id) gid: vec3<u32>) {
    let X = gid.x;
    let Y = gid.y;
    if (s.ow <= X || s.oh <= Y) {
        return;
    }
    let n = gid.z / 3u;
    let c = gid.z % 3u;
    let py = Y + 4u * s.pad;
    let px = X + 4u * s.pad;
    let hy = py / 4u;
    let wx = px / 4u;
    let plane = s.h * s.w;
    let ch = c * 16u + (py % 4u) * 4u + px % 4u;
    let v = last[(n * 48u + ch) * plane + hy * s.w + wx] + input[(n * 3u + c) * plane + hy * s.w + wx];
    out[((n * 3u + c) * s.oh + Y) * s.ow + X] = clamp(v, 0.0, 1.0);
}
"#;

/// The network on a wgpu device of its own.
pub struct Upscaler {
    device: wgpu::Device,
    queue: wgpu::Queue,
    weights: wgpu::Buffer,
    convs: Vec<(u32, u32, Option<u32>, u32, u32)>,
    conv: wgpu::ComputePipeline,
    shuffle: wgpu::ComputePipeline,
    /// The largest storage buffer a binding may hold.
    max_binding: u64,
    pub adapter: String,
}

/// Bytes one network item needs in its largest buffer (64 channels).
pub fn item_bytes(padded_w: usize, padded_h: usize) -> u64 {
    64 * 4 * padded_w as u64 * padded_h as u64
}

impl Upscaler {
    pub fn new(weights: &Weights) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?;
        let info = adapter.get_info();
        let limits = adapter.limits();
        let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gdl hd textures"),
            required_limits: limits.clone(),
            ..Default::default()
        }))?;
        // Each convolution's weights as [in][tap][out] at a 16-float aligned
        // offset, its bias and PReLU after them.
        let mut floats = Vec::with_capacity(weights.floats.len() + 64 * weights.convs.len());
        let mut convs = Vec::new();
        for &(w_off, b_off, a_off, cin, cout) in &weights.convs {
            floats.resize(floats.len().next_multiple_of(16), 0.0);
            let w = floats.len() as u32;
            for ci in 0..cin {
                for k in 0..9 {
                    for co in 0..cout {
                        floats.push(weights.floats[(w_off + (co * cin + ci) * 9 + k) as usize]);
                    }
                }
            }
            let b = floats.len() as u32;
            floats.extend_from_slice(&weights.floats[b_off as usize..(b_off + cout) as usize]);
            let a = a_off.map(|a_off| {
                let a = floats.len() as u32;
                floats.extend_from_slice(&weights.floats[a_off as usize..a_off as usize + 64]);
                a
            });
            convs.push((w, b, a, cin, cout));
        }
        let weights_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("srvgg weights"),
            contents: &floats.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let pipeline = |source: String, entry: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(entry), source: wgpu::ShaderSource::Wgsl(source.into()) });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (conv, shuffle) = (pipeline(conv_shader(), "conv"), pipeline(SHADER.to_string(), "shuffle"));
        let max_binding = u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        Ok(Self {
            device,
            queue,
            weights: weights_buf,
            convs,
            conv,
            shuffle,
            max_binding,
            adapter: format!("{} ({:?}, {})", info.name, info.backend, info.driver_info),
        })
    }

    /// How many items of this padded size one batch takes: about 2 M
    /// texels, so one layer's dispatch stays short (the device may also
    /// drive the desktop, and drivers reset long-running submissions).
    pub fn batch_size(&self, padded_w: usize, padded_h: usize) -> usize {
        let per = item_bytes(padded_w, padded_h).max(1);
        let by_memory = (self.max_binding.min(256 << 20) / per) as usize;
        by_memory.min((1 << 21) / (padded_w * padded_h).max(1)).clamp(1, 4096)
    }

    /// Run the network on `items` (each 3 × ph × pw, channel-major, the
    /// same padded size), returning each item's crop inside `pad` of the 4×
    /// output (3 × 4(ph − 2 pad) × 4(pw − 2 pad)), clamped to 0..1.
    pub fn run(&self, items: &[&[f32]], pw: usize, ph: usize, pad: usize) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
        let n = items.len();
        let plane = pw * ph;
        if n == 0 {
            return Ok(Vec::new());
        }
        if items.iter().any(|i| i.len() != 3 * plane) || ph <= 2 * pad || pw <= 2 * pad {
            return Err("network input size".into());
        }
        let d = &self.device;
        let input: Vec<u8> = items.iter().flat_map(|i| i.iter().flat_map(|f| f.to_le_bytes())).collect();
        let input = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("srvgg input"), contents: &input, usage: wgpu::BufferUsages::STORAGE });
        let feat = (n * 64 * plane * 4) as u64;
        let storage = |label| d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: feat, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let (a, b) = (storage("srvgg a"), storage("srvgg b"));
        let (oh, ow) = (4 * (ph - 2 * pad), 4 * (pw - 2 * pad));
        let out_bytes = (n * 3 * oh * ow * 4) as u64;
        let out = d.create_buffer(&wgpu::BufferDescriptor { label: Some("srvgg out"), size: out_bytes, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = d.create_buffer(&wgpu::BufferDescriptor { label: Some("srvgg read"), size: out_bytes, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("srvgg") });
        let words = |w: [u32; 12]| w.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let mut keep = Vec::new();
        for (k, &(w_off, b_off, a_off, cin, cout)) in self.convs.iter().enumerate() {
            let groups = cout.div_ceil(16);
            let params = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("srvgg conv"),
                contents: &words([n as u32, cin, cout, ph as u32, pw as u32, w_off, b_off, a_off.unwrap_or(u32::MAX), groups, 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let (src, dst) = match k {
                0 => (&input, &a),
                _ if k % 2 == 1 => (&a, &b),
                _ => (&b, &a),
            };
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.conv.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.weights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: dst.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&self.conv);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(pw.div_ceil(16) as u32, ph.div_ceil(16) as u32, n as u32 * groups);
            drop(pass);
            keep.push(params);
            // One layer per submission (each short for the driver's
            // watchdog), queued back to back.
            self.queue.submit([std::mem::replace(&mut enc, d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("srvgg") })).finish()]);
        }
        let last = if self.convs.len() % 2 == 1 { &a } else { &b };
        let params = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("srvgg shuffle"),
            contents: &words([n as u32, ph as u32, pw as u32, pad as u32, oh as u32, ow as u32, 0, 0, 0, 0, 0, 0])[..32],
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.shuffle.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: last.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: input.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: out.as_entire_binding() },
            ],
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&self.shuffle);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(ow.div_ceil(8) as u32, oh.div_ceil(8) as u32, 3 * n as u32);
        }
        enc.copy_buffer_to_buffer(&out, 0, &read, 0, out_bytes);
        self.queue.submit([enc.finish()]);
        let slice = read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range();
        let floats: Vec<f32> = data.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        drop(data);
        read.unmap();
        let per = 3 * oh * ow;
        Ok(floats.chunks_exact(per).map(<[f32]>::to_vec).collect())
    }
}
