// Vertex: unit quad positions scaled by transform, UV coords into the image texture.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv:       vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Transform {
    scale_x:     f32,
    scale_y:     f32,
    translate_x: f32,
    translate_y: f32,
};

@group(1) @binding(0) var<uniform> transform: Transform;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = vec4<f32>(
        in.position.x * transform.scale_x + transform.translate_x,
        in.position.y * transform.scale_y + transform.translate_y,
        0.0, 1.0
    );
    out.uv = in.uv;
    return out;
}

@group(0) @binding(0) var t_image: texture_2d<f32>;
@group(0) @binding(1) var s_image: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_image, s_image, in.uv);
}
