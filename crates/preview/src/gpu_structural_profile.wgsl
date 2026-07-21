struct ProfileHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    _pad: u32,
};

struct ProfileCommand {
    program: u32,
    lod: u32,
    supersampling: u32,
    occupancy_bits: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    edge_mask: u32,
    curve_offset: u32,
    curve_count: u32,
    sdf_kind: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    pixels_per_meter_x: f32,
    pixels_per_meter_y: f32,
    first_width_m: f32,
    second_width_m: f32,
    minimum_flat_center_m: f32,
    amplitude_m: f32,
    angle_radians: f32,
    inner_radius_m: f32,
    outer_radius_m: f32,
    _pad_float: f32,
};

struct ProfileSample {
    sdf: f32,
    height: f32,
    derivative: vec2<f32>,
    semantics: f32,
};

@group(0) @binding(0) var<uniform> header: ProfileHeader;
@group(0) @binding(1) var<storage, read> commands: array<ProfileCommand>;
@group(0) @binding(2) var height_out: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var sdf_out: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var semantics_out: texture_storage_2d<r32float, write>;
@group(0) @binding(5) var derivative_x_out: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var derivative_y_out: texture_storage_2d<r32float, write>;
@group(0) @binding(7) var<storage, read> curve_points: array<vec2<f32>>;

fn smooth_ramp_with_derivative(value: f32) -> vec2<f32> {
    let x = clamp(value, 0.0, 1.0);
    let in_ramp = select(0.0, 1.0, value > 0.0 && value < 1.0);
    return vec2<f32>(x * x * (3.0 - 2.0 * x), 6.0 * x * (1.0 - x) * in_ramp);
}

fn rectangle_distance(cmd: ProfileCommand, p: vec2<f32>) -> vec4<f32> {
    let half_size = vec2<f32>(cmd.slot_width_m, cmd.slot_height_m) * 0.5;
    let q = abs(p) - half_size;
    let outside = length(max(q, vec2<f32>(0.0)));
    let signed_distance = outside + min(max(q.x, q.y), 0.0);
    var distance = 1.0e20;
    var gradient = vec2<f32>(0.0);
    var selected_second_edge = 0.0;
    if ((cmd.edge_mask & 1u) != 0u && half_size.x + p.x < distance) {
        distance = half_size.x + p.x;
        gradient = vec2<f32>(1.0, 0.0);
        selected_second_edge = 0.0;
    }
    if ((cmd.edge_mask & 2u) != 0u && half_size.x - p.x < distance) {
        distance = half_size.x - p.x;
        gradient = vec2<f32>(-1.0, 0.0);
        selected_second_edge = 1.0;
    }
    if ((cmd.edge_mask & 4u) != 0u && half_size.y + p.y < distance) {
        distance = half_size.y + p.y;
        gradient = vec2<f32>(0.0, 1.0);
        selected_second_edge = 0.0;
    }
    if ((cmd.edge_mask & 8u) != 0u && half_size.y - p.y < distance) {
        distance = half_size.y - p.y;
        gradient = vec2<f32>(0.0, -1.0);
        selected_second_edge = 1.0;
    }
    if (cmd.edge_mask == 0u) {
        distance = -signed_distance;
        selected_second_edge = 0.0;
    }
    return vec4<f32>(distance, gradient, selected_second_edge);
}

fn radial_distance(cmd: ProfileCommand, p: vec2<f32>) -> vec4<f32> {
    let radius = length(p);
    let radial = select(vec2<f32>(0.0), p / radius, radius > 0.0000001);
    if (cmd.sdf_kind == 1u) {
        return vec4<f32>(cmd.outer_radius_m - radius, -radial, 0.0);
    }
    let outer = cmd.outer_radius_m - radius;
    let inner = radius - cmd.inner_radius_m;
    return select(vec4<f32>(outer, -radial, 0.0), vec4<f32>(inner, radial, 1.0), inner < outer);
}

