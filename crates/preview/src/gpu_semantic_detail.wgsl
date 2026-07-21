struct DetailHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    requested_field: u32,
};

struct DetailCommand {
    family: u32,
    lod: u32,
    mapping_mode: u32,
    channel_bits: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    seed: u32,
    layer_order: i32,
    occupancy_relation: u32,
    blend: u32,
    material_id: u32,
    mirror_bits: u32,
    clipping: u32,
    asset_key: u32,
    asset_layer: u32,
    asset_scalar_layer: u32,
    asset_normal_layer: u32,
    asset_material_id_layer: u32,
    asset_mask_layer: u32,
    asset_width: u32,
    asset_height: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    pixels_per_meter_x: f32,
    pixels_per_meter_y: f32,
    size_x_m: f32,
    size_y_m: f32,
    position_x_m: f32,
    position_y_m: f32,
    pivot_x: f32,
    pivot_y: f32,
    period_x_m: f32,
    period_y_m: f32,
    scatter: f32,
    jitter_x_m: f32,
    jitter_y_m: f32,
    rotation_sin: f32,
    rotation_cos: f32,
    opacity: f32,
    height_amount: f32,
    normal_amount: f32,
    scalar_amount: f32,
    color_amount: f32,
};

struct DetailSample {
    mask: f32,
    height: f32,
    normal: vec4<f32>,
    scalar: f32,
    color: vec4<f32>,
    material_id: u32,
    material_id_valid: u32,
};

@group(0) @binding(0) var<uniform> header: DetailHeader;
@group(0) @binding(1) var<storage, read> commands: array<DetailCommand>;
@group(0) @binding(2) var mask_out: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var height_out: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var normal_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(5) var scalar_out: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var color_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(7) var material_id_out: texture_storage_2d<r32uint, write>;
@group(0) @binding(8) var material_id_valid_out: texture_storage_2d<r32uint, write>;
@group(0) @binding(9) var asset_color_tex: texture_2d_array<f32>;
@group(0) @binding(10) var asset_scalar_tex: texture_2d_array<f32>;
@group(0) @binding(11) var asset_normal_tex: texture_2d_array<f32>;
@group(0) @binding(12) var asset_material_id_tex: texture_2d_array<u32>;
@group(0) @binding(13) var asset_mask_tex: texture_2d_array<f32>;
@group(0) @binding(14) var occupancy_tex: texture_2d<f32>;

fn hash01(v: vec2<u32>, seed: u32) -> f32 {
    var x = v.x * 1664525u + v.y * 1013904223u + seed * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x & 0x00ffffffu) / 16777215.0;
}

fn motif(cmd: DetailCommand, local_m: vec2<f32>, atlas_pixel: vec2<u32>) -> f32 {
    let period = max(vec2<f32>(cmd.period_x_m, cmd.period_y_m), vec2<f32>(0.000001));
    let uv = fract((local_m + period * 0.5) / period);
    let centered = abs(uv - vec2<f32>(0.5));
    let asset_phase = hash01(vec2<u32>(cmd.asset_key, cmd.seed), cmd.asset_key);
    if (cmd.family == 0u) {
        return select(0.0, cmd.opacity, centered.y < 0.12);
    }
    if (cmd.family == 4u) {
        let cell = vec2<u32>(floor((local_m + period * 16.0) / period));
        let jitter = hash01(cell, cmd.seed);
        let radius = 0.18 + jitter * (0.08 + cmd.scatter * 0.04);
        return select(0.0, cmd.opacity, length(centered) < radius);
    }
    if (cmd.family == 5u) {
        return select(0.0, cmd.opacity, centered.x < 0.08 || centered.y < 0.16);
    }
    if (cmd.family == 7u) {
        return select(0.0, cmd.opacity, abs(local_m.y) < cmd.size_y_m * 0.08);
    }
    if (cmd.family == 8u || cmd.family == 10u) {
        let half_size = max(vec2<f32>(cmd.size_x_m, cmd.size_y_m) * 0.5, vec2<f32>(0.000001));
        let q = abs(local_m) / half_size;
        return select(0.0, cmd.opacity, max(q.x, q.y) <= 1.0);
    }
    if (cmd.family == 2u) {
        let angle = atan2(local_m.y, local_m.x);
        let spoke = abs(fract(angle / 0.78539816339) - 0.5);
        return select(0.0, cmd.opacity, spoke < 0.08);
    }
    if (cmd.family == 9u) {
        return hash01(atlas_pixel, cmd.seed ^ cmd.asset_key) * cmd.opacity;
    }
    let half_size = max(vec2<f32>(cmd.size_x_m, cmd.size_y_m) * 0.5, vec2<f32>(0.000001));
    let q = abs(local_m + vec2<f32>(asset_phase, 1.0 - asset_phase) * 0.0001) / half_size;
    return select(0.0, cmd.opacity, max(q.x, q.y) <= 1.0);
}

