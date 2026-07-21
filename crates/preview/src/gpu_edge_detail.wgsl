// EdgeDetailMvpV1. All positions used by the noise evaluator are stable atlas/
// slot physical positions; no tile-local coordinate enters the noise functions.
struct EdgeHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    halo_px: u32,
};

struct EdgeCommand {
    evaluator: u32,
    source_route: u32,
    seed: u32,
    edge_mask: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    semantic_x: u32,
    semantic_y: u32,
    semantic_width: u32,
    semantic_height: u32,
    declared_halo_px: u32,
    cap_major_axis: u32,
    source_stencil_halo_px: u32,
    exposed_metal_enabled: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    meters_per_pixel_x: f32,
    meters_per_pixel_y: f32,
    wear_amount: f32,
    intensity: f32,
    edge_width_m: f32,
    bevel_radius_m: f32,
    edge_softness: f32,
    breakup_amount: f32,
    breakup_scale_m: f32,
    micro_detail_amount: f32,
    micro_detail_scale_m: f32,
    source_height_influence: f32,
    source_luminance_influence: f32,
    height_amplitude_m: f32,
    normal_detail_strength: f32,
    source_height_range_m: f32,
    requested_extent_m: f32,
    hue_shift_degrees: f32,
    saturation_multiplier: f32,
    value_multiplier: f32,
    roughness_offset: f32,
    metallic_offset: f32,
};

@group(0) @binding(0) var<uniform> header: EdgeHeader;
@group(0) @binding(1) var<storage, read> commands: array<EdgeCommand>;
@group(0) @binding(2) var stage15_sdf: texture_2d<f32>;
@group(0) @binding(3) var stage15_semantics: texture_2d<f32>;
@group(0) @binding(4) var source_height: texture_2d<f32>;
@group(0) @binding(5) var source_color: texture_2d<f32>;
@group(0) @binding(6) var core_out: texture_storage_2d<r32float, write>;
@group(0) @binding(7) var transition_out: texture_storage_2d<r32float, write>;
@group(0) @binding(8) var fade_out: texture_storage_2d<r32float, write>;
@group(0) @binding(9) var combined_out: texture_storage_2d<r32float, write>;
@group(0) @binding(10) var height_out: texture_storage_2d<r32float, write>;

fn hash_corner(cell: vec2<i32>, seed: u32) -> f32 {
    var h = u32(cell.x) * 0x9e3779b9u ^ u32(cell.y) * 0x85ebca6bu ^ seed;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h & 0x00ffffffu) / 16777215.0;
}

fn value_noise(p: vec2<f32>, seed: u32) -> f32 {
    let cell = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - vec2<f32>(15.0)) + vec2<f32>(10.0));
    let a = hash_corner(cell, seed);
    let b = hash_corner(cell + vec2<i32>(1, 0), seed);
    let c = hash_corner(cell + vec2<i32>(0, 1), seed);
    let d = hash_corner(cell + vec2<i32>(1, 1), seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn role_coordinates(cmd: EdgeCommand, physical: vec2<f32>) -> vec2<f32> {
    // 0 panel, 1 horizontal, 2 vertical, 3 radial outer, 4 radial inner/outer,
    // 5 cap, 6 unique. Strip major axes receive longer correlation.
    if (cmd.evaluator == 1u || cmd.evaluator == 5u) {
        return vec2<f32>(physical.x * 0.35, physical.y * 2.0);
    }
    if (cmd.evaluator == 2u) {
        return vec2<f32>(physical.y * 0.35, physical.x * 2.0);
    }
    if (cmd.evaluator == 3u || cmd.evaluator == 4u) {
        let centered = physical - vec2<f32>(cmd.slot_width_m, cmd.slot_height_m) * 0.5;
        let radius = length(centered);
        let angle = atan2(centered.y, centered.x);
        // Periodic angular embedding makes the +/-pi seam identical while
        // retaining radial/arc-length correlation.
        return vec2<f32>(radius + cos(angle) * radius, sin(angle) * radius);
    }
    return physical;
}

fn fbm(p: vec2<f32>, seed: u32) -> f32 {
    return value_noise(p, seed) * 0.57
        + value_noise(p * 2.031 + vec2<f32>(17.1, 3.7), seed ^ 0xa511e9b3u) * 0.29
        + value_noise(p * 4.073 + vec2<f32>(5.9, 23.3), seed ^ 0x63d83595u) * 0.14;
}

fn linear_luminance(rgb: vec3<f32>) -> f32 {
    return dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn srgb_to_linear(value: f32) -> f32 {
    return select(value / 12.92, pow((value + 0.055) / 1.055, 2.4), value > 0.04045);
}

fn source_linear_luminance(local: vec2<i32>) -> f32 {
    let encoded = textureLoad(source_color, local, 0).rgb;
    return linear_luminance(vec3<f32>(
        srgb_to_linear(encoded.r),
        srgb_to_linear(encoded.g),
        srgb_to_linear(encoded.b),
    ));
}

// When no authored Height exists, the resampled Base Color becomes an explicit
// physical surface-height layer. Centering around zero keeps the absolute
// offset neutral while preserving every luminance gradient for final Normal.
fn source_luminance_height(local: vec2<i32>, cmd: EdgeCommand) -> f32 {
    if (cmd.source_route != 2u) { return 0.0; }
    return (source_linear_luminance(local) - 0.5)
        * cmd.source_height_range_m
        * cmd.source_luminance_influence;
}

fn source_high_pass(local: vec2<i32>, cmd: EdgeCommand) -> f32 {
    if (cmd.source_route == 0u) { return 0.0; }
    let tile_origin = vec2<u32>(header.tile_x, header.tile_y);
    let semantic_min_atlas = max(vec2<u32>(cmd.semantic_x, cmd.semantic_y), tile_origin);
    let semantic_max_atlas = min(
        vec2<u32>(cmd.semantic_x + cmd.semantic_width, cmd.semantic_y + cmd.semantic_height),
        tile_origin + vec2<u32>(header.tile_width, header.tile_height),
    );
    let stencil_min = vec2<i32>(semantic_min_atlas - tile_origin);
    let stencil_max = vec2<i32>(semantic_max_atlas - tile_origin) - vec2<i32>(1);
    var center = 0.0;
    var average = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let q = clamp(local + vec2<i32>(x, y), stencil_min, stencil_max);
            let v = select(
                source_linear_luminance(q),
                textureLoad(source_height, q, 0).x,
                cmd.source_route == 1u,
            );
            average = average + v;
            if (x == 0 && y == 0) { center = v; }
        }
    }
    let high_pass = clamp(center - average / 9.0, -1.0, 1.0);
    return select(high_pass, 0.0, abs(high_pass) < 0.000001);
}

