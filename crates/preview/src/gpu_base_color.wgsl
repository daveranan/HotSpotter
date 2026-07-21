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
    source_page_width: u32,
    source_page_height: u32,
    source_page_interior_width: u32,
    source_page_interior_height: u32,
    source_page_count_x: u32,
    source_page_count_y: u32,
    source_page_halo: u32,
    source_page_mode: u32,
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
    edge_wear_flags: u32,
    edge_wear_seed: u32,
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
    edge_wear_coverage: f32,
    edge_wear_strength: f32,
    edge_wear_width_m: f32,
    edge_wear_breakup_scale_m: f32,
    edge_wear_height_m: f32,
    edge_wear_hue_degrees: f32,
    edge_wear_saturation: f32,
    edge_wear_value_offset: f32,
    edge_wear_roughness_offset: f32,
    edge_wear_metallic_offset: f32,
};

struct SourcePosition {
    primary: vec2<f32>,
    seam: vec2<f32>,
    seam_blend: f32,
    valid: bool,
};

struct SourcePageEntry {
    page_x: u32,
    page_y: u32,
    layer: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> header: AtlasHeader;
@group(0) @binding(1) var<storage, read> commands: array<RegionCommand>;
@group(0) @binding(2) var source_tex: texture_2d_array<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(4) var<storage, read> source_pages: array<SourcePageEntry>;
@group(0) @binding(5) var validity_tex: texture_2d_array<f32>;

fn transform_local(local: vec2<f32>, rotation: u32, mirror: u32) -> vec2<f32> {
    var p = local;
    if (mirror == 1u) {
        p.x = -p.x;
    } else if (mirror == 2u) {
        p.y = -p.y;
    }
    if (rotation == 1u) {
        return vec2<f32>(p.y, -p.x);
    }
    if (rotation == 2u) {
        return vec2<f32>(-p.x, -p.y);
    }
    if (rotation == 3u) {
        return vec2<f32>(-p.y, p.x);
    }
    return p;
}

fn srgb_to_linear(v: f32) -> f32 {
    if (v <= 0.04045) {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

fn linear_to_srgb(v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0);
    if (x <= 0.0031308) {
        return 12.92 * x;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

fn encode_normal(v: f32) -> f32 {
    return clamp(v, -1.0, 1.0) * 0.5 + 0.5;
}

fn hash_u32(value: u32) -> u32 {
    var x = value;
    x = (x ^ (x >> 16u)) * 0x7feb352du;
    x = (x ^ (x >> 15u)) * 0x846ca68bu;
    return x ^ (x >> 16u);
}

fn edge_noise_lattice(cell: vec2<u32>, seed: u32) -> f32 {
    let mixed = cell.x * 0x9e3779b9u ^ cell.y * 0x85ebca6bu ^ seed;
    return f32(hash_u32(mixed)) / 4294967295.0;
}

fn edge_value_noise(point: vec2<f32>, seed: u32) -> f32 {
    let base = vec2<u32>(floor(max(point, vec2<f32>(0.0, 0.0))));
    let fraction = fract(point);
    let fade = fraction * fraction * (vec2<f32>(3.0, 3.0) - 2.0 * fraction);
    let a = edge_noise_lattice(base, seed);
    let b = edge_noise_lattice(base + vec2<u32>(1u, 0u), seed);
    let c = edge_noise_lattice(base + vec2<u32>(0u, 1u), seed);
    let d = edge_noise_lattice(base + vec2<u32>(1u, 1u), seed);
    return mix(mix(a, b, fade.x), mix(c, d, fade.x), fade.y);
}

fn edge_fbm(point: vec2<f32>, seed: u32) -> f32 {
    let low = edge_value_noise(point, seed);
    let medium = edge_value_noise(point * 2.03 + vec2<f32>(17.1, 9.2), seed ^ 0x68bc21ebu);
    let micro = edge_value_noise(point * 4.11 + vec2<f32>(3.7, 29.4), seed ^ 0x02e5be93u);
    return low * 0.57 + medium * 0.29 + micro * 0.14;
}

fn edge_wear_mask(cmd: RegionCommand, pixel: vec2<u32>) -> f32 {
    if ((cmd.edge_wear_flags & 1u) == 0u) { return 0.0; }
    let q = vec2<f32>(
        (f32(clamp(pixel.x, cmd.semantic_x, cmd.semantic_x + cmd.semantic_width - 1u) - cmd.semantic_x) + 0.5) / f32(cmd.semantic_width),
        (f32(clamp(pixel.y, cmd.semantic_y, cmd.semantic_y + cmd.semantic_height - 1u) - cmd.semantic_y) + 0.5) / f32(cmd.semantic_height),
    );
    let physical = q * vec2<f32>(cmd.slot_width, cmd.slot_height);
    var distance_m = 3.402823e38;
    if ((cmd.edge_wear_flags & 2u) != 0u) { distance_m = min(distance_m, physical.x); }
    if ((cmd.edge_wear_flags & 4u) != 0u) { distance_m = min(distance_m, cmd.slot_width - physical.x); }
    if ((cmd.edge_wear_flags & 8u) != 0u) { distance_m = min(distance_m, physical.y); }
    if ((cmd.edge_wear_flags & 16u) != 0u) { distance_m = min(distance_m, cmd.slot_height - physical.y); }
    if (cmd.structural_profile == 5u || cmd.structural_profile == 6u) {
        let local = q - vec2<f32>(cmd.radial_center_x, cmd.radial_center_y);
        let radial_m = length(local * vec2<f32>(cmd.slot_width, cmd.slot_height));
        let minor = min(cmd.slot_width, cmd.slot_height);
        distance_m = abs(radial_m - cmd.radial_outer_radius * minor);
        if (cmd.structural_profile == 6u) {
            distance_m = min(distance_m, abs(radial_m - cmd.radial_inner_radius * minor));
        }
    }
    let width_m = max(cmd.edge_wear_width_m, 0.000001);
    let pixel_m = max(
        cmd.slot_width / f32(max(cmd.semantic_width, 1u)),
        cmd.slot_height / f32(max(cmd.semantic_height, 1u)),
    );
    let breakup_m = max(cmd.edge_wear_breakup_scale_m, pixel_m * 2.0);
    let noise_position = physical / breakup_m;

    // Large smooth noise moves the wear boundary; it never selects a constant
    // rectangular cell. Mid-frequency noise controls coverage and a third
    // octave adds small chips without changing the authored physical width.
    let boundary_noise = edge_fbm(noise_position * 0.55, cmd.edge_wear_seed);
    let coverage_noise = edge_fbm(noise_position * 1.35 + vec2<f32>(11.7, 5.3), cmd.edge_wear_seed ^ 0xa511e9b3u);
    let micro_noise = edge_value_noise(noise_position * 5.75 + vec2<f32>(2.1, 19.6), cmd.edge_wear_seed ^ 0x63d83595u);
    let warped_distance = max(
        0.0,
        distance_m + (boundary_noise - 0.5) * width_m * 1.15 + (micro_noise - 0.5) * width_m * 0.18,
    );
    let spatial_feather_m = max(pixel_m * 0.75, width_m * 0.06);
    let edge_fade = 1.0 - smoothstep(
        max(0.0, width_m - spatial_feather_m),
        width_m + spatial_feather_m,
        warped_distance,
    );
    let coverage = clamp(cmd.edge_wear_coverage, 0.0, 1.0);
    let coverage_mask = smoothstep(
        1.0 - coverage - 0.18,
        1.0 - coverage + 0.18,
        coverage_noise,
    );
    let chip_detail = mix(0.72, 1.0, smoothstep(0.2, 0.82, micro_noise));
    return clamp(edge_fade * coverage_mask * chip_detail * cmd.edge_wear_strength, 0.0, 1.0);
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
        hue = hue / 6.0;
        if (hue < 0.0) { hue = hue + 1.0; }
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

fn transform_tangent_normal(normal: vec3<f32>, rotation: u32, mirror: u32) -> vec3<f32> {
    var xy = normal.xy;
    if (rotation == 1u) {
        xy = vec2<f32>(-normal.y, normal.x);
    } else if (rotation == 2u) {
        xy = vec2<f32>(-normal.x, -normal.y);
    } else if (rotation == 3u) {
        xy = vec2<f32>(normal.y, -normal.x);
    }
    if (mirror == 1u) {
        xy.x = -xy.x;
    } else if (mirror == 2u) {
        xy.y = -xy.y;
    }
    return normalize(vec3<f32>(xy, normal.z));
}

fn load_linear(p: vec2<f32>) -> vec4<f32> {
    let source_min = vec2<f32>(f32(header.source_origin_x), f32(header.source_origin_y));
    let source_extent = vec2<f32>(f32(header.source_width), f32(header.source_height));
    let source_max = source_min + source_extent - vec2<f32>(1.0, 1.0);
    let global = vec2<u32>(
        u32(clamp(floor(p.x), source_min.x, source_max.x)),
        u32(clamp(floor(p.y), source_min.y, source_max.y)),
    );
    var texel = global - vec2<u32>(header.source_origin_x, header.source_origin_y);
    var layer = 0u;

    if (header.source_page_mode == 1u || header.source_page_mode == 2u) {
        let interior_size = vec2<u32>(
            max(header.source_page_interior_width, 1u),
            max(header.source_page_interior_height, 1u),
        );
        let page_count = vec2<u32>(
            max(header.source_page_count_x, 1u),
            max(header.source_page_count_y, 1u),
        );
        let page = min(texel / interior_size, page_count - vec2<u32>(1u, 1u));
        layer = page.y * page_count.x + page.x;
        if (header.source_page_mode == 2u) {
            layer = 0u;
            let page_table_len = arrayLength(&source_pages);
            for (var page_index = 0u; page_index < page_table_len; page_index = page_index + 1u) {
                let entry = source_pages[page_index];
                if (entry.page_x == page.x && entry.page_y == page.y) {
                    layer = entry.layer;
                    break;
                }
            }
        }

        let page_origin = vec2<u32>(header.source_origin_x, header.source_origin_y) + page * interior_size;
        let halo = vec2<u32>(header.source_page_halo, header.source_page_halo);
        let halo_origin = max(
            vec2<u32>(header.source_origin_x, header.source_origin_y),
            page_origin - min(page_origin, halo),
        );
        let page_size = vec2<u32>(
            max(header.source_page_width, 1u),
            max(header.source_page_height, 1u),
        );
        texel = min(global - halo_origin, page_size - vec2<u32>(1u, 1u));
    }

    let c = textureLoad(source_tex, vec2<i32>(i32(texel.x), i32(texel.y)), i32(layer), 0);
    if (header.source_role != 0u) {
        return c;
    }
    return vec4<f32>(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b), c.a);
}

fn load_validity(p: vec2<f32>) -> f32 {
    let source_min = vec2<f32>(f32(header.source_origin_x), f32(header.source_origin_y));
    let source_extent = vec2<f32>(f32(header.source_width), f32(header.source_height));
    let source_max = source_min + source_extent - vec2<f32>(1.0, 1.0);
    let global = vec2<u32>(
        u32(clamp(floor(p.x), source_min.x, source_max.x)),
        u32(clamp(floor(p.y), source_min.y, source_max.y)),
    );
    var texel = global - vec2<u32>(header.source_origin_x, header.source_origin_y);
    var layer = 0u;
    if (header.source_page_mode == 1u || header.source_page_mode == 2u) {
        let interior_size = vec2<u32>(
            max(header.source_page_interior_width, 1u),
            max(header.source_page_interior_height, 1u),
        );
        let page_count = vec2<u32>(
            max(header.source_page_count_x, 1u),
            max(header.source_page_count_y, 1u),
        );
        let page = min(texel / interior_size, page_count - vec2<u32>(1u, 1u));
        layer = page.y * page_count.x + page.x;
        if (header.source_page_mode == 2u) {
            layer = 0u;
            let page_table_len = arrayLength(&source_pages);
            for (var page_index = 0u; page_index < page_table_len; page_index = page_index + 1u) {
                let entry = source_pages[page_index];
                if (entry.page_x == page.x && entry.page_y == page.y) {
                    layer = entry.layer;
                    break;
                }
            }
        }
        let page_origin = vec2<u32>(header.source_origin_x, header.source_origin_y) + page * interior_size;
        let halo = vec2<u32>(header.source_page_halo, header.source_page_halo);
        let halo_origin = max(
            vec2<u32>(header.source_origin_x, header.source_origin_y),
            page_origin - min(page_origin, halo),
        );
        let page_size = vec2<u32>(
            max(header.source_page_width, 1u),
            max(header.source_page_height, 1u),
        );
        texel = min(global - halo_origin, page_size - vec2<u32>(1u, 1u));
    }
    return textureLoad(validity_tex, vec2<i32>(i32(texel.x), i32(texel.y)), i32(layer), 0).r;
}

fn sample_linear(p: vec2<f32>, linear_filter: bool) -> vec4<f32> {
    if (!linear_filter) {
        return load_linear(p);
    }
    let base = floor(p - vec2<f32>(0.5, 0.5));
    let f = fract(p - vec2<f32>(0.5, 0.5));
    let a = load_linear(base + vec2<f32>(0.5, 0.5));
    let b = load_linear(base + vec2<f32>(1.5, 0.5));
    let c = load_linear(base + vec2<f32>(0.5, 1.5));
    let d = load_linear(base + vec2<f32>(1.5, 1.5));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn material_height_sample(cmd: RegionCommand, pixel: vec2<u32>) -> f32 {
    if (header.source_role != 1u) {
        return 0.0;
    }
    let position = source_position(cmd, pixel);
    let p = position.primary;
    let source_origin = vec2<f32>(f32(header.source_origin_x), f32(header.source_origin_y));
    let source_limit = source_origin + vec2<f32>(f32(header.source_width), f32(header.source_height));
    if (!position.valid || p.x < source_origin.x || p.x >= source_limit.x || p.y < source_origin.y || p.y >= source_limit.y) {
        return 0.0;
    }
    let linear = sample_linear(p, cmd.sampling_filter != 0u);
    var blended = linear;
    if (position.seam_blend > 0.0) {
        let seam_linear = sample_linear(position.seam, cmd.sampling_filter != 0u);
        blended = mix(linear, seam_linear, position.seam_blend);
    }
    return clamp(blended.r, 0.0, 1.0) - 0.5;
}

fn final_height_at(cmd: RegionCommand, pixel: vec2<u32>) -> f32 {
    // Stage 15 structural Height is compiled and evaluated by the dedicated
    // physical profile pass. This material pass only samples authored Height.
    let physical_scale = max(min(cmd.slot_width, cmd.slot_height), 0.000001);
    let wear = edge_wear_mask(cmd, pixel) * cmd.edge_wear_height_m / physical_scale;
    return clamp(0.5 + material_height_sample(cmd, pixel) + wear, 0.0, 1.0);
}

fn slice_axis(value: f32, destination: f32, origin: f32, extent: f32, leading: u32, trailing: u32, scale: f32, center: u32) -> f32 {
    let leading_px = f32(leading);
    let trailing_px = f32(trailing);
    let leading_world = leading_px / scale;
    let trailing_world = trailing_px / scale;
    if (value < leading_world) {
        return origin + value * scale;
    }
    if (value >= destination - trailing_world) {
        return origin + extent - trailing_px + (value - (destination - trailing_world)) * scale;
    }
    let center_pixels = max(extent - leading_px - trailing_px, 1.0);
    let offset = (value - leading_world) * scale;
    if (center == 0u) {
        return origin + leading_px + ((offset % center_pixels) + center_pixels) % center_pixels;
    }
    if (center == 1u) {
        return origin + leading_px + offset;
    }
    let destination_center = max(destination - leading_world - trailing_world, 0.000001);
    return origin + leading_px + (value - leading_world) / destination_center * center_pixels;
}

fn source_position(cmd: RegionCommand, pixel: vec2<u32>) -> SourcePosition {
    let sem_x = clamp(pixel.x, cmd.semantic_x, cmd.semantic_x + cmd.semantic_width - 1u) - cmd.semantic_x;
    let sem_y = clamp(pixel.y, cmd.semantic_y, cmd.semantic_y + cmd.semantic_height - 1u) - cmd.semantic_y;
    let destination_q = vec2<f32>(
        (f32(sem_x) + 0.5) / f32(cmd.semantic_width),
        (f32(sem_y) + 0.5) / f32(cmd.semantic_height),
    );
    let crop_size = vec2<f32>(f32(cmd.crop_width), f32(cmd.crop_height));
    let q = destination_q;
    let crop_origin = vec2<f32>(f32(cmd.crop_x), f32(cmd.crop_y)) + vec2<f32>(
        cmd.transform_offset_x * crop_size.x,
        cmd.transform_offset_y * crop_size.y,
    );
    let destination_size = vec2<f32>(cmd.slot_width, cmd.slot_height);
    let local = (q - vec2<f32>(0.5, 0.5)) * destination_size;
    let source_local = transform_local(local, cmd.rotation, cmd.mirror);
    let source_size = select(destination_size, vec2<f32>(destination_size.y, destination_size.x), cmd.rotation == 1u || cmd.rotation == 3u);
    let m = source_local + source_size * 0.5;
    let scale = cmd.pixels_per_unit * cmd.sampling_scale;
    var p = crop_origin + crop_size * 0.5 + source_local * scale;
    var valid = true;

    if (cmd.mode == 0u) {
        p = crop_origin + crop_size * 0.5 + source_local * scale;
    } else if (cmd.mode == 1u) {
        p = crop_origin + vec2<f32>(
            ((p.x - crop_origin.x) % f32(max(cmd.period_x, 1u)) + f32(max(cmd.period_x, 1u))) % f32(max(cmd.period_x, 1u)),
            ((p.y - crop_origin.y) % f32(max(cmd.period_y, 1u)) + f32(max(cmd.period_y, 1u))) % f32(max(cmd.period_y, 1u)),
        );
    } else if (cmd.mode == 2u) {
        p.y = clamp(p.y, crop_origin.y, crop_origin.y + crop_size.y - 1.0);
        p.x = crop_origin.x + ((p.x - crop_origin.x) % f32(max(cmd.period_x, 1u)) + f32(max(cmd.period_x, 1u))) % f32(max(cmd.period_x, 1u));
    } else if (cmd.mode == 3u) {
        p.x = clamp(p.x, crop_origin.x, crop_origin.x + crop_size.x - 1.0);
        p.y = crop_origin.y + ((p.y - crop_origin.y) % f32(max(cmd.period_y, 1u)) + f32(max(cmd.period_y, 1u))) % f32(max(cmd.period_y, 1u));
    } else if (cmd.mode == 4u) {
        let delta = q - vec2<f32>(cmd.radial_center_x, cmd.radial_center_y);
        let radius = length(delta);
        let span = max(cmd.radial_outer_radius - cmd.radial_inner_radius, 0.000001);
        var warped_radius = cmd.radial_inner_radius + pow(clamp((radius - cmd.radial_inner_radius) / span, 0.0, 1.0), cmd.radial_falloff) * span;
        if (radius >= cmd.radial_outer_radius) {
            let inset = min(1.5, max(min(crop_size.x, crop_size.y) * 0.5, 0.5));
            let normalized_inset = inset / max(min(crop_size.x, crop_size.y), 1.0);
            warped_radius = max(cmd.radial_inner_radius, cmd.radial_outer_radius - span * normalized_inset);
        }
        let radial_scale = select(0.0, warped_radius / radius, radius > 0.000001);
        let radial_local = transform_local(vec2<f32>(delta.x * radial_scale * destination_size.x, delta.y * radial_scale * destination_size.y), cmd.rotation, cmd.mirror);
        p = crop_origin + vec2<f32>(cmd.radial_center_x * crop_size.x, cmd.radial_center_y * crop_size.y) + radial_local * scale;
        p = clamp(p, crop_origin + vec2<f32>(0.5, 0.5), crop_origin + crop_size - vec2<f32>(0.5, 0.5));
    } else if (cmd.mode == 5u) {
        let radial_local = transform_local(q - vec2<f32>(cmd.radial_center_x, cmd.radial_center_y), cmd.rotation, cmd.mirror);
        let radius = length(radial_local);
        // The rectangular atlas allocation owns every pixel. Outside the authored
        // circular coverage, extend the nearest radial boundary sample instead of
        // publishing transparent/black corners that bleed through filtering and mips.
        let span = max(cmd.radial_outer_radius - cmd.radial_inner_radius, 0.000001);
        if (radius < cmd.radial_inner_radius) {
            let planar = crop_origin + vec2<f32>(
                (cmd.radial_center_x + radial_local.x) * crop_size.x,
                (cmd.radial_center_y + radial_local.y) * crop_size.y,
            );
            return SourcePosition(planar, planar, 0.0, true);
        }
        let radial_inset = min(1.5, max(crop_size.y * 0.5, 0.5));
        let outer_extension_radius = max(
            cmd.radial_inner_radius,
            cmd.radial_outer_radius - span * radial_inset / max(crop_size.y, 1.0),
        );
        let sample_radius = select(
            clamp(radius, cmd.radial_inner_radius, cmd.radial_outer_radius),
            outer_extension_radius,
            radius >= cmd.radial_outer_radius,
        );
        valid = true;
        let theta = atan2(radial_local.y, radial_local.x) / 6.28318530718;
        let wrapped_theta = theta - floor(theta);
        let polar = vec2<f32>(
            min(wrapped_theta * crop_size.x, crop_size.x - 0.000001),
            min(
                pow(clamp((sample_radius - cmd.radial_inner_radius) / span, 0.0, 1.0), cmd.radial_falloff) * crop_size.y,
                crop_size.y - 0.000001,
            ),
        );
        let planar = vec2<f32>((cmd.radial_center_x + radial_local.x) * crop_size.x, (cmd.radial_center_y + radial_local.y) * crop_size.y);
        let transition = min(cmd.radial_blend_width, span);
        let t = select(1.0, clamp((radius - cmd.radial_inner_radius) / transition, 0.0, 1.0), transition > 0.000001);
        let blend = t * t * (3.0 - 2.0 * t);
        p = crop_origin + mix(planar, polar, blend);
        let seam_distance = min(wrapped_theta, 1.0 - wrapped_theta);
        if (cmd.radial_seam_blend_width > 0.000001 && seam_distance < cmd.radial_seam_blend_width) {
            let edge_t = clamp(seam_distance / cmd.radial_seam_blend_width, 0.0, 1.0);
            let feather = 0.5 * (1.0 - edge_t * edge_t * (3.0 - 2.0 * edge_t)) * blend;
            let other_polar_x = min((1.0 - wrapped_theta) * crop_size.x, crop_size.x - 0.000001);
            let seam = crop_origin + vec2<f32>(mix(planar.x, other_polar_x, blend), mix(planar.y, polar.y, blend));
            return SourcePosition(p, seam, feather, valid);
        }
    } else if (cmd.mode == 6u) {
        p = crop_origin + vec2<f32>(m.x / source_size.x * crop_size.x, m.y / source_size.y * crop_size.y);
    } else if (cmd.mode == 7u) {
        p = vec2<f32>(
            slice_axis(m.x, source_size.x, crop_origin.x, crop_size.x, cmd.slice_left, cmd.slice_right, scale, cmd.slice_center),
            crop_origin.y + (m.y - source_size.y * 0.5) * scale + crop_size.y * 0.5,
        );
    } else if (cmd.mode == 8u || cmd.mode == 9u) {
        let contain_scale = max(crop_size.x / source_size.x, crop_size.y / source_size.y);
        let cover_scale = min(crop_size.x / source_size.x, crop_size.y / source_size.y);
        let fit_scale = select(cover_scale, contain_scale, cmd.mode == 8u) * cmd.sampling_scale;
        let extent = crop_size / fit_scale;
        let origin = (source_size - extent) * 0.5;
        valid = cmd.mode == 9u || all(m >= origin) && all(m < origin + extent);
        p = crop_origin + (m - origin) * fit_scale;
    } else if (cmd.mode == 10u) {
        p = crop_origin + crop_size * 0.5 + source_local * scale;
    } else if (cmd.mode == 11u) {
        p = vec2<f32>(
            slice_axis(m.x, source_size.x, crop_origin.x, crop_size.x, cmd.slice_left, cmd.slice_right, scale, cmd.slice_center),
            slice_axis(m.y, source_size.y, crop_origin.y, crop_size.y, cmd.slice_top, cmd.slice_bottom, scale, cmd.slice_center),
        );
    }
    return SourcePosition(p, p, 0.0, valid);
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.tile_width || id.y >= header.tile_height) {
        return;
    }
    // The tile texture is local, but Prompt 002's sampling commands are defined
    // in atlas coordinates. Keep that distinction at the only GPU boundary.
    let pixel = vec2<u32>(id.x + header.tile_x, id.y + header.tile_y);
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var matched = false;
    for (var i = 0u; i < header.command_count; i = i + 1u) {
        let cmd = commands[i];
        if (pixel.x >= cmd.dst_x && pixel.x < cmd.dst_x + cmd.dst_width &&
            pixel.y >= cmd.dst_y && pixel.y < cmd.dst_y + cmd.dst_height) {
            let position = source_position(cmd, pixel);
            let p = position.primary;
            if (position.valid &&
                p.x >= f32(header.source_origin_x) && p.x < f32(header.source_origin_x + header.source_width) &&
                p.y >= f32(header.source_origin_y) && p.y < f32(header.source_origin_y + header.source_height) &&
                load_validity(p) >= 0.5) {
                let linear = sample_linear(p, cmd.sampling_filter != 0u);
                var blended = linear;
                if (position.seam_blend > 0.0 && load_validity(position.seam) >= 0.5) {
                    let seam_linear = sample_linear(position.seam, cmd.sampling_filter != 0u);
                    blended = mix(linear, seam_linear, position.seam_blend);
                }
                let final_height = final_height_at(cmd, pixel);
                let wear = edge_wear_mask(cmd, pixel);
                if (header.map_kind == 0u) {
                    var hsv = rgb_to_hsv(blended.rgb);
                    hsv.x = fract(hsv.x + cmd.edge_wear_hue_degrees / 360.0);
                    hsv.y = clamp(hsv.y * cmd.edge_wear_saturation, 0.0, 1.0);
                    hsv.z = clamp(hsv.z + cmd.edge_wear_value_offset, 0.0, 1.0);
                    let worn = hsv_to_rgb(hsv);
                    let result = mix(blended.rgb, worn, wear);
                    color = vec4<f32>(linear_to_srgb(result.r), linear_to_srgb(result.g), linear_to_srgb(result.b), blended.a);
                } else if (header.map_kind == 1u) {
                    color = vec4<f32>(final_height, final_height, final_height, 1.0);
                } else if (header.map_kind == 2u) {
                    if (header.source_role == 2u) {
                        let decoded = blended.xyz * 2.0 - vec3<f32>(1.0);
                        let authored = transform_tangent_normal(decoded, cmd.rotation, cmd.mirror);
                        color = vec4<f32>(
                            encode_normal(authored.x),
                            encode_normal(authored.y),
                            encode_normal(authored.z),
                            blended.a,
                        );
                    } else {
                        color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
                    }
                } else if (header.map_kind == 3u) {
                    let base = select(170.0 / 255.0, clamp(blended.r, 0.0, 1.0), header.source_role == 3u);
                    let value = clamp(base + max(0.0, 0.5 - final_height) * (70.0 / 255.0) + wear * cmd.edge_wear_roughness_offset, 0.0, 1.0);
                    color = vec4<f32>(value, value, value, 1.0);
                } else if (header.map_kind == 4u) {
                    let base = select(1.0, clamp(blended.r, 0.0, 1.0), header.source_role == 4u);
                    let value = clamp(base - max(0.0, 0.5 - final_height) * (130.0 / 255.0), 0.0, 1.0);
                    color = vec4<f32>(value, value, value, 1.0);
                } else if (header.map_kind == 5u) {
                    let base = select(0.0, clamp(blended.r, 0.0, 1.0), header.source_role == 5u);
                    let explicit_metal = (cmd.edge_wear_flags & 32u) != 0u;
                    let value = clamp(base + select(0.0, wear * cmd.edge_wear_metallic_offset, explicit_metal), 0.0, 1.0);
                    color = vec4<f32>(value, value, value, 1.0);
                } else if (header.map_kind == 8u) {
                    color = vec4<f32>(wear, wear, wear, 1.0);
                }
                matched = true;
            }
        }
    }
    if (!matched) {
        return;
    }
    textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), color);
}