fn evaluate(cmd: DetailCommand, atlas_pixel: vec2<u32>) -> DetailSample {
    let q = vec2<f32>(
        (f32(atlas_pixel.x) + 0.5 - f32(cmd.dst_x)) / f32(max(cmd.dst_width, 1u)),
        (f32(atlas_pixel.y) + 0.5 - f32(cmd.dst_y)) / f32(max(cmd.dst_height, 1u)),
    );
    var local_m = (q - vec2<f32>(0.5)) * vec2<f32>(cmd.slot_width_m, cmd.slot_height_m);
    let jitter_cell = vec2<u32>(floor((local_m + vec2<f32>(cmd.slot_width_m, cmd.slot_height_m)) / max(vec2<f32>(cmd.period_x_m, cmd.period_y_m), vec2<f32>(0.000001))));
    let jitter = vec2<f32>(
        hash01(jitter_cell, cmd.seed) - 0.5,
        hash01(jitter_cell.yx, cmd.seed ^ cmd.asset_key) - 0.5,
    ) * vec2<f32>(cmd.jitter_x_m, cmd.jitter_y_m);
    local_m = local_m - jitter;
    local_m = local_m - vec2<f32>(cmd.position_x_m, cmd.position_y_m);
    local_m = local_m + (vec2<f32>(cmd.pivot_x, cmd.pivot_y) - vec2<f32>(0.5)) * vec2<f32>(cmd.size_x_m, cmd.size_y_m);
    if ((cmd.mirror_bits & 1u) != 0u) {
        local_m.x = -local_m.x;
    }
    if ((cmd.mirror_bits & 2u) != 0u) {
        local_m.y = -local_m.y;
    }
    if (cmd.mapping_mode == 1u) {
        let radius = length(local_m);
        let angle = atan2(local_m.y, local_m.x);
        local_m = vec2<f32>(radius, angle * max(cmd.size_y_m, 0.000001));
    } else {
        local_m = vec2<f32>(
            local_m.x * cmd.rotation_cos + local_m.y * cmd.rotation_sin,
            -local_m.x * cmd.rotation_sin + local_m.y * cmd.rotation_cos,
        );
    }
    var mask = motif(cmd, local_m, atlas_pixel);
    let half_size = max(vec2<f32>(cmd.size_x_m, cmd.size_y_m) * 0.5, vec2<f32>(0.000001));
    let in_bounds = max(abs(local_m.x) / half_size.x, abs(local_m.y) / half_size.y) <= 1.0;
    if (cmd.clipping == 3u && !in_bounds) {
        mask = 0.0;
    }
    if (cmd.asset_key != 0u && in_bounds) {
        let asset_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
        let asset_uv = clamp(local_m / max(vec2<f32>(cmd.size_x_m, cmd.size_y_m), vec2<f32>(0.000001)) + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(0.999999));
        let asset_xy = vec2<i32>(asset_uv * vec2<f32>(asset_dims));
        if ((cmd.channel_bits & 8u) != 0u && cmd.asset_layer != 0xffffffffu) {
            mask = mask * textureLoad(asset_color_tex, asset_xy, i32(cmd.asset_layer), 0).a;
        }
        if ((cmd.channel_bits & 32u) != 0u && cmd.asset_mask_layer != 0xffffffffu) {
            let mask_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
            let mask_xy = vec2<i32>(asset_uv * vec2<f32>(mask_dims));
            mask = mask * textureLoad(asset_mask_tex, mask_xy, i32(cmd.asset_mask_layer), 0).r;
        }
    }
    let local_tile = vec2<i32>(atlas_pixel - vec2<u32>(header.tile_x, header.tile_y));
    let occupancy = u32(textureLoad(occupancy_tex, local_tile, 0).r);
    let raised = (occupancy & 4u) != 0u;
    let flat_center = (occupancy & 2u) != 0u;
    if ((cmd.occupancy_relation == 2u && raised) || (cmd.occupancy_relation == 3u && !flat_center)) {
        mask = 0.0;
    }
    if (cmd.lod == 4u) {
        mask = 0.0;
    }
    var asset_scalar = 1.0;
    var asset_normal = vec4<f32>(0.5, 0.5, 1.0, 1.0);
    var asset_color = vec4<f32>(1.0);
    var asset_material_id = cmd.material_id;
    if (cmd.asset_key != 0u && in_bounds) {
        let asset_uv = clamp(local_m / max(vec2<f32>(cmd.size_x_m, cmd.size_y_m), vec2<f32>(0.000001)) + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(0.999999));
        if (((cmd.channel_bits & 4u) != 0u || (cmd.channel_bits & 1u) != 0u) && cmd.asset_scalar_layer != 0xffffffffu) {
            let scalar_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
            let scalar_xy = vec2<i32>(asset_uv * vec2<f32>(scalar_dims));
            asset_scalar = textureLoad(asset_scalar_tex, scalar_xy, i32(cmd.asset_scalar_layer), 0).r;
        }
        if ((cmd.channel_bits & 2u) != 0u && cmd.asset_normal_layer != 0xffffffffu) {
            let normal_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
            let normal_xy = vec2<i32>(asset_uv * vec2<f32>(normal_dims));
            asset_normal = textureLoad(asset_normal_tex, normal_xy, i32(cmd.asset_normal_layer), 0);
        }
        if ((cmd.channel_bits & 8u) != 0u && cmd.asset_layer != 0xffffffffu) {
            let color_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
            let asset_xy = vec2<i32>(asset_uv * vec2<f32>(color_dims));
            asset_color = textureLoad(asset_color_tex, asset_xy, i32(cmd.asset_layer), 0);
        }
        if ((cmd.channel_bits & 16u) != 0u && cmd.asset_material_id_layer != 0xffffffffu) {
            let id_dims = vec2<u32>(max(cmd.asset_width, 1u), max(cmd.asset_height, 1u));
            let id_xy = vec2<i32>(asset_uv * vec2<f32>(id_dims));
            asset_material_id = textureLoad(asset_material_id_tex, id_xy, i32(cmd.asset_material_id_layer), 0).r;
        }
    }
    let relief = mask;
    let height = select(relief * cmd.height_amount * asset_scalar, 0.0, cmd.lod == 2u || cmd.lod == 3u || cmd.lod == 4u);
    let sampled_normal = normalize(asset_normal.xyz * 2.0 - vec3<f32>(1.0));
    let weighted_normal = normalize(vec3<f32>(sampled_normal.xy * cmd.normal_amount, mix(1.0, sampled_normal.z, cmd.normal_amount)));
    let normal = select(vec4<f32>(weighted_normal * 0.5 + vec3<f32>(0.5), asset_normal.a), vec4<f32>(0.5, 0.5, 1.0, 0.0), cmd.lod == 3u || cmd.lod == 4u || (cmd.channel_bits & 2u) == 0u);
    let scalar = select(relief * cmd.scalar_amount * asset_scalar, 0.0, cmd.lod == 4u);
    let color = select(vec4<f32>(0.0), vec4<f32>(asset_color.rgb * cmd.color_amount, asset_color.a), (cmd.channel_bits & 8u) != 0u && mask > 0.0);
    let material_id_valid = select(0u, 1u, (cmd.channel_bits & 16u) != 0u && mask > 0.0);
    return DetailSample(mask, height, normal, scalar, color, asset_material_id, material_id_valid);
}

