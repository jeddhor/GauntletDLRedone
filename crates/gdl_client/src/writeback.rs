//! MSAA writeback for a camera that draws over the 3D view (NATIVE
//! ENHANCEMENT support). With MSAA every window camera shares one
//! multisampled texture per target; a camera that does not clear loads it
//! and resolves its own pass into the main texture. Post-processing of the
//! 3D camera (Settings::sharpen's contrast-adaptive sharpening) writes the
//! main texture only, so the next camera's resolve put the unprocessed
//! multisampled picture back over it. Bevy 0.18.1 declares the camera's
//! `msaa_writeback` but this build has no node performing it
//! (research/enhancements.md "Anti-aliasing"), so this one does what
//! bevy_camera's MsaaWriteback describes ("copies the previous camera's
//! output into the MSAA sampled texture before rendering"): before the
//! camera's main pass the current main texture is drawn into the
//! multisampled texture (each sample the resolved value), resolving into
//! the other main texture.
//! Only cameras whose `Camera::msaa_writeback` is `Always` run it (the
//! field Bevy extracts but nothing else here reads), only with MSAA. The
//! plugin adds no per-frame systems: its two pipelines are prepared once.
//! (A version with an extracted marker component and a prepare system
//! changed the draw order of equal-depth cut-out faces in G1's frozen
//! frame — 3,491 pixels of spear edges — with every effect off.)
use bevy::{
    camera::MsaaWriteback,
    core_pipeline::{
        blit::{BlitPipeline, BlitPipelineKey, init_blit_pipeline},
        core_2d::graph::{Core2d, Node2d},
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{
            CachedRenderPipelineId, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            SpecializedRenderPipelines, StoreOp,
        },
        renderer::RenderContext,
        view::ViewTarget,
    },
};

/// The blit pipelines into the HDR target at 2 and 4 samples (Settings::msaa).
#[derive(Resource)]
struct WritebackPipelines([(u32, CachedRenderPipelineId); 2]);

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct GdlMsaaWriteback;

pub struct MsaaWritebackPlugin;

impl Plugin for MsaaWritebackPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_writeback.after(init_blit_pipeline))
            .add_render_graph_node::<ViewNodeRunner<WritebackNode>>(Core2d, GdlMsaaWriteback)
            .add_render_graph_edge(Core2d, GdlMsaaWriteback, Node2d::StartMainPass);
    }
}

fn init_writeback(mut commands: Commands, pipeline_cache: Res<PipelineCache>, mut pipelines: ResMut<SpecializedRenderPipelines<BlitPipeline>>, blit: Res<BlitPipeline>) {
    let ids = [2, 4].map(|samples| {
        let key = BlitPipelineKey { texture_format: ViewTarget::TEXTURE_FORMAT_HDR, blend_state: None, samples };
        (samples, pipelines.specialize(&pipeline_cache, &blit, key))
    });
    commands.insert_resource(WritebackPipelines(ids));
}

#[derive(Default)]
struct WritebackNode;

impl ViewNode for WritebackNode {
    type ViewQuery = (&'static ViewTarget, &'static Msaa, &'static ExtractedCamera);

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (target, msaa, camera): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if camera.msaa_writeback != MsaaWriteback::Always || *msaa == Msaa::Off || !target.is_hdr() {
            return Ok(());
        }
        let Some(pipelines) = world.get_resource::<WritebackPipelines>() else { return Ok(()) };
        let Some(&(_, id)) = pipelines.0.iter().find(|(n, _)| *n == msaa.samples()) else { return Ok(()) };
        let cache = world.resource::<PipelineCache>();
        let blit = world.resource::<BlitPipeline>();
        let Some(pipeline) = cache.get_render_pipeline(id) else { return Ok(()) };
        let Some(sampled) = target.sampled_main_texture_view() else { return Ok(()) };
        // The current main texture is the input; the resolve of the
        // multisampled draw is the other main texture, which becomes the
        // main texture — a post-process write.
        let post = target.post_process_write();
        let bind_group = blit.create_bind_group(render_context.render_device(), post.source, cache);
        let pass = RenderPassDescriptor {
            label: Some("gdl_msaa_writeback"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: sampled,
                depth_slice: None,
                resolve_target: Some(post.destination),
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        };
        let mut render_pass = render_context.command_encoder().begin_render_pass(&pass);
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}
