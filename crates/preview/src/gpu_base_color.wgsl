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
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
    _pad0: u32,
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

struct SourcePosition {
    primary: vec2<f32>,
    seam: vec2<f32>,
    seam_blend: f32,
    valid: bool,
};

@group(0) @binding(0) var<uniform> header: AtlasHeader;
@group(0) @binding(1) var<storage, read> commands: array<RegionCommand>;
@group(0) @binding(2) var source_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba8unorm, write>;

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

fn load_linear(p: vec2<f32>) -> vec4<f32> {
    let sx = i32(clamp(floor(p.x), 0.0, f32(header.source_width - 1u)));
    let sy = i32(clamp(floor(p.y), 0.0, f32(header.source_height - 1u)));
    let c = textureLoad(source_tex, vec2<i32>(sx, sy), 0);
    return vec4<f32>(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b), c.a);
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
        let warped_radius = cmd.radial_inner_radius + pow(clamp((radius - cmd.radial_inner_radius) / span, 0.0, 1.0), cmd.radial_falloff) * span;
        let radial_scale = select(0.0, warped_radius / radius, radius > 0.000001);
        let radial_local = transform_local(vec2<f32>(delta.x * radial_scale * destination_size.x, delta.y * radial_scale * destination_size.y), cmd.rotation, cmd.mirror);
        p = crop_origin + vec2<f32>(cmd.radial_center_x * crop_size.x, cmd.radial_center_y * crop_size.y) + radial_local * scale;
        p = clamp(p, crop_origin + vec2<f32>(0.5, 0.5), crop_origin + crop_size - vec2<f32>(0.5, 0.5));
    } else if (cmd.mode == 5u) {
        let radial_local = transform_local(q - vec2<f32>(cmd.radial_center_x, cmd.radial_center_y), cmd.rotation, cmd.mirror);
        let radius = length(radial_local);
        valid = radius <= cmd.radial_outer_radius;
        let theta = atan2(radial_local.y, radial_local.x) / 6.28318530718;
        let wrapped_theta = theta - floor(theta);
        let span = max(cmd.radial_outer_radius - cmd.radial_inner_radius, 0.000001);
        let polar = vec2<f32>(wrapped_theta * crop_size.x, pow(clamp((radius - cmd.radial_inner_radius) / span, 0.0, 1.0), cmd.radial_falloff) * crop_size.y);
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
                p.x >= 0.0 && p.x < f32(header.source_width) &&
                p.y >= 0.0 && p.y < f32(header.source_height)) {
                let linear = sample_linear(p, cmd.sampling_filter != 0u);
                var blended = linear;
                if (position.seam_blend > 0.0) {
                    let seam_linear = sample_linear(position.seam, cmd.sampling_filter != 0u);
                    blended = mix(linear, seam_linear, position.seam_blend);
                }
                color = vec4<f32>(linear_to_srgb(blended.r), linear_to_srgb(blended.g), linear_to_srgb(blended.b), blended.a);
                matched = true;
            }
        }
    }
    if (!matched) {
        return;
    }
    textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), color);
}
