#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float2 resolution;
    float time;
    float width;
    float height;
    float opacity;
    float scale;
    float softness;
    float average;
    float peak;
    float processing;
    float post_processing;
    float capturing;
    float editing;
    float queued_count;
    float line_style;
    float line_count;
    float line_curvature;
    float line_speed;
    float line_sharpness;
    float line_glow;
    float sphere_depth;
    float light_angle;
    float sphere_outline;
    float completion;
    float recording_flash;
    float _padding;
};

struct VertexOutput {
    float4 position [[position]];
    float2 uv;
};

vertex VertexOutput indicator_vertex(uint vertex_id [[vertex_id]]) {
    const float2 positions[] = {
        float2(-1.0, -1.0),
        float2( 3.0, -1.0),
        float2(-1.0,  3.0),
    };
    VertexOutput output;
    output.position = float4(positions[vertex_id], 0.0, 1.0);
    output.uv = positions[vertex_id] * float2(0.5, -0.5) + 0.5;
    return output;
}

float rounded_box(float2 point, float2 half_size, float radius) {
    float2 q = abs(point) - half_size + radius;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

float coverage(float distance, float softness) {
    return smoothstep(softness, -softness, distance);
}

float glow(float distance, float radius) {
    float outside = max(distance, 0.0);
    return exp2(-outside * outside / max(radius * radius, 0.001));
}

void composite(thread float4 &destination, float3 color, float alpha) {
    alpha = clamp(alpha, 0.0, 1.0);
    destination.rgb = color * alpha + destination.rgb * (1.0 - alpha);
    destination.a = alpha + destination.a * (1.0 - alpha);
}

void screen(thread float4 &destination, float3 color, float amount) {
    amount = clamp(amount, 0.0, 1.0);
    destination.rgb = 1.0 - (1.0 - destination.rgb) * (1.0 - color * amount);
}

fragment float4 indicator_fragment(
    VertexOutput input [[stage_in]],
    constant Uniforms &uniforms [[buffer(0)]])
{
    float backing_scale = uniforms.resolution.x / 112.0;
    float2 point = (input.uv * uniforms.resolution - uniforms.resolution * 0.5)
        / backing_scale / max(uniforms.scale, 0.001);
    float2 half_size = float2(uniforms.width, uniforms.height) * 0.5;
    float radius = uniforms.height * 0.5;
    float distance = rounded_box(point, half_size, radius);
    float lifecycle_softness = max(uniforms.softness, 0.0);
    float edge = (max(fwidth(distance), 0.32) + lifecycle_softness * 0.72)
        / max(uniforms.scale, 0.001);
    float shape = coverage(distance, edge);
    float detail_clarity = exp2(-lifecycle_softness * 0.34);
    float4 result = 0.0;

    float processing = clamp(uniforms.processing, 0.0, 1.0);
    float post_processing = clamp(uniforms.post_processing, 0.0, 1.0);
    float editing = clamp(uniforms.editing, 0.0, 1.0);
    float completion = clamp(uniforms.completion, 0.0, 1.0);
    float recording_flash = clamp(uniforms.recording_flash, 0.0, 1.0)
        * (1.0 - processing);
    float completion_flash = smoothstep(0.0, 0.16, completion)
        * (1.0 - smoothstep(0.32, 1.0, completion));
    float recording = 1.0 - processing;
    float circularity = 1.0 - smoothstep(0.5, 2.25, abs(uniforms.width - uniforms.height));

    float3 red_accent = mix(
        float3(1.0, 0.025, 0.035),
        float3(0.05, 0.85, 0.58),
        editing);
    float3 blue_accent = float3(0.1, 0.34, 1.0);
    float3 violet_accent = float3(0.64, 0.2, 1.0);
    float3 pipeline_accent = mix(blue_accent, violet_accent, post_processing);
    float3 accent = mix(red_accent, pipeline_accent, processing);

    float average_power = clamp(uniforms.average, 0.0, 1.0);
    float peak_power = clamp(uniforms.peak, 0.0, 1.0);
    float average_activation = smoothstep(0.0, 0.1, average_power);
    float outer = recording * (
        glow(distance, 4.0 + lifecycle_softness * 0.5) * average_power * 0.72
        + glow(distance, 8.0 + lifecycle_softness) * average_power * 0.36);
    outer += processing * glow(distance, 2.6 + lifecycle_softness) * 0.045;
    outer += glow(distance, 6.0 + lifecycle_softness * 0.5) * completion_flash * 0.22;
    outer += glow(distance, 7.0 + lifecycle_softness * 0.5) * recording_flash * 0.5;
    composite(result, accent, outer * (1.0 - shape));

    float3 recording_base = mix(
        float3(0.5, 0.0, 0.0),
        float3(1.0, 0.0, 0.0),
        average_power);
    recording_base = mix(recording_base, float3(0.0, 0.48, 0.3), editing);
    recording_base = mix(recording_base, float3(1.0, 0.08, 0.06), recording_flash * 0.72);
    float sphere_light = clamp(0.48 - point.x * 0.035 - point.y * 0.045, 0.0, 1.0);
    float3 blue_base = mix(
        float3(0.0, 0.02, 0.32),
        float3(0.04, 0.18, 0.68),
        sphere_light);
    float3 violet_base = mix(
        float3(0.12, 0.0, 0.3),
        float3(0.38, 0.04, 0.68),
        sphere_light);
    float3 pipeline_base = mix(blue_base, violet_base, post_processing);
    float3 base = mix(recording_base, pipeline_base, processing);
    composite(result, base, shape);

    float inside_edge = clamp(-distance / 4.0, 0.0, 1.0);
    float inner_edge = shape * (1.0 - inside_edge);
    float inner_edge_strength = mix(0.48, 0.07, processing);
    composite(result, accent, inner_edge * inner_edge_strength);

    // Match the original Hex recording stack: a padded red fill, a nearly
    // full-width white beam, and a broad peak-driven red glow.
    float red_fill_distance = rounded_box(point, float2(22.0, 2.0), 2.0);
    float red_fill = exp2(-pow(max(red_fill_distance, 0.0) / 2.0, 2.0)) * shape;
    screen(result, red_accent, red_fill * average_activation * recording * detail_clarity);

    float white_beam_distance = rounded_box(point, float2(21.0, 1.0), 1.0);
    float white_beam = exp2(-pow(max(white_beam_distance, 0.0) / 1.0, 2.0)) * shape;
    screen(
        result,
        float3(1.0),
        white_beam * average_activation * 0.56 * recording * detail_clarity);

    float peak_half_width = 22.0 * min(peak_power + 0.6, 1.0);
    float peak_distance = rounded_box(point, float2(peak_half_width, 2.0), 2.0);
    float peak_beam = exp2(-pow(max(peak_distance, 0.0) / 4.0, 2.0)) * shape;
    screen(
        result,
        red_accent,
        peak_beam * smoothstep(0.0, 0.1, peak_power) * 0.5 * recording * detail_clarity);

    if (processing > 0.0) {
        float orb_radius = uniforms.height * 0.5 - 0.6;
        float orb_distance = length(point) - orb_radius;
        float orb_edge = max(fwidth(orb_distance), 0.26);
        float orb_mask = coverage(orb_distance, orb_edge) * circularity;
        float2 sphere_uv = point / max(orb_radius, 0.001);
        float surface_z = sqrt(max(1.0 - dot(sphere_uv, sphere_uv), 0.0));
        float depth = mix(0.55, 1.45, clamp(uniforms.sphere_depth, 0.0, 1.0));
        float normal_z = pow(surface_z, depth);
        float light_phase = uniforms.light_angle * 6.2831853
            + uniforms.time * uniforms.line_speed * 0.22;
        float2 light_direction = float2(cos(light_phase), sin(light_phase));
        float light = clamp(
            0.32 + dot(sphere_uv, light_direction) * 0.18 + normal_z * 0.28,
            0.0,
            1.0);
        float3 orb_shadow = mix(float3(0.0, 0.015, 0.24), float3(0.1, 0.0, 0.26), post_processing);
        float3 orb_light = mix(float3(0.05, 0.22, 0.72), float3(0.4, 0.05, 0.7), post_processing);
        composite(result, mix(orb_shadow, orb_light, light), orb_mask * processing * detail_clarity);

        float rim = pow(1.0 - normal_z, 2.4);
        composite(result, pipeline_accent, rim * orb_mask * processing * 0.3 * detail_clarity);

        float style = clamp(uniforms.line_style, 0.0, 2.0);
        float sharpness = clamp(uniforms.line_sharpness, 0.0, 1.0);
        float blur_amount = 1.0 - sharpness;
        float highlight_count = clamp(round(uniforms.line_count), 1.0, 6.0);
        float shine = 0.0;
        float soft_shine = 0.0;
        if (style < 0.5) {
            // A moving area light gives the orb motion without drawing stripes
            // across its face.
            float focus = clamp(
                dot(sphere_uv, light_direction) * 0.55 + normal_z * 0.72,
                0.0,
                1.0);
            shine = pow(focus, mix(3.0, 12.0, sharpness));
        } else if (style < 1.5) {
            // Meridian highlights wrap around the sphere instead of crossing
            // it as flat screen-space lines.
            float longitude = atan2(sphere_uv.x, normal_z) / 6.2831853;
            float travel = uniforms.time * uniforms.line_speed * 0.11;
            for (int index = 0; index < 6; index++) {
                if (float(index) >= highlight_count) {
                    break;
                }
                float offset = float(index) / highlight_count;
                float angular_delta = fract(longitude - travel - offset + 0.5) - 0.5;
                float angular_distance = abs(angular_delta);
                float core = exp2(-pow(angular_distance / 0.018, 2.0));
                float halo_radius = mix(0.025, 0.14, blur_amount);
                float halo = exp2(-pow(angular_distance / halo_radius, 2.0));
                shine = max(shine, core);
                soft_shine = max(soft_shine, halo);
            }
        } else {
            // Curved ribbons remain available in the lab for comparison, but
            // are no longer the production default.
            float curvature = mix(0.0, 3.0, clamp(uniforms.line_curvature, 0.0, 1.0));
            float sweep_coordinate = point.x + sphere_uv.y * sphere_uv.y * curvature;
            float sweep_travel = orb_radius * 2.0 + 18.0;
            float sweep_start = fmod(
                uniforms.time * 38.0 * uniforms.line_speed,
                sweep_travel) - orb_radius - 7.0;
            float spacing = mix(7.0, 2.8, (highlight_count - 1.0) / 5.0);
            for (int index = 0; index < 6; index++) {
                if (float(index) >= highlight_count) {
                    break;
                }
                float line_distance = abs(
                    sweep_coordinate - (sweep_start - float(index) * spacing));
                float aa = max(fwidth(line_distance), 0.24);
                float band = 1.0 - smoothstep(
                    mix(1.4, 0.25, sharpness) - aa,
                    mix(4.8, 1.5, sharpness) + aa,
                    line_distance);
                shine = max(shine, band);
            }
        }
        float3 shine_color = mix(float3(0.66, 0.84, 1.0), float3(0.92, 0.72, 1.0), post_processing);
        float shine_strength = mix(0.16, 0.68, clamp(uniforms.line_glow, 0.0, 1.0));
        float surface_wrap = orb_mask * mix(0.28, 1.0, normal_z);
        float rim_spill = (1.0 - orb_mask)
            * exp2(-pow(max(orb_distance, 0.0) / 2.2, 2.0));
        composite(
            result,
            shine_color,
            soft_shine * (surface_wrap + rim_spill * 0.72) * processing * blur_amount
                * shine_strength * 0.52
                * detail_clarity);
        composite(
            result,
            shine_color,
            shine * surface_wrap * processing * shine_strength
                * mix(1.0, 0.36, blur_amount) * detail_clarity);
    }

    // Pending jobs sit beside the foreground state. Keeping this geometry
    // stationary prevents queue status from reading as part of the sphere.
    float queued_count = min(uniforms.queued_count, 4.0);
    for (int index = 0; index < 4; index++) {
        if (float(index) >= queued_count) {
            break;
        }
        float2 center = float2(
            uniforms.width * 0.5 + 10.0 + float(index) * 3.4,
            0.0);
        float dot_distance = length(point - center) - 0.68;
        float dot = coverage(dot_distance, 0.22);
        float leading_processing = index == 0
            ? post_processing * clamp(uniforms.capturing, 0.0, 1.0)
            : 0.0;
        float3 queued_blue = float3(0.12, 0.58, 1.0);
        float3 dot_color = mix(queued_blue, violet_accent, leading_processing);
        composite(result, dot_color, dot * (index == 0 ? 0.92 : 0.52));
    }

    screen(result, float3(0.72, 0.88, 1.0), shape * completion_flash * 0.32);
    float completion_rim = exp2(-pow(abs(distance) / 0.72, 2.0));
    screen(result, float3(0.82, 0.93, 1.0), completion_rim * completion_flash * 0.58);

    float sphere_outline = clamp(uniforms.sphere_outline, 0.0, 1.0);
    float processing_stroke = mix(0.04, 0.4, sphere_outline);
    float stroke = coverage(
        abs(distance) - 0.4,
        0.38 + lifecycle_softness * 0.3) * mix(0.56, processing_stroke, processing) * detail_clarity;
    composite(result, mix(accent, float3(1.0), 0.1), stroke);
    screen(result, float3(1.0, 0.2, 0.16), shape * recording_flash * 0.36);
    float recording_flash_rim = exp2(-pow(abs(distance) / 0.9, 2.0));
    screen(result, float3(1.0, 0.58, 0.48), recording_flash_rim * recording_flash * 0.82);

    result *= uniforms.opacity;
    return result;
}
