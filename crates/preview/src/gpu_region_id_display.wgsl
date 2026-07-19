@group(0) @binding(0) var id_tex: texture_2d<u32>;
@group(0) @binding(1) var<storage, read> display_colors: array<u32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba8unorm, write>;

fn unpack_rgba8(packed: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(packed & 0xffu),
        f32((packed >> 8u) & 0xffu),
        f32((packed >> 16u) & 0xffu),
        f32((packed >> 24u) & 0xffu),
    ) / 255.0;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(out_tex);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let compact = textureLoad(id_tex, vec2<i32>(i32(id.x), i32(id.y)), 0).r;
    if (compact == 0xffffffffu) {
        textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(0.0, 0.0, 0.0, 0.0));
        return;
    }
    textureStore(
        out_tex,
        vec2<i32>(i32(id.x), i32(id.y)),
        unpack_rgba8(display_colors[compact]),
    );
}