fn inside_rect(pixel: vec2<u32>, origin: vec2<u32>, size: vec2<u32>) -> bool {
    return pixel.x >= origin.x && pixel.y >= origin.y
        && pixel.x < origin.x + size.x && pixel.y < origin.y + size.y;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.tile_width || id.y >= header.tile_height) { return; }
    let atlas_pixel = id.xy + vec2<u32>(header.tile_x, header.tile_y);
    let local = vec2<i32>(id.xy);
    for (var command_index = 0u; command_index < header.command_count; command_index = command_index + 1u) {
        let cmd = commands[command_index];
        if (!inside_rect(atlas_pixel, vec2<u32>(cmd.semantic_x, cmd.semantic_y),
                vec2<u32>(cmd.semantic_width, cmd.semantic_height))) { continue; }
        let semantic_bits = u32(round(textureLoad(stage15_semantics, local, 0).x));
        if ((semantic_bits & 1u) == 0u) { continue; }
        let surface_height = source_luminance_height(local, cmd);
        let q = (vec2<f32>(atlas_pixel - vec2<u32>(cmd.dst_x, cmd.dst_y)) + vec2<f32>(0.5))
            / vec2<f32>(f32(max(cmd.dst_width, 1u)), f32(max(cmd.dst_height, 1u)));
        let physical = q * vec2<f32>(cmd.slot_width_m, cmd.slot_height_m);
        let role_p = role_coordinates(cmd, physical);
        let low = fbm(role_p / max(cmd.breakup_scale_m * 2.5, 0.0000001), cmd.seed);
        let middle = fbm(role_p / max(cmd.breakup_scale_m, 0.0000001), cmd.seed ^ 0x68bc21ebu);
        let high = fbm(role_p / max(cmd.micro_detail_scale_m, 0.0000001), cmd.seed ^ 0x02e5be93u);
        let source_detail = source_high_pass(local, cmd);
        let source_influence = select(cmd.source_luminance_influence, cmd.source_height_influence, cmd.source_route == 1u);
        let source_bias = source_detail * source_influence;
        let warp = (low - 0.5) * cmd.breakup_amount * cmd.edge_width_m * 0.75
            + source_bias * cmd.edge_width_m * 0.15;
        let distance = max(textureLoad(stage15_sdf, local, 0).x, 0.0);
        let warped = max(0.0, distance + warp);
        if (warped > cmd.requested_extent_m) {
            textureStore(height_out, local, vec4<f32>(surface_height));
            continue;
        }
        let pixel_feather = max(cmd.meters_per_pixel_x, cmd.meters_per_pixel_y);
        let feather = max(pixel_feather, cmd.edge_width_m * (0.04 + cmd.edge_softness * 0.16));
        let coverage = smoothstep(1.0 - cmd.wear_amount - 0.12, 1.0 - cmd.wear_amount + 0.12, middle + source_bias * 0.2);
        let core_micro = mix(1.0, 0.72 + high * 0.28, cmd.micro_detail_amount);
        let transition_micro = mix(1.0, 0.82 + high * 0.18, cmd.micro_detail_amount);
        var core = (1.0 - smoothstep(cmd.edge_width_m * 0.18, cmd.edge_width_m * 0.18 + feather, warped)) * coverage * core_micro;
        var transition = (1.0 - smoothstep(cmd.edge_width_m * 0.62, cmd.edge_width_m * 0.62 + feather, warped)) * coverage * transition_micro;
        var fade = 1.0 - smoothstep(cmd.edge_width_m - feather, cmd.edge_width_m + feather, warped);
        if (cmd.evaluator == 5u) {
            let along = select(q.x, q.y, cmd.cap_major_axis == 1u);
            let taper = smoothstep(0.0, 0.2, 1.0 - along);
            core = core * taper;
            transition = transition * taper;
            fade = fade * taper;
        }
        core = clamp(core, 0.0, 1.0);
        transition = clamp(transition, 0.0, 1.0);
        fade = clamp(fade, 0.0, 1.0);
        let combined = clamp(max(core, max(transition * 0.72, fade * 0.30)) * cmd.intensity, 0.0, 1.0);
        let radius = max(cmd.bevel_radius_m, pixel_feather);
        let x = clamp(warped / radius, 0.0, 1.0);
        let rounded = sqrt(max(0.0, 1.0 - (1.0 - x) * (1.0 - x)));
        let edge_height = cmd.height_amplitude_m * (1.0 - rounded) * combined;
        textureStore(core_out, local, vec4<f32>(core));
        textureStore(transition_out, local, vec4<f32>(transition));
        textureStore(fade_out, local, vec4<f32>(fade));
        textureStore(combined_out, local, vec4<f32>(combined));
        textureStore(height_out, local, vec4<f32>(surface_height + edge_height));
    }
}