fn blend_scalar(previous: f32, value: f32, mask: f32, blend: u32) -> f32 {
    if (blend == 0u) {
        return select(previous, value, mask > 0.0);
    }
    if (blend == 2u) {
        return previous * select(1.0, value, mask > 0.0);
    }
    if (blend == 3u) {
        return max(previous, value);
    }
    return previous + value;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.tile_width || id.y >= header.tile_height) {
        return;
    }
    let atlas_pixel = vec2<u32>(id.xy) + vec2<u32>(header.tile_x, header.tile_y);
    var out = DetailSample(0.0, 0.0, vec4<f32>(0.5, 0.5, 1.0, 0.0), 0.0, vec4<f32>(0.0), 0u, 0u);
    for (var i = 0u; i < header.command_count; i = i + 1u) {
        let cmd = commands[i];
        if (atlas_pixel.x < cmd.dst_x || atlas_pixel.x >= cmd.dst_x + cmd.dst_width ||
            atlas_pixel.y < cmd.dst_y || atlas_pixel.y >= cmd.dst_y + cmd.dst_height) {
            continue;
        }
        let sample = evaluate(cmd, atlas_pixel);
        out.mask = max(out.mask, sample.mask);
        out.height = blend_scalar(out.height, sample.height, sample.mask, cmd.blend);
        out.normal = select(out.normal, sample.normal, sample.mask > 0.0);
        out.scalar = blend_scalar(out.scalar, sample.scalar, sample.mask, cmd.blend);
        out.color = select(out.color, sample.color, sample.mask > 0.0);
        out.material_id = select(out.material_id, sample.material_id, sample.material_id_valid != 0u);
        out.material_id_valid = max(out.material_id_valid, sample.material_id_valid);
    }
    let local = vec2<i32>(id.xy);
    let write_all = header.requested_field == 99u;
    if (write_all || header.requested_field == 0u) {
        textureStore(mask_out, local, vec4<f32>(out.mask, 0.0, 0.0, 0.0));
    }
    if (write_all || header.requested_field == 1u) {
        textureStore(height_out, local, vec4<f32>(out.height, 0.0, 0.0, 0.0));
    }
    if (write_all || header.requested_field == 2u) {
        textureStore(normal_out, local, out.normal);
    }
    if (write_all || header.requested_field == 3u) {
        textureStore(scalar_out, local, vec4<f32>(out.scalar, 0.0, 0.0, 0.0));
    }
    if (write_all || header.requested_field == 4u) {
        textureStore(color_out, local, out.color);
    }
    if (write_all || header.requested_field == 5u) {
        textureStore(material_id_out, local, vec4<u32>(out.material_id, 0u, 0u, 0u));
        textureStore(material_id_valid_out, local, vec4<u32>(out.material_id_valid, 0u, 0u, 0u));
    }
}
