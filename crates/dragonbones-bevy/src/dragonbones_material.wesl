// Fragment shader for the adapter's shared DragonBones material.
//
// One material per (atlas page image, blend mode): the texture binding is the
// atlas page, the vertex color carries the per-slot DragonBones tint (color
// multiplier + global alpha), and the blend mode is baked into the pipeline's
// blend state by `DragonBonesMaterial::specialize` on the Rust side.
//
// Mirrors `bevy_sprite_render/src/mesh2d/color_material.wesl`: sample the
// texture, multiply by the vertex color, then run the view's tonemapping and
// output color-space conversion so this material composes identically to
// `ColorMaterial`.
//
// Imports use the owning crate's WESL package name: `package::` would resolve
// to this crate (`dragonbones_bevy`), while the mesh2d modules are registered
// under `bevy_sprite_render`.
import bevy_sprite_render::mesh2d::{vertex_output::VertexOutput, view_bindings::view};

@if(TONEMAP_IN_SHADER)
import bevy_core_pipeline::tonemapping;
@if(SRGB_OUTPUT)
import bevy_render::color_operations::linear_to_srgb;
@if(OKLAB_OUTPUT)
import bevy_render::color_operations::linear_rgb_to_oklab;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var atlas_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(atlas_texture, atlas_sampler, in.uv);

    @if(VERTEX_COLORS)
    color = color * in.color;

    @if(TONEMAP_IN_SHADER)
    color = tonemapping::tone_mapping(color, view.color_grading);
    @if(SRGB_OUTPUT)
    color = vec4(linear_to_srgb(color.rgb), color.a);
    @if(OKLAB_OUTPUT)
    color = vec4(linear_rgb_to_oklab(color.rgb), color.a);

    return color;
}