fn curve_value(cmd: ProfileCommand, t: f32) -> vec2<f32> {
    if (cmd.curve_count < 2u) {
        return vec2<f32>(0.0);
    }
    let x = clamp(t, 0.0, 1.0);
    for (var i = 0u; i + 1u < cmd.curve_count; i = i + 1u) {
        let a = curve_points[cmd.curve_offset + i];
        let b = curve_points[cmd.curve_offset + i + 1u];
        if (x <= b.x) {
            let span = max(b.x - a.x, 0.0000001);
            let local = clamp((x - a.x) / span, 0.0, 1.0);
            return vec2<f32>(mix(a.y, b.y, local), (b.y - a.y) / span);
        }
    }
    return vec2<f32>(curve_points[cmd.curve_offset + cmd.curve_count - 1u].y, 0.0);
}

fn evaluate(cmd: ProfileCommand, atlas_position: vec2<f32>) -> ProfileSample {
    let q = vec2<f32>(
        (atlas_position.x - f32(cmd.dst_x)) / f32(max(cmd.dst_width, 1u)),
        (atlas_position.y - f32(cmd.dst_y)) / f32(max(cmd.dst_height, 1u)),
    );
    let p = (q - vec2<f32>(0.5)) * vec2<f32>(cmd.slot_width_m, cmd.slot_height_m);
    let distance_data = select(
        rectangle_distance(cmd, p),
        radial_distance(cmd, p),
        cmd.sdf_kind != 0u,
    );
    let distance = distance_data.x;
    let distance_gradient = distance_data.yz;
    let selected_width = select(cmd.first_width_m, cmd.second_width_m, distance_data.w > 0.5);
    let width = max(selected_width, 0.0000001);
    let lod_enabled = cmd.lod != 4u;
    // Physical allocation distance remains authoritative even when the optional
    // structural profile contribution is flat or disabled by its own LOD policy.
    let output_sdf = distance;
    let t = clamp(distance / width, 0.0, 1.0);
    let in_profile = select(0.0, 1.0, distance > 0.0 && distance < width);
    var height = 0.0;
    var dh_dd = 0.0;
    if (cmd.program == 1u) {
        height = cmd.amplitude_m * (2.0 * t - t * t);
        dh_dd = cmd.amplitude_m * (2.0 - 2.0 * t) / width * in_profile;
    } else if (cmd.program == 2u) {
        height = -cmd.amplitude_m * (1.0 - t);
        dh_dd = cmd.amplitude_m / width * in_profile;
    } else if (cmd.program == 3u) {
        height = cmd.amplitude_m * sin(t * 1.57079632679);
        dh_dd = cmd.amplitude_m * 1.57079632679 * cos(t * 1.57079632679) / width * in_profile;
    } else if (cmd.program == 4u) {
        height = cmd.amplitude_m * (1.0 - abs(2.0 * t - 1.0));
        dh_dd = cmd.amplitude_m * select(2.0, -2.0, t >= 0.5) / width * in_profile;
    } else if (cmd.program == 5u || cmd.program == 6u || cmd.program == 7u) {
        let outer = smooth_ramp_with_derivative(distance / width);
        let inner_distance = max(cmd.second_width_m - distance, 0.0);
        let inner = smooth_ramp_with_derivative(inner_distance / width);
        let sign = select(1.0, -1.0, cmd.program == 6u);
        height = sign * cmd.amplitude_m * outer.x * inner.x;
        dh_dd = sign * cmd.amplitude_m * (outer.y / width * inner.x - outer.x * inner.y / width);
    } else if (cmd.program == 8u || cmd.program == 9u) {
        let minor = min(cmd.slot_width_m, cmd.slot_height_m);
        let across = clamp(distance / max(minor * 0.5, 0.0000001), 0.0, 1.0);
        let rounded = sqrt(max(0.0, 1.0 - (1.0 - across) * (1.0 - across)));
        height = cmd.amplitude_m * rounded;
        dh_dd = cmd.amplitude_m * (1.0 - across) / max(rounded * minor * 0.5, 0.0000001);
    } else if (cmd.program == 10u || cmd.program == 11u) {
        let ramp = smooth_ramp_with_derivative(distance / width);
        height = cmd.amplitude_m * ramp.x;
        dh_dd = cmd.amplitude_m * ramp.y / width;
    } else if (cmd.program == 12u) {
        let curve = curve_value(cmd, t);
        height = cmd.amplitude_m * curve.x;
        dh_dd = cmd.amplitude_m * curve.y / width * in_profile;
    }
    let derivative = distance_gradient * dh_dd;
    // Allocation membership is authoritative semantic data, not a structural-height
    // LOD feature. Flat and LOD-disabled profiles must still publish it for ED-2.
    let inside = distance >= 0.0;
    let flat = lod_enabled && (cmd.occupancy_bits & 4u) != 0u && distance >= width;
    let raised = lod_enabled && (cmd.occupancy_bits & 8u) != 0u && height > 0.0;
    let recessed = lod_enabled && (cmd.occupancy_bits & 16u) != 0u && height < 0.0;
    let cap = lod_enabled && (cmd.occupancy_bits & 32u) != 0u;
    let groove = lod_enabled && (cmd.occupancy_bits & 64u) != 0u;
    let profile_exclusion = lod_enabled && (cmd.occupancy_bits & 128u) != 0u && distance > 0.0 && distance < width;
    var semantics = 0u;
    semantics = semantics | select(0u, 1u, inside);
    semantics = semantics | select(0u, 2u, flat);
    semantics = semantics | select(0u, 4u, raised);
    semantics = semantics | select(0u, 8u, recessed);
    semantics = semantics | select(0u, 16u, cap);
    semantics = semantics | select(0u, 32u, groove);
    semantics = semantics | select(0u, 64u, profile_exclusion);
    if (cmd.lod == 2u) {
        height = 0.0;
    } else if (cmd.lod == 3u || cmd.lod == 4u) {
        height = 0.0;
        return ProfileSample(output_sdf, height, vec2<f32>(0.0), f32(semantics));
    }
    return ProfileSample(output_sdf, height, derivative, f32(semantics));
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= header.tile_width || id.y >= header.tile_height) {
        return;
    }
    let atlas_pixel = vec2<u32>(id.xy) + vec2<u32>(header.tile_x, header.tile_y);
    var matched = false;
    var center_sample = ProfileSample(0.0, 0.0, vec2<f32>(0.0), 0.0);
    var averaged_height = 0.0;
    var averaged_derivative = vec2<f32>(0.0);
    for (var command_index = 0u; command_index < header.command_count; command_index = command_index + 1u) {
        let cmd = commands[command_index];
        if (atlas_pixel.x < cmd.dst_x || atlas_pixel.x >= cmd.dst_x + cmd.dst_width ||
            atlas_pixel.y < cmd.dst_y || atlas_pixel.y >= cmd.dst_y + cmd.dst_height) {
            continue;
        }
        matched = true;
        center_sample = evaluate(cmd, vec2<f32>(atlas_pixel) + vec2<f32>(0.5));
        let ss = max(cmd.supersampling, 1u);
        var sum_height = 0.0;
        var sum_derivative = vec2<f32>(0.0);
        for (var sy = 0u; sy < 8u; sy = sy + 1u) {
            if (sy >= ss) { break; }
            for (var sx = 0u; sx < 8u; sx = sx + 1u) {
                if (sx >= ss) { break; }
                let offset = (vec2<f32>(f32(sx), f32(sy)) + vec2<f32>(0.5)) / f32(ss);
                let sample = evaluate(cmd, vec2<f32>(atlas_pixel) + offset);
                sum_height = sum_height + sample.height;
                sum_derivative = sum_derivative + sample.derivative;
            }
        }
        let count = f32(ss * ss);
        averaged_height = sum_height / count;
        averaged_derivative = sum_derivative / count;
    }
    if (!matched) {
        return;
    }
    let local = vec2<i32>(id.xy);
    textureStore(height_out, local, vec4<f32>(averaged_height, 0.0, 0.0, 0.0));
    textureStore(sdf_out, local, vec4<f32>(center_sample.sdf, 0.0, 0.0, 0.0));
    textureStore(semantics_out, local, vec4<f32>(center_sample.semantics, 0.0, 0.0, 0.0));
    textureStore(derivative_x_out, local, vec4<f32>(averaged_derivative.x, 0.0, 0.0, 0.0));
    textureStore(derivative_y_out, local, vec4<f32>(averaged_derivative.y, 0.0, 0.0, 0.0));
}
