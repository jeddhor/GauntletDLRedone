# wgpu-hal 27.0.4, patched for Gauntlet: Dark Legacy (native port)

This is wgpu-hal 27.0.4 from crates.io (license: MIT OR Apache-2.0, see
LICENSE.MIT and LICENSE.APACHE), used through `[patch.crates-io]` in the
workspace Cargo.toml, with one change in `src/vulkan/instance.rs`
(`Surface::acquire_texture`, marked `GDL PATCH`):

After `vkAcquireNextImageKHR` succeeds, the Vulkan backend waits for the
swapchain's acquire fence with the frame timeout (wgpu issues 8310/8354:
frame pacing with DXGI-backed swapchains on Windows). A `VK_TIMEOUT` from that
wait went through `map_host_device_oom_and_lost_err`, i.e. it was reported as
an unexpected device error, and the device was lost. On an NVIDIA GTX 1080
under Windows this happened on a cold driver shader cache, where a frame of
the first level load took 3.7 s (2026-11-13: the build with wgpu-hal's
`internal_error_panic` panicked there with "Unexpected Vulkan error:
TIMEOUT", backtrace through `acquire_texture` and Bevy's `prepare_windows`;
the release builds reported "Unexpected error variant (driver
implementation is at fault)"). The patch waits for the fence again without
a timeout: the image is already acquired, so the frame is late, not lost.
wgpu-hal 30.0.1 has the same mapping (`vulkan/swapchain/native.rs`).

Research record: docs/research/rendering.md, "NVIDIA".
