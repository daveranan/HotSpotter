@group(0) @binding(0) var out_tex: texture_storage_2d<r32float, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(-1.0, 0.0, 0.0, 0.0));
}
