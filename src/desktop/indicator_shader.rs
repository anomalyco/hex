//! CPU port of the Metal dictation-indicator fragment shader.
//!
//! This module is a pure-Rust, pixel-for-pixel translation of
//! `src/platform/macos/dictation_indicator.metal` so the Windows and Linux
//! shells can render the exact macOS dictation HUD into a pixel buffer.
//! It MUST be kept in lockstep with that shader: any change to the Metal
//! source has to be mirrored here (and vice versa).
//!
//! Translation notes:
//! - `fwidth(x)` has no CPU equivalent (there is no screen-space derivative
//!   when shading a single sample). Every use in the Metal source is wrapped
//!   in `max(fwidth(x), K)` where the constant `K` was authored as the
//!   anti-aliasing floor, so we substitute `fwidth(x) = 0.0` and let the
//!   floor constant win.
//! - Metal `fract(x)` is `x - floor(x)`; Rust's `f32::fract` truncates
//!   instead, so a local `fract` helper reproduces the Metal semantics.
//! - Metal `fmod(a, b)` is the truncated remainder, which is exactly Rust's
//!   `%` on floats, so `%` is used directly.
//! - Metal `smoothstep` is used with descending edges (`coverage`); the
//!   helper below implements the raw Hermite formula, which handles that the
//!   same way the GPU does.

// TODO: remove once the Windows/Linux shells render the HUD through this
// module.
#![allow(dead_code)]

/// Mirrors the Metal `Uniforms` struct field-for-field (minus `_padding`).
#[derive(Clone, Copy, Debug, Default)]
pub struct IndicatorUniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
    pub scale: f32,
    pub softness: f32,
    pub average: f32,
    pub peak: f32,
    pub processing: f32,
    pub post_processing: f32,
    pub capturing: f32,
    pub editing: f32,
    pub queued_count: f32,
    pub line_style: f32,
    pub line_count: f32,
    pub line_curvature: f32,
    pub line_speed: f32,
    pub line_sharpness: f32,
    pub line_glow: f32,
    pub sphere_depth: f32,
    pub light_angle: f32,
    pub sphere_outline: f32,
    pub completion: f32,
    pub recording_flash: f32,
}

/// The literal the Metal source uses for two pi.
const TAU: f32 = std::f32::consts::TAU;

// --- Minimal vector helpers (Metal float2/float3 stand-ins) ---

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)]
}

fn sub2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn length2(v: [f32; 2]) -> f32 {
    dot2(v, v).sqrt()
}

/// Raw Hermite smoothstep, matching the GPU behavior even when
/// `edge0 > edge1` (which `coverage` relies on).
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Metal `fract`: `x - floor(x)`, always in `[0, 1)`. NOT Rust's
/// `f32::fract`, which is `x - trunc(x)` and goes negative for negative `x`.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

// --- Shader helpers, ported one-for-one ---

pub fn rounded_box(point: [f32; 2], half_size: [f32; 2], radius: f32) -> f32 {
    let q = [
        point[0].abs() - half_size[0] + radius,
        point[1].abs() - half_size[1] + radius,
    ];
    length2([q[0].max(0.0), q[1].max(0.0)]) + q[0].max(q[1]).min(0.0) - radius
}

pub fn coverage(distance: f32, softness: f32) -> f32 {
    smoothstep(softness, -softness, distance)
}

pub fn glow(distance: f32, radius: f32) -> f32 {
    let outside = distance.max(0.0);
    (-outside * outside / (radius * radius).max(0.001)).exp2()
}

pub fn composite(destination: &mut [f32; 4], color: [f32; 3], alpha: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    destination[0] = color[0] * alpha + destination[0] * (1.0 - alpha);
    destination[1] = color[1] * alpha + destination[1] * (1.0 - alpha);
    destination[2] = color[2] * alpha + destination[2] * (1.0 - alpha);
    destination[3] = alpha + destination[3] * (1.0 - alpha);
}

