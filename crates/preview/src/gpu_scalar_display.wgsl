struct DisplayHeader {
    width: u32,
    height: u32,
    map_kind: u32,
    signed_unit_delta: u32,
    source_height_range_m: f32,
    _pad_f0: f32,
    _pad_f1: f32,
    _pad_f2: f32,
};

@group(0) @binding(0) var<uniform> header: DisplayHeader;
@group(0) @binding(1) var physical_tex: texture_2d<f32>;
@group(0) @binding(2) var allocation_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.width || id.y >= header.height) { return; }
    let allocated = textureLoad(allocation_tex, vec2<i32>(id.xy), 0).r != 0.0;
    if (!allocated) {
        textureStore(out_tex, vec2<i32>(id.xy), vec4<f32>(0.0));
        return;
    }
    let value = textureLoad(physical_tex, vec2<i32>(id.xy), 0).r;
    var encoded = clamp(value, 0.0, 1.0);
    if (header.signed_unit_delta != 0u) {
        encoded = clamp(0.5 + value * 0.5, 0.0, 1.0);
    } else if (header.map_kind == 1u) {
        // Exact inverse of the normalized source Height -> signed meters contract.
        encoded = clamp(0.5 + value / header.source_height_range_m, 0.0, 1.0);
    }
    textureStore(out_tex, vec2<i32>(id.xy), vec4<f32>(encoded, encoded, encoded, 1.0));
}
