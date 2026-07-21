struct CompositionHeader {
    width: u32,
    height: u32,
    map_kind: u32,
    command_count: u32,
    tile_x: u32,
    tile_y: u32,
    base_height_is_physical: u32,
    _pad_u0: u32,
};

struct EdgeDetailCommand {
    evaluator: u32, source_route: u32, seed: u32, edge_mask: u32,
    dst_x: u32, dst_y: u32, dst_width: u32, dst_height: u32,
    semantic_x: u32, semantic_y: u32, semantic_width: u32, semantic_height: u32,
    declared_halo_px: u32, cap_major_axis: u32, source_stencil_halo_px: u32, exposed_metal_enabled: u32,
    slot_width_m: f32, slot_height_m: f32, meters_per_pixel_x: f32, meters_per_pixel_y: f32,
    wear_amount: f32, intensity: f32, edge_width_m: f32, bevel_radius_m: f32,
    edge_softness: f32, breakup_amount: f32, breakup_scale_m: f32, micro_detail_amount: f32,
    micro_detail_scale_m: f32, source_height_influence: f32, source_luminance_influence: f32,
    height_amplitude_m: f32, normal_detail_strength: f32, source_height_range_m: f32,
    requested_extent_m: f32, hue_shift_degrees: f32,
    saturation_multiplier: f32, value_multiplier: f32, roughness_offset: f32, metallic_offset: f32,
};

@group(0) @binding(0) var<uniform> header: CompositionHeader;
@group(0) @binding(1) var<storage, read> commands: array<EdgeDetailCommand>;
@group(0) @binding(2) var base_tex: texture_2d<f32>;
@group(0) @binding(3) var stage15_height_tex: texture_2d<f32>;
@group(0) @binding(4) var stage16_height_tex: texture_2d<f32>;
@group(0) @binding(5) var core_tex: texture_2d<f32>;
@group(0) @binding(6) var transition_tex: texture_2d<f32>;
@group(0) @binding(7) var fade_tex: texture_2d<f32>;
@group(0) @binding(8) var combined_tex: texture_2d<f32>;
@group(0) @binding(9) var edge_height_tex: texture_2d<f32>;
@group(0) @binding(10) var out_tex: texture_storage_2d<rgba8unorm, write>;

fn srgb_to_linear(v: f32) -> f32 {
    return select(v / 12.92, pow((v + 0.055) / 1.055, 2.4), v > 0.04045);
}

fn linear_to_srgb(v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0);
    return select(12.92 * x, 1.055 * pow(x, 1.0 / 2.4) - 0.055, x > 0.0031308);
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let maximum = max(c.r, max(c.g, c.b));
    let minimum = min(c.r, min(c.g, c.b));
    let delta = maximum - minimum;
    var hue = 0.0;
    if (delta > 0.000001) {
        if (maximum == c.r) { hue = ((c.g - c.b) / delta) % 6.0; }
        else if (maximum == c.g) { hue = (c.b - c.r) / delta + 2.0; }
        else { hue = (c.r - c.g) / delta + 4.0; }
        hue = fract(hue / 6.0 + 1.0);
    }
    return vec3<f32>(hue, select(0.0, delta / maximum, maximum > 0.000001), maximum);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let h = fract(c.x) * 6.0;
    let chroma = c.z * c.y;
    let x = chroma * (1.0 - abs((h % 2.0) - 1.0));
    var rgb = vec3<f32>(0.0);
    if (h < 1.0) { rgb = vec3<f32>(chroma, x, 0.0); }
    else if (h < 2.0) { rgb = vec3<f32>(x, chroma, 0.0); }
    else if (h < 3.0) { rgb = vec3<f32>(0.0, chroma, x); }
    else if (h < 4.0) { rgb = vec3<f32>(0.0, x, chroma); }
    else if (h < 5.0) { rgb = vec3<f32>(x, 0.0, chroma); }
    else { rgb = vec3<f32>(chroma, 0.0, x); }
    return rgb + vec3<f32>(c.z - chroma);
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.width || id.y >= header.height) { return; }
    let p = vec2<i32>(id.xy);
    let atlas_pixel = id.xy + vec2<u32>(header.tile_x, header.tile_y);
    let base = textureLoad(base_tex, p, 0);
    var result = base;
    for (var i = 0u; i < header.command_count; i = i + 1u) {
        let cmd = commands[i];
        if (atlas_pixel.x >= cmd.dst_x && atlas_pixel.x < cmd.dst_x + cmd.dst_width && atlas_pixel.y >= cmd.dst_y && atlas_pixel.y < cmd.dst_y + cmd.dst_height) {
            let core = clamp(textureLoad(core_tex, p, 0).r * cmd.intensity, 0.0, 1.0);
            let transition = clamp(textureLoad(transition_tex, p, 0).r * cmd.intensity, 0.0, 1.0);
            let fade = clamp(textureLoad(fade_tex, p, 0).r * cmd.intensity, 0.0, 1.0);
            let mask = clamp(textureLoad(combined_tex, p, 0).r, 0.0, 1.0);
            if (header.map_kind == 0u) {
                // Decode once, respond to the three authored zones independently,
                // then encode exactly once at publication.
                let linear = vec3<f32>(srgb_to_linear(base.r), srgb_to_linear(base.g), srgb_to_linear(base.b));
                let hsv = rgb_to_hsv(linear);
                let core_rgb = hsv_to_rgb(vec3<f32>(fract(hsv.x + cmd.hue_shift_degrees / 360.0), hsv.y, hsv.z));
                let transition_rgb = hsv_to_rgb(vec3<f32>(hsv.x, clamp(hsv.y * cmd.saturation_multiplier, 0.0, 1.0), hsv.z));
                let fade_rgb = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y, clamp(hsv.z * cmd.value_multiplier, 0.0, 1.0)));
                var composed = mix(linear, fade_rgb, fade);
                composed = mix(composed, transition_rgb, transition);
                composed = mix(composed, core_rgb, core);
                result = vec4<f32>(linear_to_srgb(composed.r), linear_to_srgb(composed.g), linear_to_srgb(composed.b), base.a);
            } else if (header.map_kind == 1u || header.map_kind == 2u) {
                // This is the authoritative signed physical intermediate. Range
                // conversion belongs only to the separate display publication.
                let authored_height_m = select(
                    0.0,
                    (base.r - 0.5) * cmd.source_height_range_m,
                    base.r != -1.0,
                );
                let edge_height_scale = select(1.0, cmd.normal_detail_strength, header.map_kind == 2u);
                var physical_height_m = authored_height_m + textureLoad(stage15_height_tex, p, 0).r
                    + textureLoad(stage16_height_tex, p, 0).r
                    + textureLoad(edge_height_tex, p, 0).r * edge_height_scale;
                if (header.base_height_is_physical != 0u) {
                    // Cached Height already contains the full Edge Detail Height.
                    // Reweight only that contribution for Normal regeneration.
                    physical_height_m = base.r
                        + textureLoad(edge_height_tex, p, 0).r * (cmd.normal_detail_strength - 1.0);
                }
                result = vec4<f32>(physical_height_m);
            } else if (header.map_kind == 3u) {
                let roughness = clamp(base.r + mask * cmd.roughness_offset, 0.0, 1.0);
                result = vec4<f32>(roughness);
            } else if (header.map_kind == 5u && cmd.exposed_metal_enabled != 0u) {
                let metallic = clamp(base.r + mask * cmd.metallic_offset, 0.0, 1.0);
                result = vec4<f32>(metallic);
            }
        }
    }
    textureStore(out_tex, p, result);
}