pub fn screen(destination: &mut [f32; 4], color: [f32; 3], amount: f32) {
    let amount = amount.clamp(0.0, 1.0);
    destination[0] = 1.0 - (1.0 - destination[0]) * (1.0 - color[0] * amount);
    destination[1] = 1.0 - (1.0 - destination[1]) * (1.0 - color[1] * amount);
    destination[2] = 1.0 - (1.0 - destination[2]) * (1.0 - color[2] * amount);
}

/// Port of `indicator_fragment`. Returns premultiplied RGBA exactly as the
/// Metal code computes `result` (including the final `result *= opacity`).
pub fn shade(uv: [f32; 2], uniforms: &IndicatorUniforms) -> [f32; 4] {
    let backing_scale = uniforms.resolution[0] / 112.0;
    let scale = uniforms.scale.max(0.001);
    let point = [
        (uv[0] * uniforms.resolution[0] - uniforms.resolution[0] * 0.5) / backing_scale / scale,
        (uv[1] * uniforms.resolution[1] - uniforms.resolution[1] * 0.5) / backing_scale / scale,
    ];
    let half_size = [uniforms.width * 0.5, uniforms.height * 0.5];
    let radius = uniforms.height * 0.5;
    let distance = rounded_box(point, half_size, radius);
    let lifecycle_softness = uniforms.softness.max(0.0);
    // fwidth(distance) has no CPU equivalent; the 0.32 floor in the Metal
    // `max(fwidth(distance), 0.32)` was authored as the anti-aliasing floor,
    // so substituting fwidth = 0.0 leaves exactly that constant.
    let edge = (0.0f32.max(0.32) + lifecycle_softness * 0.72) / scale;
    let shape = coverage(distance, edge);
    let detail_clarity = (-lifecycle_softness * 0.34).exp2();
    let mut result = [0.0f32; 4];

    let processing = uniforms.processing.clamp(0.0, 1.0);
    let post_processing = uniforms.post_processing.clamp(0.0, 1.0);
    let editing = uniforms.editing.clamp(0.0, 1.0);
    let completion = uniforms.completion.clamp(0.0, 1.0);
    let recording_flash = uniforms.recording_flash.clamp(0.0, 1.0) * (1.0 - processing);
    let completion_flash =
        smoothstep(0.0, 0.16, completion) * (1.0 - smoothstep(0.32, 1.0, completion));
    let recording = 1.0 - processing;
    let circularity = 1.0 - smoothstep(0.5, 2.25, (uniforms.width - uniforms.height).abs());

    let red_accent = mix3([1.0, 0.025, 0.035], [0.05, 0.85, 0.58], editing);
    let blue_accent = [0.1, 0.34, 1.0];
    let violet_accent = [0.64, 0.2, 1.0];
    let pipeline_accent = mix3(blue_accent, violet_accent, post_processing);
    let accent = mix3(red_accent, pipeline_accent, processing);

    let average_power = uniforms.average.clamp(0.0, 1.0);
    let peak_power = uniforms.peak.clamp(0.0, 1.0);
    let average_activation = smoothstep(0.0, 0.1, average_power);
    let mut outer = recording
        * (glow(distance, 4.0 + lifecycle_softness * 0.5) * average_power * 0.72
            + glow(distance, 8.0 + lifecycle_softness) * average_power * 0.36);
    outer += processing * glow(distance, 2.6 + lifecycle_softness) * 0.045;
    outer += glow(distance, 6.0 + lifecycle_softness * 0.5) * completion_flash * 0.22;
    outer += glow(distance, 7.0 + lifecycle_softness * 0.5) * recording_flash * 0.5;
    composite(&mut result, accent, outer * (1.0 - shape));

    let mut recording_base = mix3([0.5, 0.0, 0.0], [1.0, 0.0, 0.0], average_power);
    recording_base = mix3(recording_base, [0.0, 0.48, 0.3], editing);
    recording_base = mix3(recording_base, [1.0, 0.08, 0.06], recording_flash * 0.72);
    let sphere_light = (0.48 - point[0] * 0.035 - point[1] * 0.045).clamp(0.0, 1.0);
    let blue_base = mix3([0.0, 0.02, 0.32], [0.04, 0.18, 0.68], sphere_light);
    let violet_base = mix3([0.12, 0.0, 0.3], [0.38, 0.04, 0.68], sphere_light);
    let pipeline_base = mix3(blue_base, violet_base, post_processing);
    let base = mix3(recording_base, pipeline_base, processing);
    composite(&mut result, base, shape);

    let inside_edge = (-distance / 4.0).clamp(0.0, 1.0);
    let inner_edge = shape * (1.0 - inside_edge);
    let inner_edge_strength = mix(0.48, 0.07, processing);
    composite(&mut result, accent, inner_edge * inner_edge_strength);

    // Match the original Hex recording stack: a padded red fill, a nearly
    // full-width white beam, and a broad peak-driven red glow.
    let beam_point = sub2(point, [0.0, -0.5]);
    let red_fill_distance = rounded_box(beam_point, [22.0, 2.0], 2.0);
    let red_fill = glow(red_fill_distance, 2.0) * shape;
    screen(
        &mut result,
        red_accent,
        red_fill * average_activation * recording * detail_clarity,
    );

    let white_beam_distance = rounded_box(beam_point, [21.0, 1.0], 1.0);
    let white_beam = glow(white_beam_distance, 1.0) * shape;
    screen(
        &mut result,
        [1.0, 1.0, 1.0],
        white_beam * average_activation * 0.56 * recording * detail_clarity,
    );

    let peak_half_width = 22.0 * (peak_power + 0.6).min(1.0);
    let peak_distance = rounded_box(beam_point, [peak_half_width, 2.0], 2.0);
    let peak_beam = glow(peak_distance, 4.0) * shape;
    screen(
        &mut result,
        red_accent,
        peak_beam * smoothstep(0.0, 0.1, peak_power) * 0.5 * recording * detail_clarity,
    );

    if processing > 0.0 {
        let orb_radius = uniforms.height * 0.5 - 0.6;
        let orb_distance = length2(point) - orb_radius;
        // fwidth(orb_distance) -> 0.0 on the CPU; 0.26 is the authored floor.
        let orb_edge = 0.0f32.max(0.26);
        let orb_mask = coverage(orb_distance, orb_edge) * circularity;
        let sphere_uv = [
            point[0] / orb_radius.max(0.001),
            point[1] / orb_radius.max(0.001),
        ];
        let surface_z = (1.0 - dot2(sphere_uv, sphere_uv)).max(0.0).sqrt();
        let depth = mix(0.55, 1.45, uniforms.sphere_depth.clamp(0.0, 1.0));
        let normal_z = surface_z.powf(depth);
        let light_phase = uniforms.light_angle * TAU + uniforms.time * uniforms.line_speed * 0.22;
        let light_direction = [light_phase.cos(), light_phase.sin()];
        let light =
            (0.32 + dot2(sphere_uv, light_direction) * 0.18 + normal_z * 0.28).clamp(0.0, 1.0);
        let orb_shadow = mix3([0.0, 0.015, 0.24], [0.1, 0.0, 0.26], post_processing);
        let orb_light = mix3([0.05, 0.22, 0.72], [0.4, 0.05, 0.7], post_processing);
        composite(
            &mut result,
            mix3(orb_shadow, orb_light, light),
            orb_mask * processing * detail_clarity,
        );

        let rim = (1.0 - normal_z).powf(2.4);
        composite(
            &mut result,
            pipeline_accent,
            rim * orb_mask * processing * 0.3 * detail_clarity,
        );

        let style = uniforms.line_style.clamp(0.0, 2.0);
        let sharpness = uniforms.line_sharpness.clamp(0.0, 1.0);
        let blur_amount = 1.0 - sharpness;
        let highlight_count = uniforms.line_count.round().clamp(1.0, 6.0);
        let mut shine = 0.0f32;
        let mut soft_shine = 0.0f32;
        if style < 0.5 {
            // A moving area light gives the orb motion without drawing stripes
            // across its face.
            let focus = (dot2(sphere_uv, light_direction) * 0.55 + normal_z * 0.72).clamp(0.0, 1.0);
            shine = focus.powf(mix(3.0, 12.0, sharpness));
        } else if style < 1.5 {
            // Meridian highlights wrap around the sphere instead of crossing
            // it as flat screen-space lines.
            let longitude = sphere_uv[0].atan2(normal_z) / TAU;
            let travel = uniforms.time * uniforms.line_speed * 0.11;
            for index in 0..6 {
                if index as f32 >= highlight_count {
                    break;
                }
                let offset = index as f32 / highlight_count;
                let angular_delta = fract(longitude - travel - offset + 0.5) - 0.5;
                let angular_distance = angular_delta.abs();
                let core = (-(angular_distance / 0.018).powf(2.0)).exp2();
                let halo_radius = mix(0.025, 0.14, blur_amount);
                let halo = (-(angular_distance / halo_radius).powf(2.0)).exp2();
                shine = shine.max(core);
                soft_shine = soft_shine.max(halo);
            }
        } else {
            // Curved ribbons remain available in the lab for comparison, but
            // are no longer the production default.
            let curvature = mix(0.0, 3.0, uniforms.line_curvature.clamp(0.0, 1.0));
            let sweep_coordinate = point[0] + sphere_uv[1] * sphere_uv[1] * curvature;
            let sweep_travel = orb_radius * 2.0 + 18.0;
            // Metal fmod is the truncated remainder, exactly Rust's `%`.
            let sweep_start =
                (uniforms.time * 38.0 * uniforms.line_speed) % sweep_travel - orb_radius - 7.0;
            let spacing = mix(7.0, 2.8, (highlight_count - 1.0) / 5.0);
            for index in 0..6 {
                if index as f32 >= highlight_count {
                    break;
                }
                let line_distance =
                    (sweep_coordinate - (sweep_start - index as f32 * spacing)).abs();
                // fwidth(line_distance) -> 0.0 on the CPU; 0.24 is the floor.
                let aa = 0.0f32.max(0.24);
                let band = 1.0
                    - smoothstep(
                        mix(1.4, 0.25, sharpness) - aa,
                        mix(4.8, 1.5, sharpness) + aa,
                        line_distance,
                    );
                shine = shine.max(band);
            }
        }
        let shine_color = mix3([0.66, 0.84, 1.0], [0.92, 0.72, 1.0], post_processing);
        let shine_strength = mix(0.16, 0.68, uniforms.line_glow.clamp(0.0, 1.0));
        let surface_wrap = orb_mask * mix(0.28, 1.0, normal_z);
        let rim_spill = (1.0 - orb_mask) * glow(orb_distance, 2.2);
        composite(
            &mut result,
            shine_color,
            soft_shine
                * (surface_wrap + rim_spill * 0.72)
                * processing
                * blur_amount
                * shine_strength
                * 0.52
                * detail_clarity,
        );
        composite(
            &mut result,
            shine_color,
            shine
                * surface_wrap
                * processing
                * shine_strength
                * mix(1.0, 0.36, blur_amount)
                * detail_clarity,
        );
    }

    // Pending jobs sit beside the foreground state. Keeping this geometry
    // stationary prevents queue status from reading as part of the sphere.
    let queued_count = uniforms.queued_count.min(4.0);
    for index in 0..4 {
        if index as f32 >= queued_count {
            break;
        }
        let center = [uniforms.width * 0.5 + 10.0 + index as f32 * 3.4, 0.0];
        let dot_distance = length2(sub2(point, center)) - 0.68;
        // `dot` in the Metal source; renamed to avoid shadowing confusion.
        let dot_coverage = coverage(dot_distance, 0.22);
        let leading_processing = if index == 0 {
            post_processing * uniforms.capturing.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let queued_blue = [0.12, 0.58, 1.0];
        let dot_color = mix3(queued_blue, violet_accent, leading_processing);
        composite(
            &mut result,
            dot_color,
            dot_coverage * if index == 0 { 0.92 } else { 0.52 },
        );
    }

    screen(
        &mut result,
        [0.72, 0.88, 1.0],
        shape * completion_flash * 0.32,
    );
    let completion_rim = (-(distance.abs() / 0.72).powf(2.0)).exp2();
    screen(
        &mut result,
        [0.82, 0.93, 1.0],
        completion_rim * completion_flash * 0.58,
    );

    let sphere_outline = uniforms.sphere_outline.clamp(0.0, 1.0);
    let processing_stroke = mix(0.04, 0.4, sphere_outline);
    let stroke = coverage(distance.abs() - 0.4, 0.38 + lifecycle_softness * 0.3)
        * mix(0.56, processing_stroke, processing)
        * detail_clarity;
    composite(&mut result, mix3(accent, [1.0, 1.0, 1.0], 0.1), stroke);
    screen(
        &mut result,
        [1.0, 0.2, 0.16],
        shape * recording_flash * 0.36,
    );
    let recording_flash_rim = (-(distance.abs() / 0.9).powf(2.0)).exp2();
    screen(
        &mut result,
        [1.0, 0.58, 0.48],
        recording_flash_rim * recording_flash * 0.82,
    );

    result[0] *= uniforms.opacity;
    result[1] *= uniforms.opacity;
    result[2] *= uniforms.opacity;
    result[3] *= uniforms.opacity;
    result
}

/// Renders the indicator into a `width * height * 4` byte buffer of
/// row-major, straight-alpha (unpremultiplied) 8-bit RGBA, which is what
/// gpui's `Image::from_bytes` RGBA path expects.
pub fn render(uniforms: &IndicatorUniforms, width: usize, height: usize) -> Vec<u8> {
    fn to_byte(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let uv = [
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            ];
            let premultiplied = shade(uv, uniforms);
            let alpha = premultiplied[3];
            let (r, g, b) = if alpha > 0.001 {
                (
                    premultiplied[0] / alpha,
                    premultiplied[1] / alpha,
                    premultiplied[2] / alpha,
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            pixels.push(to_byte(r));
            pixels.push(to_byte(g));
            pixels.push(to_byte(b));
            pixels.push(to_byte(alpha));
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_uniforms() -> IndicatorUniforms {
        IndicatorUniforms {
            resolution: [224.0, 128.0],
            width: 44.0,
            height: 14.0,
            opacity: 1.0,
            scale: 1.0,
            average: 0.5,
            peak: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn zero_opacity_yields_fully_transparent_pixels() {
        let uniforms = IndicatorUniforms {
            opacity: 0.0,
            ..recording_uniforms()
        };
        let pixels = render(&uniforms, 32, 32);
        assert_eq!(pixels.len(), 32 * 32 * 4);
        assert!(pixels.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn recording_pill_center_is_red_dominant() {
        let uniforms = recording_uniforms();
        let pixels = render(&uniforms, 224, 128);
        let index = (64 * 224 + 112) * 4;
        let (r, g, b, a) = (
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        );
        assert!(a > 128, "expected opaque center, alpha = {a}");
        assert!(r > g, "expected red-dominant center, r = {r}, g = {g}");
        assert!(r > b, "expected red-dominant center, r = {r}, b = {b}");
    }

    #[test]
    fn processing_orb_center_is_blue_dominant() {
        let uniforms = IndicatorUniforms {
            resolution: [224.0, 128.0],
            width: 26.0,
            height: 26.0,
            opacity: 1.0,
            scale: 1.0,
            processing: 1.0,
            line_style: 1.0,
            line_count: 3.0,
            line_speed: 1.0,
            sphere_depth: 0.5,
            light_angle: 0.3,
            line_sharpness: 0.7,
            line_glow: 0.5,
            ..Default::default()
        };
        let pixels = render(&uniforms, 224, 128);
        let index = (64 * 224 + 112) * 4;
        let (r, b, a) = (pixels[index], pixels[index + 2], pixels[index + 3]);
        assert!(a > 128, "expected opaque center, alpha = {a}");
        assert!(b > r, "expected blue-dominant center, r = {r}, b = {b}");
    }
}
