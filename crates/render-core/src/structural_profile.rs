use hot_trimmer_domain::{PixelBounds, PixelSize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileKind {
    Flat,
    ConvexBevel45,
    ConcaveGroove45,
    RoundedBevel,
    PanelFrame,
    RadialDisc,
    Annulus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalConvention {
    OpenGl,
    DirectX,
}

/// Resolution-independent parameters. Lengths are fractions of the hotspot's shorter edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralProfile {
    pub kind: ProfileKind,
    pub amplitude: f64,
    pub edge_width: f64,
    pub frame_width: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
}

impl StructuralProfile {
    #[must_use]
    pub const fn for_kind(kind: ProfileKind) -> Self {
        match kind {
            ProfileKind::Flat => Self {
                kind, amplitude: 0.0, edge_width: 0.0, frame_width: 0.0,
                inner_radius: 0.0, outer_radius: 0.0,
            },
            ProfileKind::ConvexBevel45 | ProfileKind::ConcaveGroove45 => Self {
                kind, amplitude: 0.125, edge_width: 0.125, frame_width: 0.0,
                inner_radius: 0.0, outer_radius: 0.0,
            },
            ProfileKind::RoundedBevel => Self {
                kind, amplitude: 0.125, edge_width: 0.125, frame_width: 0.0,
                inner_radius: 0.0, outer_radius: 0.0,
            },
            ProfileKind::PanelFrame => Self {
                kind, amplitude: 0.04, edge_width: 0.04, frame_width: 0.20,
                inner_radius: 0.0, outer_radius: 0.0,
            },
            ProfileKind::RadialDisc => Self {
                kind, amplitude: 0.06, edge_width: 0.06, frame_width: 0.0,
                inner_radius: 0.0, outer_radius: 0.42,
            },
            ProfileKind::Annulus => Self {
                kind, amplitude: 0.04, edge_width: 0.04, frame_width: 0.0,
                inner_radius: 0.24, outer_radius: 0.44,
            },
        }
    }
}

impl Default for StructuralProfile {
    fn default() -> Self {
        Self::for_kind(ProfileKind::Flat)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralProfileRequest {
    pub profile: StructuralProfile,
    pub hotspot: PixelBounds,
    pub sheet_size: PixelSize,
    pub normal_convention: NormalConvention,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructuralProfileMaps {
    pub hotspot: PixelBounds,
    pub height_f32: Vec<f32>,
    pub normal_rgba8: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StructuralProfileError {
    #[error("sheet and hotspot dimensions must be nonzero")]
    InvalidDimensions,
    #[error("hotspot is outside the material sheet")]
    HotspotOutsideSheet,
    #[error("profile map exceeds the bounded allocation limit")]
    OutputTooLarge,
    #[error("profile parameters are invalid for the selected shape")]
    InvalidProfileParameters,
    #[error("a normal-map sample would escape the hotspot boundary")]
    NormalSampleOutsideHotspot,
}

/// Provides a stable default assignment for semantically named template slots.
#[must_use]
pub fn profile_kind_for_slot_key(slot_key: &str) -> ProfileKind {
    let key = slot_key.to_ascii_lowercase();
    if key.contains("annulus") || key.contains("ring") {
        ProfileKind::Annulus
    } else if key.contains("panel") || key.contains("frame") {
        ProfileKind::PanelFrame
    } else if key.contains("disc") || key.contains("radial") || key.contains("round_fixture") {
        ProfileKind::RadialDisc
    } else if key.contains("groove") || key.contains("seam") || key.contains("recess") {
        ProfileKind::ConcaveGroove45
    } else if key.contains("rounded") {
        ProfileKind::RoundedBevel
    } else if key.contains("bevel") {
        ProfileKind::ConvexBevel45
    } else {
        ProfileKind::Flat
    }
}

/// Verifies sheet containment and the clamped, one-sided normal sample footprint.
///
/// # Errors
/// Returns a typed error for empty/out-of-sheet bounds or a sample leaving the hotspot.
pub fn validate_hotspot_normal_sampling(
    hotspot: PixelBounds,
    sheet_size: PixelSize,
) -> Result<(), StructuralProfileError> {
    validate_hotspot(hotspot, sheet_size)?;
    let right = hotspot
        .x
        .checked_add(hotspot.width)
        .ok_or(StructuralProfileError::HotspotOutsideSheet)?;
    let bottom = hotspot
        .y
        .checked_add(hotspot.height)
        .ok_or(StructuralProfileError::HotspotOutsideSheet)?;

    for local_x in [0, hotspot.width - 1] {
        let (before, after) = normal_sample_axis(local_x, hotspot.width);
        for sample in [before, after] {
            let global = hotspot
                .x
                .checked_add(sample)
                .ok_or(StructuralProfileError::NormalSampleOutsideHotspot)?;
            if global < hotspot.x || global >= right {
                return Err(StructuralProfileError::NormalSampleOutsideHotspot);
            }
        }
    }
    for local_y in [0, hotspot.height - 1] {
        let (before, after) = normal_sample_axis(local_y, hotspot.height);
        for sample in [before, after] {
            let global = hotspot
                .y
                .checked_add(sample)
                .ok_or(StructuralProfileError::NormalSampleOutsideHotspot)?;
            if global < hotspot.y || global >= bottom {
                return Err(StructuralProfileError::NormalSampleOutsideHotspot);
            }
        }
    }
    Ok(())
}

/// Compiles local height and opaque tangent-space normal maps for one hotspot.
/// Boundary derivatives are one-sided, so adjacent slot pixels cannot contribute.
///
/// # Errors
/// Returns a typed error for invalid bounds, parameters, or excessive allocation.
pub fn compile_structural_profile(
    request: StructuralProfileRequest,
) -> Result<StructuralProfileMaps, StructuralProfileError> {
    validate_hotspot_normal_sampling(request.hotspot, request.sheet_size)?;
    validate_profile(request.profile)?;
    let pixel_count = checked_pixel_count(request.hotspot)?;
    let mut height_f32 = Vec::with_capacity(pixel_count);
    for y in 0..request.hotspot.height {
        for x in 0..request.hotspot.width {
            height_f32.push(profile_height(
                request.profile,
                x,
                y,
                request.hotspot.width,
                request.hotspot.height,
            ) as f32);
        }
    }

    let normal_bytes = pixel_count
        .checked_mul(4)
        .ok_or(StructuralProfileError::OutputTooLarge)?;
    let mut normal_rgba8 = Vec::with_capacity(normal_bytes);
    let height_scale = f64::from(request.hotspot.width.min(request.hotspot.height));
    for y in 0..request.hotspot.height {
        for x in 0..request.hotspot.width {
            let (left, right) = normal_sample_axis(x, request.hotspot.width);
            let (top, bottom) = normal_sample_axis(y, request.hotspot.height);
            let horizontal_span = f64::from(right - left).max(1.0);
            let vertical_span = f64::from(bottom - top).max(1.0);
            let dx = (f64::from(read_height(&height_f32, request.hotspot.width, right, y))
                - f64::from(read_height(&height_f32, request.hotspot.width, left, y)))
                * height_scale
                / horizontal_span;
            let dy = (f64::from(read_height(&height_f32, request.hotspot.width, x, bottom))
                - f64::from(read_height(&height_f32, request.hotspot.width, x, top)))
                * height_scale
                / vertical_span;
            let convention_y = match request.normal_convention {
                NormalConvention::OpenGl => -dy,
                NormalConvention::DirectX => dy,
            };
            let inverse_length = (-dx).hypot(convention_y).hypot(1.0).recip();
            normal_rgba8.extend_from_slice(&[
                encode_normal(-dx * inverse_length),
                encode_normal(convention_y * inverse_length),
                encode_normal(inverse_length),
                255,
            ]);
        }
    }

    Ok(StructuralProfileMaps {
        hotspot: request.hotspot,
        height_f32,
        normal_rgba8,
    })
}

fn validate_hotspot(
    hotspot: PixelBounds,
    sheet_size: PixelSize,
) -> Result<(), StructuralProfileError> {
    if sheet_size.width == 0
        || sheet_size.height == 0
        || hotspot.width == 0
        || hotspot.height == 0
    {
        return Err(StructuralProfileError::InvalidDimensions);
    }
    let right = hotspot
        .x
        .checked_add(hotspot.width)
        .ok_or(StructuralProfileError::HotspotOutsideSheet)?;
    let bottom = hotspot
        .y
        .checked_add(hotspot.height)
        .ok_or(StructuralProfileError::HotspotOutsideSheet)?;
    if right > sheet_size.width || bottom > sheet_size.height {
        return Err(StructuralProfileError::HotspotOutsideSheet);
    }
    checked_pixel_count(hotspot)?;
    Ok(())
}

fn checked_pixel_count(hotspot: PixelBounds) -> Result<usize, StructuralProfileError> {
    let pixels = u64::from(hotspot.width)
        .checked_mul(u64::from(hotspot.height))
        .ok_or(StructuralProfileError::OutputTooLarge)?;
    // Height plus RGBA normals consume eight bytes per pixel.
    if pixels > 1_073_741_824 / 8 {
        return Err(StructuralProfileError::OutputTooLarge);
    }
    usize::try_from(pixels).map_err(|_| StructuralProfileError::OutputTooLarge)
}

fn validate_profile(profile: StructuralProfile) -> Result<(), StructuralProfileError> {
    let values = [
        profile.amplitude,
        profile.edge_width,
        profile.frame_width,
        profile.inner_radius,
        profile.outer_radius,
    ];
    if values.into_iter().any(|value| !value.is_finite() || value < 0.0) {
        return Err(StructuralProfileError::InvalidProfileParameters);
    }
    let valid = match profile.kind {
        ProfileKind::Flat => true,
        ProfileKind::ConvexBevel45
        | ProfileKind::ConcaveGroove45
        | ProfileKind::RoundedBevel => profile.amplitude > 0.0 && profile.edge_width > 0.0,
        ProfileKind::PanelFrame => {
            profile.amplitude > 0.0
                && profile.edge_width > 0.0
                && profile.frame_width > profile.edge_width
                && profile.frame_width <= 0.5
        }
        ProfileKind::RadialDisc => {
            profile.amplitude > 0.0
                && profile.edge_width > 0.0
                && profile.outer_radius > profile.edge_width
                && profile.outer_radius <= 0.5
        }
        ProfileKind::Annulus => {
            profile.amplitude > 0.0
                && profile.edge_width > 0.0
                && profile.inner_radius < profile.outer_radius
                && profile.outer_radius <= 0.5
                && profile.edge_width * 2.0 <= profile.outer_radius - profile.inner_radius
        }
    };
    if valid {
        Ok(())
    } else {
        Err(StructuralProfileError::InvalidProfileParameters)
    }
}

fn profile_height(
    profile: StructuralProfile,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> f64 {
    let scale = f64::from(width.min(height));
    let pixel_x = f64::from(x) + 0.5;
    let pixel_y = f64::from(y) + 0.5;
    let edge_distance = pixel_x
        .min(f64::from(width) - pixel_x)
        .min(pixel_y.min(f64::from(height) - pixel_y))
        / scale;
    let center_x = pixel_x - f64::from(width) * 0.5;
    let center_y = pixel_y - f64::from(height) * 0.5;
    let radius = center_x.hypot(center_y) / scale;

    match profile.kind {
        ProfileKind::Flat => 0.0,
        ProfileKind::ConvexBevel45 => {
            profile.amplitude * linear_ramp(edge_distance / profile.edge_width)
        }
        ProfileKind::ConcaveGroove45 => {
            -profile.amplitude * (1.0 - linear_ramp(edge_distance / profile.edge_width))
        }
        ProfileKind::RoundedBevel => {
            let phase = linear_ramp(edge_distance / profile.edge_width)
                * std::f64::consts::FRAC_PI_2;
            profile.amplitude * phase.sin()
        }
        ProfileKind::PanelFrame => {
            let outer = smooth_ramp(edge_distance / profile.edge_width);
            let inner = smooth_ramp((profile.frame_width - edge_distance) / profile.edge_width);
            profile.amplitude * outer * inner
        }
        ProfileKind::RadialDisc => {
            let inside = (profile.outer_radius - radius) / profile.edge_width;
            profile.amplitude * smooth_ramp(inside)
        }
        ProfileKind::Annulus => {
            let inside = (radius - profile.inner_radius)
                .min(profile.outer_radius - radius)
                / profile.edge_width;
            profile.amplitude * smooth_ramp(inside)
        }
    }
}

fn linear_ramp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn smooth_ramp(value: f64) -> f64 {
    let value = linear_ramp(value);
    value * value * (3.0 - 2.0 * value)
}

fn normal_sample_axis(coordinate: u32, extent: u32) -> (u32, u32) {
    if extent <= 1 {
        (0, 0)
    } else if coordinate == 0 {
        (0, 1)
    } else if coordinate + 1 >= extent {
        (extent - 2, extent - 1)
    } else {
        (coordinate - 1, coordinate + 1)
    }
}

fn read_height(heights: &[f32], width: u32, x: u32, y: u32) -> f32 {
    let index = usize::try_from(u64::from(y) * u64::from(width) + u64::from(x))
        .expect("validated local height index");
    heights[index]
}

fn encode_normal(value: f64) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let encoded = ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0).round() as u8;
    encoded
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{PixelBounds, PixelSize};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    use super::{
        NormalConvention, ProfileKind, StructuralProfile, StructuralProfileError,
        StructuralProfileMaps, StructuralProfileRequest, compile_structural_profile,
        profile_kind_for_slot_key, validate_hotspot_normal_sampling,
    };

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProfileGolden {
        width: u32,
        height: u32,
        cases: Vec<ProfileGoldenCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProfileGoldenCase {
        kind: String,
        open_gl_sha256: String,
        direct_x_sha256: String,
    }

    fn profile_kind(name: &str) -> ProfileKind {
        match name {
            "flat" => ProfileKind::Flat,
            "convexBevel45" => ProfileKind::ConvexBevel45,
            "concaveGroove45" => ProfileKind::ConcaveGroove45,
            "roundedBevel" => ProfileKind::RoundedBevel,
            "panelFrame" => ProfileKind::PanelFrame,
            "radialDisc" => ProfileKind::RadialDisc,
            "annulus" => ProfileKind::Annulus,
            other => panic!("unknown profile fixture kind: {other}"),
        }
    }

    fn compile(kind: ProfileKind, convention: NormalConvention) -> StructuralProfileMaps {
        compile_at(kind, convention, 9, 7, 32, 24)
    }

    fn compile_at(
        kind: ProfileKind,
        convention: NormalConvention,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> StructuralProfileMaps {
        compile_structural_profile(StructuralProfileRequest {
            profile: StructuralProfile::for_kind(kind),
            hotspot: PixelBounds { x, y, width, height },
            sheet_size: PixelSize {
                width: x + width + 5,
                height: y + height + 5,
            },
            normal_convention: convention,
        })
        .expect("structural profile")
    }

    fn map_digest(maps: &StructuralProfileMaps) -> String {
        let mut digest = Sha256::new();
        for height in &maps.height_f32 {
            digest.update(height.to_bits().to_le_bytes());
        }
        digest.update(&maps.normal_rgba8);
        format!("{:x}", digest.finalize())
    }

    #[test]
    fn golden_profiles_are_stable_for_each_normal_convention() {
        let golden: ProfileGolden = serde_json::from_str(include_str!(
            "../../../fixtures/renders/slice-6-structural-profiles.json"
        ))
        .expect("valid structural profile golden fixture");
        let mut pending = Vec::new();

        for case in golden.cases {
            let kind = profile_kind(&case.kind);
            for (convention, expected, label) in [
                (NormalConvention::OpenGl, case.open_gl_sha256, "OpenGL"),
                (NormalConvention::DirectX, case.direct_x_sha256, "DirectX"),
            ] {
                let maps = compile_at(kind, convention, 9, 7, golden.width, golden.height);
                let digest = map_digest(&maps);
                if expected == "pending" {
                    pending.push(format!("{} {}={digest}", case.kind, label));
                } else {
                    assert_eq!(digest, expected, "{} {} golden", case.kind, label);
                }
            }
        }

        assert!(
            pending.is_empty(),
            "record structural profile golden hashes: {}",
            pending.join(", ")
        );
    }

    #[test]
    fn normal_boundary_sampling_is_local_to_each_hotspot() {
        let first = compile_at(
            ProfileKind::ConvexBevel45,
            NormalConvention::OpenGl,
            0,
            0,
            32,
            24,
        );
        let second = compile_at(
            ProfileKind::ConvexBevel45,
            NormalConvention::OpenGl,
            32,
            0,
            32,
            24,
        );
        assert_eq!(first.height_f32, second.height_f32);
        assert_eq!(first.normal_rgba8, second.normal_rgba8);
        assert_eq!(
            validate_hotspot_normal_sampling(
                PixelBounds {
                    x: 32,
                    y: 0,
                    width: 32,
                    height: 24,
                },
                PixelSize {
                    width: 64,
                    height: 24,
                },
            ),
            Ok(())
        );
        assert_eq!(
            validate_hotspot_normal_sampling(
                PixelBounds {
                    x: 32,
                    y: 0,
                    width: 33,
                    height: 24,
                },
                PixelSize {
                    width: 64,
                    height: 24,
                },
            ),
            Err(StructuralProfileError::HotspotOutsideSheet)
        );
    }

    #[test]
    fn normal_conventions_invert_only_the_green_channel() {
        let open_gl = compile(ProfileKind::RadialDisc, NormalConvention::OpenGl);
        let direct_x = compile(ProfileKind::RadialDisc, NormalConvention::DirectX);
        assert!(open_gl
            .normal_rgba8
            .chunks_exact(4)
            .zip(direct_x.normal_rgba8.chunks_exact(4))
            .any(|(open_gl, direct_x)| {
                open_gl[0] == direct_x[0]
                    && open_gl[1] != direct_x[1]
                    && open_gl[2] == direct_x[2]
                    && open_gl[3] == direct_x[3]
            }));
    }

    #[test]
    fn slot_key_profile_defaults_are_stable() {
        assert_eq!(profile_kind_for_slot_key("surface_01"), ProfileKind::Flat);
        assert_eq!(profile_kind_for_slot_key("trim_bevel"), ProfileKind::ConvexBevel45);
        assert_eq!(profile_kind_for_slot_key("recessed_seam"), ProfileKind::ConcaveGroove45);
        assert_eq!(profile_kind_for_slot_key("rounded_cap"), ProfileKind::RoundedBevel);
        assert_eq!(profile_kind_for_slot_key("panel_frame"), ProfileKind::PanelFrame);
        assert_eq!(profile_kind_for_slot_key("radial_fixture"), ProfileKind::RadialDisc);
        assert_eq!(profile_kind_for_slot_key("ring_detail"), ProfileKind::Annulus);
    }

    #[test]
    fn invalid_profile_parameters_are_rejected() {
        let result = compile_structural_profile(StructuralProfileRequest {
            profile: StructuralProfile {
                edge_width: 0.0,
                ..StructuralProfile::for_kind(ProfileKind::ConvexBevel45)
            },
            hotspot: PixelBounds {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            sheet_size: PixelSize {
                width: 8,
                height: 8,
            },
            normal_convention: NormalConvention::OpenGl,
        });
        assert_eq!(result, Err(StructuralProfileError::InvalidProfileParameters));
    }
}
