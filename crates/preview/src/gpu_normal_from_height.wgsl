struct AtlasHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    source_width: u32,
    source_height: u32,
    source_origin_x: u32,
    source_origin_y: u32,
    map_kind: u32,
    normal_convention: u32,
    source_role: u32,
};

struct RegionCommand {
    region_index: u32,
    mode: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    semantic_x: u32,
    semantic_y: u32,
    semantic_width: u32,
    semantic_height: u32,
    period_x: u32,
    period_y: u32,
    rotation: u32,
    mirror: u32,
    sampling_filter: u32,
    transform_mirror_x: u32,
    transform_mirror_y: u32,
    structural_profile: u32,
    slice_left: u32,
    slice_right: u32,
    slice_top: u32,
    slice_bottom: u32,
    slice_center: u32,
    slot_width: f32,
    slot_height: f32,
    pixels_per_unit: f32,
    sampling_scale: f32,
    radial_center_x: f32,
    radial_center_y: f32,
    radial_inner_radius: f32,
    radial_outer_radius: f32,
    radial_falloff: f32,
    radial_blend_width: f32,
    radial_seam_blend_width: f32,
    transform_scale_x: f32,
    transform_scale_y: f32,
    transform_offset_x: f32,
    transform_offset_y: f32,
    transform_rotation_sin: f32,
    transform_rotation_cos: f32,
};

@group(0) @binding(0) var<uniform> header: AtlasHeader;
@group(0) @binding(1) var<storage, read> commands: array<RegionCommand>;
@group(0) @binding(2) var final_height_tex: texture_2d<f32>;
@group(0) @binding(3) var authored_normal_tex: texture_2d<f32>;
@group(0) @binding(4) var out_tex: texture_storage_2d<rgba8unorm, write>;

fn encode_normal(v: f32) -> f32 {
    return clamp(v, -1.0, 1.0) * 0.5 + 0.5;
}

fn height_at(atlas_pixel: vec2<u32>) -> f32 {
    let atlas_x = clamp(atlas_pixel.x, header.tile_x, header.tile_x + header.tile_width - 1u);
    let atlas_y = clamp(atlas_pixel.y, header.tile_y, header.tile_y + header.tile_height - 1u);
    let local_x = atlas_x - header.tile_x;
    let local_y = atlas_y - header.tile_y;
    return textureLoad(final_height_tex, vec2<i32>(i32(local_x), i32(local_y)), 0).r;
}

fn valid_height(candidate: f32, fallback: f32) -> f32 {
    return select(fallback, candidate, candidate == candidate && candidate != -1.0);
}

// Reoriented Normal Mapping: both inputs are decoded tangent vectors. A flat
// imported normal is the identity, so generated bevel structure is preserved.
fn compose_rnm(base: vec3<f32>, detail: vec3<f32>) -> vec3<f32> {
    let t = base + vec3<f32>(0.0, 0.0, 1.0);
    let u = detail * vec3<f32>(-1.0, -1.0, 1.0);
    return t * dot(t, u) / max(t.z, 0.000001) - u;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.tile_width || id.y >= header.tile_height) {
        return;
    }
    let pixel = vec2<u32>(id.x + header.tile_x, id.y + header.tile_y);
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var matched = false;
    for (var i = 0u; i < header.command_count; i = i + 1u) {
        let cmd = commands[i];
        if (pixel.x >= cmd.dst_x && pixel.x < cmd.dst_x + cmd.dst_width &&
            pixel.y >= cmd.dst_y && pixel.y < cmd.dst_y + cmd.dst_height) {
            let center_h = height_at(pixel);
            if (center_h == center_h && center_h != -1.0) {
                let semantic_min = vec2<u32>(cmd.semantic_x, cmd.semantic_y);
                let semantic_max = semantic_min + vec2<u32>(cmd.semantic_width, cmd.semantic_height) - vec2<u32>(1u);
                var h: array<f32, 9>;
                var sample_index = 0u;
                for (var oy = -1; oy <= 1; oy = oy + 1) {
                    for (var ox = -1; ox <= 1; ox = ox + 1) {
                        let candidate = clamp(vec2<i32>(pixel) + vec2<i32>(ox, oy), vec2<i32>(semantic_min), vec2<i32>(semantic_max));
                        h[sample_index] = valid_height(height_at(vec2<u32>(candidate)), center_h);
                        sample_index = sample_index + 1u;
                    }
                }
                // Physical Scharr derivative, with independent texel pitch on each axis.
                let meters_per_pixel_x = max(cmd.slot_width / f32(max(cmd.semantic_width, 1u)), 0.000001);
                let meters_per_pixel_y = max(cmd.slot_height / f32(max(cmd.semantic_height, 1u)), 0.000001);
                let dHdx = ((3.0 * (h[2] - h[0])) + (10.0 * (h[5] - h[3])) + (3.0 * (h[8] - h[6]))) / (32.0 * meters_per_pixel_x);
                let dHdy = ((3.0 * (h[6] - h[0])) + (10.0 * (h[7] - h[1])) + (3.0 * (h[8] - h[2]))) / (32.0 * meters_per_pixel_y);
                let height_normal = vec3<f32>(-dHdx, -dHdy, 1.0);
                let authored_sample = textureLoad(
                    authored_normal_tex,
                    vec2<i32>(i32(id.x), i32(id.y)),
                    0,
                );
                let authored_decoded = authored_sample.xyz * 2.0 - vec3<f32>(1.0);
                var n = height_normal;
                if (header.source_role == 2u && authored_sample.a > 0.0 && dot(authored_decoded, authored_decoded) > 0.0001) {
                    // Vector-correct RNM; encoded normal RGB is never scalar-blended.
                    n = compose_rnm(authored_decoded, height_normal);
                }
                if (header.normal_convention == 1u) {
                    n.y = -n.y;
                }
                n = normalize(n);
                color = vec4<f32>(encode_normal(n.x), encode_normal(n.y), encode_normal(n.z), 1.0);
                matched = true;
            }
        }
    }
    if (!matched) {
        return;
    }
    textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), color);
}
